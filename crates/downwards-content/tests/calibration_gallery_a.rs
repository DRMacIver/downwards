#[path = "../src/generated_calibration_witnesses.rs"]
mod generated_calibration_witnesses;

#[path = "../src/calibration_gallery_a.rs"]
mod calibration_gallery_a;

use calibration_gallery_a::{
    CALIBRATION_GALLERY_A_ABILITIES, CALIBRATION_GALLERY_A_DIMENSIONS,
    CALIBRATION_GALLERY_A_TARGET, calibration_gallery_a_cases, calibration_gallery_a1_scenario,
    calibration_gallery_a1_witness_actions, calibration_gallery_a2_scenario,
    calibration_gallery_a3_scenario, calibration_gallery_a4_scenario,
    calibration_gallery_a4_witness_actions, calibration_gallery_a5_scenario,
    calibration_gallery_a5_witness_actions,
};
use downwards_ai::{
    DirectProbeAuditStatus, Replay, SearchTarget, SolverConfig, TargetSolveOutcome,
    audit_direct_controller_probes, solve_target,
};
use downwards_core::{
    AbilitySet, Action, HazardDirection, JumpKind, Rect, Simulation, SimulationEvent, Tile,
    WallSide,
};
use std::collections::HashSet;

fn clean_reached_at(initial: &Simulation, actions: &[Action]) -> Option<usize> {
    let mut simulation = initial.clone();
    for (index, &action) in actions.iter().enumerate() {
        if action.dash || action.restart {
            return None;
        }
        let report = simulation.step(action);
        if report.events.iter().any(|event| {
            matches!(
                event,
                SimulationEvent::Died(_) | SimulationEvent::Dashed { .. } | SimulationEvent::Reset
            )
        }) {
            return None;
        }
        if simulation.reached_exit() == Some(CALIBRATION_GALLERY_A_TARGET) {
            return Some(index + 1);
        }
    }
    None
}

fn greedily_simplify(initial: &Simulation, original: &[Action]) -> Vec<Action> {
    let reached_at = clean_reached_at(initial, original).expect("candidate must complete");
    let mut actions = original[..reached_at].to_vec();
    loop {
        let before_pass = actions.clone();

        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if clean_reached_at(initial, &candidate).is_some() {
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
            if clean_reached_at(initial, &candidate).is_some() {
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
                    && clean_reached_at(initial, &candidate).is_some()
                {
                    actions = candidate;
                }
            }
        }

        if actions == before_pass {
            return actions;
        }
    }
}

fn print_spans(actions: &[Action]) {
    let mut start = 0;
    while start < actions.len() {
        let action = actions[start];
        let end = actions[start..]
            .iter()
            .position(|candidate| *candidate != action)
            .map_or(actions.len(), |offset| start + offset);
        eprintln!("({action:?}, {}),", end - start);
        start = end;
    }
}

fn search_simplified(label: &str, initial: &Simulation) -> Vec<Action> {
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_A_ABILITIES);
    let outcome = solve_target(
        initial,
        SearchTarget::exit(CALIBRATION_GALLERY_A_TARGET),
        &config,
    )
    .expect("target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("{label} not solved: {outcome:?}");
    };
    let raw = solution.replay.actions().collect::<Vec<_>>();
    let simplified = greedily_simplify(initial, &raw);
    eprintln!("{label} {} -> {} ticks", raw.len(), simplified.len());
    let mut replay = initial.clone();
    for (tick, &action) in simplified.iter().enumerate() {
        let report = replay.step(action);
        for event in report.events {
            if matches!(event, SimulationEvent::Jumped(_) | SimulationEvent::Landed) {
                eprintln!(
                    "{label} tick {} {event:?} {:?}",
                    tick + 1,
                    replay.player().bounds()
                );
            }
        }
    }
    print_spans(&simplified);
    simplified
}

#[test]
#[ignore = "authoring-only solver"]
fn representative_a3_is_frozen() {
    let initial = calibration_gallery_a3_scenario();
    assert!(!search_simplified("A3", &initial).is_empty());
}

#[test]
#[ignore = "authoring-only solver"]
fn search_a1() {
    assert!(!search_simplified("A1", &calibration_gallery_a1_scenario()).is_empty());
}

#[test]
#[ignore = "authoring-only solver"]
fn search_a2() {
    assert!(!search_simplified("A2", &calibration_gallery_a2_scenario()).is_empty());
}

