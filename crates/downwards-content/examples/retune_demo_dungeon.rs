//! Deterministically regenerate one exact representative route for every authored dungeon floor.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example retune_demo_dungeon`

use std::{cmp::Ordering, collections::BTreeMap, env, fs, path::PathBuf};

use downwards_ai::{
    GroundedStandingRegion, GroundedSupportSolveOutcome, GroundedSupportTarget, NoiseFamily,
    ReachedTarget, Replay, SearchStats, SearchTarget, ShakyHandConfig, SolverConfig,
    TargetSolution, TargetSolveOutcome, audit_direct_controller_probes, evaluate_shaky_hand,
    solve_grounded_support, solve_target,
};
use downwards_content::{
    DemoDungeonRoom, DemoDungeonRouteSpec, DemoDungeonRouteTarget, demo_dungeon_room,
    demo_dungeon_route_specs,
};
use downwards_core::{
    AbilitySet, Action, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION, Simulation, SimulationEvent,
};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;

const DEFAULT_OUTPUT: &str = "crates/downwards-content/generated/demo-dungeon-witnesses-v1.txt";
const PREVIOUS_WITNESS_ARTIFACT: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");
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

type StaticRouteKey = (usize, usize, usize, usize, Vec<(i8, i8, bool, bool, bool)>);

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

fn clean_completion(
    initial: &Simulation,
    target: DemoDungeonRouteTarget,
    actions: &[Action],
) -> bool {
    let mut simulation = initial.clone();
    for (index, &action) in actions.iter().enumerate() {
        let report = simulation.step(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset))
        {
            return false;
        }
        if target_reached(&simulation, target) {
            return index + 1 == actions.len();
        }
    }
    false
}

fn static_route_key(actions: &[Action]) -> StaticRouteKey {
    let mut previous = Action::default();
    let mut previous_nonzero_x = 0;
    let mut presses = 0;
    let mut reversals = 0;
    for &action in actions {
        presses += usize::from(action.jump && !previous.jump);
        presses += usize::from(action.dash && !previous.dash);
        if action.move_x != 0 {
            reversals +=
                usize::from(previous_nonzero_x != 0 && previous_nonzero_x != action.move_x);
            previous_nonzero_x = action.move_x;
        }
        previous = action;
    }
    (
        actions.windows(2).filter(|pair| pair[0] != pair[1]).count()
            + usize::from(!actions.is_empty()),
        presses,
        reversals,
        actions.len(),
        actions.iter().copied().map(action_key).collect(),
    )
}

