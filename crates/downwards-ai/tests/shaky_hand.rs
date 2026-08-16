use downwards_ai::{
    ExactConvergence, NoiseFamily, NoisyReplayOutcome, PerturbationEdit, ReachedTarget,
    ReplanningSupport, Replay, SHAKY_HAND_CONFIG_VERSION, SHAKY_HAND_POLICY_VERSION, SearchStats,
    SearchTarget, ShakyHandConfig, ShakyHandError, TargetSolution,
    evaluate_recorded_shaky_hand_study, evaluate_shaky_hand, record_shaky_hand_study,
};
use downwards_core::{
    AbilitySet, Action, BoundarySide, Door, Exit, Pickup, Point, Rect, Room, Simulation,
    SimulationEvent, Tile, TimedHazard,
};

const WIDTH: usize = 32;
const HEIGHT: usize = 18;

fn floor_tiles() -> Vec<Tile> {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    tiles
}

fn exit_room(id: &str, exit_x: i32) -> Room {
    Room::new(
        id,
        id,
        WIDTH as u16,
        HEIGHT as u16,
        10,
        floor_tiles(),
        Point::new(20, 148),
        vec![Exit {
            id: "goal".into(),
            bounds: Rect::new(exit_x, 136, 12, 24),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn door_room() -> Room {
    Room::new(
        "shaky-doors",
        "Shaky doors",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        floor_tiles(),
        Point::new(30, 148),
        vec![],
    )
    .unwrap()
    .with_doors(vec![
        Door {
            id: "west".into(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 136, 4, 24),
            arrival: Point::new(30, 148),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east".into(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 136, 4, 24),
            arrival: Point::new(280, 148),
            destination_room: None,
            destination_door: None,
        },
    ])
    .unwrap()
}

fn pickup_room() -> Room {
    Room::new(
        "shaky-pickup",
        "Shaky pickup",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        floor_tiles(),
        Point::new(20, 148),
        vec![],
    )
    .unwrap()
    .with_objects(
        vec![],
        vec![Pickup::new("coin", Rect::new(90, 146, 6, 8)).unwrap()],
    )
    .unwrap()
}

fn reached(simulation: &Simulation, target: &SearchTarget) -> bool {
    match target {
        SearchTarget::AnyExit => simulation.reached_exit().is_some(),
        SearchTarget::Exit(id) | SearchTarget::Door(id) => simulation.reached_exit() == Some(id),
        SearchTarget::Pickup(id) => simulation
            .collected_pickups()
            .any(|pickup| pickup.id() == id),
    }
}

fn reached_target(target: &SearchTarget) -> ReachedTarget {
    match target {
        SearchTarget::AnyExit => panic!("test helper requires an exact target"),
        SearchTarget::Exit(id) => ReachedTarget::Exit(id.clone()),
        SearchTarget::Door(id) => ReachedTarget::Door(id.clone()),
        SearchTarget::Pickup(id) => ReachedTarget::Pickup(id.clone()),
    }
}

fn witness(
    initial: &Simulation,
    target: SearchTarget,
    mut action_at: impl FnMut(usize) -> Action,
) -> TargetSolution {
    let mut simulation = initial.clone();
    let mut actions = Vec::new();
    while !reached(&simulation, &target) {
        assert!(actions.len() < 500, "test witness should reach {target:?}");
        let action = action_at(actions.len());
        actions.push(action);
        let report = simulation.step(action);
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_))),
            "exact fixture died before reaching {target:?} at tick {}",
            actions.len()
        );
    }
    let replay = Replay::record(initial, actions);
    TargetSolution {
        reached: reached_target(&target),
        target,
        replay,
        stats: SearchStats::default(),
    }
}

fn run_right(_: usize) -> Action {
    Action {
        move_x: 1,
        ..Action::default()
    }
}

fn small_config(seed: u64) -> ShakyHandConfig {
    ShakyHandConfig {
        seed,
        trials_per_curve_point: 24,
        grace_ticks: 12,
        correlated_boundaries: 3,
        convergence_confirmation_ticks: 2,
    }
}

#[test]
fn exact_zero_noise_control_supports_exit_door_and_pickup_targets() {
    let exit_initial = Simulation::new(exit_room("shaky-exit", 100));
    let exit_solution = witness(&exit_initial, SearchTarget::exit("goal"), run_right);

    let door_initial = Simulation::enter_via_door(door_room(), AbilitySet::NONE, "west").unwrap();
    let door_solution = witness(&door_initial, SearchTarget::door("east"), run_right);

    let pickup_initial = Simulation::new(pickup_room());
    let pickup_solution = witness(&pickup_initial, SearchTarget::pickup("coin"), run_right);

    for (initial, solution) in [
        (&exit_initial, &exit_solution),
        (&door_initial, &door_solution),
        (&pickup_initial, &pickup_solution),
    ] {
        let report = evaluate_shaky_hand(initial, solution, small_config(7)).unwrap();
        assert!(report.exact_control_succeeded);
        assert_eq!(report.exact_reached, reached_target(&solution.target));
        assert_eq!(report.replanning, ReplanningSupport::Unsupported);
        let exact = report
            .curves
            .iter()
            .find(|curve| curve.family == NoiseFamily::Exact)
            .unwrap();
        assert_eq!(exact.strength_ticks, 0);
        assert_eq!((exact.trials, exact.successes), (1, 1));
        assert_eq!(exact.success_probability, Some(1.0));
        assert!(exact.first_divergence.is_none());
        assert_eq!(
            exact.trials_detail[0].convergence,
            ExactConvergence::NoDivergence
        );
    }
}

