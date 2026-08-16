use downwards_ai::{
    DirectProbeAudit, DirectProbeAuditStatus, Replay, SearchTarget, SolverConfig,
    TargetSolveOutcome, audit_direct_controller_probes, solve_target,
};
use downwards_content::{
    HARD_NO_DASH_TARGET, MEDIUM_NO_DASH_ABILITIES, MEDIUM_NO_DASH_TARGET, hard_no_dash_scenario,
    hard_no_dash_witness_actions, medium_no_dash_scenario, medium_no_dash_witness_actions,
};
use downwards_core::{
    AbilitySet, Action, JumpKind, Rect, Room, Simulation, SimulationEvent, Tile, WallSide,
};

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

fn observe_route(initial: &Simulation, target: &str, actions: &[Action]) -> RouteObservation {
    let mut simulation = initial.clone();
    let mut observation = RouteObservation::default();
    let mut previous_action = Action::default();

    for (index, &action) in actions.iter().enumerate() {
        if action.jump && !previous_action.jump {
            observation.jump_presses += 1;
        }
        if action.dash {
            // Input itself is forbidden even when this loadout cannot accept a
            // Dash event.
            observation.dashes += 1;
        }
        if action.restart {
            observation.resets += 1;
        }

        for event in simulation.step(action).events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { .. }) => observation.wall_jumps += 1,
                SimulationEvent::Jumped(_) => observation.non_wall_jumps += 1,
                SimulationEvent::Dashed { .. } => observation.dashes += 1,
                SimulationEvent::Died(_) => observation.deaths += 1,
                SimulationEvent::Reset => observation.resets += 1,
                SimulationEvent::ExitReached { ref id } if id == target => {
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

    observation.deaths = observation
        .deaths
        .max(usize::try_from(simulation.deaths()).expect("the simulation death count fits usize"));
    if simulation.reached_exit() != Some(target) {
        observation.reached_at_tick = None;
    }
    observation
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GeometryObservation {
    wall_contact_pad_heights: Vec<usize>,
    landing_support_widths: Vec<usize>,
}

fn vertical_solid_run_at_contact(room: &Room, player: Rect, side: WallSide) -> Option<usize> {
    let tile_size = room.tile_size();
    let column = match side {
        WallSide::Left => (player.x - 1).div_euclid(tile_size),
        WallSide::Right => player.right().div_euclid(tile_size),
    };
    let first_row = player.y.div_euclid(tile_size).max(0);
    let last_row = (player.bottom() - 1)
        .div_euclid(tile_size)
        .min(i32::from(room.height()) - 1);
    let run_in_column = |column: i32| {
        if column < 0 || column >= i32::from(room.width()) {
            return None;
        }
        (first_row..=last_row)
            .filter(|&row| room.tile(column as u16, row as u16) == Some(Tile::Solid))
            .map(|row| {
                let mut first = row;
                while first > 0 && room.tile(column as u16, (first - 1) as u16) == Some(Tile::Solid)
                {
                    first -= 1;
                }
                let mut last = row;
                while last + 1 < i32::from(room.height())
                    && room.tile(column as u16, (last + 1) as u16) == Some(Tile::Solid)
                {
                    last += 1;
                }
                usize::try_from(last + 1 - first).expect("a positive tile run fits usize")
            })
            .max()
    };

    // Wall-jump grace can accept input one pixel after leaving the surface.
    // Only when the direct column is empty, inspect the immediately adjacent
    // column toward that remembered wall; looking any farther would measure a
    // backing column rather than the actual contact face.
    run_in_column(column).or_else(|| {
        run_in_column(match side {
            WallSide::Left => column - 1,
            WallSide::Right => column + 1,
        })
    })
}

fn tile_is_safe_support(tile: Option<Tile>) -> bool {
    matches!(tile, Some(Tile::Solid | Tile::OneWay))
}

fn horizontal_support_run_at_landing(room: &Room, player: Rect) -> Option<usize> {
    let tile_size = room.tile_size();
    let row = player.bottom().div_euclid(tile_size);
    if row < 0 || row >= i32::from(room.height()) {
        return None;
    }
    let first_column = player.x.div_euclid(tile_size).max(0);
    let last_column = (player.right() - 1)
        .div_euclid(tile_size)
        .min(i32::from(room.width()) - 1);

    (first_column..=last_column)
        .filter(|&column| tile_is_safe_support(room.tile(column as u16, row as u16)))
        .map(|column| {
            let mut first = column;
            while first > 0 && tile_is_safe_support(room.tile((first - 1) as u16, row as u16)) {
                first -= 1;
            }
            let mut last = column;
            while last + 1 < i32::from(room.width())
                && tile_is_safe_support(room.tile((last + 1) as u16, row as u16))
            {
                last += 1;
            }
            usize::try_from(last + 1 - first).expect("a positive tile run fits usize")
        })
        .max()
}

fn observe_route_geometry(initial: &Simulation, actions: &[Action]) -> GeometryObservation {
    let mut simulation = initial.clone();
    let mut observation = GeometryObservation::default();
    let mut last_contact_pad = None;

    for (tick, &action) in actions.iter().enumerate() {
        let before_step = simulation.player().bounds();
        let contact_pad = simulation.player().wall_contact().and_then(|side| {
            vertical_solid_run_at_contact(simulation.room(), before_step, side)
                .map(|height| (side, height))
        });
        if contact_pad.is_some() {
            last_contact_pad = contact_pad;
        }
        let report = simulation.step(action);
        for event in report.events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    let pad_height =
                        vertical_solid_run_at_contact(simulation.room(), before_step, side)
                            .or_else(|| {
                                contact_pad
                                    .filter(|(contact_side, _)| *contact_side == side)
                                    .map(|(_, height)| height)
                            })
                            .or_else(|| {
                                last_contact_pad
                                    .filter(|(contact_side, _)| *contact_side == side)
                                    .map(|(_, height)| height)
                            })
                    .unwrap_or_else(|| {
                        panic!(
                            "wall jump at tick {} on {side:?} has no measurable solid pad; before {before_step:?}, contact {contact_pad:?}, last {last_contact_pad:?}",
                            tick + 1
                        )
                    });
                    observation.wall_contact_pad_heights.push(pad_height);
                }
                SimulationEvent::Landed => {
                    if let Some(width) = horizontal_support_run_at_landing(
                        simulation.room(),
                        simulation.player().bounds(),
                    ) {
                        observation.landing_support_widths.push(width);
                    }
                }
                _ => {}
            }
        }
        if simulation.reached_exit().is_some() {
            break;
        }
    }

    observation
}

