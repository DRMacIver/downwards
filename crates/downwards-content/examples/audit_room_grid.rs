//! Standalone auditor for a prototype room that is not yet registered in the
//! demo dungeon: verifies an ASCII grid plus a small spec file with the real
//! simulation and solver, so new rooms can be iterated before integration.
//!
//! Usage:
//! `cargo run --release -p downwards-content --example audit_room_grid -- <grid.txt> <spec.txt> [--shaky]`
//!
//! Spec file format (one directive per line, `#` comments allowed):
//! ```text
//! door west            # sides: west east ceiling floor
//! door east
//! coin 210 60          # coin at pixel x y (8x10 rect)
//! hazard 52 0 52 170 96 70 45   # x y w h period active_ticks phase
//! ```
//!
//! For every ability loadout (none/wall/dash/both) it solves every ordered
//! door pair (including self-pairs: bounced-off-a-gate retreats) and every
//! (entry door, coin) pair with the banked-coin success criterion. `--shaky`
//! additionally reports worst strength-one shaky-hand survival for each coin
//! route. Output lines are machine-readable: `pair`, `coin`, `shaky`,
//! `aperture-error`, ending with `verdict ok` or `verdict FAIL <n> problems`.

use std::{fs, path::PathBuf, process::ExitCode};

use downwards_ai::{
    NoiseFamily, SearchTarget, ShakyHandConfig, SolverConfig, TargetSolveOutcome,
    evaluate_shaky_hand, solve_target,
};
use downwards_core::{
    AbilitySet, BoundarySide, Door, Pickup, Point, Rect, Room, Simulation, Tile, TimedHazard,
};
use downwards_gen::parse_room_grid;

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