#[test]
fn schedules_are_versioned_seeded_reusable_and_deterministic() {
    let initial = Simulation::new(exit_room("shaky-schedules", 280));
    let solution = witness(&initial, SearchTarget::exit("goal"), |tick| {
        if tick < 20 {
            Action {
                move_x: 1,
                ..Action::default()
            }
        } else if tick < 28 {
            Action {
                move_x: 1,
                jump: true,
                ..Action::default()
            }
        } else if tick < 48 {
            Action {
                move_x: 1,
                ..Action::default()
            }
        } else if tick < 56 {
            Action::default()
        } else {
            run_right(tick)
        }
    });
    let config = small_config(0x1234_5678);

    let first_study = record_shaky_hand_study(&solution.replay, config).unwrap();
    let second_study = record_shaky_hand_study(&solution.replay, config).unwrap();
    assert_eq!(first_study, second_study);
    assert_eq!(
        first_study.identity.policy_version,
        SHAKY_HAND_POLICY_VERSION
    );
    assert_eq!(
        first_study.identity.config_version,
        SHAKY_HAND_CONFIG_VERSION
    );
    assert_eq!(first_study.identity.seed, config.seed);

    let first = evaluate_recorded_shaky_hand_study(&initial, &solution, &first_study).unwrap();
    let second = evaluate_recorded_shaky_hand_study(&initial, &solution, &second_study).unwrap();
    assert_eq!(first, second);

    for strength in [1, 2, 4] {
        for family in [NoiseFamily::BoundaryTiming, NoiseFamily::CorrelatedTiming] {
            let curve = first
                .curves
                .iter()
                .find(|curve| curve.family == family && curve.strength_ticks == strength)
                .unwrap();
            assert_eq!(curve.trials, config.trials_per_curve_point);
            assert_eq!(curve.not_applicable_trials, 0);
        }
    }
    for family in [NoiseFamily::HoldRelease, NoiseFamily::DropRepeatFrame] {
        let curve = first
            .curves
            .iter()
            .find(|curve| curve.family == family)
            .unwrap();
        assert_eq!(curve.strength_ticks, 1);
        assert_eq!(curve.trials, config.trials_per_curve_point);
    }

    assert!(first.study.curves.iter().any(|curve| {
        curve.schedules.iter().any(|schedule| {
            matches!(
                schedule.edits.as_slice(),
                [PerturbationEdit::ShiftCorrelatedBoundaries { .. }]
            )
        })
    }));
    let hold_release = first
        .study
        .curves
        .iter()
        .find(|curve| curve.family == NoiseFamily::HoldRelease)
        .unwrap();
    assert!(hold_release.schedules.iter().any(|schedule| matches!(
        schedule.edits.as_slice(),
        [PerturbationEdit::HoldControlOneFrame { .. }]
    )));
    assert!(hold_release.schedules.iter().any(|schedule| matches!(
        schedule.edits.as_slice(),
        [PerturbationEdit::ReleaseControlOneFrameEarly { .. }]
    )));
    let drop_repeat = first
        .study
        .curves
        .iter()
        .find(|curve| curve.family == NoiseFamily::DropRepeatFrame)
        .unwrap();
    assert!(drop_repeat.schedules.iter().any(|schedule| matches!(
        schedule.edits.as_slice(),
        [PerturbationEdit::DropSemanticFrame { .. }]
    )));
    assert!(drop_repeat.schedules.iter().any(|schedule| matches!(
        schedule.edits.as_slice(),
        [PerturbationEdit::RepeatSemanticFrame { .. }]
    )));
    let different_seed =
        record_shaky_hand_study(&solution.replay, small_config(config.seed + 1)).unwrap();
    assert_ne!(first_study.identity, different_seed.identity);
    assert_ne!(first_study.curves, different_seed.curves);
}

#[test]
fn recorded_study_rejects_config_or_replay_identity_tampering() {
    let initial = Simulation::new(exit_room("shaky-identity", 120));
    let solution = witness(&initial, SearchTarget::exit("goal"), run_right);
    let mut study = record_shaky_hand_study(&solution.replay, small_config(9)).unwrap();
    study.config.grace_ticks += 1;
    assert!(matches!(
        evaluate_recorded_shaky_hand_study(&initial, &solution, &study),
        Err(ShakyHandError::ConfigIdentityMismatch { .. })
    ));

    let other_initial = Simulation::new(exit_room("shaky-other", 160));
    let other_solution = witness(&other_initial, SearchTarget::exit("goal"), run_right);
    let study = record_shaky_hand_study(&solution.replay, small_config(9)).unwrap();
    assert!(matches!(
        evaluate_recorded_shaky_hand_study(&other_initial, &other_solution, &study),
        Err(ShakyHandError::ReplayFingerprintMismatch { .. })
    ));
}

