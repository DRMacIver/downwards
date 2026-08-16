//! Deterministically regenerate one exact representative route for every authored dungeon floor.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example retune_demo_dungeon`

use std::{cmp::Ordering, collections::BTreeMap, env, fs, path::PathBuf};

use downwards_ai::{
    GroundedStandingRegion, GroundedSupportSolveOutcome, GroundedSupportTarget, NoiseFamily,
    ReachedTarget, Replay, SearchStats, SearchTarget, ShakyHandConfig, SolverConfig,
    TargetSolution, TargetSolveOutcome, audit_direct_controller_probes, evaluate_shaky_hand,
    solve_grounded_support, solve_target,
};
use downwards_content::{
    DemoDungeonRoom, DemoDungeonRouteSpec, DemoDungeonRouteTarget, demo_dungeon_room,
    demo_dungeon_route_specs,
};
use downwards_core::{
    AbilitySet, Action, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION, Simulation,
    SimulationEvent, WallSide,
};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;

const DEFAULT_OUTPUT: &str = "crates/downwards-content/generated/demo-dungeon-witnesses-v1.txt";
const PREVIOUS_WITNESS_ARTIFACT: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");
const SHAKY_CONFIG: ShakyHandConfig = ShakyHandConfig {
    seed: 0,
    trials_per_curve_point: 64,
    grace_ticks: 18,
    correlated_boundaries: 2,
    convergence_confirmation_ticks: 2,
};

#[derive(Clone, Copy, Debug, Default)]
struct RouteObservation {
    ticks: usize,
    action_spans: usize,
    jump_presses: usize,
    accepted_jumps: usize,
    accepted_wall_jumps: usize,
    accepted_dashes: usize,
    accepted_dashes_before_first_wall_jump: usize,
    horizontal_reversals: usize,
}

type StaticRouteKey = (usize, usize, usize, usize, Vec<(i8, i8, bool, bool, bool)>);

fn search_target(spec: DemoDungeonRouteSpec) -> SearchTarget {
    match spec.target {
        DemoDungeonRouteTarget::Door(id) => SearchTarget::door(id),
        DemoDungeonRouteTarget::Pickup(id) => SearchTarget::pickup(id),
        DemoDungeonRouteTarget::GoalExit => SearchTarget::exit(spec.target.id()),
    }
}

fn shaky_seed(route_id: &str) -> u64 {
    route_id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
        ^ 0xD06E_7000
}

fn initial_simulation(spec: DemoDungeonRouteSpec) -> Simulation {
    let room = demo_dungeon_room(spec.room, spec.inventory);
    let mut simulation = match spec.entry_door {
        Some(door) => Simulation::enter_via_door(room, spec.inventory.abilities(), door)
            .unwrap_or_else(|error| panic!("{} cannot enter by {door:?}: {error}", spec.id())),
        None => Simulation::with_abilities(room, spec.inventory.abilities()),
    };
    simulation.enable_current_player_movement();
    simulation
}

fn target_reached(simulation: &Simulation, target: DemoDungeonRouteTarget) -> bool {
    match target {
        DemoDungeonRouteTarget::Door(id) => simulation.reached_exit() == Some(id),
        DemoDungeonRouteTarget::GoalExit => simulation.reached_exit() == Some(target.id()),
        DemoDungeonRouteTarget::Pickup(id) => simulation
            .collected_pickups()
            .any(|pickup| pickup.id() == id),
    }
}

fn clean_completion(
    initial: &Simulation,
    target: DemoDungeonRouteTarget,
    actions: &[Action],
) -> bool {
    let mut simulation = initial.clone();
    for (index, &action) in actions.iter().enumerate() {
        let report = simulation.step(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset))
        {
            return false;
        }
        if target_reached(&simulation, target) {
            return index + 1 == actions.len();
        }
    }
    false
}

fn static_route_key(actions: &[Action]) -> StaticRouteKey {
    let mut previous = Action::default();
    let mut previous_nonzero_x = 0;
    let mut presses = 0;
    let mut reversals = 0;
    for &action in actions {
        presses += usize::from(action.jump && !previous.jump);
        presses += usize::from(action.dash && !previous.dash);
        if action.move_x != 0 {
            reversals +=
                usize::from(previous_nonzero_x != 0 && previous_nonzero_x != action.move_x);
            previous_nonzero_x = action.move_x;
        }
        previous = action;
    }
    (
        actions.windows(2).filter(|pair| pair[0] != pair[1]).count()
            + usize::from(!actions.is_empty()),
        presses,
        reversals,
        actions.len(),
        actions.iter().copied().map(action_key).collect(),
    )
}

/// Delete redundant time and input components while preserving an exact clean completion.
///
/// This is intentionally authoritative resimulation rather than a metric-based rewrite. Besides
/// producing a more useful demonstration, it exposes geometry that admits a much easier route
/// than the beam solver's first positive.
fn greedily_simplify(
    initial: &Simulation,
    target: DemoDungeonRouteTarget,
    original: &[Action],
) -> Vec<Action> {
    let mut actions = original.to_vec();
    loop {
        let before = actions.clone();
        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if clean_completion(initial, target, &candidate)
                    && static_route_key(&candidate) < static_route_key(&actions)
                {
                    actions = candidate;
                } else {
                    start += chunk;
                }
            }
            chunk /= 2;
        }

        for index in (0..actions.len()).rev() {
            for clear in [
                |action: &mut Action| action.jump = false,
                |action: &mut Action| action.dash = false,
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index]
                    && clean_completion(initial, target, &candidate)
                    && static_route_key(&candidate) < static_route_key(&actions)
                {
                    actions = candidate;
                }
            }
        }
        if actions == before {
            return actions;
        }
    }
}

fn observe(
    initial: &Simulation,
    spec: DemoDungeonRouteSpec,
    actions: &[Action],
) -> RouteObservation {
    let mut simulation = initial.clone();
    let mut observation = RouteObservation {
        ticks: actions.len(),
        ..RouteObservation::default()
    };
    let mut previous = Action::default();
    let mut previous_nonzero_x = 0;
    let mut saw_wall_jump = false;
    for (index, &action) in actions.iter().enumerate() {
        observation.action_spans += usize::from(index == 0 || action != previous);
        observation.jump_presses += usize::from(action.jump && !previous.jump);
        if action.move_x != 0 {
            observation.horizontal_reversals +=
                usize::from(previous_nonzero_x != 0 && previous_nonzero_x != action.move_x);
            previous_nonzero_x = action.move_x;
        }
        let report = simulation.step(action);
        for event in report.events {
            match event {
                SimulationEvent::Jumped(kind) => {
                    observation.accepted_jumps += 1;
                    if matches!(kind, downwards_core::JumpKind::Wall { .. }) {
                        observation.accepted_wall_jumps += 1;
                        saw_wall_jump = true;
                    }
                }
                SimulationEvent::Dashed { .. } => {
                    observation.accepted_dashes += 1;
                    observation.accepted_dashes_before_first_wall_jump +=
                        usize::from(!saw_wall_jump);
                }
                SimulationEvent::Died(reason) => {
                    panic!(
                        "{} exact witness dies at tick {}: {reason:?}",
                        spec.id(),
                        index + 1
                    )
                }
                SimulationEvent::Reset => {
                    panic!("{} exact witness resets at tick {}", spec.id(), index + 1)
                }
                SimulationEvent::Landed
                | SimulationEvent::PickupCollected { .. }
                | SimulationEvent::ExitReached { .. } => {}
            }
        }
        assert!(
            !target_reached(&simulation, spec.target) || index + 1 == actions.len(),
            "{} reaches its target before the stored final tick",
            spec.id()
        );
        previous = action;
    }
    assert!(
        target_reached(&simulation, spec.target),
        "{} exact witness does not reach {:?}",
        spec.id(),
        spec.target
    );
    observation
}

fn print_behavior_trace(initial: &Simulation, spec: DemoDungeonRouteSpec, actions: &[Action]) {
    let mut simulation = initial.clone();
    eprintln!("  selected behavior trace for {}:", spec.id());
    for (index, &action) in actions.iter().enumerate() {
        let report = simulation.step(action);
        for event in report.events.iter().filter(|event| {
            matches!(
                event,
                SimulationEvent::Jumped(_)
                    | SimulationEvent::Dashed { .. }
                    | SimulationEvent::Landed
                    | SimulationEvent::PickupCollected { .. }
                    | SimulationEvent::ExitReached { .. }
            )
        }) {
            let bounds = simulation.player().bounds();
            let velocity = simulation.player().velocity_subpixels();
            eprintln!(
                "    t{:>3} at ({:>3},{:>3}) v=({:>5},{:>5}) input=({:+},{:+},j{},d{}) {event:?}",
                index + 1,
                bounds.x,
                bounds.y,
                velocity.x,
                velocity.y,
                action.move_x,
                action.move_y,
                u8::from(action.jump),
                u8::from(action.dash),
            );
        }
    }
}

