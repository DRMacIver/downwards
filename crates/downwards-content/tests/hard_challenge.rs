use downwards_ai::{
    DirectProbeAudit, DirectProbeAuditStatus, DirectProbePolicy, Replay, SearchTarget,
    SolverConfig, TargetSolveOutcome, audit_direct_controller_probes, solve_target,
};
use downwards_content::{
    HARD_NO_DASH_ABILITIES, HARD_NO_DASH_TARGET, hard_no_dash_scenario,
    hard_no_dash_witness_actions,
};
use downwards_core::{AbilitySet, Action, JumpKind, Simulation, SimulationEvent};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RouteObservation {
    reached_at_tick: Option<usize>,
    jump_presses: usize,
    non_wall_jumps: usize,
    wall_jumps: usize,
    dashes: usize,
    deaths: usize,
    resets: usize,
}

impl RouteObservation {
    fn is_clean_completion(self) -> bool {
        self.reached_at_tick.is_some() && self.dashes == 0 && self.deaths == 0 && self.resets == 0
    }
}

fn observe_route(initial: &Simulation, actions: &[Action]) -> RouteObservation {
    let mut simulation = initial.clone();
    let mut observation = RouteObservation::default();
    let mut previous_action = Action::default();

    for (index, &action) in actions.iter().enumerate() {
        if action.jump && !previous_action.jump {
            observation.jump_presses += 1;
        }
        if action.dash {
            // Count held dash input as forbidden even if this loadout cannot
            // accept it as an ability event.
            observation.dashes += 1;
        }
        if action.restart {
            observation.resets += 1;
        }

        let report = simulation.step(action);
        for event in report.events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { .. }) => observation.wall_jumps += 1,
                SimulationEvent::Jumped(_) => observation.non_wall_jumps += 1,
                SimulationEvent::Dashed { .. } => observation.dashes += 1,
                SimulationEvent::Died(_) => observation.deaths += 1,
                SimulationEvent::Reset => observation.resets += 1,
                SimulationEvent::ExitReached { ref id } if id == HARD_NO_DASH_TARGET => {
                    observation.reached_at_tick.get_or_insert(index + 1);
                }
                SimulationEvent::Landed
                | SimulationEvent::PickupTouched { .. }
                | SimulationEvent::PickupCollected { .. }
                | SimulationEvent::ExitReached { .. } => {}
            }
        }
        previous_action = action;

        if observation.reached_at_tick.is_some() {
            break;
        }
    }

    // Keep the event-based observation honest against the simulation's
    // accumulated counters and terminal state.
    if simulation.deaths() > 0 {
        observation.deaths = observation
            .deaths
            .max(usize::try_from(simulation.deaths()).expect("death count fits usize"));
    }
    if simulation.reached_exit() != Some(HARD_NO_DASH_TARGET) {
        observation.reached_at_tick = None;
    }
    observation
}

fn target() -> SearchTarget {
    SearchTarget::exit(HARD_NO_DASH_TARGET)
}

fn witness_probe_labels(audit: &DirectProbeAudit) -> Vec<String> {
    audit
        .witnesses
        .iter()
        .flat_map(|witness| {
            witness.probes.iter().map(|provenance| {
                format!(
                    "#{} x={} {:?}",
                    provenance.ordinal, provenance.move_x, provenance.policy
                )
            })
        })
        .collect()
}

fn is_walk_or_auto_hop(policy: DirectProbePolicy) -> bool {
    matches!(
        policy,
        DirectProbePolicy::Run
            | DirectProbePolicy::PeriodicJump { .. }
            | DirectProbePolicy::ReactiveJump { .. }
            | DirectProbePolicy::AutoJump
    )
}

