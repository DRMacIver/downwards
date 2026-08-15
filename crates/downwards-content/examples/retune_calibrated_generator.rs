//! Deterministically solve and simplify the generated calibration playtest set.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example retune_calibrated_generator`

use std::{env, fmt::Write as _, fs, path::PathBuf};

use downwards_ai::{
    DirectProbeAuditStatus, SearchTarget, SolverConfig, TargetSolveOutcome,
    audit_direct_controller_probes, solve_target,
};
use downwards_core::{
    AbilitySet, Action, JumpKind, PLAYER_MOVEMENT_POLICY_VERSION, Simulation, SimulationEvent,
    WallSide,
};
use downwards_gen::{
    CALIBRATED_WALL_JUMP_ABILITIES, CALIBRATED_WALL_JUMP_GENERATION_VERSION,
    CALIBRATED_WALL_JUMP_TARGET, CalibratedWallJumpKey,
};

const DEFAULT_OUTPUT: &str =
    "crates/downwards-content/generated/calibrated-wall-jump-witnesses-v1.txt";
const PLAYTEST_SEEDS: std::ops::Range<u64> = 0..12;
type ActionKey = (i8, i8, bool, bool, bool);
type RouteKey = (bool, usize, usize, usize, usize, Vec<ActionKey>);

#[derive(Debug)]
struct Observation {
    physically_clean: bool,
    clean: bool,
    reached_tick: usize,
    jump_presses: usize,
    accepted_jumps: usize,
    wall_sides: Vec<WallSide>,
    action_spans: usize,
}

fn observe(initial: &Simulation, actions: &[Action]) -> Observation {
    let mut simulation = initial.clone();
    let mut previous = Action::default();
    let mut jump_presses = 0;
    let mut accepted_jumps = 0;
    let mut wall_sides = Vec::new();
    let mut invalid = false;
    let mut reached_tick = 0;

    for (index, &action) in actions.iter().enumerate() {
        jump_presses += usize::from(action.jump && !previous.jump);
        invalid |= action.dash || action.restart;
        for event in simulation.step(action).events {
            match event {
                SimulationEvent::Jumped(kind) => {
                    accepted_jumps += 1;
                    if let JumpKind::Wall { side } = kind {
                        wall_sides.push(side);
                    }
                }
                SimulationEvent::Died(_)
                | SimulationEvent::Reset
                | SimulationEvent::Dashed { .. } => invalid = true,
                SimulationEvent::ExitReached { ref id }
                    if id == CALIBRATED_WALL_JUMP_TARGET && reached_tick == 0 =>
                {
                    reached_tick = index + 1;
                }
                SimulationEvent::Landed
                | SimulationEvent::PickupCollected { .. }
                | SimulationEvent::ExitReached { .. } => {}
            }
        }
        previous = action;
        if reached_tick > 0 {
            break;
        }
    }

    let physically_clean = !invalid
        && reached_tick == actions.len()
        && simulation.reached_exit() == Some(CALIBRATED_WALL_JUMP_TARGET);
    Observation {
        physically_clean,
        clean: physically_clean && jump_presses == accepted_jumps,
        reached_tick,
        jump_presses,
        accepted_jumps,
        wall_sides,
        action_spans: actions.windows(2).filter(|pair| pair[0] != pair[1]).count()
            + usize::from(!actions.is_empty()),
    }
}

fn clean_completion(initial: &Simulation, actions: &[Action]) -> bool {
    observe(initial, actions).physically_clean
}