struct Spec {
    doors: Vec<(&'static str, BoundarySide)>,
    coins: Vec<(i32, i32)>,
    hazards: Vec<TimedHazard>,
}

fn parse_spec(source: &str) -> Spec {
    let mut spec = Spec {
        doors: Vec::new(),
        coins: Vec::new(),
        hazards: Vec::new(),
    };
    for (line_number, raw) in source.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let keyword = parts.next().expect("non-empty line has a keyword");
        let numbers = |parts: std::str::SplitWhitespace<'_>, expected: usize| -> Vec<i32> {
            let values = parts
                .map(|part| {
                    part.parse::<i32>().unwrap_or_else(|_| {
                        panic!("spec line {}: bad number {part:?}", line_number + 1)
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(
                values.len(),
                expected,
                "spec line {}: expected {expected} numbers",
                line_number + 1
            );
            values
        };
        match keyword {
            "door" => {
                let side = match parts.next() {
                    Some("west") => ("west", BoundarySide::Left),
                    Some("east") => ("east", BoundarySide::Right),
                    Some("ceiling") => ("ceiling", BoundarySide::Ceiling),
                    Some("floor") => ("floor", BoundarySide::Floor),
                    other => panic!("spec line {}: unknown door {other:?}", line_number + 1),
                };
                spec.doors.push(side);
            }
            "coin" => {
                let values = numbers(parts, 2);
                spec.coins.push((values[0], values[1]));
            }
            "hazard" => {
                let values = numbers(parts, 7);
                spec.hazards.push(
                    TimedHazard::new(
                        Rect::new(values[0], values[1], values[2], values[3]),
                        u32::try_from(values[4]).expect("period fits u32"),
                        u32::try_from(values[5]).expect("active window fits u32"),
                        u32::try_from(values[6]).expect("phase fits u32"),
                    )
                    .unwrap_or_else(|error| {
                        panic!("spec line {}: bad hazard: {error:?}", line_number + 1)
                    }),
                );
            }
            other => panic!("spec line {}: unknown directive {other:?}", line_number + 1),
        }
    }
    assert!(!spec.doors.is_empty(), "spec declares no doors");
    spec
}

fn door_geometry(side: BoundarySide) -> (Rect, Point) {
    match side {
        BoundarySide::Left => (Rect::new(0, 130, 8, 40), Point::new(12, 148)),
        BoundarySide::Right => (Rect::new(312, 130, 8, 40), Point::new(300, 148)),
        BoundarySide::Ceiling => (Rect::new(140, 0, 40, 8), Point::new(150, 12)),
        BoundarySide::Floor => (Rect::new(140, 172, 40, 8), Point::new(150, 148)),
    }
}

/// Boundary tiles that must be open for a door mouth, as (column, row) pairs.
fn aperture_tiles(side: BoundarySide) -> Vec<(u16, u16)> {
    match side {
        BoundarySide::Left => (13..=16).map(|row| (0, row)).collect(),
        BoundarySide::Right => (13..=16).map(|row| (WIDTH - 1, row)).collect(),
        BoundarySide::Ceiling => (14..=17).map(|column| (column, 0)).collect(),
        BoundarySide::Floor => (14..=17).map(|column| (column, HEIGHT - 1)).collect(),
    }
}

fn check_apertures(tiles: &[Tile], spec: &Spec) -> Vec<String> {
    let mut problems = Vec::new();
    let tile_at =
        |column: u16, row: u16| tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)];
    for side in [
        BoundarySide::Left,
        BoundarySide::Right,
        BoundarySide::Ceiling,
        BoundarySide::Floor,
    ] {
        let declared = spec.doors.iter().any(|&(_, declared)| declared == side);
        let open_tiles = aperture_tiles(side)
            .into_iter()
            .filter(|&(column, row)| tile_at(column, row) == Tile::Empty)
            .count();
        if declared && open_tiles < aperture_tiles(side).len() {
            problems.push(format!(
                "aperture-error {side:?} declared but its boundary mouth is not fully open ({open_tiles}/4 tiles empty)"
            ));
        }
        if !declared && open_tiles > 0 {
            problems.push(format!(
                "aperture-error {side:?} not declared but the boundary has {open_tiles} open mouth tiles"
            ));
        }
    }
    problems
}

fn build_room(tiles: Vec<Tile>, spec: &Spec) -> Room {
    let doors = spec
        .doors
        .iter()
        .map(|&(id, side)| {
            let (trigger_bounds, arrival) = door_geometry(side);
            Door {
                id: id.to_owned(),
                side,
                trigger_bounds,
                arrival,
                destination_room: None,
                destination_door: None,
            }
        })
        .collect::<Vec<_>>();
    let pickups = spec
        .coins
        .iter()
        .enumerate()
        .map(|(index, &(x, y))| {
            Pickup::new(format!("coin-{index}"), Rect::new(x, y, 8, 10))
                .expect("coin geometry is valid")
        })
        .collect::<Vec<_>>();
    Room::new(
        "prototype-room",
        "Prototype room",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(20, 148),
        vec![],
    )
    .expect("grid builds a valid room")
    .with_objects(spec.hazards.clone(), pickups)
    .expect("objects fit the room")
    .with_doors(doors)
    .expect("declared doors are valid")
}

fn loadouts() -> [(&'static str, AbilitySet); 4] {
    [
        ("none", AbilitySet::new(false, false)),
        ("wall", AbilitySet::new(true, false)),
        ("dash", AbilitySet::new(false, true)),
        ("both", AbilitySet::new(true, true)),
    ]
}

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let shaky = arguments
        .iter()
        .any(|argument| argument.to_str() == Some("--shaky"));
    let paths: Vec<PathBuf> = arguments
        .iter()
        .filter(|argument| argument.to_str() != Some("--shaky"))
        .map(PathBuf::from)
        .collect();
    let [grid_path, spec_path] = paths.as_slice() else {
        eprintln!("usage: audit_room_grid <grid.txt> <spec.txt> [--shaky]");
        return ExitCode::FAILURE;
    };
    let grid_source = fs::read_to_string(grid_path).expect("grid file readable");
    let spec_source = fs::read_to_string(spec_path).expect("spec file readable");
    let tiles = parse_room_grid(&grid_source);
    let spec = parse_spec(&spec_source);

    let mut problems = check_apertures(&tiles, &spec);
    for problem in &problems {
        println!("{problem}");
    }
    let room = build_room(tiles, &spec);

    for (loadout_name, abilities) in loadouts() {
        let config = SolverConfig::for_abilities(abilities);
        for &(entry, _) in &spec.doors {
            let initial = match Simulation::enter_via_door(room.clone(), abilities, entry) {
                Ok(mut simulation) => {
                    simulation.enable_current_player_movement();
                    simulation
                }
                Err(error) => {
                    problems.push(format!("entry-error {entry} {loadout_name}: {error:?}"));
                    println!("entry-error {entry} {loadout_name}: {error:?}");
                    continue;
                }
            };
            for &(exit, _) in &spec.doors {
                let outcome = solve_target(&initial, SearchTarget::door(exit), &config)
                    .expect("door target resolves");
                match outcome {
                    TargetSolveOutcome::Solved(solution) => println!(
                        "pair {entry} {exit} {loadout_name} solved {}",
                        solution.replay.frames.len()
                    ),
                    other => println!("pair {entry} {exit} {loadout_name} inconclusive {other:?}"),
                }
            }
            for (coin_index, _) in spec.coins.iter().enumerate() {
                let target = SearchTarget::pickup(format!("coin-{coin_index}"));
                let outcome =
                    solve_target(&initial, target.clone(), &config).expect("coin target resolves");
                match outcome {
                    TargetSolveOutcome::Solved(solution) => {
                        println!(
                            "coin {coin_index} from {entry} {loadout_name} solved {}",
                            solution.replay.frames.len()
                        );
                        if shaky {
                            let report = evaluate_shaky_hand(
                                &initial,
                                &solution,
                                ShakyHandConfig {
                                    seed: 0x5EED_0001,
                                    trials_per_curve_point: 32,
                                    grace_ticks: 18,
                                    correlated_boundaries: 2,
                                    convergence_confirmation_ticks: 2,
                                },
                            )
                            .expect("shaky evaluation runs");
                            let worst = report
                                .curves
                                .iter()
                                .filter(|curve| {
                                    curve.family != NoiseFamily::Exact
                                        && curve.strength_ticks == 1
                                        && curve.trials > 0
                                })
                                .map(|curve| {
                                    (
                                        curve.family,
                                        curve.successes as f64 / curve.trials as f64,
                                        curve.successes,
                                        curve.trials,
                                    )
                                })
                                .min_by(|left, right| left.1.total_cmp(&right.1));
                            if let Some((family, _, successes, trials)) = worst {
                                println!(
                                    "shaky {coin_index} from {entry} {loadout_name} worst {family:?} {successes}/{trials}"
                                );
                            }
                        }
                    }
                    other => println!(
                        "coin {coin_index} from {entry} {loadout_name} inconclusive {other:?}"
                    ),
                }
            }
        }
    }

    if problems.is_empty() {
        println!("verdict ok");
        ExitCode::SUCCESS
    } else {
        println!("verdict FAIL {} problems", problems.len());
        ExitCode::FAILURE
    }
}
