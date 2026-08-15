//! Deterministically regenerate one exact representative route for every authored dungeon floor.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example retune_demo_dungeon`

use std::{env, fs, path::PathBuf};

use downwards_ai::{
    NoiseFamily, SearchTarget, ShakyHandConfig, SolverConfig, TargetSolveOutcome,
    evaluate_shaky_hand, solve_target,
};
use downwards_content::{
    DemoDungeonRouteSpec, DemoDungeonRouteTarget, demo_dungeon_room, demo_dungeon_route_specs,
};
use downwards_core::{
    Action, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION, Simulation, SimulationEvent,
};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;

const DEFAULT_OUTPUT: &str = "crates/downwards-content/generated/demo-dungeon-witnesses-v1.txt";
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
    horizontal_reversals: usize,
}

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
                    observation.accepted_wall_jumps +=
                        usize::from(matches!(kind, downwards_core::JumpKind::Wall { .. }));
                }
                SimulationEvent::Dashed { .. } => observation.accepted_dashes += 1,
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

    for (index, spec) in demo_dungeon_route_specs().into_iter().enumerate() {
        let initial = initial_simulation(spec);
        assert_eq!(initial.movement_tuning(), Some(tuning));
        let outcome = solve_target(
            &initial,
            search_target(spec),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap_or_else(|error| panic!("{} solve failed: {error}", spec.id()));
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("{} has no exact generated witness: {outcome:?}", spec.id());
        };
        let actions = solution.replay.actions().collect::<Vec<_>>();
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

    if check_only {
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
