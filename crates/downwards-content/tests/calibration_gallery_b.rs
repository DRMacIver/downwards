use downwards_content::CalibrationLevel;

#[path = "../src/generated_calibration_witnesses.rs"]
mod generated_calibration_witnesses;

#[path = "../src/calibration_gallery_b.rs"]
mod calibration_gallery_b;

use calibration_gallery_b::{
    CALIBRATION_GALLERY_B_ABILITIES, CALIBRATION_GALLERY_B_TARGET, calibration_gallery_b_cases,
    calibration_gallery_b1_room, calibration_gallery_b1_scenario,
    calibration_gallery_b1_witness_actions, calibration_gallery_b2_room,
    calibration_gallery_b2_scenario, calibration_gallery_b2_witness_actions,
    calibration_gallery_b3_room, calibration_gallery_b3_scenario,
    calibration_gallery_b3_witness_actions, calibration_gallery_b4_room,
    calibration_gallery_b4_scenario, calibration_gallery_b4_witness_actions,
    calibration_gallery_b5_room, calibration_gallery_b5_scenario,
    calibration_gallery_b5_witness_actions,
};
use downwards_ai::{
    DirectProbeAuditStatus, DirectProbePolicy, SearchTarget, SolverConfig, TargetSolveOutcome,
    audit_direct_controller_probes, solve_target,
};
use downwards_core::{
    AbilitySet, Action, DeathReason, JumpKind, Rect, Room, Simulation, SimulationEvent, Tile,
    WallSide,
};

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
        if simulation.reached_exit() == Some(CALIBRATION_GALLERY_B_TARGET) {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RouteObservation {
    reached_at_tick: Option<usize>,
    jump_presses: usize,
    accepted_jumps: usize,
    wall_jumps: usize,
    dash_inputs: usize,
    dash_events: usize,
    deaths: usize,
    resets: usize,
}

type CompletedCase = (&'static str, fn() -> Simulation, fn() -> Vec<Action>);

fn observe_route(initial: &Simulation, actions: &[Action]) -> RouteObservation {
    let mut simulation = initial.clone();
    let mut observation = RouteObservation::default();
    let mut previous_action = Action::default();

    for (index, &action) in actions.iter().enumerate() {
        if action.jump && !previous_action.jump {
            observation.jump_presses += 1;
        }
        observation.dash_inputs += usize::from(action.dash);
        for event in simulation.step(action).events {
            match event {
                SimulationEvent::Jumped(kind) => {
                    observation.accepted_jumps += 1;
                    observation.wall_jumps += usize::from(matches!(kind, JumpKind::Wall { .. }));
                }
                SimulationEvent::Landed => {}
                SimulationEvent::Dashed { .. } => observation.dash_events += 1,
                SimulationEvent::Died(_) => observation.deaths += 1,
                SimulationEvent::Reset => observation.resets += 1,
                SimulationEvent::ExitReached { ref id } if id == CALIBRATION_GALLERY_B_TARGET => {
                    observation.reached_at_tick.get_or_insert(index + 1);
                }
                SimulationEvent::PickupCollected { .. } | SimulationEvent::ExitReached { .. } => {}
            }
        }
        previous_action = action;
    }
    observation
}

fn one_way_runs(room: &Room, row: u16) -> Vec<usize> {
    let mut runs = Vec::new();
    let mut current = 0;
    for column in 0..room.width() {
        if room.tile(column, row) == Some(Tile::OneWay) {
            current += 1;
        } else if current > 0 {
            runs.push(current);
            current = 0;
        }
    }
    if current > 0 {
        runs.push(current);
    }
    runs
}

fn wall_jump_trace(initial: &Simulation, actions: &[Action]) -> Vec<(WallSide, i32)> {
    let mut simulation = initial.clone();
    let mut trace = Vec::new();
    for &action in actions {
        for event in simulation.step(action).events {
            if let SimulationEvent::Jumped(JumpKind::Wall { side }) = event {
                trace.push((side, simulation.player().bounds().y));
            }
        }
    }
    trace
}

fn wall_jump_sides(initial: &Simulation, actions: &[Action]) -> Vec<WallSide> {
    wall_jump_trace(initial, actions)
        .into_iter()
        .map(|(side, _)| side)
        .collect()
}

fn one_way_support_width(room: &Room, player: Rect) -> Option<usize> {
    let row = player.bottom().div_euclid(room.tile_size());
    if row < 0 || row >= i32::from(room.height()) {
        return None;
    }
    let first = player.x.div_euclid(room.tile_size()).max(0);
    let last = (player.right() - 1)
        .div_euclid(room.tile_size())
        .min(i32::from(room.width()) - 1);
    let mut candidates = (first..=last).collect::<Vec<_>>();
    // A landing event may resolve with the player exactly beside the support
    // on the same tick. Retain that zero-overlap edge contact: it is the most
    // precise landing in this authored chain, not an unrelated nearby tile.
    if player.right().rem_euclid(room.tile_size()) == 0 {
        candidates.push(player.right().div_euclid(room.tile_size()));
    }
    if player.x.rem_euclid(room.tile_size()) == 0 {
        candidates.push(player.x.div_euclid(room.tile_size()) - 1);
    }
    candidates
        .into_iter()
        .filter(|&column| column >= 0 && column < i32::from(room.width()))
        .filter(|&column| room.tile(column as u16, row as u16) == Some(Tile::OneWay))
        .map(|column| {
            let mut left = column;
            while left > 0 && room.tile((left - 1) as u16, row as u16) == Some(Tile::OneWay) {
                left -= 1;
            }
            let mut right = column;
            while right + 1 < i32::from(room.width())
                && room.tile((right + 1) as u16, row as u16) == Some(Tile::OneWay)
            {
                right += 1;
            }
            usize::try_from(right + 1 - left).expect("a positive support width fits usize")
        })
        .max()
}

fn one_way_landing_trace(initial: &Simulation, actions: &[Action]) -> Vec<(i32, usize)> {
    let mut simulation = initial.clone();
    let mut trace = Vec::new();
    for &action in actions {
        for event in simulation.step(action).events {
            if matches!(event, SimulationEvent::Landed) {
                let bounds = simulation.player().bounds();
                if let Some(width) = one_way_support_width(simulation.room(), bounds) {
                    trace.push((bounds.x, width));
                }
            }
        }
    }
    trace
}

#[test]
fn all_five_witnesses_are_exact_clean_simplified_accepted_wall_routes() {
    let completed: [CompletedCase; 5] = [
        (
            "B1",
            calibration_gallery_b1_scenario,
            calibration_gallery_b1_witness_actions,
        ),
        (
            "B2",
            calibration_gallery_b2_scenario,
            calibration_gallery_b2_witness_actions,
        ),
        (
            "B3",
            calibration_gallery_b3_scenario,
            calibration_gallery_b3_witness_actions,
        ),
        (
            "B4",
            calibration_gallery_b4_scenario,
            calibration_gallery_b4_witness_actions,
        ),
        (
            "B5",
            calibration_gallery_b5_scenario,
            calibration_gallery_b5_witness_actions,
        ),
    ];

    for (label, scenario, witness) in completed {
        let initial = scenario();
        let actions = witness();
        assert!(!actions.is_empty(), "{label} witness is empty");
        assert_eq!(
            clean_reached_at(&initial, &actions),
            Some(actions.len()),
            "{label} stored route is not an exact clean completion"
        );
        let observation = observe_route(&initial, &actions);
        assert_eq!(observation.reached_at_tick, Some(actions.len()));
        assert!(observation.wall_jumps > 0);
        assert_eq!(observation.jump_presses, observation.accepted_jumps);
        assert_eq!(observation.dash_inputs, 0);
        assert_eq!(observation.dash_events, 0);
        assert_eq!(observation.deaths, 0);
        assert_eq!(observation.resets, 0);
    }
}

#[test]
fn b1_live_wall_assists_make_brief_taps_sufficient() {
    let mut simulation = calibration_gallery_b1_scenario();
    simulation.enable_human_wall_assists();
    let mut actions = vec![Action::default(); 42];
    for tick in [1_usize, 13, 27] {
        actions[tick - 1].jump = true;
    }

    let mut wall_sides = Vec::new();
    for (index, action) in actions.into_iter().enumerate() {
        for event in simulation.step(action).events {
            if let SimulationEvent::Jumped(JumpKind::Wall { side }) = event {
                wall_sides.push(side);
            }
            assert!(
                !matches!(event, SimulationEvent::Died(_)),
                "brief-tap route died at tick {} after {wall_sides:?}",
                index + 1
            );
        }
        if simulation.reached_exit().is_some() {
            break;
        }
    }

    assert_eq!(
        wall_sides,
        [WallSide::Left, WallSide::Right, WallSide::Left]
    );
    assert_eq!(
        simulation.reached_exit(),
        Some(CALIBRATION_GALLERY_B_TARGET)
    );
}

#[test]
fn all_five_generated_wall_traces_make_strict_vertical_progress() {
    for case in calibration_gallery_b_cases() {
        let trace = wall_jump_trace(&case.scenario(), &case.witness_actions());
        assert!(!trace.is_empty(), "{}", case.id());
        assert!(
            trace.windows(2).all(|pair| pair[1].1 < pair[0].1),
            "{} must gain height on every retained wall transfer",
            case.id()
        );
    }
}

#[test]
fn descriptors_have_stable_ids_titles_axes_and_locked_loadouts() {
    let cases = calibration_gallery_b_cases();
    assert_eq!(
        cases.map(|case| case.id()),
        ["cal-07", "cal-08", "cal-09", "cal-10", "cal-11"]
    );
    let native_ids = [
        "calibration.wall_jump.b1_needle_chimney",
        "calibration.wall_jump.b2_long_ascent",
        "calibration.wall_jump.b3_open_shaft",
        "calibration.wall_jump.b4_broken_causeway",
        "calibration.wall_jump.b5_low_bridge",
    ];

    for (case, native_id) in cases.into_iter().zip(native_ids) {
        assert!(
            ["B1", "B2", "B3", "B4", "B5"]
                .into_iter()
                .all(|cohort_label| !case.title().contains(cohort_label))
        );
        assert!(!case.mechanic_axis().is_empty());
        assert_eq!(case.target(), CALIBRATION_GALLERY_B_TARGET);
        assert_eq!(case.abilities(), CALIBRATION_GALLERY_B_ABILITIES);
        assert!(case.abilities().wall_jump);
        assert!(!case.abilities().dash);

        let scenario = case.scenario();
        assert_eq!(scenario.room().id(), native_id);
        assert_eq!(scenario.room().width(), 32);
        assert_eq!(scenario.room().height(), 18);
        assert_eq!(scenario.room().exits().len(), 1);
        assert_eq!(scenario.room().exits()[0].id, case.target());
        let witness = case.witness_actions();
        assert_eq!(
            witness.len(),
            observe_route(&scenario, &witness).reached_at_tick.unwrap()
        );
    }
}

#[test]
fn authored_structures_separate_the_five_intended_mechanic_axes() {
    let b1 = calibration_gallery_b1_room();
    assert_eq!(b1.exits()[0].bounds, Rect::new(120, 0, 30, 28));
    for (column, rows) in [(11, [10, 11]), (15, [7, 8]), (11, [4, 5]), (15, [1, 2])] {
        for row in rows {
            assert_eq!(b1.tile(column, row), Some(Tile::Solid));
        }
    }
    assert!(
        calibration_gallery_b1_witness_actions().len()
            < calibration_gallery_b3_witness_actions().len()
    );

    let b2 = calibration_gallery_b2_room();
    assert_eq!(one_way_runs(&b2, 9), [3]);
    let b2_observation = observe_route(
        &calibration_gallery_b2_scenario(),
        &calibration_gallery_b2_witness_actions(),
    );
    let b3 = calibration_gallery_b3_room();
    assert!(b3.tiles().iter().all(|tile| *tile != Tile::OneWay));
    let b3_observation = observe_route(
        &calibration_gallery_b3_scenario(),
        &calibration_gallery_b3_witness_actions(),
    );
    assert!(b2_observation.wall_jumps > b3_observation.wall_jumps);
    assert_ne!(b2.tiles(), b3.tiles());

    let b4 = calibration_gallery_b4_room();
    assert_eq!(one_way_runs(&b4, 7), [2, 1, 1, 2]);
    assert!((17..32).all(|column| b4.tile(column, 9) == Some(Tile::HazardUp)));
    let landing_trace = one_way_landing_trace(
        &calibration_gallery_b4_scenario(),
        &calibration_gallery_b4_witness_actions(),
    );
    assert!(
        landing_trace.len() >= 3
            && landing_trace
                .iter()
                .filter(|(_, width)| *width == 1)
                .count()
                >= 2,
        "the landing-chain witness must use multiple tiny supports: {landing_trace:?}"
    );
    let b4_sides = wall_jump_sides(
        &calibration_gallery_b4_scenario(),
        &calibration_gallery_b4_witness_actions(),
    );
    assert!(b4_sides.len() >= 5);
    assert!(b4_sides.windows(2).all(|pair| pair[0] != pair[1]));

    let b5 = calibration_gallery_b5_room();
    for column in 20..=22 {
        assert_eq!(b5.tile(column, 1), Some(Tile::HazardUp));
        assert_eq!(b4.tile(column, 1), Some(Tile::Empty));
    }
    for row in [2, 3] {
        for column in 20..=22 {
            assert_eq!(b5.tile(column, row), Some(Tile::HazardDown));
            assert_eq!(b4.tile(column, row), Some(Tile::Empty));
        }
    }
    let b5_sides = wall_jump_sides(
        &calibration_gallery_b5_scenario(),
        &calibration_gallery_b5_witness_actions(),
    );
    assert!(b5_sides.len() >= 5);
    assert!(b5_sides.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_and_simplify_b1_representative() {
    let initial = calibration_gallery_b1_scenario();
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &initial,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B1 target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B1 candidate geometry was not solved: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &found);
    eprintln!(
        "B1 solver {} ticks; simplified {}",
        found.len(),
        actions.len()
    );

    let mut replay = initial;
    let mut wall_jumps = 0;
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if matches!(kind, JumpKind::Wall { .. }) {
                    wall_jumps += 1;
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps > 0);
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_and_simplify_b2_endurance() {
    let initial = calibration_gallery_b2_scenario();
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &initial,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B2 target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B2 candidate geometry was not solved: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &found);
    eprintln!(
        "B2 solver {} ticks; simplified {}",
        found.len(),
        actions.len()
    );

    let mut replay = initial;
    let mut wall_jumps = 0;
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if matches!(kind, JumpKind::Wall { .. }) {
                    wall_jumps += 1;
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps >= 5, "B2 should retain the longer climb axis");
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only segmented solver"]
fn discover_segmented_b2_sane_route() {
    let initial = calibration_gallery_b2_scenario();
    let prefix = calibration_gallery_b3_witness_actions();
    let mut waypoint = initial.clone();
    let mut prefix_wall_jumps = 0;
    for &action in &prefix {
        for event in waypoint.step(action).events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { .. }) => prefix_wall_jumps += 1,
                SimulationEvent::Died(reason) => panic!("B3-like B2 prefix died: {reason:?}"),
                _ => {}
            }
        }
    }
    eprintln!(
        "B2 segmented prefix: {} ticks / {} wall jumps / position {:?} / reached {:?}",
        prefix.len(),
        prefix_wall_jumps,
        waypoint.player().bounds(),
        waypoint.reached_exit()
    );

    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &waypoint,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B2 target exists after prefix");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B2 segmented suffix was not solved: {outcome:?}");
    };
    let mut combined = prefix;
    combined.extend(solution.replay.actions());
    let actions = greedily_simplify(&initial, &combined);

    let mut replay = initial;
    let mut wall_jumps = 0;
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if matches!(kind, JumpKind::Wall { .. }) {
                    wall_jumps += 1;
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps <= 7, "segmented B2 route still hops excessively");
    eprintln!(
        "B2 segmented simplified {} ticks / {wall_jumps} WJ",
        actions.len()
    );
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_and_simplify_b3_no_recovery() {
    let initial = calibration_gallery_b3_scenario();
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &initial,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B3 target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B3 candidate geometry was not solved: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &found);
    eprintln!(
        "B3 solver {} ticks; simplified {}",
        found.len(),
        actions.len()
    );

    let mut replay = initial;
    let mut wall_jumps = 0;
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if matches!(kind, JumpKind::Wall { .. }) {
                    wall_jumps += 1;
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps >= 4, "B3 should retain a moderate wall climb");
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only solver"]
fn discover_and_simplify_b4_landing_chain() {
    let initial = calibration_gallery_b4_scenario();
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &initial,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B4 target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B4 candidate geometry was not solved: {outcome:?}");
    };
    let found = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &found);
    eprintln!(
        "B4 solver {} ticks; simplified {}",
        found.len(),
        actions.len()
    );

    let mut replay = initial;
    let mut wall_jumps = 0;
    let mut landings = 0;
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            match event {
                SimulationEvent::Jumped(kind) => {
                    if matches!(kind, JumpKind::Wall { .. }) {
                        wall_jumps += 1;
                    }
                    eprintln!("tick {} {kind:?}", tick + 1);
                }
                SimulationEvent::Landed => landings += 1,
                _ => {}
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps > 0);
    assert!(landings >= 2, "B4 should retain repeated landing control");
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only segmented solver"]
fn discover_segmented_b4_with_alternating_climb() {
    let initial = calibration_gallery_b4_scenario();
    let prefix = calibration_gallery_b2_witness_actions();
    let mut waypoint = initial.clone();
    let mut prefix_sides = Vec::new();
    for (tick, &action) in prefix.iter().enumerate() {
        for event in waypoint.step(action).events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    prefix_sides.push(side);
                    eprintln!("prefix tick {} wall {side:?}", tick + 1);
                }
                SimulationEvent::Died(reason) => {
                    panic!(
                        "alternating B4 prefix died at tick {}: {reason:?}",
                        tick + 1
                    )
                }
                _ => {}
            }
        }
    }
    eprintln!(
        "B4 alternating prefix: {} ticks / sides {prefix_sides:?} / bounds {:?}",
        prefix.len(),
        waypoint.player().bounds()
    );

    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &waypoint,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B4 target exists after alternating prefix");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B4 landing-chain suffix was not solved: {outcome:?}");
    };
    let mut combined = prefix;
    combined.extend(solution.replay.actions());
    let actions = greedily_simplify(&initial, &combined);

    let mut replay = initial;
    let mut sides = Vec::new();
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if let JumpKind::Wall { side } = kind {
                    sides.push(side);
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(sides.windows(2).all(|pair| pair[0] != pair[1]));
    eprintln!("B4 sane route {} ticks / sides {sides:?}", actions.len());
    print_spans(&actions);
}

