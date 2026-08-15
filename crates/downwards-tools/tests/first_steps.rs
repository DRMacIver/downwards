use downwards_ai::{SolveOutcome, SolverConfig, solve};
use downwards_content::first_steps_room;
use downwards_core::Simulation;

#[test]
fn first_steps_has_a_verified_solver_witness() {
    let initial = Simulation::new(first_steps_room());
    let SolveOutcome::Solved(solution) = solve(&initial, &SolverConfig::default()).unwrap() else {
        panic!("default solver budget should find the development room exit");
    };

    let verification = solution
        .replay
        .verify(&initial)
        .expect("solver witness must replay exactly");
    assert_eq!(solution.exit_id, "right");
    assert_eq!(verification.reached_exit.as_deref(), Some("right"));
}
