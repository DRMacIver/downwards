//! Small built-in rooms used by the mechanics lab and regression tests.

#![forbid(unsafe_code)]

mod calibrated_generator_playtest;
mod calibration_gallery;
mod calibration_gallery_a;
mod calibration_gallery_b;
mod dungeon_v2;
mod generated_calibration_witnesses;
pub mod generated_tracks;
mod hard_no_dash;
mod medium_no_dash;
mod movement_obstacle_courses;
mod rooms_v2;

pub use calibrated_generator_playtest::{
    CalibratedGeneratorPlaytestLevel, calibrated_generator_playtest,
};
pub use calibration_gallery::{CalibrationLevel, calibration_gallery};
pub use calibration_gallery_a::{
    CALIBRATION_GALLERY_A_ABILITIES, CALIBRATION_GALLERY_A_DIMENSIONS,
    CALIBRATION_GALLERY_A_TARGET, CalibrationGalleryACase, CalibrationGalleryADimensions,
    calibration_gallery_a_cases,
};
pub use calibration_gallery_b::{
    CALIBRATION_GALLERY_B_ABILITIES, CALIBRATION_GALLERY_B_TARGET, CalibrationGalleryBCase,
    calibration_gallery_b_cases,
};
pub use dungeon_v2::{
    DUNGEON_V2_BOOT_PICKUP, DUNGEON_V2_CROWN_PICKUP, DUNGEON_V2_EXIT_DOOR, DUNGEON_V2_GLOVE_PICKUP,
    DUNGEON_V2_GOAL_EXIT,
    DungeonV2, DungeonV2Instance, DungeonV2Inventory, DungeonV2Requirement, dungeon_v2_coin_id,
    dungeon_v2_crown_respawn_point, dungeon_v2_definition, dungeon_v2_door_requirement,
    dungeon_v2_exit_gate_bounds,
    dungeon_v2_room, dungeon_v2_total_coins,
};
pub use rooms_v2::{rooms_v2_room, rooms_v2_slugs};
pub use hard_no_dash::{
    HARD_NO_DASH_ABILITIES, HARD_NO_DASH_TARGET, hard_no_dash_room, hard_no_dash_scenario,
    hard_no_dash_witness_actions,
};
pub use medium_no_dash::{
    MEDIUM_NO_DASH_ABILITIES, MEDIUM_NO_DASH_TARGET, medium_no_dash_room, medium_no_dash_scenario,
    medium_no_dash_witness_actions,
};
pub use movement_obstacle_courses::{
    MOVEMENT_COURSE_ABILITIES, MOVEMENT_COURSE_TARGET, movement_obstacle_course_cases,
};

use downwards_core::{AbilitySet, Exit, Pickup, Point, Rect, Room, Simulation, Tile, TimedHazard};

fn current_player_scenario(room: Room, abilities: AbilitySet) -> Simulation {
    let mut simulation = Simulation::with_abilities(room, abilities);
    simulation.enable_current_player_movement();
    simulation
}

const FIRST_STEPS_HIGH_ROUTE_PICKUP: Rect = Rect::new(242, 110, 6, 6);

const FIRST_STEPS_ROWS: [&str; 18] = [
    "################################",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#.................====.........#",
    "#..............................#",
    "#.....................#####....#",
    "#..............................#",
    "#..........#########...........#",
    "#...............................",
    "#..............^^..............#",
    "################################",
];