#[test]
#[ignore = "authoring-only solver"]
fn search_a4() {
    let simplified = search_simplified("A4", &calibration_gallery_a4_scenario());
    assert!(!simplified.is_empty());
    let mut held = simplified.clone();
    for action in &mut held[82..89] {
        action.jump = true;
    }
    let mut replay = calibration_gallery_a4_scenario();
    for (tick, action) in held.into_iter().enumerate() {
        for event in replay.step(action).events {
            if matches!(event, SimulationEvent::Died(_)) {
                eprintln!("A4 held variant tick {} {event:?}", tick + 1);
            }
        }
    }
    eprintln!(
        "A4 held reached {:?}, deaths {}",
        replay.reached_exit(),
        replay.deaths()
    );
}

#[test]
#[ignore = "authoring-only solver"]
fn search_a5() {
    assert!(!search_simplified("A5", &calibration_gallery_a5_scenario()).is_empty());
}

#[derive(Debug, Default)]
struct RouteObservation {
    exit_tick: Option<usize>,
    jump_press_ticks: Vec<usize>,
    jump_event_ticks: Vec<usize>,
    wall_jumps: Vec<(usize, WallSide, i32)>,
    ordinary_jumps: usize,
    deaths: usize,
    dashes: usize,
    resets: usize,
    landings: Vec<(usize, Rect)>,
}

fn observe_route(initial: &Simulation, actions: &[Action]) -> RouteObservation {
    let mut simulation = initial.clone();
    let mut observation = RouteObservation::default();
    let mut previous = Action::default();

    for (index, &action) in actions.iter().enumerate() {
        let tick = index + 1;
        if action.jump && !previous.jump {
            observation.jump_press_ticks.push(tick);
        }
        if action.dash {
            observation.dashes += 1;
        }
        if action.restart {
            observation.resets += 1;
        }

        for event in simulation.step(action).events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    observation.jump_event_ticks.push(tick);
                    observation
                        .wall_jumps
                        .push((tick, side, simulation.player().bounds().y));
                }
                SimulationEvent::Jumped(_) => {
                    observation.jump_event_ticks.push(tick);
                    observation.ordinary_jumps += 1;
                }
                SimulationEvent::Landed => observation
                    .landings
                    .push((tick, simulation.player().bounds())),
                SimulationEvent::Died(_) => observation.deaths += 1,
                SimulationEvent::Dashed { .. } => observation.dashes += 1,
                SimulationEvent::Reset => observation.resets += 1,
                SimulationEvent::ExitReached { ref id } if id == CALIBRATION_GALLERY_A_TARGET => {
                    observation.exit_tick.get_or_insert(tick);
                }
                SimulationEvent::PickupCollected { .. } | SimulationEvent::ExitReached { .. } => {}
            }
        }
        previous = action;
        if observation.exit_tick.is_some() {
            break;
        }
    }

    observation.deaths = observation
        .deaths
        .max(usize::try_from(simulation.deaths()).expect("death count fits usize"));
    if simulation.reached_exit() != Some(CALIBRATION_GALLERY_A_TARGET) {
        observation.exit_tick = None;
    }
    observation
}

fn support_run_width(room: &downwards_core::Room, player: Rect) -> Option<usize> {
    let row = player.bottom().div_euclid(room.tile_size());
    if row < 0 || row >= i32::from(room.height()) {
        return None;
    }
    let first = player.x.div_euclid(room.tile_size()).max(0);
    let last = (player.right() - 1)
        .div_euclid(room.tile_size())
        .min(i32::from(room.width()) - 1);
    (first..=last)
        .filter(|&col| {
            matches!(
                room.tile(col as u16, row as u16),
                Some(Tile::Solid | Tile::OneWay)
            )
        })
        .map(|col| {
            let mut left = col;
            while left > 0
                && matches!(
                    room.tile((left - 1) as u16, row as u16),
                    Some(Tile::Solid | Tile::OneWay)
                )
            {
                left -= 1;
            }
            let mut right = col;
            while right + 1 < i32::from(room.width())
                && matches!(
                    room.tile((right + 1) as u16, row as u16),
                    Some(Tile::Solid | Tile::OneWay)
                )
            {
                right += 1;
            }
            usize::try_from(right + 1 - left).expect("support width fits usize")
        })
        .max()
}