#[test]
#[ignore = "authoring-only witness derivation"]
fn simplify_b5_from_the_proven_cut_route() {
    let initial = calibration_gallery_b5_scenario();
    let config = SolverConfig::for_abilities(CALIBRATION_GALLERY_B_ABILITIES);
    let outcome = solve_target(
        &initial,
        SearchTarget::exit(CALIBRATION_GALLERY_B_TARGET),
        &config,
    )
    .expect("B5 target exists");
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("B5 candidate geometry was not solved: {outcome:?}");
    };
    let candidate = solution.replay.actions().collect::<Vec<_>>();
    let actions = greedily_simplify(&initial, &candidate);

    let mut replay = initial;
    let mut wall_jumps = 0;
    let mut non_wall_jumps = Vec::new();
    for (tick, &action) in actions.iter().enumerate() {
        for event in replay.step(action).events {
            if let SimulationEvent::Jumped(kind) = event {
                if matches!(kind, JumpKind::Wall { .. }) {
                    wall_jumps += 1;
                } else {
                    non_wall_jumps.push((tick + 1, kind));
                }
                eprintln!("tick {} {kind:?}", tick + 1);
            }
        }
    }
    assert_eq!(replay.reached_exit(), Some(CALIBRATION_GALLERY_B_TARGET));
    assert!(wall_jumps > 0);
    assert!(!non_wall_jumps.is_empty());
    eprintln!(
        "B5 inherited {} ticks; simplified {}; non-wall jumps {non_wall_jumps:?}",
        candidate.len(),
        actions.len()
    );
    print_spans(&actions);
}