/// Greedily remove time and held intents, retaining a change only after an
/// authoritative clean completion. This is deliberately not an optimizer or
/// evidence of a minimum route; it is a deterministic attempt to expose an
/// obviously padded witness, especially one padded with needless jumps.
fn greedily_simplify(initial: &Simulation, original: &[Action]) -> Vec<Action> {
    let reached_at_tick = observe_route(initial, original)
        .reached_at_tick
        .expect("the stored witness must complete before simplification");
    let mut actions = original[..reached_at_tick].to_vec();

    loop {
        let before_pass = actions.clone();

        // Try large contiguous deletions before individual ticks. Deleting
        // ticks also shifts timed-hazard phase, so every candidate is
        // re-simulated.
        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if observe_route(initial, &candidate).is_clean_completion() {
                    actions = candidate;
                } else {
                    start += chunk;
                }
            }
            chunk /= 2;
        }

        // Preserve timing while trying to neutralize semantic input. Clearing
        // a whole held-jump span first directly tests for gratuitous hop
        // cycles.
        let mut start = 0;
        while start < actions.len() {
            if !actions[start].jump {
                start += 1;
                continue;
            }
            let end = actions[start..]
                .iter()
                .position(|action| !action.jump)
                .map_or(actions.len(), |offset| start + offset);
            let mut candidate = actions.clone();
            for action in &mut candidate[start..end] {
                action.jump = false;
            }
            if observe_route(initial, &candidate).is_clean_completion() {
                actions = candidate;
            } else {
                start = end;
            }
        }

        // A final per-tick pass catches smaller redundant holds and steering.
        for index in (0..actions.len()).rev() {
            for clear in [
                |action: &mut Action| action.jump = false,
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index]
                    && observe_route(initial, &candidate).is_clean_completion()
                {
                    actions = candidate;
                }
            }
        }

        if actions == before_pass {
            break;
        }
    }

    actions
}

#[test]
fn stored_witness_is_an_exact_clean_no_dash_completion() {
    let initial = hard_no_dash_scenario();
    assert_eq!(initial.abilities(), HARD_NO_DASH_ABILITIES);
    assert!(initial.abilities().wall_jump);
    assert!(!initial.abilities().dash);

    let actions = hard_no_dash_witness_actions();
    assert!(!actions.is_empty());
    assert!(actions.iter().all(|action| !action.dash && !action.restart));

    let replay = Replay::record(&initial, actions.iter().copied());
    let verified = replay
        .verify(&initial)
        .expect("the stored witness must replay exactly through authoritative physics");
    assert_eq!(verified.frames_verified, actions.len());
    assert_eq!(verified.reached_exit.as_deref(), Some(HARD_NO_DASH_TARGET));

    let observation = observe_route(&initial, &actions);
    assert!(observation.is_clean_completion(), "{observation:?}");
    assert_eq!(observation.reached_at_tick, Some(actions.len()));
    assert!(
        observation.wall_jumps >= 5,
        "the generated route must exercise the full wall-jump climb"
    );
    assert_eq!(
        observation.jump_presses,
        observation.non_wall_jumps + observation.wall_jumps,
        "the stored route has an unaccepted, potentially gratuitous jump press"
    );
    eprintln!(
        "hard no-Dash stored witness: {} ticks, {} jump presses, {} grounded/coyote/buffered jumps, {} wall jumps",
        actions.len(),
        observation.jump_presses,
        observation.non_wall_jumps,
        observation.wall_jumps,
    );
}

#[test]
fn baseline_full_search_and_finite_probes_find_no_known_bypass() {
    let mut initial =
        Simulation::with_abilities(hard_no_dash_scenario().room().clone(), AbilitySet::NONE);
    initial.enable_current_player_movement();
    let config = SolverConfig::for_abilities(AbilitySet::NONE);

    let solve = solve_target(&initial, target(), &config).expect("the target is defined");
    let TargetSolveOutcome::Inconclusive { reason, stats } = solve else {
        panic!("the bounded baseline solver found a no-wall-jump bypass: {solve:?}");
    };
    eprintln!(
        "baseline full solver: bounded {reason:?} miss after {} expanded nodes and {} simulated ticks (deepest path {})",
        stats.expanded_nodes, stats.simulated_ticks, stats.deepest_path_ticks,
    );

    let audit = audit_direct_controller_probes(&initial, &[target()], &config)
        .expect("the finite baseline direct-controller audit runs");
    assert_eq!(
        audit.status,
        DirectProbeAuditStatus::Complete,
        "a budget-limited probe audit would not support a no-known-bypass observation"
    );
    assert!(
        audit.witnesses.is_empty(),
        "baseline direct-controller bypasses: {:?}",
        witness_probe_labels(&audit)
    );
    eprintln!(
        "baseline finite direct-controller audit: {:?}, {} probes, {} simulated ticks, zero positives",
        audit.status, audit.stats.expanded_nodes, audit.stats.simulated_ticks,
    );
}