#[test]
fn evaluator_rejects_a_noncanonical_or_divergent_zero_noise_control() {
    let initial = Simulation::new(exit_room("shaky-canonical", 120));
    let solution = witness(&initial, SearchTarget::exit("goal"), run_right);

    let mut trailing = solution.clone();
    trailing.replay = Replay::record(
        &initial,
        solution
            .replay
            .actions()
            .chain(std::iter::once(Action::default())),
    );
    assert!(matches!(
        evaluate_shaky_hand(&initial, &trailing, small_config(10)),
        Err(ShakyHandError::ExactReplayHasTrailingFrames { .. })
    ));

    let mut divergent = solution;
    divergent.replay.frames[0].expected_digest = downwards_core::StateDigest(0);
    assert!(matches!(
        evaluate_shaky_hand(&initial, &divergent, small_config(10)),
        Err(ShakyHandError::ReplayDiverged(_))
    ));
}

fn fragile_hazard_room(phase_ticks: u32) -> Room {
    exit_room("shaky-fragile", 150)
        .with_objects(
            vec![TimedHazard::new(Rect::new(82, 136, 16, 24), 60, 30, phase_ticks).unwrap()],
            vec![],
        )
        .unwrap()
}

fn find_fragile_witness() -> (Simulation, TargetSolution) {
    // Scan for the last safe phase: the exact replay clears the hazard, while
    // delaying the whole crossing by one tick meets activation and dies. The
    // scan keeps this calibrated to the actual movement policy instead of a
    // hard-coded phase.
    for phase in 0..60 {
        let initial = Simulation::new(fragile_hazard_room(phase));
        let mut exact = initial.clone();
        let mut exact_clean = true;
        for tick in 0..240 {
            let report = exact.step(run_right(tick));
            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
            {
                exact_clean = false;
                break;
            }
            if exact.reached_exit() == Some("goal") {
                break;
            }
        }
        if !exact_clean || exact.reached_exit() != Some("goal") {
            continue;
        }
        let mut delayed = initial.clone();
        let mut delayed_died = false;
        delayed.step(Action::default());
        for tick in 0..240 {
            let report = delayed.step(run_right(tick));
            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
            {
                delayed_died = true;
                break;
            }
            if delayed.reached_exit() == Some("goal") {
                break;
            }
        }
        if delayed_died {
            let solution = witness(&initial, SearchTarget::exit("goal"), run_right);
            return (initial, solution);
        }
    }
    panic!("no phase makes the uninterrupted crossing one-tick fragile");
}

#[test]
fn robust_route_dominates_a_phase_fragile_route_under_the_same_noise_policy() {
    let config = ShakyHandConfig {
        seed: 0xfeed_beef,
        trials_per_curve_point: 128,
        grace_ticks: 12,
        correlated_boundaries: 2,
        convergence_confirmation_ticks: 2,
    };
    let robust_initial = Simulation::new(exit_room("shaky-robust", 150));
    let robust_solution = witness(&robust_initial, SearchTarget::exit("goal"), |tick| {
        if tick < 12 {
            Action::default()
        } else {
            run_right(tick)
        }
    });
    let (fragile_initial, fragile_solution) = find_fragile_witness();

    let robust = evaluate_shaky_hand(&robust_initial, &robust_solution, config).unwrap();
    let fragile = evaluate_shaky_hand(&fragile_initial, &fragile_solution, config).unwrap();
    let noisy_totals = |report: &downwards_ai::ShakyHandReport| {
        report
            .curves
            .iter()
            .filter(|curve| curve.family != NoiseFamily::Exact)
            .fold((0, 0, 0_u64), |(trials, successes, deaths), curve| {
                (
                    trials + curve.trials,
                    successes + curve.successes,
                    deaths + curve.death_events,
                )
            })
    };
    let robust_totals = noisy_totals(&robust);
    let fragile_totals = noisy_totals(&fragile);
    assert_eq!(robust_totals.0, fragile_totals.0);
    assert!(
        robust_totals.1 > fragile_totals.1,
        "robust {robust_totals:?}, fragile {fragile_totals:?}"
    );
    assert!(
        robust_totals.2 < fragile_totals.2,
        "robust {robust_totals:?}, fragile {fragile_totals:?}"
    );
    assert!(fragile.first_failure.is_some());
    assert!(fragile.curves.iter().any(|curve| {
        curve.death_events != 0
            || curve
                .trials_detail
                .iter()
                .any(|trial| matches!(trial.outcome, NoisyReplayOutcome::Timeout { .. }))
    }));
}
