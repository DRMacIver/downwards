//! Deterministically regenerate the authored gallery's player-movement witness artifact.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example retune_gallery`

use std::{cmp::Ordering, collections::BTreeMap, env, fs, path::PathBuf};

use downwards_ai::{SearchTarget, SolverConfig, TargetSolveOutcome, solve_target};
use downwards_content::{CalibrationLevel, calibration_gallery};
use downwards_core::{
    Action, JumpKind, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION, Simulation, SimulationEvent,
};

const DEFAULT_OUTPUT: &str = "crates/downwards-content/generated/calibration-witnesses-v2.txt";

#[derive(Clone, Debug)]
struct Observation {
    physically_clean: bool,
    clean: bool,
    reached_tick: usize,
    jump_presses: usize,
    accepted_jumps: usize,
    wall_jumps: usize,
    repeated_wall_sides: usize,
    action_spans: usize,
}

fn observe(initial: &Simulation, target: &str, actions: &[Action]) -> Observation {
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
                | SimulationEvent::Dashed { .. } => {
                    invalid = true;
                }
                SimulationEvent::ExitReached { ref id } if id == target => {
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
        && reached_tick > 0
        && reached_tick == actions.len()
        && simulation.reached_exit() == Some(target);
    Observation {
        physically_clean,
        clean: physically_clean && jump_presses == accepted_jumps,
        reached_tick,
        jump_presses,
        accepted_jumps,
        wall_jumps: wall_sides.len(),
        repeated_wall_sides: wall_sides
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .count(),
        action_spans: actions.windows(2).filter(|pair| pair[0] != pair[1]).count()
            + usize::from(!actions.is_empty()),
    }
}

fn clean_completion(initial: &Simulation, target: &str, actions: &[Action]) -> bool {
    observe(initial, target, actions).physically_clean
}

fn greedily_simplify(initial: &Simulation, target: &str, original: &[Action]) -> Vec<Action> {
    let reached = observe(initial, target, original).reached_tick;
    assert!(reached > 0, "candidate does not reach {target:?}");
    let mut actions = original[..reached].to_vec();

    loop {
        let before = actions.clone();
        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if clean_completion(initial, target, &candidate) {
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
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index]
                    && clean_completion(initial, target, &candidate)
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

fn minimum_wall_jumps(level_id: &str) -> usize {
    match level_id {
        "cal-01" => 3,
        "cal-02" => 5,
        "cal-03" => 2,
        "cal-04" => 4,
        "cal-05" => 3,
        "cal-06" => 2,
        "cal-07" => 3,
        "cal-08" => 5,
        "cal-09" => 4,
        "cal-10" | "cal-11" | "cal-12" => 5,
        "cal-13" => 0,
        "cal-14" | "cal-15" => 3,
        _ => panic!("unregistered calibration ID {level_id:?}"),
    }
}

fn compare_candidates(level: CalibrationLevel, left: &[Action], right: &[Action]) -> Ordering {
    let initial = level.scenario();
    let left_observation = observe(&initial, level.target(), left);
    let right_observation = observe(&initial, level.target(), right);
    let minimum_walls = minimum_wall_jumps(level.id());
    let left_key = (
        left_observation.wall_jumps < minimum_walls,
        left_observation.repeated_wall_sides,
        left_observation.jump_presses,
        left_observation.action_spans,
        left.len(),
        left.iter().copied().map(action_key).collect::<Vec<_>>(),
    );
    let right_key = (
        right_observation.wall_jumps < minimum_walls,
        right_observation.repeated_wall_sides,
        right_observation.jump_presses,
        right_observation.action_spans,
        right.len(),
        right.iter().copied().map(action_key).collect::<Vec<_>>(),
    );
    left_key.cmp(&right_key)
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

fn select_witness(
    level: CalibrationLevel,
    previous: Option<&[Action]>,
) -> (Vec<Action>, Observation) {
    let initial = level.scenario();
    assert_eq!(
        initial.movement_tuning(),
        Some(MovementTuning::GAMEPLAY_DEFAULT),
        "{} scenario is outside the promoted movement policy",
        level.id()
    );

    let mut candidates = Vec::new();
    if let Some(previous) = previous
        && observe(&initial, level.target(), previous).reached_tick > 0
    {
        candidates.push(greedily_simplify(&initial, level.target(), previous));
    }

    let mut config = SolverConfig::for_abilities(level.abilities());
    config.max_ticks_per_path = 1_200;
    config.max_expanded_nodes = 250_000;
    config.max_simulated_ticks = 8_000_000;
    let outcome = solve_target(&initial, SearchTarget::exit(level.target()), &config)
        .unwrap_or_else(|error| panic!("{} solve failed: {error}", level.id()));
    if let TargetSolveOutcome::Solved(solution) = outcome {
        let found = solution.replay.actions().collect::<Vec<_>>();
        candidates.push(greedily_simplify(&initial, level.target(), &found));
    }

    candidates.sort_by(|left, right| compare_candidates(level, left, right));
    candidates.dedup();
    let selected = candidates
        .into_iter()
        .find(|candidate| {
            let observation = observe(&initial, level.target(), candidate);
            observation.clean && observation.wall_jumps >= minimum_wall_jumps(level.id())
        })
        .unwrap_or_else(|| panic!("{} has no clean policy-compatible witness", level.id()));
    let observation = observe(&initial, level.target(), &selected);
    (selected, observation)
}

fn parse_previous_artifact() -> BTreeMap<String, Vec<Action>> {
    let Ok(artifact) = fs::read_to_string(DEFAULT_OUTPUT) else {
        return BTreeMap::new();
    };
    let mut parsed = BTreeMap::new();
    let mut current: Option<(String, Vec<Action>)> = None;
    for line in artifact.lines().skip(3) {
        if let Some(id) = line.strip_prefix("level ") {
            assert!(current.is_none(), "nested generated witness level");
            current = Some((id.to_owned(), Vec::new()));
        } else if line == "end" {
            let (id, actions) = current.take().expect("end without generated level");
            assert!(!actions.is_empty(), "empty generated witness for {id}");
            assert!(parsed.insert(id, actions).is_none());
        } else if let Some(span) = line.strip_prefix("span ") {
            let (_, actions) = current.as_mut().expect("span without generated level");
            let fields = span.split_ascii_whitespace().collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "malformed generated span");
            let action = Action {
                move_x: fields[0].parse().expect("invalid generated move_x"),
                move_y: fields[1].parse().expect("invalid generated move_y"),
                jump: fields[2] == "1",
                dash: fields[3] == "1",
                restart: fields[4] == "1",
            };
            let ticks = fields[5].parse().expect("invalid generated span ticks");
            actions.extend(std::iter::repeat_n(action, ticks));
        } else {
            panic!("unknown generated witness record {line:?}");
        }
    }
    assert!(current.is_none(), "unterminated generated witness level");
    parsed
}

fn render_span(action: Action, ticks: usize) -> String {
    format!(
        "span {} {} {} {} {} {ticks}\n",
        action.move_x,
        action.move_y,
        u8::from(action.jump),
        u8::from(action.dash),
        u8::from(action.restart)
    )
}

fn main() {
    let argument = env::args_os().nth(1);
    let check_only = argument.as_deref() == Some(std::ffi::OsStr::new("--check"));
    let output = if check_only {
        PathBuf::from(DEFAULT_OUTPUT)
    } else {
        argument
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT))
    };
    let tuning = MovementTuning::GAMEPLAY_DEFAULT;
    let previous = parse_previous_artifact();
    let mut rendered = format!(
        "schema downwards-calibration-witnesses-v2\nplayer-movement-policy {PLAYER_MOVEMENT_POLICY_VERSION}\ntuning {} {} {} {} {} {}\n",
        tuning.top_speed_pixels_per_second,
        tuning.acceleration_milliseconds,
        tuning.braking_milliseconds,
        tuning.wall_ascent_carry_percent,
        tuning.wall_carry_percent,
        tuning.wall_momentum_milliseconds
    );

    for level in calibration_gallery().iter().copied() {
        let (actions, observation) =
            select_witness(level, previous.get(level.id()).map(Vec::as_slice));
        eprintln!(
            "{} {:<18} {:>3} ticks / {:>2} jumps / {:>2} walls / {} same-wall",
            level.id(),
            level.title(),
            actions.len(),
            observation.accepted_jumps,
            observation.wall_jumps,
            observation.repeated_wall_sides
        );
        rendered.push_str(&format!("level {}\n", level.id()));
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

    if check_only {
        let existing = fs::read_to_string(&output).expect("read generated witness artifact");
        assert_eq!(
            existing, rendered,
            "generated witness artifact is stale; run `cargo run -p downwards-content --example retune_gallery`"
        );
        eprintln!("{} is current", output.display());
    } else {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).expect("create generated witness directory");
        }
        fs::write(&output, rendered).expect("write generated witness artifact");
        eprintln!("wrote {}", output.display());
    }
}
