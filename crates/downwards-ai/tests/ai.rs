use downwards_ai::{
    BatchTargetSolveError, DifficultyConfig, EventDigest, InconclusiveReason, ReachedTarget,
    Replay, ReplayDivergence, SearchTarget, SolveOutcome, SolverConfig, TargetSolveError,
    TargetSolveOutcome, analyze_solution, digest_events, solve, solve_target, solve_targets,
};
use downwards_core::{
    AbilitySet, Action, BoundarySide, DASH_TICKS, DashDirection, DeathReason, Door, Exit, JumpKind,
    ONE_WAY_DROP_TICKS, Pickup, Point, Rect, Room, Simulation, SimulationEvent, StateDigest, Tile,
    WallSide,
};
const WIDTH: usize = 32;
const HEIGHT: usize = 18;

fn toy_room(exit_x: i32) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "toy",
        "Toy room",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(20, 148),
        vec![Exit {
            id: "east".into(),
            bounds: Rect::new(exit_x, 140, 10, 20),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn pickup_room() -> Room {
    toy_room(280)
        .with_objects(
            vec![],
            vec![Pickup::new("coin", Rect::new(90, 148, 6, 6)).unwrap()],
        )
        .unwrap()
}

fn door_room() -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "doors",
        "Door room",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(150, 148),
        vec![],
    )
    .unwrap()
    .with_doors(vec![
        Door {
            id: "west".into(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 140, 4, 20),
            arrival: Point::new(30, 148),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east".into(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 140, 4, 20),
            arrival: Point::new(250, 148),
            destination_room: None,
            destination_door: None,
        },
    ])
    .unwrap()
}

fn multi_target_room() -> Room {
    door_room()
        .with_objects(
            vec![],
            vec![Pickup::new("coin", Rect::new(90, 148, 6, 6)).unwrap()],
        )
        .unwrap()
}

fn floor_hatch_room() -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        tiles[y * WIDTH + 11] = Tile::Solid;
        tiles[y * WIDTH + 21] = Tile::Solid;
    }
    for x in 12..21 {
        tiles[14 * WIDTH + x] = Tile::OneWay;
    }
    Room::new(
        "floor-hatch",
        "Floor hatch",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(150, 128),
        vec![],
    )
    .unwrap()
    .with_doors(vec![Door {
        id: "down".into(),
        side: BoundarySide::Floor,
        trigger_bounds: Rect::new(120, 176, 90, 4),
        arrival: Point::new(150, 128),
        destination_room: None,
        destination_door: None,
    }])
    .unwrap()
}