/// Greedily remove ticks and held intents, accepting each edit only after an
/// authoritative clean completion. This catches obviously padded movement;
/// it does not establish a globally minimal route.
fn greedily_simplify(initial: &Simulation, target: &str, original: &[Action]) -> Vec<Action> {
    let reached_at_tick = observe_route(initial, target, original)
        .reached_at_tick
        .expect("the stored witness must complete before simplification");
    let mut actions = original[..reached_at_tick].to_vec();

    loop {
        let before_pass = actions.clone();

        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if observe_route(initial, target, &candidate).is_clean_completion() {
                    actions = candidate;
                } else {
                    start += chunk;
                }
            }
            chunk /= 2;
        }

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
            if observe_route(initial, target, &candidate).is_clean_completion() {
                actions = candidate;
            } else {
                start = end;
            }
        }

        for index in (0..actions.len()).rev() {
            for clear in [
                |action: &mut Action| action.jump = false,
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index]
                    && observe_route(initial, target, &candidate).is_clean_completion()
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
fn medium_witness_is_an_exact_clean_no_dash_wall_jump_completion() {
    let initial = medium_no_dash_scenario();
    assert_eq!(initial.abilities(), MEDIUM_NO_DASH_ABILITIES);
    assert!(initial.abilities().wall_jump);
    assert!(!initial.abilities().dash);

    let actions = medium_no_dash_witness_actions();
    assert!(!actions.is_empty());
    assert!(actions.iter().all(|action| !action.dash && !action.restart));

    let replay = Replay::record(&initial, actions.iter().copied());
    let verified = replay
        .verify(&initial)
        .expect("the stored medium witness must replay through authoritative physics");
    assert_eq!(verified.frames_verified, actions.len());
    assert_eq!(
        verified.reached_exit.as_deref(),
        Some(MEDIUM_NO_DASH_TARGET)
    );

    let observation = observe_route(&initial, MEDIUM_NO_DASH_TARGET, &actions);
    assert!(observation.is_clean_completion(), "{observation:?}");
    assert_eq!(observation.reached_at_tick, Some(actions.len()));
    assert!(
        observation.wall_jumps > 0,
        "the intended medium route must actually exercise WallJump"
    );
    assert_eq!(
        observation.jump_presses,
        observation.non_wall_jumps + observation.wall_jumps,
        "the stored route has an unaccepted, potentially gratuitous jump press"
    );
}

#[test]
fn simplifying_the_medium_witness_does_not_remove_its_wall_jump_mechanic() {
    let initial = medium_no_dash_scenario();
    let stored = medium_no_dash_witness_actions();
    let simplified = greedily_simplify(&initial, MEDIUM_NO_DASH_TARGET, &stored);
    let observation = observe_route(&initial, MEDIUM_NO_DASH_TARGET, &simplified);

    assert!(observation.is_clean_completion(), "{observation:?}");
    assert!(observation.wall_jumps > 0);
}

#[test]
fn baseline_search_trace_and_finite_probes_find_no_known_medium_bypass() {
    let medium = medium_no_dash_scenario();
    let mut initial = Simulation::with_abilities(medium.room().clone(), AbilitySet::NONE);
    initial.enable_current_player_movement();
    let target = SearchTarget::exit(MEDIUM_NO_DASH_TARGET);
    let config = SolverConfig::for_abilities(AbilitySet::NONE);

    let solve = solve_target(&initial, target.clone(), &config).expect("the target is defined");
    let TargetSolveOutcome::Inconclusive { reason, stats } = solve else {
        panic!("the bounded baseline solver found a medium no-wall-jump bypass: {solve:?}");
    };

    let audit = audit_direct_controller_probes(&initial, &[target], &config)
        .expect("the finite baseline direct-controller audit runs");
    assert_eq!(
        audit.status,
        DirectProbeAuditStatus::Complete,
        "an incomplete finite audit cannot support a no-known-bypass observation"
    );
    assert!(
        audit.witnesses.is_empty(),
        "baseline direct-controller bypasses: {:?}",
        witness_probe_labels(&audit)
    );

    let actions = medium_no_dash_witness_actions();
    let baseline_trace = observe_route(&initial, MEDIUM_NO_DASH_TARGET, &actions);
    assert_eq!(baseline_trace.reached_at_tick, None);

    eprintln!(
        "medium baseline no-known-bypass audit: solver {reason:?}, {} nodes / {} ticks; direct {:?}, {} probes / {} ticks",
        stats.expanded_nodes,
        stats.simulated_ticks,
        audit.status,
        audit.stats.expanded_nodes,
        audit.stats.simulated_ticks,
    );
}

#[test]
fn finite_no_dash_controller_portfolio_is_recorded_without_a_difficulty_claim() {
    let initial = medium_no_dash_scenario();
    let target = SearchTarget::exit(MEDIUM_NO_DASH_TARGET);
    let config = SolverConfig::for_abilities(MEDIUM_NO_DASH_ABILITIES);
    let audit = audit_direct_controller_probes(&initial, &[target], &config)
        .expect("the finite no-Dash direct-controller audit runs");

    assert_eq!(audit.status, DirectProbeAuditStatus::Complete);
    eprintln!(
        "current-policy direct-controller positives (coverage evidence, not difficulty): {:?}",
        witness_probe_labels(&audit)
    );
}

#[test]
fn medium_exact_route_has_fewer_explicit_actions_than_the_hard_fixture() {
    let medium_actions = medium_no_dash_witness_actions();
    let medium = observe_route(
        &medium_no_dash_scenario(),
        MEDIUM_NO_DASH_TARGET,
        &medium_actions,
    );
    let hard_actions = hard_no_dash_witness_actions();
    let hard = observe_route(&hard_no_dash_scenario(), HARD_NO_DASH_TARGET, &hard_actions);

    assert!(medium.is_clean_completion());
    assert!(hard.is_clean_completion());
    assert!(
        medium_actions.len() < hard_actions.len(),
        "medium stored route should be shorter: {} vs {} ticks",
        medium_actions.len(),
        hard_actions.len()
    );
    assert!(
        medium.jump_presses < hard.jump_presses,
        "medium stored route should have fewer accepted jump decisions: {} vs {}",
        medium.jump_presses,
        hard.jump_presses
    );
    assert!(
        medium.wall_jumps < hard.wall_jumps,
        "medium stored route should have fewer accepted wall jumps: {} vs {}",
        medium.wall_jumps,
        hard.wall_jumps
    );

    eprintln!(
        "explicit stored-route comparison (not a human-difficulty score): medium {} ticks / {} jump presses / {} wall jumps; hard {} / {} / {}",
        medium_actions.len(),
        medium.jump_presses,
        medium.wall_jumps,
        hard_actions.len(),
        hard.jump_presses,
        hard.wall_jumps,
    );
}

#[test]
fn medium_witness_uses_wider_contact_and_recovery_geometry_than_hard() {
    let medium = observe_route_geometry(
        &medium_no_dash_scenario(),
        &medium_no_dash_witness_actions(),
    );
    let hard = observe_route_geometry(&hard_no_dash_scenario(), &hard_no_dash_witness_actions());

    assert!(!medium.wall_contact_pad_heights.is_empty());
    assert!(!hard.wall_contact_pad_heights.is_empty());
    assert!(!medium.landing_support_widths.is_empty());
    assert!(!hard.landing_support_widths.is_empty());

    let medium_narrowest_pad = *medium.wall_contact_pad_heights.iter().min().unwrap();
    let hard_narrowest_pad = *hard.wall_contact_pad_heights.iter().min().unwrap();
    let medium_narrowest_landing = *medium.landing_support_widths.iter().min().unwrap();
    let hard_narrowest_landing = *hard.landing_support_widths.iter().min().unwrap();

    assert!(
        medium_narrowest_pad > hard_narrowest_pad,
        "witnessed medium wall pads should have more vertical safe tiles: {medium:?} vs {hard:?}"
    );
    assert!(
        medium_narrowest_landing > hard_narrowest_landing,
        "witnessed medium recovery landings should have more horizontal safe tiles: {medium:?} vs {hard:?}"
    );

    eprintln!(
        "explicit witnessed geometry (not a human-difficulty score): medium pads {:?}, landings {:?}; hard pads {:?}, landings {:?}",
        medium.wall_contact_pad_heights,
        medium.landing_support_widths,
        hard.wall_contact_pad_heights,
        hard.landing_support_widths,
    );
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_current_player_movement_witness() {
    let initial = medium_no_dash_scenario();
    let mut config = SolverConfig::for_abilities(MEDIUM_NO_DASH_ABILITIES);
    config.max_ticks_per_path = 1_000;
    config.max_expanded_nodes = 200_000;
    config.max_simulated_ticks = 6_000_000;
    let target = SearchTarget::exit(MEDIUM_NO_DASH_TARGET);
    let outcome = solve_target(&initial, target, &config).expect("the target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("current movement did not solve the medium challenge: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, MEDIUM_NO_DASH_TARGET, &found);
    let observation = observe_route(&initial, MEDIUM_NO_DASH_TARGET, &actions);
    assert!(observation.is_clean_completion());
    eprintln!(
        "MEDIUM {} -> {} ticks / {} presses / {} wall jumps",
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