#[test]
fn b5_requires_releasing_the_first_transfer_before_the_low_ceiling() {
    let cut = calibration_gallery_b5_witness_actions();
    let mut probe = calibration_gallery_b5_scenario();
    let mut transfer_ticks = Vec::new();
    for (index, &action) in cut.iter().enumerate() {
        for event in probe.step(action).events {
            if matches!(event, SimulationEvent::Jumped(kind) if !matches!(kind, JumpKind::Wall { .. }))
            {
                transfer_ticks.push(index);
            }
        }
    }
    let first_transfer = transfer_ticks[0];
    let first_release = cut[first_transfer..]
        .iter()
        .position(|action| !action.jump)
        .map(|offset| first_transfer + offset)
        .expect("the stored transfer has a jump release");
    assert!(first_release - first_transfer < 10);
    let low_gesture = cut.clone();
    let mut low_simulation = calibration_gallery_b5_scenario();
    let mut low_deaths = Vec::new();
    for action in low_gesture {
        for event in low_simulation.step(action).events {
            if let SimulationEvent::Died(reason) = event {
                low_deaths.push(reason);
            }
        }
    }
    assert!(
        low_deaths.is_empty(),
        "the semantic low gesture must be safe"
    );
    assert_eq!(
        low_simulation.reached_exit(),
        Some(CALIBRATION_GALLERY_B_TARGET),
        "the semantic low gesture must remain a viable authored route"
    );
    let mut uncut = cut.clone();
    for action in uncut.iter_mut().skip(first_transfer).take(10) {
        action.jump = true;
    }

    let mut simulation = calibration_gallery_b5_scenario();
    let mut deaths = Vec::new();
    for action in uncut {
        for event in simulation.step(action).events {
            if let SimulationEvent::Died(reason) = event {
                deaths.push(reason);
            }
        }
    }
    eprintln!(
        "B5 uncut first transfer at tick {}; deaths {deaths:?}; reached {:?}",
        first_transfer + 1,
        simulation.reached_exit()
    );
    assert!(deaths.iter().any(|reason| matches!(
        reason,
        DeathReason::Hazard {
            tile_x: 20..=22,
            tile_y: 3
        }
    )));
    assert_ne!(
        simulation.reached_exit(),
        Some(CALIBRATION_GALLERY_B_TARGET)
    );
}

