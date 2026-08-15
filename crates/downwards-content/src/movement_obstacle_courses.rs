//! Non-lethal movement courses for learning the live WallJump control contract.

use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

use crate::CalibrationLevel;

pub const MOVEMENT_COURSE_ABILITIES: AbilitySet = AbilitySet::new(true, false);
pub const MOVEMENT_COURSE_TARGET: &str = "finish";

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

fn gym_room(
    id: &str,
    title: &str,
    spawn: Point,
    finish: Rect,
    solids: &[(u16, u16, u16, u16)],
    one_ways: &[(u16, u16, u16)],
) -> Room {
    let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
    for x in 0..WIDTH {
        tiles[usize::from(HEIGHT - 1) * usize::from(WIDTH) + usize::from(x)] = Tile::Solid;
    }
    for &(x, y, width, height) in solids {
        for row in y..y + height {
            for column in x..x + width {
                tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)] = Tile::Solid;
            }
        }
    }
    for &(x, y, width) in one_ways {
        for column in x..x + width {
            tiles[usize::from(y) * usize::from(WIDTH) + usize::from(column)] = Tile::OneWay;
        }
    }
    Room::new(
        id,
        title,
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        spawn,
        vec![Exit {
            id: MOVEMENT_COURSE_TARGET.to_owned(),
            bounds: finish,
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("built-in movement course must satisfy room invariants")
}

pub fn movement_course_long_jumps_room() -> Room {
    gym_room(
        "movement.course.long_jumps",
        "Long-Jump Yard",
        Point::new(18, 128),
        Rect::new(292, 92, 18, 38),
        &[],
        &[
            (1, 14, 5),
            (8, 12, 4),
            (14, 14, 5),
            (21, 11, 5),
            (28, 13, 4),
        ],
    )
}

pub fn movement_course_long_jumps_scenario() -> Simulation {
    crate::current_player_scenario(movement_course_long_jumps_room(), MOVEMENT_COURSE_ABILITIES)
}

pub fn movement_course_long_jumps_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-13")
}

pub fn movement_course_wall_gym_room() -> Room {
    gym_room(
        "movement.course.wall_gym",
        "Wall Gym",
        Point::new(75, 158),
        Rect::new(70, 0, 30, 28),
        &[(5, 1, 2, 16), (10, 1, 2, 16)],
        &[],
    )
}

pub fn movement_course_wall_gym_scenario() -> Simulation {
    crate::current_player_scenario(movement_course_wall_gym_room(), MOVEMENT_COURSE_ABILITIES)
}

pub fn movement_course_wall_gym_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-14")
}

pub fn movement_course_mixed_room() -> Room {
    gym_room(
        "movement.course.mixed_circuit",
        "Mixed Circuit",
        Point::new(18, 148),
        Rect::new(220, 40, 30, 28),
        &[(20, 6, 2, 7), (25, 6, 2, 11)],
        &[(1, 15, 5), (7, 12, 4), (13, 14, 4), (17, 11, 3)],
    )
}

pub fn movement_course_mixed_scenario() -> Simulation {
    crate::current_player_scenario(movement_course_mixed_room(), MOVEMENT_COURSE_ABILITIES)
}

pub fn movement_course_mixed_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-15")
}

#[cfg(test)]
mod tests {
    use downwards_core::{SimulationEvent, Tile};

    use super::*;

    #[test]
    fn courses_are_non_lethal_and_exact_witnesses_finish_cleanly() {
        for course in movement_obstacle_course_cases() {
            let mut simulation = course.scenario();
            assert_eq!(
                simulation.movement_tuning(),
                Some(downwards_core::MovementTuning::GAMEPLAY_DEFAULT)
            );
            assert!(
                simulation
                    .room()
                    .tiles()
                    .iter()
                    .all(|tile| *tile != Tile::Hazard)
            );
            assert_eq!(simulation.room().timed_hazards().len(), 0);
            let actions = course.witness_actions();
            assert!(!actions.is_empty());
            let mut wall_jumps = 0;
            for (index, action) in actions.iter().copied().enumerate() {
                assert_eq!(simulation.reached_exit(), None);
                for event in simulation.step(action).events {
                    assert!(!matches!(
                        event,
                        SimulationEvent::Died(_) | SimulationEvent::Reset
                    ));
                    wall_jumps += usize::from(matches!(
                        event,
                        SimulationEvent::Jumped(downwards_core::JumpKind::Wall { .. })
                    ));
                }
                if index + 1 < actions.len() {
                    assert_eq!(simulation.reached_exit(), None);
                }
            }
            assert_eq!(simulation.reached_exit(), Some(course.target()));
            if matches!(course.id(), "cal-14" | "cal-15") {
                assert!(wall_jumps >= 3, "{} needs wall-jump practice", course.id());
            }
        }
    }