fn greedily_simplify(initial: &Simulation, original: &[Action]) -> Vec<Action> {
    let reached = observe(initial, original).reached_tick;
    assert!(reached > 0, "candidate does not reach the generated target");
    let mut actions = original[..reached].to_vec();

    loop {
        let before = actions.clone();
        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if clean_completion(initial, &candidate) {
                    actions = candidate;
                } else {
                    start += chunk;
                }
            }
            chunk /= 2;
        }

        let mut jump_start = 0;
        while jump_start < actions.len() {
            if !actions[jump_start].jump {
                jump_start += 1;
                continue;
            }
            let mut jump_end = jump_start + 1;
            while jump_end < actions.len() && actions[jump_end].jump {
                jump_end += 1;
            }
            let mut candidate = actions.clone();
            for action in &mut candidate[jump_start..jump_end] {
                action.jump = false;
            }
            if clean_completion(initial, &candidate) {
                actions = candidate;
            }
            jump_start = jump_end;
        }

        for index in (0..actions.len()).rev() {
            for clear in [
                |action: &mut Action| action.jump = false,
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index] && clean_completion(initial, &candidate) {
                    actions = candidate;
                }
            }
        }

        if actions == before {
            return actions;
        }
    }
}

fn action_key(action: Action) -> ActionKey {
    (
        action.move_x,
        action.move_y,
        action.jump,
        action.dash,
        action.restart,
    )
}

fn route_key(initial: &Simulation, actions: &[Action]) -> RouteKey {
    let observation = observe(initial, actions);
    let repeated_wall_sides = observation
        .wall_sides
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .count();
    (
        !observation.clean || !(3..=7).contains(&observation.wall_sides.len()),
        repeated_wall_sides,
        observation.jump_presses,
        observation.action_spans,
        actions.len(),
        actions.iter().copied().map(action_key).collect(),
    )
}

fn push_rle(output: &mut String, actions: &[Action]) {
    let mut start = 0;
    while start < actions.len() {
        let action = actions[start];
        let mut end = start + 1;
        while end < actions.len() && actions[end] == action {
            end += 1;
        }
        let (move_x, move_y, jump, dash, restart) = action_key(action);
        writeln!(
            output,
            "span {} {} {} {} {} {}",
            end - start,
            move_x,
            move_y,
            u8::from(jump),
            u8::from(dash),
            u8::from(restart)
        )
        .expect("writing to a string cannot fail");
        start = end;
    }
}