fn high_exit_room() -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "dash-required",
        "Dash required",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(20, 148),
        vec![Exit {
            id: "high".into(),
            bounds: Rect::new(16, 88, 20, 16),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn wall_jump_room() -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    for y in 0..16 {
        tiles[y * WIDTH + 10] = Tile::Solid;
    }
    Room::new(
        "wall-jump-required",
        "Wall jump required",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(110, 120),
        vec![Exit {
            id: "high".into(),
            bounds: Rect::new(110, 75, 60, 25),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn retry_only_room() -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    tiles[16 * WIDTH + 4] = Tile::HazardUp;
    Room::new(
        "retry-only",
        "Retry-only false solution",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(20, 148),
        vec![Exit {
            id: "west".into(),
            bounds: Rect::new(0, 140, 10, 20),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn ability_test_config(abilities: AbilitySet) -> SolverConfig {
    SolverConfig {
        max_expanded_nodes: 2_000,
        max_simulated_ticks: 100_000,
        max_ticks_per_path: 100,
        beam_width: 48,
        ..SolverConfig::for_abilities(abilities)
    }
}

fn actions() -> Vec<Action> {
    (0..80)
        .map(|tick| Action {
            move_x: if tick < 50 { 1 } else { -1 },
            jump: (10..18).contains(&tick),
            restart: false,
            ..Action::default()
        })
        .collect()
}

#[test]
fn recorded_replay_verifies_exactly() {
    let initial = Simulation::new(toy_room(280));
    let replay = Replay::record(&initial, actions());
    let verified = replay.verify(&initial).unwrap();
    assert_eq!(verified.frames_verified, 80);
    assert_eq!(verified.final_tick, 80);
    assert_eq!(
        verified.final_digest,
        replay.frames.last().unwrap().expected_digest
    );
}

#[test]
fn replay_reports_the_first_divergent_frame() {
    let initial = Simulation::new(toy_room(280));
    let mut replay = Replay::record(&initial, actions());
    replay.frames[7].expected_digest = StateDigest(0);
    replay.frames[12].expected_digest = StateDigest(1);

    let error = replay.verify(&initial).unwrap_err();
    assert!(matches!(
        error,
        ReplayDivergence::Frame { frame_index: 7, .. }
    ));
    assert!(error.to_string().contains("frame 7"));
}

#[test]
fn replay_detects_transient_event_tampering_when_state_digest_matches() {
    let initial = Simulation::new(toy_room(280));
    let restart = Action {
        restart: true,
        ..Action::default()
    };
    let mut replay = Replay::record(&initial, [restart]);

    let mut independently_stepped = initial.clone();
    let report = independently_stepped.step(restart);
    assert_eq!(report.digest, replay.frames[0].expected_digest);
    assert_eq!(report.events, vec![SimulationEvent::Reset]);

    replay.frames[0].expected_event_digest = digest_events(&[]);
    let error = replay.verify(&initial).unwrap_err();
    assert!(matches!(
        &error,
        ReplayDivergence::EventStream {
            frame_index: 0,
            actual_events,
            ..
        } if actual_events == &[SimulationEvent::Reset]
    ));
    assert!(error.to_string().contains("event stream"));
    assert!(error.to_string().contains("Reset"));
}

#[test]
fn event_digest_encoding_has_a_stable_exhaustive_golden_value() {
    let events = [
        SimulationEvent::Jumped(JumpKind::Grounded),
        SimulationEvent::Jumped(JumpKind::Coyote),
        SimulationEvent::Jumped(JumpKind::Buffered),
        SimulationEvent::Jumped(JumpKind::Wall {
            side: WallSide::Left,
        }),
        SimulationEvent::Jumped(JumpKind::Wall {
            side: WallSide::Right,
        }),
        SimulationEvent::Dashed {
            direction: DashDirection::Up,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::UpRight,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::Right,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::DownRight,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::Down,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::DownLeft,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::Left,
        },
        SimulationEvent::Dashed {
            direction: DashDirection::UpLeft,
        },
        SimulationEvent::Landed,
        SimulationEvent::Died(DeathReason::Hazard {
            tile_x: 0x1234,
            tile_y: 0xabcd,
        }),
        SimulationEvent::Died(DeathReason::TimedHazard {
            hazard_index: 0x0123_4567,
        }),
        SimulationEvent::PickupCollected {
            id: "moon-key".to_owned(),
        },
        SimulationEvent::Reset,
        SimulationEvent::ExitReached {
            id: "deep-exit".to_owned(),
        },
    ];

    assert_eq!(digest_events(&events), EventDigest(0xf026_9251_19ba_c642));
    let mut reordered = events.clone();
    reordered.swap(0, 1);
    assert_ne!(digest_events(&events), digest_events(&reordered));
}

#[test]
fn solver_returns_a_verified_witness_for_a_toy_room() {
    let initial = Simulation::new(toy_room(90));
    let config = SolverConfig {
        max_expanded_nodes: 2_000,
        max_simulated_ticks: 40_000,
        max_ticks_per_path: 160,
        beam_width: 128,
        ..SolverConfig::default()
    };
    let SolveOutcome::Solved(solution) = solve(&initial, &config).unwrap() else {
        panic!("toy room should be solved within the configured budget");
    };
    assert_eq!(solution.exit_id, "east");
    let verified = solution.replay.verify(&initial).unwrap();
    assert_eq!(verified.reached_exit.as_deref(), Some("east"));
    assert!(!solution.replay.frames.is_empty());
}

#[test]
fn targeted_pickup_search_returns_a_pickup_witness_not_an_exit_claim() {
    let initial = Simulation::new(pickup_room());
    let outcome = solve_target(
        &initial,
        SearchTarget::pickup("coin"),
        &SolverConfig::default(),
    )
    .unwrap();
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("nearby pickup should be reached: {outcome:?}");
    };

    assert_eq!(solution.target, SearchTarget::pickup("coin"));
    assert_eq!(solution.reached, ReachedTarget::Pickup("coin".to_owned()));
    let verified = solution.replay.verify(&initial).unwrap();
    assert_eq!(verified.collected_pickup_ids, ["coin"]);
    assert_eq!(verified.reached_exit, None);
}

#[test]
fn door_targets_and_any_exit_distinguish_doors_from_legacy_exits() {
    let initial = Simulation::enter_via_door(door_room(), AbilitySet::NONE, "west").unwrap();
    let outcome = solve_target(
        &initial,
        SearchTarget::door("east"),
        &SolverConfig::default(),
    )
    .unwrap();
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("nearby door should be reached: {outcome:?}");
    };
    assert_eq!(solution.target, SearchTarget::door("east"));
    assert_eq!(solution.reached, ReachedTarget::Door("east".to_owned()));
    assert_eq!(
        solution.replay.verify(&initial).unwrap().reached_exit,
        Some("east".to_owned())
    );

    let SolveOutcome::Solved(any_solution) = solve(&initial, &SolverConfig::default()).unwrap()
    else {
        panic!("AnyExit should include doors");
    };
    assert_eq!(any_solution.exit_id, "west");
}

#[test]
fn shared_target_search_matches_individual_searches_and_verifies_every_replay() {
    let initial =
        Simulation::enter_via_door(multi_target_room(), AbilitySet::NONE, "west").unwrap();
    let targets = vec![
        SearchTarget::door("east"),
        SearchTarget::pickup("coin"),
        SearchTarget::door("west"),
    ];
    let config = SolverConfig {
        probe_direct_routes: false,
        ..SolverConfig::default()
    };

    let batch = solve_targets(&initial, &targets, &config).unwrap();
    assert_eq!(batch.results.len(), targets.len());
    assert_eq!(
        batch
            .results
            .iter()
            .map(|result| &result.target)
            .collect::<Vec<_>>(),
        targets.iter().collect::<Vec<_>>()
    );

    let mut individual_ticks = 0_usize;
    for (result, target) in batch.results.iter().zip(&targets) {
        let individual = solve_target(&initial, target.clone(), &config).unwrap();
        let TargetSolveOutcome::Solved(batch_solution) = &result.outcome else {
            panic!(
                "shared search did not solve {target:?}: {:?}",
                result.outcome
            );
        };
        let TargetSolveOutcome::Solved(individual_solution) = individual else {
            panic!("individual search did not solve {target:?}: {individual:?}");
        };
        individual_ticks += individual_solution.stats.simulated_ticks;
        assert_eq!(batch_solution.reached, individual_solution.reached);

        let verified = batch_solution.replay.verify(&initial).unwrap();
        match target {
            SearchTarget::Door(id) => {
                assert_eq!(verified.reached_exit.as_deref(), Some(id.as_str()));
            }
            SearchTarget::Pickup(id) => {
                assert!(verified.collected_pickup_ids.contains(id));
            }
            SearchTarget::AnyExit | SearchTarget::Exit(_) => unreachable!(),
        }
    }

    // The eastbound branch collects the pickup before freezing at the east
    // door, while a separate westbound branch remains available for west.
    let east_stats = match &batch.results[0].outcome {
        TargetSolveOutcome::Solved(solution) => solution.stats,
        TargetSolveOutcome::Inconclusive { .. } => unreachable!(),
    };
    let pickup_stats = match &batch.results[1].outcome {
        TargetSolveOutcome::Solved(solution) => solution.stats,
        TargetSolveOutcome::Inconclusive { .. } => unreachable!(),
    };
    assert!(pickup_stats.simulated_ticks <= east_stats.simulated_ticks);
    assert!(batch.stats.simulated_ticks < individual_ticks);
}

#[test]
fn shared_target_search_is_deterministic_and_preserves_duplicate_request_order() {
    let initial = Simulation::new(multi_target_room());
    let targets = vec![
        SearchTarget::pickup("coin"),
        SearchTarget::door("west"),
        SearchTarget::pickup("coin"),
        SearchTarget::door("east"),
    ];

    let first = solve_targets(&initial, &targets, &SolverConfig::default()).unwrap();
    let second = solve_targets(&initial, &targets, &SolverConfig::default()).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first
            .results
            .iter()
            .map(|result| result.target.clone())
            .collect::<Vec<_>>(),
        targets
    );
    assert_eq!(first.results[0].outcome, first.results[2].outcome);
}

#[test]
fn shared_target_search_reports_bounded_non_success_per_target() {
    let initial = Simulation::new(multi_target_room());
    let targets = vec![SearchTarget::door("east"), SearchTarget::pickup("coin")];
    let config = SolverConfig {
        max_expanded_nodes: 0,
        ..SolverConfig::default()
    };

    let batch = solve_targets(&initial, &targets, &config).unwrap();
    assert_eq!(batch.stats, Default::default());
    for (result, target) in batch.results.iter().zip(targets) {
        assert_eq!(result.target, target);
        assert_eq!(
            result.outcome,
            TargetSolveOutcome::Inconclusive {
                reason: InconclusiveReason::ExpandedNodeBudget,
                stats: Default::default(),
            }
        );
    }
}

#[test]
fn shared_target_search_indexes_invalid_targets_before_exploring() {
    let initial = Simulation::new(multi_target_room());
    let targets = vec![
        SearchTarget::pickup("coin"),
        SearchTarget::door("missing"),
        SearchTarget::door("east"),
    ];

    assert_eq!(
        solve_targets(&initial, &targets, &SolverConfig::default()).unwrap_err(),
        BatchTargetSolveError::InvalidTarget {
            index: 1,
            target: SearchTarget::door("missing"),
            error: TargetSolveError::DoorNotDefined {
                door_id: "missing".to_owned(),
            },
        }
    );
}

#[test]
fn targeted_search_rejects_undefined_and_empty_ids_before_search() {
    let initial = Simulation::new(pickup_room());
    assert_eq!(
        solve_target(
            &initial,
            SearchTarget::pickup("missing"),
            &SolverConfig::default(),
        )
        .unwrap_err(),
        TargetSolveError::PickupNotDefined {
            pickup_id: "missing".to_owned(),
        }
    );
    assert_eq!(
        solve_target(&initial, SearchTarget::exit(""), &SolverConfig::default(),).unwrap_err(),
        TargetSolveError::EmptyExitId
    );

    let door_initial = Simulation::new(door_room());
    assert_eq!(
        solve_target(
            &door_initial,
            SearchTarget::door("missing"),
            &SolverConfig::default(),
        )
        .unwrap_err(),
        TargetSolveError::DoorNotDefined {
            door_id: "missing".to_owned(),
        }
    );
    assert_eq!(
        solve_target(
            &door_initial,
            SearchTarget::door(" "),
            &SolverConfig::default(),
        )
        .unwrap_err(),
        TargetSolveError::EmptyDoorId
    );
}

#[test]
fn budget_exhaustion_is_explicitly_inconclusive() {
    let initial = Simulation::new(toy_room(280));
    let config = SolverConfig {
        max_expanded_nodes: 0,
        ..SolverConfig::default()
    };
    assert_eq!(
        solve(&initial, &config).unwrap(),
        SolveOutcome::Inconclusive {
            reason: InconclusiveReason::ExpandedNodeBudget,
            stats: Default::default(),
        }
    );
}

#[test]
fn solver_discards_a_candidate_on_its_first_death() {
    let right = Action {
        move_x: 1,
        ..Action::default()
    };
    let left = Action {
        move_x: -1,
        ..Action::default()
    };
    let mut retry_sequence = vec![right; 20];
    retry_sequence.extend([left; 30]);

    // The full input stream can reach the exit only because hazard death
    // resets it before the leftward portion. That is not a valid room route.
    let mut permissive_replay = Simulation::new(retry_only_room());
    for &action in &retry_sequence {
        permissive_replay.step(action);
    }
    assert!(permissive_replay.deaths() > 0);
    assert_eq!(permissive_replay.reached_exit(), Some("west"));

    let initial = Simulation::new(retry_only_room());
    let config = SolverConfig {
        max_expanded_nodes: 100,
        max_simulated_ticks: 1_000,
        max_ticks_per_path: 100,
        beam_width: 8,
        probe_direct_routes: false,
        macros: vec![downwards_ai::ActionMacro {
            name: "die-then-recover".into(),
            actions: retry_sequence,
        }],
        ..SolverConfig::default()
    };
    assert!(matches!(
        solve(&initial, &config).unwrap(),
        SolveOutcome::Inconclusive { .. }
    ));
}

#[test]
fn solver_vocabulary_is_loadout_specific_and_dash_macros_have_exact_edges() {
    let baseline = SolverConfig::for_abilities(AbilitySet::NONE);
    let wall_jump = SolverConfig::for_abilities(AbilitySet::new(true, false));
    let dash = SolverConfig::for_abilities(AbilitySet::new(false, true));

    assert_eq!(SolverConfig::default(), baseline);
    assert_eq!(baseline.macros, wall_jump.macros);
    assert!(
        baseline
            .macros
            .iter()
            .all(|action_macro| !action_macro.actions.iter().any(|action| action.dash))
    );
    let drop_macros = baseline
        .macros
        .iter()
        .filter(|action_macro| action_macro.name.starts_with("drop"))
        .collect::<Vec<_>>();
    assert_eq!(drop_macros.len(), 3);
    for action_macro in drop_macros {
        assert_eq!(
            action_macro.actions.len(),
            usize::from(ONE_WAY_DROP_TICKS) + 1
        );
        assert!(action_macro.actions[0].jump);
        assert_eq!(action_macro.actions[0].move_y, 1);
        assert!(
            action_macro.actions[1..]
                .iter()
                .all(|action| { action.move_y == 1 && !action.jump && !action.dash })
        );
    }
    let dash_macros: Vec<_> = dash
        .macros
        .iter()
        .filter(|action_macro| action_macro.name.starts_with("dash-"))
        .collect();
    assert_eq!(dash_macros.len(), 8);
    for action_macro in dash_macros {
        assert_eq!(action_macro.actions.len(), usize::from(DASH_TICKS));
        assert!(action_macro.actions[0].dash);
        assert!(action_macro.actions[1..].iter().all(|action| !action.dash));
    }
}

#[test]
fn exact_floor_door_search_uses_drop_through_press_and_release() {
    let initial = Simulation::new(floor_hatch_room());
    let direct_outcome = solve_target(
        &initial,
        SearchTarget::door("down"),
        &SolverConfig::default(),
    )
    .unwrap();
    let TargetSolveOutcome::Solved(direct_solution) = direct_outcome else {
        panic!("floor-aware direct probe should reach the hatch: {direct_outcome:?}");
    };
    assert!(direct_solution.stats.simulated_ticks < 100);

    // Disable probes to prove that the authoritative beam vocabulary itself
    // contains the edge-triggered press/release sequence.
    let config = SolverConfig {
        probe_direct_routes: false,
        max_expanded_nodes: 5_000,
        max_simulated_ticks: 100_000,
        max_ticks_per_path: 100,
        beam_width: 64,
        ..SolverConfig::default()
    };
    let outcome = solve_target(&initial, SearchTarget::door("down"), &config).unwrap();
    let TargetSolveOutcome::Solved(solution) = outcome else {
        panic!("drop-through floor door should be reachable: {outcome:?}");
    };
    assert_eq!(solution.reached, ReachedTarget::Door("down".to_owned()));
    let actions = solution.replay.actions().collect::<Vec<_>>();
    assert!(actions.windows(2).any(|window| {
        window[0].move_y == 1 && window[0].jump && window[1].move_y == 1 && !window[1].jump
    }));
    assert_eq!(
        solution
            .replay
            .verify(&initial)
            .unwrap()
            .reached_exit
            .as_deref(),
        Some("down")
    );
}

#[test]
fn full_width_static_room_stays_within_batch_friendly_search_effort() {
    let initial = Simulation::new(toy_room(280));
    let SolveOutcome::Solved(solution) = solve(&initial, &SolverConfig::default()).unwrap() else {
        panic!("full-width flat room should remain a baseline solve");
    };
    assert!(
        solution.stats.simulated_ticks < 1_000,
        "simple static solve took {:?}",
        solution.stats
    );
    assert_eq!(
        solution
            .replay
            .verify(&initial)
            .unwrap()
            .reached_exit
            .as_deref(),
        Some("east")
    );
}

#[test]
fn dash_required_fixture_solves_only_with_dash_loadout_and_vocabulary() {
    let matching_initial =
        Simulation::with_abilities(high_exit_room(), AbilitySet::new(false, true));
    let matching_config = ability_test_config(AbilitySet::new(false, true));
    let SolveOutcome::Solved(solution) = solve(&matching_initial, &matching_config).unwrap() else {
        panic!("upward dash should reach the high exit");
    };
    assert_eq!(solution.exit_id, "high");
    assert!(solution.replay.actions().any(|action| action.dash));
    assert_eq!(
        solution
            .replay
            .verify(&matching_initial)
            .unwrap()
            .reached_exit
            .as_deref(),
        Some("high")
    );
    let report =
        analyze_solution(&matching_initial, &solution, &DifficultyConfig::default()).unwrap();
    assert!(report.successful_dashes >= 1);

    let no_dash_vocabulary = ability_test_config(AbilitySet::NONE);
    assert!(matches!(
        solve(&matching_initial, &no_dash_vocabulary).unwrap(),
        SolveOutcome::Inconclusive { .. }
    ));
    let no_dash_initial = Simulation::new(high_exit_room());
    assert!(matches!(
        solve(&no_dash_initial, &matching_config).unwrap(),
        SolveOutcome::Inconclusive { .. }
    ));
}

#[test]
fn wall_jump_required_fixture_uses_baseline_jump_macros_only_when_enabled() {
    let abilities = AbilitySet::new(true, false);
    let matching_initial = Simulation::with_abilities(wall_jump_room(), abilities);
    let config = ability_test_config(abilities);
    let SolveOutcome::Solved(solution) = solve(&matching_initial, &config).unwrap() else {
        panic!("wall jump should reach the high exit");
    };
    assert_eq!(solution.exit_id, "high");
    assert!(solution.replay.actions().any(|action| action.jump));
    assert_eq!(
        solution
            .replay
            .verify(&matching_initial)
            .unwrap()
            .reached_exit
            .as_deref(),
        Some("high")
    );
    let report =
        analyze_solution(&matching_initial, &solution, &DifficultyConfig::default()).unwrap();
    assert!(report.successful_jumps >= 1);
    assert!(report.successful_wall_jumps >= 1);

    let disabled_initial = Simulation::new(wall_jump_room());
    assert!(matches!(
        solve(&disabled_initial, &config).unwrap(),
        SolveOutcome::Inconclusive { .. }
    ));
}

#[test]
fn authored_baseline_route_survives_superset_ability_branching() {
    let room = downwards_content::first_steps_room();
    let tuned_config = |abilities| SolverConfig {
        max_expanded_nodes: 10_000,
        max_simulated_ticks: 600_000,
        max_ticks_per_path: 400,
        beam_width: 48,
        ..SolverConfig::for_abilities(abilities)
    };

    let baseline_initial = Simulation::new(room.clone());
    let baseline_outcome = solve(&baseline_initial, &tuned_config(AbilitySet::NONE)).unwrap();
    let SolveOutcome::Solved(baseline) = baseline_outcome else {
        panic!("authored baseline fixture must remain solvable: {baseline_outcome:?}");
    };
    baseline.replay.verify(&baseline_initial).unwrap();

    let all_initial = Simulation::with_abilities(room, AbilitySet::ALL);
    let superset_outcome = solve(&all_initial, &tuned_config(AbilitySet::ALL)).unwrap();
    let SolveOutcome::Solved(superset) = superset_outcome else {
        panic!("dash branches must not displace an existing baseline route: {superset_outcome:?}");
    };
    superset.replay.verify(&all_initial).unwrap();
    assert!(
        !superset.replay.actions().any(|action| action.dash),
        "the staged baseline fallback should retain a no-dash witness"
    );
}