#[test]
fn bounded_baseline_and_direct_controller_audits_record_known_routes_without_ranking() {
    for case in calibration_gallery_b_cases() {
        let scenario = case.scenario();
        let target = SearchTarget::exit(case.target());

        let mut baseline = Simulation::with_abilities(scenario.room().clone(), AbilitySet::NONE);
        baseline.enable_current_player_movement();
        let baseline_config = SolverConfig::for_abilities(AbilitySet::NONE);
        let baseline_solve = solve_target(&baseline, target.clone(), &baseline_config).unwrap();
        let baseline_audit = audit_direct_controller_probes(
            &baseline,
            std::slice::from_ref(&target),
            &baseline_config,
        )
        .unwrap();

        let wall_config = SolverConfig::for_abilities(case.abilities());
        let wall_audit =
            audit_direct_controller_probes(&scenario, &[target], &wall_config).unwrap();
        assert!(matches!(
            baseline_solve,
            TargetSolveOutcome::Inconclusive { .. }
        ));
        assert_eq!(baseline_audit.status, DirectProbeAuditStatus::Complete);
        assert!(baseline_audit.witnesses.is_empty());
        assert_eq!(wall_audit.status, DirectProbeAuditStatus::Complete);

        let wall_probes = wall_audit
            .witnesses
            .iter()
            .flat_map(|witness| &witness.probes)
            .collect::<Vec<_>>();
        eprintln!(
            "{} current-movement direct routes: {wall_probes:?}",
            case.id()
        );
        if matches!(case.id(), "cal-07" | "cal-08") {
            assert!(
                wall_probes
                    .iter()
                    .any(|probe| probe.policy == DirectProbePolicy::BufferedWallClimb)
            );
        }
    }
}