fn action_key(action: Action) -> (i8, i8, bool, bool, bool) {
    (
        action.move_x,
        action.move_y,
        action.jump,
        action.dash,
        action.restart,
    )
}

fn compare_routes(
    initial: &Simulation,
    spec: DemoDungeonRouteSpec,
    left: &[Action],
    right: &[Action],
) -> Ordering {
    let left_observation = observe(initial, spec, left);
    let right_observation = observe(initial, spec, right);
    let key = |observation: RouteObservation, actions: &[Action]| {
        (
            if matches!(
                spec.room,
                DemoDungeonRoom::VacuumGallery
                    | DemoDungeonRoom::LunarCache
                    | DemoDungeonRoom::StarThreshold
                    | DemoDungeonRoom::ShadowDuct
                    | DemoDungeonRoom::Observatory
                    | DemoDungeonRoom::AuroraSpire
                    | DemoDungeonRoom::Gatehouse
                    | DemoDungeonRoom::CrownSanctum
            ) {
                observation.accepted_dashes_before_first_wall_jump
            } else {
                0
            },
            observation.action_spans,
            observation.horizontal_reversals,
            observation
                .jump_presses
                .saturating_sub(observation.accepted_jumps),
            observation.accepted_jumps + observation.accepted_dashes,
            observation.ticks,
            actions.iter().copied().map(action_key).collect::<Vec<_>>(),
        )
    };
    key(left_observation, left).cmp(&key(right_observation, right))
}

fn right_action(jump: bool, dash: bool) -> Action {
    Action {
        move_x: 1,
        move_y: 0,
        jump,
        dash,
        restart: false,
    }
}

fn clean_step(simulation: &mut Simulation, action: Action) -> bool {
    !simulation
        .step(action)
        .events
        .iter()
        .any(|event| matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset))
}

fn approach_x(initial: &Simulation, target_x: i32) -> Option<(Simulation, Vec<Action>)> {
    let mut simulation = initial.clone();
    let mut actions = Vec::new();
    while simulation.player().bounds().x < target_x && actions.len() < 120 {
        let action = right_action(false, false);
        if !clean_step(&mut simulation, action) || simulation.reached_exit().is_some() {
            return None;
        }
        actions.push(action);
    }
    (simulation.player().grounded() && simulation.player().bounds().x >= target_x)
        .then_some((simulation, actions))
}