#[test]
fn gallery_metadata_and_room_contracts_are_stable_and_unanchored() {
    let cases = calibration_gallery_a_cases();
    let expected = [
        (
            "calibration.wall_jump.a1_broad_ascent",
            "Open Chimney",
            "long broad-pad wall-jump climb",
        ),
        (
            "calibration.wall_jump.a2_needle_step",
            "Two-Tile Turn",
            "short narrow-pad wall-jump precision",
        ),
        (
            "calibration.wall_jump.a3_even_tempo",
            "Even Tempo",
            "moderate rhythmic alternating wall-jump climb",
        ),
        (
            "calibration.wall_jump.a4_low_clearance",
            "Low Clearance",
            "low ceiling: release Jump early for a low hop",
        ),
        (
            "calibration.wall_jump.a5_safe_harbor",
            "Safe Harbor",
            "short wall-jump climb with recovery landings",
        ),
    ];
    let mut ids = HashSet::new();
    let mut titles = HashSet::new();

    for (index, case) in cases.iter().enumerate() {
        assert_eq!(
            (case.native_id, case.title, case.mechanic_axis),
            expected[index]
        );
        assert_eq!(case.dimensions, CALIBRATION_GALLERY_A_DIMENSIONS[index]);
        assert!(ids.insert(case.native_id));
        assert!(titles.insert(case.title));
        assert!(!case.title.contains("Calibration"));
        assert!(!case.title.contains(&format!("A{}", index + 1)));

        let room = (case.room_factory)();
        assert_eq!(room.id(), case.native_id);
        assert_eq!(room.name(), case.title);
        assert_eq!(
            (room.width(), room.height(), room.tile_size()),
            (32, 18, 10)
        );
        assert_eq!(room.tiles().len(), 32 * 18);
        assert_eq!(room.exits().len(), 1);
        assert_eq!(room.exits()[0].id, CALIBRATION_GALLERY_A_TARGET);
        assert!(room.doors().is_empty());
        assert!(room.timed_hazards().is_empty());
        assert!(room.pickups().is_empty());

        let scenario = (case.scenario_factory)();
        assert_eq!(scenario.room().id(), case.native_id);
        assert_eq!(scenario.abilities(), CALIBRATION_GALLERY_A_ABILITIES);
        assert!(scenario.abilities().wall_jump);
        assert!(!scenario.abilities().dash);
        assert!(!(case.witness_factory)().is_empty());

        let dimensions = case.dimensions;
        assert!(dimensions.climb_span_tiles > 0);
        assert!(dimensions.contact_bands > 0);
        assert!(dimensions.minimum_pad_height_tiles > 0);
        assert!(dimensions.jump_cut_checks <= 1);
        assert_eq!(
            dimensions.witnessed_jump_cut_clearance_pixels.is_some(),
            dimensions.jump_cut_checks == 1
        );
        assert!(
            dimensions
                .recovery_support_widths_tiles
                .iter()
                .all(|&width| width >= 3)
        );
    }

    for left in 0..cases.len() {
        for right in left + 1..cases.len() {
            assert_ne!(
                (cases[left].room_factory)().tiles(),
                (cases[right].room_factory)().tiles(),
                "gallery A rooms {left} and {right} must remain geometrically distinct"
            );
        }
    }
}

#[test]
fn structural_axes_are_explicit_in_the_authored_tiles() {
    let cases = calibration_gallery_a_cases();
    let a1 = (cases[0].room_factory)();
    assert!(a1.tiles().iter().all(|tile| !tile.is_hazard()));
    for row in 0..18 {
        assert_eq!(a1.tile(11, row), Some(Tile::Solid));
    }
    for row in 3..18 {
        assert_eq!(a1.tile(15, row), Some(Tile::Solid));
    }
    for col in 15..=22 {
        assert_eq!(a1.tile(col, 3), Some(Tile::Solid));
    }

    let a2 = (cases[1].room_factory)();
    for row in 0..=8 {
        assert_eq!(a2.tile(10, row), Some(Tile::Empty));
        assert_eq!(a2.tile(11, row), Some(Tile::Empty));
    }
    for row in 8..=9 {
        assert_eq!(a2.tile(15, row), Some(Tile::Solid));
    }
    for row in 11..=12 {
        assert_eq!(a2.tile(11, row), Some(Tile::Solid));
    }

    let a3 = (cases[2].room_factory)();
    for (column, first, last) in [(11, 1, 3), (15, 4, 6), (11, 7, 9), (15, 10, 12)] {
        for row in first..=last {
            assert_eq!(a3.tile(column, row), Some(Tile::Solid));
        }
    }
    for row in 13..=15 {
        assert_eq!(a3.tile(11, row), Some(Tile::Solid));
    }
    for col in 15..=22 {
        assert_eq!(a3.tile(col, 4), Some(Tile::Solid));
    }

    let a4 = (cases[3].room_factory)();
    for col in 20..=23 {
        assert_eq!(a4.tile(col, 3), Some(Tile::HazardUp));
        assert_eq!(a4.tile(col, 4), Some(Tile::HazardDown));
    }
    for col in 17..=20 {
        assert_eq!(a4.tile(col, 7), Some(Tile::OneWay));
    }
    for col in 23..=31 {
        assert_eq!(a4.tile(col, 8), Some(Tile::OneWay));
    }
    for col in 17..=31 {
        assert_eq!(a4.tile(col, 12), Some(Tile::HazardUp));
    }

    let a5 = (cases[4].room_factory)();
    assert_eq!(a5.tile(16, 11), Some(Tile::Solid));
    for col in 17..=20 {
        assert_eq!(a5.tile(col, 11), Some(Tile::OneWay));
    }
    for col in 23..=25 {
        assert_eq!(a5.tile(col, 10), Some(Tile::OneWay));
    }
    for col in 17..=27 {
        assert_eq!(a5.tile(col, 13), Some(Tile::HazardUp));
    }
    for col in 28..=31 {
        assert_eq!(a5.tile(col, 13), Some(Tile::Solid));
    }
}