/// Construct the trusted, built-in First Steps mechanics room.
///
/// This is infallible at the API boundary because all inputs are constants
/// owned and tested by this crate. A failed core invariant therefore indicates
/// a programming error in this built-in definition rather than bad runtime
/// content.
#[must_use]
pub fn first_steps_room() -> Room {
    let tiles = FIRST_STEPS_ROWS
        .iter()
        .flat_map(|row| {
            row.bytes().map(|tile| match tile {
                b'.' => Tile::Empty,
                b'#' => Tile::Solid,
                b'^' => Tile::HazardUp,
                b'v' => Tile::HazardDown,
                b'<' => Tile::HazardLeft,
                b'>' => Tile::HazardRight,
                b'=' => Tile::OneWay,
                _ => unreachable!("First Steps contains an unsupported tile"),
            })
        })
        .collect();
    let exits = vec![Exit {
        id: "right".to_owned(),
        bounds: Rect::new(304, 140, 6, 30),
        destination: Some("dev.first_steps".to_owned()),
        destination_entrance: Some("left".to_owned()),
    }];
    let timed_hazards = vec![
        TimedHazard::new(Rect::new(278, 55, 8, 35), 120, 45, 30)
            .expect("First Steps timed hazard must satisfy core invariants"),
    ];
    let pickups = vec![
        Pickup::new("high_route", FIRST_STEPS_HIGH_ROUTE_PICKUP)
            .expect("First Steps pickup must satisfy core invariants"),
    ];

    Room::new(
        "dev.first_steps",
        "First Steps",
        32,
        18,
        10,
        tiles,
        Point::new(20, 145),
        exits,
    )
    .expect("First Steps room must satisfy core invariants")
    .with_objects(timed_hazards, pickups)
    .expect("First Steps objects must satisfy core room invariants")
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_ai::{
        ReachedTarget, SearchTarget, SolverConfig, TargetSolveOutcome, solve_target,
    };
    use downwards_core::{
        AbilitySet, JumpKind, PLAYER_HEIGHT, PLAYER_WIDTH, Simulation, SimulationEvent,
    };

    const FIRST_STEPS_LOWER_PLATFORM: Rect = Rect::new(110, 140, 90, 10);
    const FIRST_STEPS_HIGH_PLATFORM: Rect = Rect::new(220, 120, 50, 10);

    #[test]
    fn first_steps_has_the_exact_authored_tile_field() {
        let room = first_steps_room();
        assert!(FIRST_STEPS_ROWS.iter().all(|row| row.len() == 32));
        assert_eq!(room.tiles().len(), 32 * 18);
        let actual_rows = room
            .tiles()
            .chunks(usize::from(room.width()))
            .map(|row| {
                row.iter()
                    .map(|tile| match tile {
                        Tile::Empty => '.',
                        Tile::Solid => '#',
                        Tile::HazardUp => '^',
                        Tile::HazardDown => 'v',
                        Tile::HazardLeft => '<',
                        Tile::HazardRight => '>',
                        Tile::OneWay => '=',
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(actual_rows, FIRST_STEPS_ROWS);
        assert_eq!(room.tiles()[10 * 32 + 18], Tile::OneWay);
        assert_eq!(room.tiles()[16 * 32 + 15], Tile::HazardUp);
        assert_eq!(room.tiles()[15 * 32 + 31], Tile::Empty);
        assert_eq!(
            room.tiles()
                .iter()
                .filter(|&&tile| tile == Tile::Solid)
                .count(),
            109
        );
        assert_eq!(
            room.tiles()
                .iter()
                .filter(|&&tile| tile == Tile::OneWay)
                .count(),
            4
        );
        assert_eq!(
            room.tiles()
                .iter()
                .filter(|&&tile| tile == Tile::HazardUp)
                .count(),
            2
        );
    }

    #[test]
    fn first_steps_has_the_exact_authored_shape_and_objects() {
        let room = first_steps_room();

        assert_eq!(room.id(), "dev.first_steps");
        assert_eq!(room.name(), "First Steps");
        assert_eq!(
            (room.width(), room.height(), room.tile_size()),
            (32, 18, 10)
        );
        assert_eq!(room.spawn(), Point::new(20, 145));
        assert_eq!(
            room.exits(),
            &[Exit {
                id: "right".to_owned(),
                bounds: Rect::new(304, 140, 6, 30),
                destination: Some("dev.first_steps".to_owned()),
                destination_entrance: Some("left".to_owned()),
            }]
        );
        assert!(room.doors().is_empty());
        assert_eq!(
            room.timed_hazards(),
            &[TimedHazard::new(Rect::new(278, 55, 8, 35), 120, 45, 30).unwrap()]
        );
        assert_eq!(
            room.pickups(),
            &[Pickup::new("high_route", FIRST_STEPS_HIGH_ROUTE_PICKUP).unwrap()]
        );
    }

    #[test]
    fn first_steps_coin_route_has_comfortable_structural_margins() {
        let room = first_steps_room();

        for x in 11..20 {
            assert_eq!(room.tile(x, 14), Some(Tile::Solid));
        }
        assert_eq!(room.tile(10, 14), Some(Tile::Empty));
        assert_eq!(room.tile(20, 14), Some(Tile::Empty));
        for x in 22..27 {
            assert_eq!(room.tile(x, 12), Some(Tile::Solid));
        }
        assert_eq!(room.tile(21, 12), Some(Tile::Empty));
        assert_eq!(room.tile(27, 12), Some(Tile::Empty));

        let gap = FIRST_STEPS_HIGH_PLATFORM.x - FIRST_STEPS_LOWER_PLATFORM.right();
        let rise = FIRST_STEPS_LOWER_PLATFORM.y - FIRST_STEPS_HIGH_PLATFORM.y;
        assert_eq!(gap, 20);
        assert!(gap <= 3 * PLAYER_WIDTH);
        assert_eq!(rise, 20);

        let pickup = room.pickups()[0].bounds();
        assert_eq!(pickup.x - FIRST_STEPS_HIGH_PLATFORM.x, 22);
        assert_eq!(FIRST_STEPS_HIGH_PLATFORM.right() - pickup.right(), 22);
        assert!(pickup.x - FIRST_STEPS_HIGH_PLATFORM.x >= 2 * PLAYER_WIDTH);
        assert!(FIRST_STEPS_HIGH_PLATFORM.right() - pickup.right() >= 2 * PLAYER_WIDTH);
        assert_eq!(FIRST_STEPS_HIGH_PLATFORM.y - pickup.bottom(), 4);

        let standing_collector = Rect::new(
            pickup.x,
            FIRST_STEPS_HIGH_PLATFORM.y - PLAYER_HEIGHT,
            PLAYER_WIDTH,
            PLAYER_HEIGHT,
        );
        assert!(standing_collector.intersects(pickup));
        assert!(standing_collector.x >= FIRST_STEPS_HIGH_PLATFORM.x);
        assert!(standing_collector.right() <= FIRST_STEPS_HIGH_PLATFORM.right());
    }

    #[test]
    fn first_steps_coin_has_a_deterministic_verified_baseline_route() {
        let initial = Simulation::new(first_steps_room());
        assert_eq!(initial.abilities(), AbilitySet::NONE);
        let solve_coin = || {
            solve_target(
                &initial,
                SearchTarget::pickup("high_route"),
                &SolverConfig::default(),
            )
            .unwrap()
        };

        let first = solve_coin();
        let second = solve_coin();
        assert_eq!(first, second, "the First Steps coin witness must be stable");
        let TargetSolveOutcome::Solved(solution) = first else {
            panic!("First Steps coin needs a baseline route: {first:?}");
        };
        assert_eq!(solution.target, SearchTarget::pickup("high_route"));
        assert_eq!(
            solution.reached,
            ReachedTarget::Pickup("high_route".to_owned())
        );
        assert!(solution.replay.frames.len() <= 180);
        assert!(solution.stats.expanded_nodes <= 100);
        assert!(
            solution
                .replay
                .actions()
                .all(|action| !action.dash && !action.restart)
        );

        let verified = solution.replay.verify(&initial).unwrap();
        assert_eq!(verified.frames_verified, solution.replay.frames.len());
        assert_eq!(verified.collected_pickup_ids, ["high_route"]);
        // Leaving the room banks a touched coin, so the deterministic baseline
        // route may legitimately end on the door.

        let mut replayed = initial;
        let mut grounded_jumps = 0;
        let mut pickup_events = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                match event {
                    SimulationEvent::Jumped(JumpKind::Grounded) => grounded_jumps += 1,
                    SimulationEvent::Jumped(kind) => {
                        panic!("First Steps coin route used a non-grounded jump: {kind:?}")
                    }
                    SimulationEvent::PickupCollected { ref id } if id == "high_route" => {
                        pickup_events += 1;
                    }
                    SimulationEvent::Died(reason) => {
                        panic!("First Steps coin route died: {reason:?}")
                    }
                    SimulationEvent::Reset => panic!("First Steps coin route reset"),
                    _ => {}
                }
            }
        }
        assert_eq!(grounded_jumps, 2);
        assert_eq!(pickup_events, 1);
        assert_eq!(replayed.deaths(), 0);
    }
}