#[test]
fn full_no_dash_direct_controller_portfolio_finds_no_known_easy_route() {
    let initial = hard_no_dash_scenario();
    let config = SolverConfig::for_abilities(HARD_NO_DASH_ABILITIES);
    let audit = audit_direct_controller_probes(&initial, &[target()], &config)
        .expect("the finite no-Dash direct-controller audit runs");

    assert_eq!(audit.status, DirectProbeAuditStatus::Complete);
    let simple_positives = audit
        .witnesses
        .iter()
        .flat_map(|witness| &witness.probes)
        .filter(|provenance| is_walk_or_auto_hop(provenance.policy))
        .collect::<Vec<_>>();
    assert!(
        simple_positives.is_empty(),
        "walk/periodic/reactive/auto-jump controllers found a route: {simple_positives:?}"
    );
    assert!(
        audit.witnesses.is_empty(),
        "a built-in direct controller found a route: {:?}",
        witness_probe_labels(&audit)
    );
    eprintln!(
        "no-Dash finite direct-controller audit: {:?}, {} probes, {} simulated ticks, zero positives",
        audit.status, audit.stats.expanded_nodes, audit.stats.simulated_ticks,
    );
}

#[test]
fn simplifying_the_hard_witness_does_not_erase_the_wall_jump_route() {
    let initial = hard_no_dash_scenario();
    let stored = hard_no_dash_witness_actions();
    let stored_observation = observe_route(&initial, &stored);
    assert!(stored_observation.is_clean_completion());

    let simplified = greedily_simplify(&initial, &stored);
    let simplified_observation = observe_route(&initial, &simplified);
    assert!(simplified_observation.is_clean_completion());
    assert!(
        simplified_observation.wall_jumps > 0,
        "greedy authoritative simplification exposed a wall-jump-free challenge-loadout route"
    );

    eprintln!(
        "hard no-Dash witness greedy simplification: {} -> {} ticks; {} -> {} jump presses; {} -> {} wall jumps",
        stored.len(),
        simplified.len(),
        stored_observation.jump_presses,
        simplified_observation.jump_presses,
        stored_observation.wall_jumps,
        simplified_observation.wall_jumps,
    );
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_current_player_movement_witness() {
    let initial = hard_no_dash_scenario();
    let mut config = SolverConfig::for_abilities(HARD_NO_DASH_ABILITIES);
    config.max_ticks_per_path = 1_200;
    config.max_expanded_nodes = 250_000;
    config.max_simulated_ticks = 8_000_000;
    let outcome = solve_target(&initial, target(), &config).expect("the target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("current movement did not solve the hard challenge: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &found);
    let observation = observe_route(&initial, &actions);
    assert!(observation.is_clean_completion());
    eprintln!(
        "HARD {} -> {} ticks / {} presses / {} wall jumps",
        found.len(),
        actions.len(),
        observation.jump_presses,
        observation.wall_jumps
    );
    let mut start = 0;
    while start < actions.len() {
        let current = actions[start];
        let end = actions[start..]
            .iter()
            .position(|candidate| *candidate != current)
            .map_or(actions.len(), |offset| start + offset);
        eprintln!("({current:?}, {}),", end - start);
        start = end;
    }
}