#[test]
fn exact_witnesses_are_clean_terminal_and_every_jump_press_is_accepted() {
    for case in calibration_gallery_a_cases() {
        let initial = (case.scenario_factory)();
        let actions = (case.witness_factory)();
        assert!(!actions.is_empty(), "{}", case.native_id);
        assert!(actions.iter().all(|action| !action.dash && !action.restart));

        let replay = Replay::record(&initial, actions.iter().copied());
        let verified = replay
            .verify(&initial)
            .expect("stored gallery replay must verify exactly");
        assert_eq!(verified.frames_verified, actions.len());
        assert_eq!(
            verified.reached_exit.as_deref(),
            Some(CALIBRATION_GALLERY_A_TARGET)
        );

        let observation = observe_route(&initial, &actions);
        assert_eq!(
            observation.exit_tick,
            Some(actions.len()),
            "{observation:?}"
        );
        assert_eq!(observation.deaths, 0, "{observation:?}");
        assert_eq!(observation.dashes, 0, "{observation:?}");
        assert_eq!(observation.resets, 0, "{observation:?}");
        assert_eq!(
            observation.jump_press_ticks.len(),
            observation.jump_event_ticks.len(),
            "every jump press must correspond to one accepted jump: {observation:?}"
        );
        for (&press_tick, &event_tick) in observation
            .jump_press_ticks
            .iter()
            .zip(&observation.jump_event_ticks)
        {
            assert!(
                event_tick >= press_tick && event_tick - press_tick <= 5,
                "a jump press was not accepted within the core buffer window: {observation:?}"
            );
        }
        assert!(!observation.wall_jumps.is_empty(), "{}", case.native_id);
        assert!(
            observation
                .wall_jumps
                .windows(2)
                .all(|pair| pair[1].2 < pair[0].2),
            "every retained wall transfer must gain height: {observation:?}"
        );
    }
}

#[test]
fn broad_climb_contacts_are_alternating_and_make_vertical_progress() {
    let actions = calibration_gallery_a1_witness_actions();
    let observation = observe_route(&calibration_gallery_a1_scenario(), &actions);
    assert_eq!(observation.wall_jumps.len(), 5);
    for pair in observation.wall_jumps.windows(2) {
        assert_ne!(
            pair[0].1, pair[1].1,
            "contacts must alternate: {observation:?}"
        );
        assert!(
            pair[0].2 - pair[1].2 >= 8,
            "each retained broad-wall transfer must gain height: {observation:?}"
        );
    }
}

#[test]
fn recovery_route_lands_on_the_authored_recovery_supports() {
    let initial = calibration_gallery_a5_scenario();
    let observation = observe_route(&initial, &calibration_gallery_a5_witness_actions());
    let widths = observation
        .landings
        .iter()
        .filter_map(|(_, bounds)| support_run_width(initial.room(), *bounds))
        .collect::<Vec<_>>();
    assert_eq!(widths, [4, 3, 4]);
}