    #[test]
    fn doing_nothing_is_safe_recovery_not_death() {
        for course in movement_obstacle_course_cases() {
            let mut simulation = course.scenario();
            for _ in 0..600 {
                assert!(
                    simulation
                        .step(Action::default())
                        .events
                        .iter()
                        .all(|event| !matches!(
                            event,
                            SimulationEvent::Died(_) | SimulationEvent::Reset
                        ))
                );
            }
        }
    }
}

pub fn movement_obstacle_course_cases() -> [CalibrationLevel; 3] {
    [
        CalibrationLevel::new(
            "cal-13",
            "Long-Jump Yard",
            "non-lethal horizontal momentum / varied platform spacing",
            MOVEMENT_COURSE_TARGET,
            MOVEMENT_COURSE_ABILITIES,
            movement_course_long_jumps_scenario,
            movement_course_long_jumps_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-14",
            "Wall Gym",
            "non-lethal wall-jump rhythm / pillar recovery",
            MOVEMENT_COURSE_TARGET,
            MOVEMENT_COURSE_ABILITIES,
            movement_course_wall_gym_scenario,
            movement_course_wall_gym_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-15",
            "Mixed Circuit",
            "non-lethal horizontal and wall-jump transitions",
            MOVEMENT_COURSE_TARGET,
            MOVEMENT_COURSE_ABILITIES,
            movement_course_mixed_scenario,
            movement_course_mixed_witness_actions,
        ),
    ]
}

#[cfg(test)]
mod authoring {
    use downwards_ai::{
        SearchTarget, SolverConfig, TargetSolveOutcome, audit_direct_controller_probes,
        solve_target,
    };

    use super::*;

    #[test]
    #[ignore = "authoring-only solver"]
    fn discover_course_witnesses() {
        for (name, scenario) in [
            (
                "LONG",
                movement_course_long_jumps_scenario as fn() -> Simulation,
            ),
            ("WALL", movement_course_wall_gym_scenario),
            ("MIXED", movement_course_mixed_scenario),
        ] {
            let initial = scenario();
            let mut config = SolverConfig::for_abilities(MOVEMENT_COURSE_ABILITIES);
            config.max_ticks_per_path = 1_200;
            config.max_expanded_nodes = 120_000;
            config.max_simulated_ticks = 4_000_000;
            let target = SearchTarget::exit(MOVEMENT_COURSE_TARGET);
            let direct =
                audit_direct_controller_probes(&initial, std::slice::from_ref(&target), &config)
                    .unwrap();
            if let Some(witness) = direct.witnesses.first() {
                eprintln!(
                    "{name} DIRECT {} ticks {:?}",
                    witness.replay.frames.len(),
                    witness.probes
                );
            }
            let outcome = solve_target(&initial, target, &config).unwrap();
            let TargetSolveOutcome::Solved(solution) = outcome else {
                panic!("{name} unsolved: {outcome:?}");
            };
            let actions = solution.replay.actions().collect::<Vec<_>>();
            eprintln!("{name}: {} ticks", actions.len());
            let mut start = 0;
            while start < actions.len() {
                let current = actions[start];
                let end = actions[start..]
                    .iter()
                    .position(|candidate| *candidate != current)
                    .map_or(actions.len(), |offset| start + offset);
                eprintln!("({current:?}, {}),", end - start);
                start = end;
            }
        }
    }
}