/// Delete redundant time and input components while preserving an exact clean completion.
///
/// This is intentionally authoritative resimulation rather than a metric-based rewrite. Besides
/// producing a more useful demonstration, it exposes geometry that admits a much easier route
/// than the beam solver's first positive.
fn greedily_simplify(
    initial: &Simulation,
    target: DemoDungeonRouteTarget,
    original: &[Action],
) -> Vec<Action> {
    let mut actions = original.to_vec();
    loop {
        let before = actions.clone();
        let mut chunk = actions.len().next_power_of_two() / 2;
        while chunk > 0 {
            let mut start = 0;
            while start + chunk <= actions.len() {
                let mut candidate = actions.clone();
                candidate.drain(start..start + chunk);
                if clean_completion(initial, target, &candidate)
                    && static_route_key(&candidate) < static_route_key(&actions)
                {
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
                |action: &mut Action| action.dash = false,
                |action: &mut Action| action.move_y = 0,
                |action: &mut Action| action.move_x = 0,
            ] {
                let mut candidate = actions.clone();
                clear(&mut candidate[index]);
                if candidate[index] != actions[index]
                    && clean_completion(initial, target, &candidate)
                    && static_route_key(&candidate) < static_route_key(&actions)
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

fn action_key(action: Action) -> (i8, i8, bool, bool, bool) {
    (
        action.move_x,
        action.move_y,
        action.jump,
        action.dash,
        action.restart,
    )
}

fn compare_routes(
    initial: &Simulation,
    spec: DemoDungeonRouteSpec,
    left: &[Action],
    right: &[Action],
) -> Ordering {
    let left_observation = observe(initial, spec, left);
    let right_observation = observe(initial, spec, right);
    let key = |observation: RouteObservation, actions: &[Action]| {
        (
            observation.action_spans,
            observation.horizontal_reversals,
            observation
                .jump_presses
                .saturating_sub(observation.accepted_jumps),
            observation.accepted_jumps + observation.accepted_dashes,
            observation.ticks,
            actions.iter().copied().map(action_key).collect::<Vec<_>>(),
        )
    };
    key(left_observation, left).cmp(&key(right_observation, right))
}

fn segmented_candidate(
    initial: &Simulation,
    spec: DemoDungeonRouteSpec,
    target: &SearchTarget,
) -> Option<TargetSolution> {
    let waypoint = match spec.room {
        DemoDungeonRoom::VoidPass => GroundedSupportTarget::new(
            170,
            240,
            30,
            [GroundedStandingRegion::new(170, 232).expect("valid authored standing range")],
        )
        .expect("valid authored support waypoint"),
        _ => return None,
    };
    let wall_only_config = SolverConfig::for_abilities(AbilitySet::new(true, false));
    let GroundedSupportSolveOutcome::Solved(first) =
        solve_grounded_support(initial, &waypoint, &wall_only_config).ok()?
    else {
        return None;
    };
    let mut intermediate = initial.clone();
    let mut actions = first.replay.actions().collect::<Vec<_>>();
    for &action in &actions {
        intermediate.step(action);
    }
    let direct = audit_direct_controller_probes(
        &intermediate,
        std::slice::from_ref(target),
        &wall_only_config,
    )
    .ok()?;
    let second = if let Some(witness) = direct
        .witnesses
        .into_iter()
        .min_by_key(|witness| witness.replay.frames.len())
    {
        TargetSolution {
            target: witness.target,
            reached: witness.reached,
            replay: witness.replay,
            stats: witness.stats_at_first_discovery,
        }
    } else {
        let TargetSolveOutcome::Solved(second) =
            solve_target(&intermediate, target.clone(), &wall_only_config).ok()?
        else {
            return None;
        };
        second
    };
    actions.extend(second.replay.actions());
    Some(TargetSolution {
        target: second.target,
        reached: second.reached,
        replay: Replay::record(initial, actions),
        stats: second.stats,
    })
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

fn parse_previous_witness_actions(input: &str) -> BTreeMap<String, Vec<Action>> {
    let mut routes = BTreeMap::new();
    let mut current_id = None::<String>;
    let mut actions = Vec::new();
    for (line_index, line) in input.lines().enumerate() {
        let mut fields = line.split_ascii_whitespace();
        match fields.next() {
            Some("route") => {
                assert!(
                    current_id.is_none(),
                    "nested previous route at line {}",
                    line_index + 1
                );
                current_id = Some(
                    fields
                        .next()
                        .unwrap_or_else(|| {
                            panic!("missing previous route id at line {}", line_index + 1)
                        })
                        .to_owned(),
                );
                actions.clear();
            }
            Some("span") => {
                let parse = |field: Option<&str>, name: &str| {
                    field
                        .unwrap_or_else(|| panic!("missing {name} at line {}", line_index + 1))
                        .parse::<i64>()
                        .unwrap_or_else(|error| {
                            panic!("invalid {name} at line {}: {error}", line_index + 1)
                        })
                };
                let action = Action {
                    move_x: i8::try_from(parse(fields.next(), "move_x"))
                        .expect("stored move_x fits i8"),
                    move_y: i8::try_from(parse(fields.next(), "move_y"))
                        .expect("stored move_y fits i8"),
                    jump: parse(fields.next(), "jump") != 0,
                    dash: parse(fields.next(), "dash") != 0,
                    restart: parse(fields.next(), "restart") != 0,
                };
                let ticks = usize::try_from(parse(fields.next(), "span ticks"))
                    .expect("stored span ticks fit usize");
                actions.extend(std::iter::repeat_n(action, ticks));
            }
            Some("end") => {
                let id = current_id.take().unwrap_or_else(|| {
                    panic!("previous end outside route at line {}", line_index + 1)
                });
                assert!(
                    routes.insert(id, actions.clone()).is_none(),
                    "duplicate previous route"
                );
            }
            _ => {}
        }
    }
    assert!(
        current_id.is_none(),
        "previous artifact ends inside a route"
    );
    routes
}

fn reached_target(spec: DemoDungeonRouteSpec) -> ReachedTarget {
    match spec.target {
        DemoDungeonRouteTarget::Door(id) => ReachedTarget::Door(id.to_owned()),
        DemoDungeonRouteTarget::GoalExit => ReachedTarget::Exit(spec.target.id().to_owned()),
        DemoDungeonRouteTarget::Pickup(id) => ReachedTarget::Pickup(id.to_owned()),
    }
}

fn main() {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let selected_route = if arguments.first().is_some_and(|arg| arg == "--route") {
        let route = arguments
            .get(1)
            .and_then(|arg| arg.to_str())
            .unwrap_or_else(|| panic!("usage: retune_demo_dungeon --route <room-id>"));
        assert_eq!(
            arguments.len(),
            2,
            "usage: retune_demo_dungeon --route <room-id>"
        );
        Some(route)
    } else {
        None
    };
    let argument = arguments
        .first()
        .filter(|_| selected_route.is_none())
        .cloned();
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
    let previous_actions = parse_previous_witness_actions(PREVIOUS_WITNESS_ARTIFACT);

    let mut generated_routes = 0;
    for (index, spec) in demo_dungeon_route_specs()
        .into_iter()
        .enumerate()
        .filter(|(_, spec)| selected_route.is_none_or(|route| spec.id() == route))
    {
        generated_routes += 1;
        let initial = initial_simulation(spec);
        assert_eq!(initial.movement_tuning(), Some(tuning));
        let target = search_target(spec);
        let solver_config = SolverConfig::for_abilities(spec.inventory.abilities());
        let outcome = solve_target(&initial, target.clone(), &solver_config)
            .unwrap_or_else(|error| panic!("{} solve failed: {error}", spec.id()));
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("{} has no exact generated witness: {outcome:?}", spec.id());
        };
        let direct =
            audit_direct_controller_probes(&initial, std::slice::from_ref(&target), &solver_config)
                .unwrap_or_else(|error| panic!("{} direct audit failed: {error}", spec.id()));
        let mut candidates = vec![(solution, true)];
        candidates
            .extend(segmented_candidate(&initial, spec, &target).map(|solution| (solution, true)));
        candidates.extend(direct.witnesses.into_iter().map(|witness| {
            (
                TargetSolution {
                    target: witness.target,
                    reached: witness.reached,
                    replay: witness.replay,
                    stats: witness.stats_at_first_discovery,
                },
                true,
            )
        }));
        if let Some(actions) = previous_actions.get(spec.id())
            && clean_completion(&initial, spec.target, actions)
        {
            candidates.push((
                TargetSolution {
                    target: target.clone(),
                    reached: reached_target(spec),
                    replay: Replay::record(&initial, actions.iter().copied()),
                    stats: SearchStats::default(),
                },
                false,
            ));
        }
        let mut simplified = candidates
            .into_iter()
            .map(|(mut solution, should_simplify)| {
                let found_actions = solution.replay.actions().collect::<Vec<_>>();
                let actions = if should_simplify {
                    greedily_simplify(&initial, spec.target, &found_actions)
                } else {
                    found_actions
                };
                solution.replay = Replay::record(&initial, actions.iter().copied());
                (solution, actions)
            })
            .collect::<Vec<_>>();
        simplified.sort_by(|(_, left), (_, right)| compare_routes(&initial, spec, left, right));
        simplified.dedup_by(|(_, left), (_, right)| left == right);
        let mut assessed = Vec::with_capacity(simplified.len());
        for (candidate_index, (solution, actions)) in simplified.into_iter().enumerate() {
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
            let fragile = shaky.curves.iter().any(|curve| {
                curve.family != NoiseFamily::Exact
                    && curve.strength_ticks == 1
                    && curve.trials > 0
                    && curve.successes == 0
            });
            if selected_route.is_some() {
                eprintln!(
                    "  candidate {:>2}: {:>3}t {:>2}sp {:>2}j {:>2}wj {:>2}d {:>2}rev{}",
                    candidate_index + 1,
                    observation.ticks,
                    observation.action_spans,
                    observation.accepted_jumps,
                    observation.accepted_wall_jumps,
                    observation.accepted_dashes,
                    observation.horizontal_reversals,
                    if fragile { " FRAGILE" } else { "" },
                );
            }
            assessed.push((fragile, solution, actions, shaky));
            if !fragile {
                break;
            }
        }
        let selected_index = assessed
            .iter()
            .position(|(fragile, _, _, _)| !fragile)
            .unwrap_or(0);
        let (_, _solution, actions, shaky) = assessed.remove(selected_index);
        let observation = observe(&initial, spec, &actions);
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

    if let Some(route) = selected_route {
        assert_eq!(generated_routes, 1, "unknown demo dungeon route {route:?}");
        print!("{rendered}");
    } else if check_only {
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