/// Search a deliberately small, human-readable controller vocabulary for the two Dash-Chasm
/// transfers. This is an authoring candidate, not a production solver policy: run to a visible
/// takeoff point, hold one jump, press Dash once, then either brake on the recovery island or keep
/// running to the exit.
fn readable_dash_chasm_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let island = GroundedSupportTarget::new(
        150,
        180,
        160,
        [GroundedStandingRegion::new(150, 172).expect("valid authored standing range")],
    )
    .expect("valid authored support waypoint");
    let mut first_candidates = Vec::new();
    for takeoff_x in 65..=82 {
        let Some((takeoff, approach)) = approach_x(initial, takeoff_x) else {
            continue;
        };
        for jump_hold in 1..=10 {
            for dash_delay in 0..=8 {
                for coast in 0..=18 {
                    let mut simulation = takeoff.clone();
                    let mut actions = approach.clone();
                    let suffix = std::iter::repeat_n(right_action(true, false), jump_hold)
                        .chain(std::iter::repeat_n(right_action(false, false), dash_delay))
                        .chain(std::iter::once(right_action(false, true)))
                        .chain(std::iter::repeat_n(right_action(false, false), coast))
                        .chain(std::iter::repeat_n(Action::default(), 30));
                    for action in suffix {
                        if !clean_step(&mut simulation, action) {
                            break;
                        }
                        actions.push(action);
                        if island.is_reached(&simulation) {
                            first_candidates.push((simulation, actions));
                            break;
                        }
                    }
                }
            }
        }
    }
    first_candidates
        .sort_by(|(_, left), (_, right)| static_route_key(left).cmp(&static_route_key(right)));
    first_candidates.dedup_by(|(_, left), (_, right)| left == right);

    let mut complete = Vec::new();
    for (island_state, first_actions) in first_candidates.into_iter().take(8) {
        for takeoff_x in 150..=172 {
            let Some((takeoff, approach)) = approach_x(&island_state, takeoff_x) else {
                continue;
            };
            for jump_hold in 1..=10 {
                for dash_delay in 0..=8 {
                    let mut simulation = takeoff.clone();
                    let mut actions = first_actions.clone();
                    actions.extend(approach.iter().copied());
                    let suffix = std::iter::repeat_n(right_action(true, false), jump_hold)
                        .chain(std::iter::repeat_n(right_action(false, false), dash_delay))
                        .chain(std::iter::once(right_action(false, true)))
                        .chain(std::iter::repeat_n(right_action(false, false), 120));
                    for action in suffix {
                        if !clean_step(&mut simulation, action) {
                            break;
                        }
                        actions.push(action);
                        if simulation.reached_exit() == Some("east") {
                            complete.push(actions);
                            break;
                        }
                    }
                }
            }
        }
    }
    let actions = complete
        .into_iter()
        .min_by(|left, right| static_route_key(left).cmp(&static_route_key(right)))?;
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Door("east".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn segmented_candidate(
    initial: &Simulation,
    spec: DemoDungeonRouteSpec,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    if spec.room == DemoDungeonRoom::DashChasm {
        return readable_dash_chasm_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::CometRun {
        return segmented_comet_run_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::LunarCache {
        return segmented_lunar_cache_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::MoonVault {
        return segmented_moon_vault_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::MeteorRun {
        return readable_meteor_run_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::StarThreshold {
        return segmented_star_threshold_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::ConstellationHall {
        return segmented_constellation_hall_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::ShadowDuct {
        return segmented_shadow_duct_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::Observatory {
        return segmented_observatory_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::GravityLift {
        return segmented_gravity_lift_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::AuroraSpire {
        return segmented_aurora_spire_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::Skybridge {
        return segmented_skybridge_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::Gatehouse {
        return segmented_gatehouse_candidate(initial, target);
    }
    if spec.room == DemoDungeonRoom::CrownSanctum {
        return segmented_crown_sanctum_candidate(initial, target);
    }
    let (waypoint, first_config, second_config) = match spec.room {
        DemoDungeonRoom::VoidPass => (
            GroundedSupportTarget::new(
                170,
                240,
                30,
                [GroundedStandingRegion::new(170, 232).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            SolverConfig::for_abilities(AbilitySet::new(true, false)),
            SolverConfig::for_abilities(AbilitySet::new(true, false)),
        ),
        DemoDungeonRoom::VacuumGallery => (
            GroundedSupportTarget::new(
                170,
                210,
                30,
                [GroundedStandingRegion::new(170, 202).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            // The first segment is deliberately Wall-Jump-only so the candidate cannot waste
            // Dashes while approaching or climbing the authored shaft. Dash is restored only
            // after the player is standing on the visible launch shelf.
            SolverConfig::for_abilities(AbilitySet::new(true, false)),
            SolverConfig::for_abilities(AbilitySet::new(true, true)),
        ),
        DemoDungeonRoom::NovaNiche => (
            GroundedSupportTarget::new(
                180,
                220,
                30,
                [GroundedStandingRegion::new(180, 212).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            SolverConfig::for_abilities(AbilitySet::new(true, false)),
            SolverConfig::for_abilities(AbilitySet::new(false, true)),
        ),
        _ => return None,
    };
    let first_outcome = solve_grounded_support(initial, &waypoint, &first_config).ok()?;
    let GroundedSupportSolveOutcome::Solved(first) = first_outcome else {
        if matches!(
            spec.room,
            DemoDungeonRoom::VacuumGallery | DemoDungeonRoom::NovaNiche
        ) {
            eprintln!(
                "  segmented {} missed its authored shelf: {first_outcome:?}",
                spec.id()
            );
        }
        return None;
    };
    let mut intermediate = initial.clone();
    let mut actions = first.replay.actions().collect::<Vec<_>>();
    for &action in &actions {
        intermediate.step(action);
    }
    let second = if spec.room == DemoDungeonRoom::NovaNiche {
        readable_nova_niche_suffix(&intermediate, target)?
    } else {
        let direct = audit_direct_controller_probes(
            &intermediate,
            std::slice::from_ref(target),
            &second_config,
        )
        .ok()?;
        if let Some(witness) = direct
            .witnesses
            .into_iter()
            .min_by_key(|witness| witness.replay.frames.len())
        {
            TargetSolution {
                target: witness.target,
                reached: witness.reached,
                replay: witness.replay,
                stats: witness.stats_at_first_discovery,
            }
        } else {
            let second_outcome =
                solve_target(&intermediate, target.clone(), &second_config).ok()?;
            let TargetSolveOutcome::Solved(second) = second_outcome else {
                if spec.room == DemoDungeonRoom::VacuumGallery {
                    eprintln!(
                        "  segmented Vacuum Gallery suffix missed its target: {second_outcome:?}"
                    );
                }
                return None;
            };
            second
        }
    };
    actions.extend(second.replay.actions());
    Some(TargetSolution {
        target: second.target,
        reached: second.reached,
        replay: Replay::record(initial, actions),
        stats: second.stats,
    })
}

fn segmented_constellation_hall_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let waypoints = [
        (
            GroundedSupportTarget::new(
                100,
                140,
                140,
                [GroundedStandingRegion::new(100, 132).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            AbilitySet::new(false, true),
        ),
        (
            GroundedSupportTarget::new(
                170,
                220,
                70,
                [GroundedStandingRegion::new(170, 212).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            AbilitySet::new(true, true),
        ),
        (
            GroundedSupportTarget::new(
                200,
                240,
                140,
                [GroundedStandingRegion::new(200, 232).expect("valid authored standing range")],
            )
            .expect("valid authored support waypoint"),
            AbilitySet::new(true, true),
        ),
    ];
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    for (waypoint, abilities) in waypoints {
        let outcome = solve_grounded_support(
            &intermediate,
            &waypoint,
            &SolverConfig::for_abilities(abilities),
        )
        .ok()?;
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            return None;
        };
        actions.extend(solution.replay.actions());
        for action in solution.replay.actions() {
            intermediate.step(action);
        }
    }
    let outcome = solve_target(
        &intermediate,
        target.clone(),
        &SolverConfig::for_abilities(AbilitySet::new(false, true)),
    )
    .ok()?;
    let TargetSolveOutcome::Solved(solution) = outcome else {
        return None;
    };
    actions.extend(solution.replay.actions());
    Some(TargetSolution {
        target: solution.target,
        reached: solution.reached,
        replay: Replay::record(initial, actions),
        stats: solution.stats,
    })
}

fn segmented_shadow_duct_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Author each visible idea independently. Dash-only reaches the safe shaft floor through the
    // standing-height-blocked entrance. The mixed climb may spend one visible upward Dash before
    // settling into its alternating wall rhythm. A deliberately small controller vocabulary then
    // makes the single exposed Dash commitment to the coin shelf. The concatenated replay, not any
    // leg's scalar search effort, is the retained evidence.
    let mut candidates = Vec::new();
    for approach_ticks in 1..=20 {
        for coast_ticks in 0..=10 {
            for settle_ticks in 1..=20 {
                let mut simulation = initial.clone();
                let mut entry = vec![Action::default()];
                entry.extend(std::iter::repeat_n(
                    Action {
                        move_x: -1,
                        move_y: 0,
                        jump: false,
                        dash: false,
                        restart: false,
                    },
                    approach_ticks,
                ));
                entry.push(Action {
                    move_x: -1,
                    move_y: 0,
                    jump: false,
                    dash: true,
                    restart: false,
                });
                entry.extend(std::iter::repeat_n(
                    Action {
                        move_x: -1,
                        move_y: 0,
                        jump: false,
                        dash: false,
                        restart: false,
                    },
                    coast_ticks,
                ));
                entry.extend(std::iter::repeat_n(Action::default(), settle_ticks));
                if entry
                    .iter()
                    .copied()
                    .any(|action| !clean_step(&mut simulation, action))
                {
                    continue;
                }
                let player = simulation.player();
                let bounds = player.bounds();
                if player.grounded()
                    && player.dash_ticks_remaining() == 0
                    && (50..=92).contains(&bounds.x)
                    && bounds.y == 148
                {
                    candidates.push((entry, simulation));
                }
            }
        }
    }
    let Some((mut actions, mut intermediate)) = candidates
        .into_iter()
        .min_by(|(left, _), (right, _)| static_route_key(left).cmp(&static_route_key(right)))
    else {
        eprintln!("  segmented Shadow Duct could not enter the low aperture cleanly");
        return None;
    };

    let upper_shelf = GroundedSupportTarget::new(
        90,
        150,
        30,
        [GroundedStandingRegion::new(90, 142).expect("valid Shadow upper shelf")],
    )
    .expect("valid Shadow upper-shelf waypoint");
    let outcome = solve_grounded_support(
        &intermediate,
        &upper_shelf,
        &SolverConfig::for_abilities(AbilitySet::new(true, true)),
    )
    .ok()?;
    let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Shadow Duct climb missed the upper shelf: {outcome:?}");
        return None;
    };
    for action in solution.replay.actions() {
        intermediate.step(action);
        actions.push(action);
        let player = intermediate.player();
        let bounds = player.bounds();
        if player.grounded() && bounds.y == 18 && (90..=142).contains(&bounds.x) {
            break;
        }
    }
    if !intermediate.player().grounded()
        || intermediate.player().bounds().y != 18
        || !(90..=142).contains(&intermediate.player().bounds().x)
    {
        eprintln!("  segmented Shadow Duct climb replay lost its first upper-shelf arrival");
        return None;
    }
    let mut crossings = Vec::new();
    for takeoff_x in 96..=142 {
        let Some((takeoff, approach)) = approach_x(&intermediate, takeoff_x) else {
            continue;
        };
        for run_off_ticks in 0..=8 {
            for jump_hold in 1..=10 {
                for dash_delay in 0..=8 {
                    let mut simulation = takeoff.clone();
                    let mut crossing = approach.clone();
                    let suffix = std::iter::repeat_n(right_action(false, false), run_off_ticks)
                        .chain(std::iter::repeat_n(right_action(true, false), jump_hold))
                        .chain(std::iter::repeat_n(right_action(false, false), dash_delay))
                        .chain(std::iter::once(right_action(false, true)))
                        .chain(std::iter::repeat_n(right_action(false, false), 80));
                    for action in suffix {
                        if !clean_step(&mut simulation, action) {
                            break;
                        }
                        crossing.push(action);
                        if simulation
                            .collected_pickups()
                            .any(|pickup| pickup.id() == "dungeon-coin-56")
                        {
                            crossings.push(crossing);
                            break;
                        }
                    }
                }
            }
        }
    }
    let Some(crossing) = crossings
        .into_iter()
        .min_by(|left, right| static_route_key(left).cmp(&static_route_key(right)))
    else {
        eprintln!("  segmented Shadow Duct found no single-Dash reward crossing");
        return None;
    };
    actions.extend(crossing);
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Pickup("dungeon-coin-56".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn segmented_observatory_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Keep the three visible acts independent while choosing a demonstration. Ordinary movement
    // approaches the tower; a mixed wall rhythm climbs it; Dash-only crosses each roof gap from a full
    // recovery. This prevents a global beam from substituting a diagonal Dash staircase for the
    // authored out-and-back silhouette.
    let Some((mut intermediate, mut actions)) = approach_x(initial, 220) else {
        eprintln!("  segmented Observatory could not run into the tower floor");
        return None;
    };
    while intermediate.player().bounds().x < 240 && actions.len() < 140 {
        let action = right_action(false, false);
        if !clean_step(&mut intermediate, action) {
            return None;
        }
        actions.push(action);
    }
    if intermediate.player().bounds().x < 240 || !intermediate.player().grounded() {
        eprintln!("  segmented Observatory could not settle inside the tower floor");
        return None;
    }
    let tower_roof = GroundedSupportTarget::new(
        190,
        230,
        30,
        [GroundedStandingRegion::new(190, 222).expect("valid Observatory tower roof")],
    )
    .expect("valid Observatory tower-roof waypoint");
    let outcome = solve_grounded_support(
        &intermediate,
        &tower_roof,
        &SolverConfig::for_abilities(AbilitySet::new(true, true)),
    )
    .ok()?;
    let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Observatory tower climb failed: {outcome:?}");
        return None;
    };
    for action in solution.replay.actions() {
        intermediate.step(action);
        actions.push(action);
        let player = intermediate.player();
        let bounds = player.bounds();
        if player.grounded() && bounds.y == 18 && (190..=230).contains(&bounds.x) {
            break;
        }
    }
    if !intermediate.player().grounded()
        || intermediate.player().bounds().y != 18
        || !(190..=230).contains(&intermediate.player().bounds().x)
    {
        eprintln!("  segmented Observatory climb lost its first tower-roof landing");
        return None;
    }

    let (next, crossing) =
        readable_directional_landing(&intermediate, 100..=122, 48, &[-1, 0, 1], -1)?;
    intermediate = next;
    actions.extend(crossing);
    let (next, crossing) = readable_directional_landing(&intermediate, 10..=42, 18, &[-1, 0], -1)?;
    intermediate = next;
    actions.extend(crossing);
    for _ in 0..40 {
        if intermediate
            .collected_pickups()
            .any(|pickup| pickup.id() == "dungeon-coin-57")
        {
            break;
        }
        let action = Action {
            move_x: -1,
            move_y: 0,
            jump: false,
            dash: false,
            restart: false,
        };
        if !clean_step(&mut intermediate, action) {
            return None;
        }
        actions.push(action);
    }
    if !intermediate
        .collected_pickups()
        .any(|pickup| pickup.id() == "dungeon-coin-57")
    {
        eprintln!("  segmented Observatory final shelf did not reach its coin");
        return None;
    }
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Pickup("dungeon-coin-57".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn readable_directional_landing(
    initial: &Simulation,
    target_x: std::ops::RangeInclusive<i32>,
    target_y: i32,
    dash_verticals: &[i8],
    move_x: i8,
) -> Option<(Simulation, Vec<Action>)> {
    let mut candidates = Vec::new();
    for run_off_ticks in 0..=20 {
        for jump_hold in 1..=10 {
            for dash_delay in 0..=8 {
                for &move_y in dash_verticals {
                    let mut simulation = initial.clone();
                    let mut actions = Vec::new();
                    let suffix = std::iter::repeat_n(
                        Action {
                            move_x,
                            move_y: 0,
                            jump: false,
                            dash: false,
                            restart: false,
                        },
                        run_off_ticks,
                    )
                    .chain(std::iter::repeat_n(
                        Action {
                            move_x,
                            move_y: 0,
                            jump: true,
                            dash: false,
                            restart: false,
                        },
                        jump_hold,
                    ))
                    .chain(std::iter::repeat_n(
                        Action {
                            move_x,
                            move_y: 0,
                            jump: false,
                            dash: false,
                            restart: false,
                        },
                        dash_delay,
                    ))
                    .chain(std::iter::once(Action {
                        move_x,
                        move_y,
                        jump: false,
                        dash: true,
                        restart: false,
                    }))
                    .chain(std::iter::repeat_n(
                        Action {
                            move_x,
                            move_y: 0,
                            jump: false,
                            dash: false,
                            restart: false,
                        },
                        80,
                    ));
                    for action in suffix {
                        if !clean_step(&mut simulation, action) {
                            break;
                        }
                        actions.push(action);
                        let player = simulation.player();
                        let bounds = player.bounds();
                        if player.grounded() && bounds.y == target_y && target_x.contains(&bounds.x)
                        {
                            candidates.push((simulation.clone(), actions));
                            break;
                        }
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|(_, left), (_, right)| static_route_key(left).cmp(&static_route_key(right)))
}

fn segmented_aurora_spire_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let Some((mut intermediate, mut actions)) = approach_x(initial, 145) else {
        eprintln!("  segmented Aurora Spire could not enter its climb");
        return None;
    };
    let top = GroundedSupportTarget::new(
        180,
        220,
        30,
        [GroundedStandingRegion::new(180, 212).expect("valid Aurora crown shelf")],
    )
    .expect("valid Aurora crown waypoint");
    let climb_config = SolverConfig::for_abilities(AbilitySet::new(true, false));
    let outcome = solve_grounded_support(&intermediate, &top, &climb_config).ok()?;
    let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Aurora Spire climb failed: {outcome:?}");
        return None;
    };
    let climb = solution.replay.actions().collect::<Vec<_>>();
    for &action in &climb {
        if !clean_step(&mut intermediate, action) {
            return None;
        }
    }
    actions.extend(climb);

    let Some((next, crossing)) =
        readable_directional_landing(&intermediate, 294..=294, 58, &[-1, 0, 1], 1)
    else {
        eprintln!("  segmented Aurora Spire could not cross its light sheet");
        return None;
    };
    intermediate = next;
    actions.extend(crossing);
    if !intermediate
        .collected_pickups()
        .any(|pickup| pickup.id() == "dungeon-coin-61")
    {
        eprintln!("  segmented Aurora Spire upper landing did not collect its coin");
        return None;
    }
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Pickup("dungeon-coin-61".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn segmented_gravity_lift_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Solve each visible lift bay independently. The alternating standing regions force the
    // right-left-right switchback while still allowing the ordinary solver to choose the exact
    // jump, Wall-Jump, and Dash timing inside each bay.
    let waypoints = [
        GroundedSupportTarget::new(
            10,
            280,
            130,
            [GroundedStandingRegion::new(240, 272).expect("valid lower lift landing")],
        )
        .expect("valid lower lift waypoint"),
        GroundedSupportTarget::new(
            40,
            310,
            90,
            [GroundedStandingRegion::new(40, 72).expect("valid middle lift landing")],
        )
        .expect("valid middle lift waypoint"),
        GroundedSupportTarget::new(
            10,
            280,
            50,
            [GroundedStandingRegion::new(240, 272).expect("valid upper lift landing")],
        )
        .expect("valid upper lift waypoint"),
    ];
    let config = SolverConfig::for_abilities(AbilitySet::new(true, true));
    let Some((mut intermediate, mut actions)) = approach_x(initial, 220) else {
        eprintln!("  segmented Gravity Lift could not reach its lower launch bay");
        return None;
    };
    while intermediate.player().bounds().x < 288 && actions.len() < 180 {
        let action = right_action(false, false);
        if !clean_step(&mut intermediate, action) || intermediate.reached_exit().is_some() {
            return None;
        }
        actions.push(action);
    }
    if intermediate.player().bounds().x < 288 || !intermediate.player().grounded() {
        return None;
    }
    for (index, waypoint) in waypoints.iter().enumerate() {
        let outcome = solve_grounded_support(&intermediate, waypoint, &config).ok()?;
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            eprintln!(
                "  segmented Gravity Lift leg {} failed: {outcome:?}",
                index + 1
            );
            return None;
        };
        let leg = solution.replay.actions().collect::<Vec<_>>();
        for &action in &leg {
            if !clean_step(&mut intermediate, action) {
                return None;
            }
        }
        actions.extend(leg);
        let (target_x, move_x, expected_y) = match index {
            0 => (20, -1, 118),
            1 => (288, 1, 78),
            2 => continue,
            _ => unreachable!("Gravity Lift has exactly three waypoints"),
        };
        for _ in 0..200 {
            let x = intermediate.player().bounds().x;
            if (move_x < 0 && x <= target_x) || (move_x > 0 && x >= target_x) {
                break;
            }
            let action = Action {
                move_x,
                move_y: 0,
                jump: false,
                dash: false,
                restart: false,
            };
            if !clean_step(&mut intermediate, action) {
                return None;
            }
            actions.push(action);
        }
        let x = intermediate.player().bounds().x;
        if !intermediate.player().grounded()
            || intermediate.player().bounds().y != expected_y
            || (move_x < 0 && x > target_x)
            || (move_x > 0 && x < target_x)
        {
            eprintln!(
                "  segmented Gravity Lift leg {} lost its recovery traverse",
                index + 1
            );
            return None;
        }
    }
    let outcome = solve_target(&intermediate, target.clone(), &config).ok()?;
    let TargetSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Gravity Lift final leg failed: {outcome:?}");
        return None;
    };
    actions.extend(solution.replay.actions());
    Some(TargetSolution {
        target: solution.target,
        reached: solution.reached,
        replay: Replay::record(initial, actions),
        stats: solution.stats,
    })
}

fn segmented_skybridge_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Treat the two bridge readings as separate legs: first land at the mast's foot, then climb
    // onto its roof. From that stable state the ordinary target solver only has to discover the
    // intended drop-and-Dash through the hanging mast's low aperture.
    let waypoints = [
        GroundedSupportTarget::new(
            40,
            110,
            130,
            [GroundedStandingRegion::new(80, 102).expect("valid Skybridge lower island")],
        )
        .expect("valid Skybridge lower waypoint"),
        GroundedSupportTarget::new(
            100,
            180,
            50,
            [GroundedStandingRegion::new(110, 172).expect("valid Skybridge mast roof")],
        )
        .expect("valid Skybridge upper waypoint"),
    ];
    let config = SolverConfig::for_abilities(AbilitySet::new(true, false));
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    for (index, waypoint) in waypoints.iter().enumerate() {
        let outcome = solve_grounded_support(&intermediate, waypoint, &config).ok()?;
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            eprintln!(
                "  segmented Skybridge leg {} failed: {outcome:?}",
                index + 1
            );
            return None;
        };
        let leg = solution.replay.actions().collect::<Vec<_>>();
        for &action in &leg {
            if !clean_step(&mut intermediate, action) {
                return None;
            }
        }
        actions.extend(leg);
    }
    let Some((mut intermediate, crossing)) = readable_drop_dash_landing(&intermediate) else {
        eprintln!("  segmented Skybridge aperture leg failed");
        return None;
    };
    actions.extend(crossing);
    for _ in 0..100 {
        if intermediate
            .collected_pickups()
            .any(|pickup| pickup.id() == "dungeon-coin-62")
        {
            break;
        }
        let action = right_action(false, false);
        if !clean_step(&mut intermediate, action) || intermediate.reached_exit().is_some() {
            return None;
        }
        actions.push(action);
    }
    if !intermediate
        .collected_pickups()
        .any(|pickup| pickup.id() == "dungeon-coin-62")
    {
        return None;
    }
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Pickup("dungeon-coin-62".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn readable_drop_dash_landing(initial: &Simulation) -> Option<(Simulation, Vec<Action>)> {
    let mut candidates = Vec::new();
    for takeoff_x in 140..=172 {
        let Some((takeoff, approach)) = approach_x(initial, takeoff_x) else {
            continue;
        };
        for drop_ticks in 1..=120 {
            for (dash_x, dash_y) in [(1, 0), (1, 1), (0, 1)] {
                let mut simulation = takeoff.clone();
                let mut actions = approach.clone();
                let suffix = std::iter::repeat_n(right_action(false, false), drop_ticks)
                    .chain(std::iter::once(Action {
                        move_x: dash_x,
                        move_y: dash_y,
                        jump: false,
                        dash: true,
                        restart: false,
                    }))
                    .chain(std::iter::repeat_n(right_action(false, false), 100));
                for action in suffix {
                    if !clean_step(&mut simulation, action) {
                        break;
                    }
                    actions.push(action);
                    let bounds = simulation.player().bounds();
                    if simulation.player().grounded()
                        && bounds.y == 118
                        && (210..=272).contains(&bounds.x)
                    {
                        candidates.push((simulation.clone(), actions));
                        break;
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|(_, left), (_, right)| static_route_key(left).cmp(&static_route_key(right)))
}

fn segmented_crown_sanctum_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let west_roof = GroundedSupportTarget::new(
        110,
        170,
        50,
        [GroundedStandingRegion::new(120, 162).expect("valid Crown west roof")],
    )
    .expect("valid Crown west waypoint");
    let wall_only = SolverConfig::for_abilities(AbilitySet::new(true, false));
    let outcome = solve_grounded_support(initial, &west_roof, &wall_only).ok()?;
    let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Crown west climb failed: {outcome:?}");
        return None;
    };
    let mut intermediate = initial.clone();
    let mut actions = solution.replay.actions().collect::<Vec<_>>();
    for &action in &actions {
        if !clean_step(&mut intermediate, action) {
            return None;
        }
    }

    let Some((next, descent)) = readable_crown_descent(&intermediate) else {
        eprintln!("  segmented Crown centre descent failed");
        return None;
    };
    intermediate = next;
    actions.extend(descent);
    for _ in 0..8 {
        let action = Action::default();
        if !clean_step(&mut intermediate, action) || !intermediate.player().grounded() {
            return None;
        }
        actions.push(action);
    }

    let outcome = solve_target(&intermediate, target.clone(), &wall_only).ok()?;
    let TargetSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Crown east climb failed: {outcome:?}");
        return None;
    };
    actions.extend(solution.replay.actions());
    Some(TargetSolution {
        target: solution.target,
        reached: solution.reached,
        replay: Replay::record(initial, actions),
        stats: solution.stats,
    })
}

fn segmented_gatehouse_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let (intermediate, climb) = readable_gatehouse_climb(initial)?;

    let mut candidates = Vec::new();
    for takeoff_x in 120..=132 {
        let Some((takeoff, approach)) = approach_x(&intermediate, takeoff_x) else {
            continue;
        };
        let mut simulation = takeoff;
        let mut suffix = approach;
        for action in std::iter::once(right_action(false, true))
            .chain(std::iter::repeat_n(right_action(false, false), 160))
        {
            if !clean_step(&mut simulation, action) {
                break;
            }
            suffix.push(action);
            if simulation.reached_exit() == Some("east") {
                let mut full_actions = climb.clone();
                full_actions.extend(suffix);
                candidates.push(full_actions);
                break;
            }
        }
    }
    let actions = candidates
        .into_iter()
        .min_by(|left, right| static_route_key(left).cmp(&static_route_key(right)))?;
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Door("east".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn readable_gatehouse_climb(initial: &Simulation) -> Option<(Simulation, Vec<Action>)> {
    let mut candidates = Vec::new();
    for takeoff_x in 84..=104 {
        let Some((takeoff, approach)) = approach_x(initial, takeoff_x) else {
            continue;
        };
        for ground_hold in 1_u8..=10 {
            for wall_hold in 1_u8..=10 {
                let mut simulation = takeoff.clone();
                let mut actions = approach.clone();
                let mut direction = 1;
                let mut jump_ticks = ground_hold;
                let mut last_wall = None;
                for _ in 0..240 {
                    let bounds = simulation.player().bounds();
                    if simulation.player().grounded()
                        && bounds.y == 48
                        && (120..=132).contains(&bounds.x)
                    {
                        candidates.push((simulation.clone(), actions));
                        break;
                    }
                    if jump_ticks == 0
                        && let Some(side) = simulation.player().wall_contact()
                        && Some(side) != last_wall
                    {
                        last_wall = Some(side);
                        direction = match side {
                            WallSide::Left => 1,
                            WallSide::Right => -1,
                        };
                        jump_ticks = wall_hold;
                    }
                    let action = Action {
                        move_x: direction,
                        move_y: 0,
                        jump: jump_ticks > 0,
                        dash: false,
                        restart: false,
                    };
                    if !clean_step(&mut simulation, action) {
                        break;
                    }
                    actions.push(action);
                    jump_ticks = jump_ticks.saturating_sub(1);
                    if simulation.player().grounded()
                        && simulation.player().bounds().y > 48
                        && actions.len() > approach.len() + 2
                    {
                        break;
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|(_, left), (_, right)| static_route_key(left).cmp(&static_route_key(right)))
}

fn readable_crown_descent(initial: &Simulation) -> Option<(Simulation, Vec<Action>)> {
    let mut candidates = Vec::new();
    for takeoff_x in 140..=162 {
        let Some((takeoff, approach)) = approach_x(initial, takeoff_x) else {
            continue;
        };
        for drop_ticks in 1..=120 {
            for (dash_x, dash_y) in [(1, 0), (1, 1), (0, 1)] {
                let mut simulation = takeoff.clone();
                let mut actions = approach.clone();
                let suffix = std::iter::repeat_n(right_action(false, false), drop_ticks)
                    .chain(std::iter::once(Action {
                        move_x: dash_x,
                        move_y: dash_y,
                        jump: false,
                        dash: true,
                        restart: false,
                    }))
                    .chain(std::iter::repeat_n(right_action(false, false), 100));
                for action in suffix {
                    if !clean_step(&mut simulation, action) {
                        break;
                    }
                    actions.push(action);
                    let bounds = simulation.player().bounds();
                    if simulation.player().grounded()
                        && bounds.y == 118
                        && (190..=222).contains(&bounds.x)
                    {
                        candidates.push((simulation.clone(), actions));
                        break;
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|(_, left), (_, right)| static_route_key(left).cmp(&static_route_key(right)))
}

fn readable_nova_niche_suffix(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let mut complete = Vec::new();
    for takeoff_x in 182..=212 {
        let Some((takeoff, approach)) = approach_x(initial, takeoff_x) else {
            continue;
        };
        for jump_hold in 1..=10 {
            for dash_delay in 0..=8 {
                let mut simulation = takeoff.clone();
                let mut actions = approach.clone();
                let suffix = std::iter::repeat_n(right_action(true, false), jump_hold)
                    .chain(std::iter::repeat_n(right_action(false, false), dash_delay))
                    .chain(std::iter::once(right_action(false, true)))
                    .chain(std::iter::repeat_n(right_action(false, false), 80));
                for action in suffix {
                    if !clean_step(&mut simulation, action) {
                        break;
                    }
                    actions.push(action);
                    if simulation
                        .collected_pickups()
                        .any(|pickup| pickup.id() == "dungeon-coin-58")
                    {
                        complete.push(actions);
                        break;
                    }
                }
            }
        }
    }
    let actions = complete
        .into_iter()
        .min_by(|left, right| static_route_key(left).cmp(&static_route_key(right)))?;
    Some(TargetSolution {
        target: target.clone(),
        reached: ReachedTarget::Pickup("dungeon-coin-58".to_owned()),
        replay: Replay::record(initial, actions),
        stats: SearchStats::default(),
    })
}

fn segmented_star_threshold_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Keep the first half deliberately legible: accelerate from the west door, commit exactly
    // one horizontal Dash through the low aperture, and coast to a grounded stop inside the
    // shaft. The suffix search receives a Wall-Jump-only controller vocabulary, so its witness
    // cannot silently replace the alternating climb with vertical Dashes.
    let mut candidates = Vec::new();
    for approach_ticks in 4..=10 {
        for settle_ticks in 8..=28 {
            let mut intermediate = initial.clone();
            let mut actions = vec![right_action(false, false); approach_ticks];
            actions.push(right_action(false, true));
            actions.extend(std::iter::repeat_n(Action::default(), settle_ticks));
            if actions
                .iter()
                .copied()
                .any(|action| !clean_step(&mut intermediate, action))
            {
                continue;
            }
            let player = intermediate.player();
            let bounds = player.bounds();
            if !player.grounded()
                || player.dash_ticks_remaining() != 0
                || !(60..=102).contains(&bounds.x)
            {
                continue;
            }
            let outcome = solve_target(
                &intermediate,
                target.clone(),
                &SolverConfig::for_abilities(AbilitySet::new(true, false)),
            )
            .ok()?;
            let TargetSolveOutcome::Solved(suffix) = outcome else {
                continue;
            };
            actions.extend(suffix.replay.actions());
            candidates.push(TargetSolution {
                target: suffix.target,
                reached: suffix.reached,
                replay: Replay::record(initial, actions),
                stats: suffix.stats,
            });
        }
    }
    candidates.into_iter().min_by(|left, right| {
        static_route_key(&left.replay.actions().collect::<Vec<_>>()).cmp(&static_route_key(
            &right.replay.actions().collect::<Vec<_>>(),
        ))
    })
}

fn readable_meteor_run_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // The three shutters open at ticks 25, 57, and 89. Keep the demonstration deliberately
    // human-readable: accelerate toward each visible gate, commit one horizontal Dash as its
    // window arrives, and preserve rightward input while braking in the intervening safe bay.
    // This is an authored timing policy, not evidence that arbitrary solver thrashing is hard.
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    for _ in 0..180 {
        let tick = intermediate.room_tick();
        let x = intermediate.player().bounds().x;
        let staging = (tick < 24 && x >= 35)
            || (tick < 56 && (104..131).contains(&x) && x >= 119)
            || (tick < 88 && (186..213).contains(&x) && x >= 201);
        let action = Action {
            move_x: if staging { -1 } else { 1 },
            move_y: 0,
            jump: false,
            dash: matches!(tick, 24 | 56 | 88),
            restart: false,
        };
        let report = intermediate.step(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset))
        {
            eprintln!(
                "  readable Meteor Run route died at room tick {} near x {}",
                tick + 1,
                x
            );
            return None;
        }
        actions.push(action);
        if intermediate.reached_exit() == Some("east") {
            return Some(TargetSolution {
                target: target.clone(),
                reached: ReachedTarget::Door("east".to_owned()),
                replay: Replay::record(initial, actions),
                stats: SearchStats::default(),
            });
        }
    }
    eprintln!("  readable Meteor Run route did not reach its east door");
    None
}

fn segmented_comet_run_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Bind the route to the three visible recovery platforms. This is not a hidden authored
    // action script: every leg is independently solved with the ordinary Dash vocabulary, while
    // the waypoints prevent a global beam from spending hundreds of ticks retrying old platforms.
    let waypoints = [
        GroundedSupportTarget::new(
            110,
            130,
            110,
            [GroundedStandingRegion::new(110, 122).expect("valid first Comet platform")],
        )
        .expect("valid first Comet waypoint"),
        GroundedSupportTarget::new(
            180,
            200,
            70,
            [GroundedStandingRegion::new(180, 192).expect("valid second Comet platform")],
        )
        .expect("valid second Comet waypoint"),
        GroundedSupportTarget::new(
            240,
            260,
            120,
            [GroundedStandingRegion::new(240, 252).expect("valid third Comet platform")],
        )
        .expect("valid third Comet waypoint"),
    ];
    let config = SolverConfig::for_abilities(AbilitySet::new(false, true));
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    let approach = right_action(false, false);
    for _ in 0..80 {
        if intermediate.player().grounded() && intermediate.player().bounds().x >= 50 {
            break;
        }
        if !clean_step(&mut intermediate, approach) {
            return None;
        }
        actions.push(approach);
    }
    if !intermediate.player().grounded() || intermediate.player().bounds().x < 50 {
        eprintln!("  segmented Comet Run approach did not reach its launch edge");
        return None;
    }
    let launch_edges = [115, 185, 245];
    for (index, waypoint) in waypoints.iter().enumerate() {
        let outcome = solve_grounded_support(&intermediate, waypoint, &config).ok()?;
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            eprintln!(
                "  segmented Comet Run leg {} failed: {outcome:?}",
                index + 1
            );
            return None;
        };
        let leg = solution.replay.actions().collect::<Vec<_>>();
        for &action in &leg {
            intermediate.step(action);
        }
        actions.extend(leg);
        for _ in 0..24 {
            if intermediate.player().grounded()
                && intermediate.player().bounds().x >= launch_edges[index]
            {
                break;
            }
            if !clean_step(&mut intermediate, approach) {
                return None;
            }
            actions.push(approach);
        }
        if !intermediate.player().grounded()
            || intermediate.player().bounds().x < launch_edges[index]
        {
            eprintln!(
                "  segmented Comet Run checkpoint {} has no readable launch edge",
                index + 1
            );
            return None;
        }
    }
    let outcome = solve_target(&intermediate, target.clone(), &config).ok()?;
    let TargetSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Comet Run final leg failed: {outcome:?}");
        return None;
    };
    actions.extend(solution.replay.actions());
    Some(TargetSolution {
        target: solution.target,
        reached: solution.reached,
        replay: Replay::record(initial, actions),
        stats: solution.stats,
    })
}

fn segmented_moon_vault_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Enter the orbit with one explicit one-way drop, then solve its visible recovery platforms as
    // independent legs. Dash-only configurations retain normal jumps but prevent the local solver
    // from substituting boundary-wall retries for the under-wall crossing and rising transfers.
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    for _ in 0..40 {
        if intermediate.player().grounded() {
            break;
        }
        let action = Action::default();
        if !clean_step(&mut intermediate, action) {
            return None;
        }
        actions.push(action);
    }
    if !intermediate.player().grounded() || intermediate.player().bounds().y > 50 {
        eprintln!("  segmented Moon Vault did not settle on its entrance shelf");
        return None;
    }
    let drop = Action {
        move_x: 0,
        move_y: 1,
        jump: true,
        dash: false,
        restart: false,
    };
    if !clean_step(&mut intermediate, drop) {
        return None;
    }
    actions.push(drop);
    for _ in 0..80 {
        if intermediate.player().grounded() && intermediate.player().bounds().y >= 128 {
            break;
        }
        let action = Action::default();
        if !clean_step(&mut intermediate, action) {
            return None;
        }
        actions.push(action);
    }
    if !intermediate.player().grounded() || intermediate.player().bounds().y < 128 {
        eprintln!("  segmented Moon Vault drop missed its low recovery platform");
        return None;
    }

    let waypoints = [
        GroundedSupportTarget::new(
            220,
            250,
            140,
            [GroundedStandingRegion::new(220, 242).expect("valid Moon lower-right platform")],
        )
        .expect("valid Moon lower-right waypoint"),
        GroundedSupportTarget::new(
            220,
            250,
            100,
            [GroundedStandingRegion::new(220, 242).expect("valid Moon right platform")],
        )
        .expect("valid Moon right waypoint"),
    ];
    let config = SolverConfig::for_abilities(AbilitySet::new(false, true));
    for (index, waypoint) in waypoints.iter().enumerate() {
        let outcome = solve_grounded_support(&intermediate, waypoint, &config).ok()?;
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            eprintln!(
                "  segmented Moon Vault leg {} failed: {outcome:?}",
                index + 1
            );
            return None;
        };
        let leg = solution.replay.actions().collect::<Vec<_>>();
        for &action in &leg {
            intermediate.step(action);
        }
        actions.extend(leg);
    }
    let outcome = solve_target(&intermediate, target.clone(), &config).ok()?;
    let TargetSolveOutcome::Solved(solution) = outcome else {
        eprintln!("  segmented Moon Vault final leg failed: {outcome:?}");
        return None;
    };
    actions.extend(solution.replay.actions());
    Some(TargetSolution {
        target: solution.target,
        reached: solution.reached,
        replay: Replay::record(initial, actions),
        stats: solution.stats,
    })
}

fn segmented_lunar_cache_candidate(
    initial: &Simulation,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    // Keep the three visible ideas separate while authoring the route: fall to the chamber floor
    // without spending Dash, traverse the low passage with Dash, then climb the isolated shaft
    // without substituting upward Dashes for its wall rhythm.
    let mut intermediate = initial.clone();
    let mut actions = Vec::new();
    for _ in 0..120 {
        if intermediate.player().grounded() && intermediate.player().bounds().y >= 158 {
            break;
        }
        let drop_action = Action {
            move_x: 0,
            move_y: 1,
            // One-way drop-through is intentionally an explicit Down+Jump edge. Press only once
            // after each landing; the airborne ticks release Jump before the next platform.
            jump: intermediate.player().grounded(),
            dash: false,
            restart: false,
        };
        intermediate.step(drop_action);
        actions.push(drop_action);
    }
    if !intermediate.player().grounded() || intermediate.player().bounds().y < 158 {
        eprintln!("  segmented Lunar Cache fall did not reach the chamber floor");
        return None;
    }

    let shaft_floor = GroundedSupportTarget::new(
        230,
        280,
        170,
        [GroundedStandingRegion::new(230, 272).expect("valid shaft standing range")],
    )
    .expect("valid shaft floor waypoint");
    let second_outcome = solve_grounded_support(
        &intermediate,
        &shaft_floor,
        &SolverConfig::for_abilities(AbilitySet::new(false, true)),
    )
    .ok()?;
    let GroundedSupportSolveOutcome::Solved(second) = second_outcome else {
        eprintln!("  segmented Lunar Cache Dash did not cross the low tunnel: {second_outcome:?}");
        return None;
    };
    let second_actions = second.replay.actions().collect::<Vec<_>>();
    for &action in &second_actions {
        intermediate.step(action);
    }
    actions.extend(second_actions);

    let third_outcome = solve_target(
        &intermediate,
        target.clone(),
        &SolverConfig::for_abilities(AbilitySet::new(true, false)),
    )
    .ok()?;
    let TargetSolveOutcome::Solved(third) = third_outcome else {
        eprintln!("  segmented Lunar Cache climb did not reach its coin: {third_outcome:?}");
        return None;
    };
    actions.extend(third.replay.actions());
    Some(TargetSolution {
        target: third.target,
        reached: third.reached,
        replay: Replay::record(initial, actions),
        stats: third.stats,
    })
}

fn render_span(action: Action, ticks: usize) -> String {
    format!(
        "span {} {} {} {} {} {ticks}\n",
        action.move_x,
        action.move_y,
        u8::from(action.jump),
        u8::from(action.dash),
        u8::from(action.restart),
    )
}

fn parse_previous_witness_actions(input: &str) -> BTreeMap<String, Vec<Action>> {
    let mut routes = BTreeMap::new();
    let mut current_id = None::<String>;
    let mut actions = Vec::new();
    for (line_index, line) in input.lines().enumerate() {
        let mut fields = line.split_ascii_whitespace();
        match fields.next() {
            Some("route") => {
                assert!(
                    current_id.is_none(),
                    "nested previous route at line {}",
                    line_index + 1
                );
                current_id = Some(
                    fields
                        .next()
                        .unwrap_or_else(|| {
                            panic!("missing previous route id at line {}", line_index + 1)
                        })
                        .to_owned(),
                );
                actions.clear();
            }
            Some("span") => {
                let parse = |field: Option<&str>, name: &str| {
                    field
                        .unwrap_or_else(|| panic!("missing {name} at line {}", line_index + 1))
                        .parse::<i64>()
                        .unwrap_or_else(|error| {
                            panic!("invalid {name} at line {}: {error}", line_index + 1)
                        })
                };
                let action = Action {
                    move_x: i8::try_from(parse(fields.next(), "move_x"))
                        .expect("stored move_x fits i8"),
                    move_y: i8::try_from(parse(fields.next(), "move_y"))
                        .expect("stored move_y fits i8"),
                    jump: parse(fields.next(), "jump") != 0,
                    dash: parse(fields.next(), "dash") != 0,
                    restart: parse(fields.next(), "restart") != 0,
                };
                let ticks = usize::try_from(parse(fields.next(), "span ticks"))
                    .expect("stored span ticks fit usize");
                actions.extend(std::iter::repeat_n(action, ticks));
            }
            Some("end") => {
                let id = current_id.take().unwrap_or_else(|| {
                    panic!("previous end outside route at line {}", line_index + 1)
                });
                assert!(
                    routes.insert(id, actions.clone()).is_none(),
                    "duplicate previous route"
                );
            }
            _ => {}
        }
    }
    assert!(
        current_id.is_none(),
        "previous artifact ends inside a route"
    );
    routes
}

fn reached_target(spec: DemoDungeonRouteSpec) -> ReachedTarget {
    match spec.target {
        DemoDungeonRouteTarget::Door(id) => ReachedTarget::Door(id.to_owned()),
        DemoDungeonRouteTarget::GoalExit => ReachedTarget::Exit(spec.target.id().to_owned()),
        DemoDungeonRouteTarget::Pickup(id) => ReachedTarget::Pickup(id.to_owned()),
    }
}

fn main() {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let selected_route = if arguments.first().is_some_and(|arg| arg == "--route") {
        let route = arguments
            .get(1)
            .and_then(|arg| arg.to_str())
            .unwrap_or_else(|| panic!("usage: retune_demo_dungeon --route <room-id>"));
        assert_eq!(
            arguments.len(),
            2,
            "usage: retune_demo_dungeon --route <room-id>"
        );
        Some(route)
    } else {
        None
    };
    let argument = arguments
        .first()
        .filter(|_| selected_route.is_none())
        .cloned();
    let check_only = argument.as_deref() == Some(std::ffi::OsStr::new("--check"));
    let output = if check_only {
        PathBuf::from(DEFAULT_OUTPUT)
    } else {
        argument
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT))
    };
    let tuning = MovementTuning::GAMEPLAY_DEFAULT;
    let mut rendered = format!(
        "schema downwards-demo-dungeon-witnesses-v1\npalette-generation {DUNGEON_PALETTE_GENERATION_VERSION}\nplayer-movement-policy {PLAYER_MOVEMENT_POLICY_VERSION}\ntuning {} {} {} {} {} {}\nshaky-policy {} {} {} {}\n",
        tuning.top_speed_pixels_per_second,
        tuning.acceleration_milliseconds,
        tuning.braking_milliseconds,
        tuning.wall_ascent_carry_percent,
        tuning.wall_carry_percent,
        tuning.wall_momentum_milliseconds,
        SHAKY_CONFIG.trials_per_curve_point,
        SHAKY_CONFIG.grace_ticks,
        SHAKY_CONFIG.correlated_boundaries,
        SHAKY_CONFIG.convergence_confirmation_ticks,
    );
    let previous_actions = parse_previous_witness_actions(PREVIOUS_WITNESS_ARTIFACT);

    let mut generated_routes = 0;
    for (index, spec) in demo_dungeon_route_specs()
        .into_iter()
        .enumerate()
        .filter(|(_, spec)| selected_route.is_none_or(|route| spec.id() == route))
    {
        generated_routes += 1;
        let initial = initial_simulation(spec);
        assert_eq!(initial.movement_tuning(), Some(tuning));
        let target = search_target(spec);
        let solver_config = SolverConfig::for_abilities(spec.inventory.abilities());
        let outcome = solve_target(&initial, target.clone(), &solver_config)
            .unwrap_or_else(|error| panic!("{} solve failed: {error}", spec.id()));
        let (mut candidates, global_miss) = match outcome {
            TargetSolveOutcome::Solved(solution) => (vec![(solution, true)], None),
            outcome => (Vec::new(), Some(format!("{outcome:?}"))),
        };
        if selected_route.is_some()
            && let Some(reason) = &global_miss
        {
            eprintln!("  global solve missed: {reason}");
        }
        let direct =
            audit_direct_controller_probes(&initial, std::slice::from_ref(&target), &solver_config)
                .unwrap_or_else(|error| panic!("{} direct audit failed: {error}", spec.id()));
        candidates
            .extend(segmented_candidate(&initial, spec, &target).map(|solution| (solution, true)));
        candidates.extend(direct.witnesses.into_iter().map(|witness| {
            (
                TargetSolution {
                    target: witness.target,
                    reached: witness.reached,
                    replay: witness.replay,
                    stats: witness.stats_at_first_discovery,
                },
                true,
            )
        }));
        if let Some(actions) = previous_actions.get(spec.id())
            && clean_completion(&initial, spec.target, actions)
        {
            candidates.push((
                TargetSolution {
                    target: target.clone(),
                    reached: reached_target(spec),
                    replay: Replay::record(&initial, actions.iter().copied()),
                    stats: SearchStats::default(),
                },
                false,
            ));
        }
        assert!(
            !candidates.is_empty(),
            "{} has no exact generated, segmented, direct, or retained witness; global outcome: {}",
            spec.id(),
            global_miss.as_deref().unwrap_or("no miss")
        );
        let mut simplified = candidates
            .into_iter()
            .map(|(mut solution, should_simplify)| {
                let found_actions = solution.replay.actions().collect::<Vec<_>>();
                let actions = if should_simplify {
                    greedily_simplify(&initial, spec.target, &found_actions)
                } else {
                    found_actions
                };
                solution.replay = Replay::record(&initial, actions.iter().copied());
                (solution, actions)
            })
            .collect::<Vec<_>>();
        simplified.sort_by(|(_, left), (_, right)| compare_routes(&initial, spec, left, right));
        simplified.dedup_by(|(_, left), (_, right)| left == right);
        let mut assessed = Vec::with_capacity(simplified.len());
        for (candidate_index, (solution, actions)) in simplified.into_iter().enumerate() {
            let observation = observe(&initial, spec, &actions);
            let shaky = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: shaky_seed(spec.id()),
                    ..SHAKY_CONFIG
                },
            )
            .unwrap_or_else(|error| panic!("{} shaky-hand audit failed: {error}", spec.id()));
            let fragile = shaky.curves.iter().any(|curve| {
                curve.family != NoiseFamily::Exact
                    && curve.strength_ticks == 1
                    && curve.trials > 0
                    && curve.successes == 0
            });
            if selected_route.is_some() {
                eprintln!(
                    "  candidate {:>2}: {:>3}t {:>2}sp {:>2}j {:>2}wj {:>2}d ({:>2} pre-WJ) {:>2}rev{}",
                    candidate_index + 1,
                    observation.ticks,
                    observation.action_spans,
                    observation.accepted_jumps,
                    observation.accepted_wall_jumps,
                    observation.accepted_dashes,
                    observation.accepted_dashes_before_first_wall_jump,
                    observation.horizontal_reversals,
                    if fragile { " FRAGILE" } else { "" },
                );
                if fragile {
                    let zero_families = shaky
                        .curves
                        .iter()
                        .filter(|curve| {
                            curve.family != NoiseFamily::Exact
                                && curve.strength_ticks == 1
                                && curve.trials > 0
                                && curve.successes == 0
                        })
                        .map(|curve| {
                            (
                                curve.family,
                                curve.trials_with_death,
                                curve.timeouts,
                                curve.wrong_target_outcomes,
                            )
                        })
                        .collect::<Vec<_>>();
                    eprintln!(
                        "    zero-success strength-one families (family, deaths, timeouts, wrong): {zero_families:?}"
                    );
                    for curve in shaky.curves.iter().filter(|curve| {
                        curve.family != NoiseFamily::Exact
                            && curve.strength_ticks == 1
                            && curve.trials > 0
                            && curve.successes == 0
                    }) {
                        eprintln!("    first failure: {:?}", curve.first_failure);
                    }
                    print_behavior_trace(&initial, spec, &actions);
                }
            }
            assessed.push((fragile, solution, actions, shaky));
            if !fragile && selected_route.is_none() {
                break;
            }
        }
        let selected_index = assessed
            .iter()
            .position(|(fragile, _, _, _)| !fragile)
            .unwrap_or(0);
        let (_, _solution, actions, shaky) = assessed.remove(selected_index);
        let observation = observe(&initial, spec, &actions);
        if selected_route.is_some() {
            print_behavior_trace(&initial, spec, &actions);
        }
        assert!(shaky.exact_control_succeeded);
        let strength_one = shaky
            .curves
            .iter()
            .filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            })
            .collect::<Vec<_>>();
        let fragile_families = strength_one
            .iter()
            .filter(|curve| curve.successes == 0)
            .map(|curve| curve.family)
            .collect::<Vec<_>>();

        eprintln!(
            "{:>2} {:<36} {:>3}t {:>2}sp {:>2}j {:>2}wj {:>2}d {:>2}rev{}",
            index + 1,
            spec.id(),
            observation.ticks,
            observation.action_spans,
            observation.accepted_jumps,
            observation.accepted_wall_jumps,
            observation.accepted_dashes,
            observation.horizontal_reversals,
            if fragile_families.is_empty() {
                String::new()
            } else {
                format!("  FRAGILE {fragile_families:?}")
            },
        );
        rendered.push_str(&format!(
            "route {}\nroom {}\nentry {}\ntarget {} {}\nabilities {} {}\nticks {}\nobservation {} {} {} {} {} {}\n",
            spec.id(),
            spec.room.id(),
            spec.entry_door.unwrap_or("none"),
            spec.target.kind(),
            spec.target.id(),
            u8::from(spec.inventory.abilities().wall_jump),
            u8::from(spec.inventory.abilities().dash),
            actions.len(),
            observation.action_spans,
            observation.jump_presses,
            observation.accepted_jumps,
            observation.accepted_wall_jumps,
            observation.accepted_dashes,
            observation.horizontal_reversals,
        ));
        for curve in strength_one {
            rendered.push_str(&format!(
                "shaky {:?} {} {} {}\n",
                curve.family, curve.successes, curve.trials, curve.death_events
            ));
        }
        let mut start = 0;
        while start < actions.len() {
            let action = actions[start];
            let end = actions[start..]
                .iter()
                .position(|candidate| *candidate != action)
                .map_or(actions.len(), |offset| start + offset);
            rendered.push_str(&render_span(action, end - start));
            start = end;
        }
        rendered.push_str("end\n");
    }

    if let Some(route) = selected_route {
        assert_eq!(generated_routes, 1, "unknown demo dungeon route {route:?}");
        print!("{rendered}");
    } else if check_only {
        let existing = fs::read_to_string(&output).expect("read dungeon witness artifact");
        assert_eq!(
            existing, rendered,
            "dungeon witness artifact is stale; run `cargo run -p downwards-content --example retune_demo_dungeon`"
        );
        eprintln!("{} is current", output.display());
    } else {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).expect("create generated witness directory");
        }
        fs::write(&output, rendered).expect("write dungeon witness artifact");
        eprintln!("wrote {}", output.display());
    }
}
