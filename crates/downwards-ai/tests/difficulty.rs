use downwards_ai::{
    ComplexityBand, DifficultyAnalysis, DifficultyCase, DifficultyConfig, DifficultyError,
    DifficultyInterpretation, Replay, SearchStats, Solution, SolveOutcome, analyze_batch,
    analyze_solution, analyze_solve_outcome,
};
use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

const WIDTH: usize = 32;
const HEIGHT: usize = 18;

fn flat_room(exit_x: i32) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "difficulty-flat",
        "Difficulty test room",
        WIDTH as u16,
        HEIGHT as u16,
        10,
        tiles,
        Point::new(20, 148),
        vec![Exit {
            id: "east".into(),
            bounds: Rect::new(exit_x, 136, 12, 24),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn solved(initial: &Simulation, actions: Vec<Action>, stats: SearchStats) -> Solution {
    let replay = Replay::record(initial, actions);
    assert_eq!(
        replay.verify(initial).unwrap().reached_exit.as_deref(),
        Some("east"),
        "test fixture must genuinely solve its room"
    );
    Solution {
        exit_id: "east".into(),
        replay,
        stats,
    }
}

fn minimal_run_right_solution(initial: &Simulation) -> Solution {
    let action = Action {
        move_x: 1,
        ..Action::default()
    };
    let mut simulation = initial.clone();
    let mut actions = Vec::new();
    while simulation.reached_exit().is_none() {
        assert!(actions.len() < 300, "flat test room should be reachable");
        actions.push(action);
        simulation.step(action);
    }
    solved(initial, actions, SearchStats::default())
}

#[test]
fn easy_replay_reports_input_counts_search_effort_and_robust_timing() {
    let initial = Simulation::with_abilities(flat_room(180), AbilitySet::ALL);
    let actions = (0..140)
        .map(|tick| Action {
            move_x: 1,
            jump: (10..16).contains(&tick),
            dash: (30..32).contains(&tick),
            ..Action::default()
        })
        .collect();
    let stats = SearchStats {
        expanded_nodes: 17,
        generated_nodes: 41,
        simulated_ticks: 203,
        deepest_path_ticks: 140,
    };
    let solution = solved(&initial, actions, stats);

    let report = analyze_solution(&initial, &solution, &DifficultyConfig::default()).unwrap();
    assert_eq!(
        report.interpretation,
        DifficultyInterpretation::HeuristicNotHumanDifficulty
    );
    assert!(report.completion_ticks < solution.replay.frames.len());
    assert_eq!(report.meaningful_input_transitions, 5);
    assert_eq!(report.jump_presses, 1);
    assert_eq!(report.dash_presses, 1);
    assert_eq!(report.successful_jumps, 1);
    assert_eq!(report.successful_wall_jumps, 0);
    assert_eq!(report.successful_dashes, 1);
    assert_eq!(report.search_effort, stats);
    assert_eq!(report.deaths, 0);
    assert!(
        report
            .temporal_robustness
            .successful_perturbation_ratio
            .is_some_and(|ratio| ratio > 0.8)
    );
    assert!(report.temporal_robustness.earliest_divergence.is_some());
}

#[test]
fn provisional_complexity_orders_robust_timing_below_fragile_timing() {
    let initial = Simulation::new(flat_room(100));
    let solution = minimal_run_right_solution(&initial);
    let robust = analyze_solution(&initial, &solution, &DifficultyConfig::default()).unwrap();
    let fragile = analyze_solution(
        &initial,
        &solution,
        &DifficultyConfig {
            perturbation_grace_ticks: 0,
        },
    )
    .unwrap();

    assert!(
        robust.provisional_complexity.components.temporal_fragility
            < fragile.provisional_complexity.components.temporal_fragility
    );
    assert!(
        robust.provisional_complexity.component_score
            < fragile.provisional_complexity.component_score
    );
    assert!(
        robust.provisional_complexity.band < fragile.provisional_complexity.band,
        "expected {:?} to order below {:?}",
        robust.provisional_complexity,
        fragile.provisional_complexity
    );
    assert_eq!(robust.provisional_complexity.band, ComplexityBand::Gentle);
}

#[test]
fn replay_that_only_just_reaches_the_exit_is_reported_as_fragile() {
    let initial = Simulation::new(flat_room(100));
    let solution = minimal_run_right_solution(&initial);
    let config = DifficultyConfig {
        perturbation_grace_ticks: 0,
    };

    let report = analyze_solution(&initial, &solution, &config).unwrap();
    let robustness = report.temporal_robustness;
    assert_eq!(robustness.attempted_perturbations, 2);
    assert_eq!(robustness.not_applicable_perturbations, 2);
    assert_eq!(robustness.successful_perturbations, 0);
    assert_eq!(robustness.successful_perturbation_ratio, Some(0.0));
    let failure = robustness.earliest_failure.unwrap();
    assert!(failure.first_divergence_tick.is_some());
    assert_eq!(failure.deaths, 0);
}

#[test]
fn difficulty_analysis_is_deterministic_down_to_trial_diagnostics() {
    let initial = Simulation::new(flat_room(90));
    let solution = minimal_run_right_solution(&initial);
    let config = DifficultyConfig::default();

    let first = analyze_solution(&initial, &solution, &config).unwrap();
    let second = analyze_solution(&initial, &solution, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn inconclusive_and_invalid_solutions_never_receive_a_difficulty_score() {
    let initial = Simulation::new(flat_room(280));
    let stats = SearchStats {
        expanded_nodes: 12,
        generated_nodes: 20,
        simulated_ticks: 80,
        deepest_path_ticks: 16,
    };
    let inconclusive = SolveOutcome::Inconclusive {
        reason: downwards_ai::InconclusiveReason::ExpandedNodeBudget,
        stats,
    };
    assert_eq!(
        analyze_solve_outcome(&initial, &inconclusive, &DifficultyConfig::default()).unwrap(),
        DifficultyAnalysis::Inconclusive {
            reason: downwards_ai::InconclusiveReason::ExpandedNodeBudget,
            search_effort: stats,
        }
    );

    let invalid = Solution {
        exit_id: "east".into(),
        replay: Replay::record(&initial, [Action::default(); 2]),
        stats,
    };
    assert!(matches!(
        analyze_solution(&initial, &invalid, &DifficultyConfig::default()),
        Err(DifficultyError::SolvedReplayDidNotReachExpectedExit { .. })
    ));
}

#[test]
fn batch_analysis_preserves_order_ids_and_per_case_inconclusive_results() {
    let solved_initial = Simulation::new(flat_room(90));
    let solution = minimal_run_right_solution(&solved_initial);
    let solved_outcome = SolveOutcome::Solved(solution);
    let inconclusive_initial = Simulation::new(flat_room(280));
    let inconclusive_outcome = SolveOutcome::Inconclusive {
        reason: downwards_ai::InconclusiveReason::PathHorizon,
        stats: SearchStats::default(),
    };

    let results = analyze_batch(
        [
            DifficultyCase {
                id: "solved-seed-7",
                initial: &solved_initial,
                outcome: &solved_outcome,
            },
            DifficultyCase {
                id: "inconclusive-seed-8",
                initial: &inconclusive_initial,
                outcome: &inconclusive_outcome,
            },
        ],
        &DifficultyConfig::default(),
    );

    assert_eq!(results[0].id, "solved-seed-7");
    assert!(matches!(
        results[0].analysis,
        Ok(DifficultyAnalysis::Solved(_))
    ));
    assert_eq!(results[1].id, "inconclusive-seed-8");
    assert!(matches!(
        results[1].analysis,
        Ok(DifficultyAnalysis::Inconclusive { .. })
    ));
}