fn main() {
    let output_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT));
    let mut artifact = String::new();
    writeln!(artifact, "calibrated-wall-jump-witnesses-v1").unwrap();
    writeln!(
        artifact,
        "generation-version {CALIBRATED_WALL_JUMP_GENERATION_VERSION}"
    )
    .unwrap();
    writeln!(
        artifact,
        "movement-policy-version {PLAYER_MOVEMENT_POLICY_VERSION}"
    )
    .unwrap();

    for seed in PLAYTEST_SEEDS {
        let candidate = CalibratedWallJumpKey::new(seed).generate();
        let mut initial =
            Simulation::with_abilities(candidate.room.clone(), CALIBRATED_WALL_JUMP_ABILITIES);
        initial.enable_current_player_movement();
        let target = SearchTarget::exit(CALIBRATED_WALL_JUMP_TARGET);

        let mut config = SolverConfig::for_abilities(CALIBRATED_WALL_JUMP_ABILITIES);
        config.max_ticks_per_path = 1_200;
        config.max_expanded_nodes = 300_000;
        config.max_simulated_ticks = 10_000_000;
        let direct_audit =
            audit_direct_controller_probes(&initial, std::slice::from_ref(&target), &config)
                .expect("WallJump direct audit must run");
        assert_eq!(direct_audit.status, DirectProbeAuditStatus::Complete);
        let solved = solve_target(&initial, target.clone(), &config)
            .unwrap_or_else(|error| panic!("seed {seed} solve failed: {error}"));
        let TargetSolveOutcome::Solved(solution) = solved else {
            panic!("seed {seed} was not solved: {solved:?}");
        };
        let mut route_candidates = direct_audit
            .witnesses
            .iter()
            .map(|witness| witness.replay.actions().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        route_candidates.push(solution.replay.actions().collect());
        let mut route_candidates = route_candidates
            .iter()
            .map(|raw| greedily_simplify(&initial, raw))
            .collect::<Vec<_>>();
        route_candidates.sort_by_key(|actions| route_key(&initial, actions));
        route_candidates.dedup();
        let actions = route_candidates
            .into_iter()
            .find(|actions| {
                let key = route_key(&initial, actions);
                !key.0 && key.1 <= 1
            })
            .unwrap_or_else(|| panic!("seed {seed} has no calibrated-quality witness"));
        let observation = observe(&initial, &actions);
        assert!(observation.clean, "seed {seed}: {observation:?}");
        assert!(
            (3..=7).contains(&observation.wall_sides.len()),
            "seed {seed} escaped the calibrated wall-jump range: {observation:?}"
        );
        let repeated_wall_sides = observation
            .wall_sides
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .count();
        assert!(
            repeated_wall_sides <= 1,
            "seed {seed} retained same-wall hopping: {observation:?}"
        );

        let mut baseline = Simulation::with_abilities(candidate.room.clone(), AbilitySet::NONE);
        baseline.enable_current_player_movement();
        let baseline_config = SolverConfig::for_abilities(AbilitySet::NONE);
        let baseline_solve = solve_target(&baseline, target.clone(), &baseline_config)
            .expect("baseline bounded solver must run");
        let TargetSolveOutcome::Inconclusive {
            reason: baseline_reason,
            stats: baseline_stats,
        } = baseline_solve
        else {
            panic!("seed {seed} has a bounded baseline solver bypass: {baseline_solve:?}");
        };
        let baseline_audit = audit_direct_controller_probes(
            &baseline,
            std::slice::from_ref(&target),
            &baseline_config,
        )
        .expect("baseline direct audit must run");
        assert_eq!(baseline_audit.status, DirectProbeAuditStatus::Complete);
        assert!(
            baseline_audit.witnesses.is_empty(),
            "seed {seed} has a baseline direct-controller bypass"
        );

        writeln!(artifact, "seed {seed}").unwrap();
        writeln!(artifact, "room-id {}", candidate.room.id()).unwrap();
        writeln!(artifact, "course {}", candidate.parameters.course.slug()).unwrap();
        writeln!(artifact, "reflected {}", candidate.parameters.reflected).unwrap();
        writeln!(artifact, "ticks {}", actions.len()).unwrap();
        writeln!(artifact, "jump-presses {}", observation.jump_presses).unwrap();
        writeln!(artifact, "accepted-jumps {}", observation.accepted_jumps).unwrap();
        writeln!(artifact, "wall-jumps {}", observation.wall_sides.len()).unwrap();
        writeln!(artifact, "repeated-wall-sides {repeated_wall_sides}").unwrap();
        writeln!(artifact, "action-spans {}", observation.action_spans).unwrap();
        writeln!(
            artifact,
            "direct-positives {}",
            direct_audit.witnesses.len()
        )
        .unwrap();
        writeln!(artifact, "baseline-status {baseline_reason:?}").unwrap();
        writeln!(
            artifact,
            "baseline-search {} {}",
            baseline_stats.expanded_nodes, baseline_stats.simulated_ticks
        )
        .unwrap();
        push_rle(&mut artifact, &actions);
        writeln!(artifact, "end").unwrap();

        eprintln!(
            "seed {seed:02}: {:?}, {} ticks, {} jumps / {} wall, {} spans, {} direct positives; baseline {:?} {}n/{}t",
            candidate.parameters.course,
            actions.len(),
            observation.jump_presses,
            observation.wall_sides.len(),
            observation.action_spans,
            direct_audit.witnesses.len(),
            baseline_reason,
            baseline_stats.expanded_nodes,
            baseline_stats.simulated_ticks,
        );
    }

    fs::write(&output_path, artifact)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", output_path.display()));
    eprintln!("wrote {}", output_path.display());
}