#[test]
fn stored_traces_do_not_complete_without_wall_jump() {
    for case in calibration_gallery_a_cases() {
        let rich = (case.scenario_factory)();
        let mut baseline = Simulation::with_abilities(rich.room().clone(), AbilitySet::NONE);
        baseline.enable_current_player_movement();
        for action in (case.witness_factory)() {
            baseline.step(action);
        }
        assert_ne!(
            baseline.reached_exit(),
            Some(CALIBRATION_GALLERY_A_TARGET),
            "{} stored trace unexpectedly works without WallJump",
            case.native_id
        );
    }
}

#[test]
fn bounded_direct_probe_audit_records_capability_evidence_without_ranking() {
    let target = SearchTarget::exit(CALIBRATION_GALLERY_A_TARGET);
    for case in calibration_gallery_a_cases() {
        let rich = (case.scenario_factory)();
        let mut baseline = Simulation::with_abilities(rich.room().clone(), AbilitySet::NONE);
        baseline.enable_current_player_movement();
        let baseline_config = SolverConfig::for_abilities(AbilitySet::NONE);
        let baseline_audit = audit_direct_controller_probes(
            &baseline,
            std::slice::from_ref(&target),
            &baseline_config,
        )
        .expect("finite baseline direct probes must run");
        assert_eq!(baseline_audit.status, DirectProbeAuditStatus::Complete);
        let baseline_positives = baseline_audit
            .witnesses
            .iter()
            .flat_map(|witness| {
                witness.probes.iter().map(|probe| {
                    format!("#{} x={} {:?}", probe.ordinal, probe.move_x, probe.policy)
                })
            })
            .collect::<Vec<_>>();
        eprintln!(
            "{} baseline direct positives: {baseline_positives:?}",
            case.native_id
        );

        let rich_config = SolverConfig::for_abilities(CALIBRATION_GALLERY_A_ABILITIES);
        let rich_audit =
            audit_direct_controller_probes(&rich, std::slice::from_ref(&target), &rich_config)
                .expect("finite WallJump direct probes must run");
        assert_eq!(rich_audit.status, DirectProbeAuditStatus::Complete);
        let positives = rich_audit
            .witnesses
            .iter()
            .flat_map(|witness| {
                witness.probes.iter().map(|probe| {
                    format!("#{} x={} {:?}", probe.ordinal, probe.move_x, probe.policy)
                })
            })
            .collect::<Vec<_>>();
        eprintln!(
            "{} WallJump direct positives: {positives:?}",
            case.native_id
        );
    }
}

#[test]
fn low_clearance_route_and_spike_faces_are_structurally_consistent() {
    let canonical = calibration_gallery_a4_witness_actions();
    let transfer_tick = canonical
        .iter()
        .enumerate()
        .filter(|(index, action)| action.jump && (*index == 0 || !canonical[*index - 1].jump))
        .map(|(index, _)| index)
        .next_back()
        .expect("Low Clearance witness has a final transfer jump");
    assert!(!canonical[transfer_tick + 1].jump);

    let clean = observe_route(&calibration_gallery_a4_scenario(), &canonical);
    assert_eq!(clean.exit_tick, Some(canonical.len()));
    assert_eq!(clean.deaths, 0);
    let mut clearance_replay = calibration_gallery_a4_scenario();
    let room = clearance_replay.room();
    for x in 20..=23 {
        assert_eq!(
            room.hazard_direction(x, 3),
            Some(HazardDirection::Up),
            "the upper face of the ceiling bank must prevent walking across its back"
        );
        assert_eq!(
            room.hazard_direction(x, 4),
            Some(HazardDirection::Down),
            "the upper bank must point into the route below"
        );
    }
    for x in 17..=31 {
        assert_eq!(
            room.hazard_direction(x, 12),
            Some(HazardDirection::Up),
            "the lower bank must point into the route above"
        );
    }
    let ceiling_point_y = room.tile_bounds(20, 4).bottom();
    let tile_size = room.tile_size();

    let mut minimum_top_under_ceiling = i32::MAX;
    for (index, &action) in canonical.iter().enumerate() {
        clearance_replay.step(action);
        let player = clearance_replay.player().bounds();
        if index >= transfer_tick && player.x < 240 && player.right() > 200 {
            minimum_top_under_ceiling = minimum_top_under_ceiling.min(player.y);
        }
    }
    let clearance = minimum_top_under_ceiling - ceiling_point_y;
    assert!(
        (1..tile_size).contains(&clearance),
        "the generated route should pass below the authored ceiling with a positive sub-tile clearance; got {clearance}px"
    );
}
