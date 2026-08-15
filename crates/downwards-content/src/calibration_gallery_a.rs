//! Isolated WallJump/no-Dash human-calibration gallery, cohort A.
//!
//! These authored rooms vary explicit structural dimensions. Neither their
//! order nor their metadata predicts human difficulty. Exact witnesses prove
//! only tractability under authoritative physics.

use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

pub const CALIBRATION_GALLERY_A_ABILITIES: AbilitySet = AbilitySet::new(true, false);
pub const CALIBRATION_GALLERY_A_TARGET: &str = "finish";

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

fn current_player_scenario(room: Room, abilities: AbilitySet) -> Simulation {
    let mut simulation = Simulation::with_abilities(room, abilities);
    simulation.enable_current_player_movement();
    simulation
}

/// Authored structural facts for one calibration room. These are descriptive
/// dimensions, not a score or prediction of human difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationGalleryADimensions {
    pub climb_span_tiles: u8,
    pub contact_bands: u8,
    pub minimum_pad_height_tiles: u8,
    pub recovery_support_widths_tiles: &'static [u8],
    pub jump_cut_checks: u8,
    pub witnessed_jump_cut_clearance_pixels: Option<u8>,
}

/// Stable native case metadata and factories for later gallery aggregation.
#[derive(Clone, Copy)]
pub struct CalibrationGalleryACase {
    pub native_id: &'static str,
    pub title: &'static str,
    pub mechanic_axis: &'static str,
    pub dimensions: CalibrationGalleryADimensions,
    pub room_factory: fn() -> Room,
    pub scenario_factory: fn() -> Simulation,
    pub witness_factory: fn() -> Vec<Action>,
}

pub const CALIBRATION_GALLERY_A_DIMENSIONS: [CalibrationGalleryADimensions; 5] = [
    CalibrationGalleryADimensions {
        climb_span_tiles: 13,
        contact_bands: 2,
        minimum_pad_height_tiles: 15,
        recovery_support_widths_tiles: &[8],
        jump_cut_checks: 0,
        witnessed_jump_cut_clearance_pixels: None,
    },
    CalibrationGalleryADimensions {
        climb_span_tiles: 4,
        contact_bands: 2,
        minimum_pad_height_tiles: 2,
        recovery_support_widths_tiles: &[],
        jump_cut_checks: 0,
        witnessed_jump_cut_clearance_pixels: None,
    },
    CalibrationGalleryADimensions {
        climb_span_tiles: 13,
        contact_bands: 5,
        minimum_pad_height_tiles: 3,
        recovery_support_widths_tiles: &[8],
        jump_cut_checks: 0,
        witnessed_jump_cut_clearance_pixels: None,
    },
    CalibrationGalleryADimensions {
        climb_span_tiles: 7,
        contact_bands: 3,
        minimum_pad_height_tiles: 4,
        recovery_support_widths_tiles: &[4, 9],
        jump_cut_checks: 1,
        witnessed_jump_cut_clearance_pixels: Some(6),
    },
    CalibrationGalleryADimensions {
        climb_span_tiles: 3,
        contact_bands: 2,
        minimum_pad_height_tiles: 3,
        recovery_support_widths_tiles: &[5, 3, 4],
        jump_cut_checks: 0,
        witnessed_jump_cut_clearance_pixels: None,
    },
];

const A1_ROWS: [&str; 18] = [
    "..........##....................",
    "..........##....................",
    "..........##....................",
    "..........##...########.........",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........##...##...............",
    "..........#######...............",
    "..........##...##...............",
];

const A2_ROWS: [&str; 18] = [
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "................................",
    "...............##...............",
    "..........#^...##...............",
    "..........#^...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
];

const A3_ROWS: [&str; 18] = [
    "..........#^....................",
    "..........##....................",
    "..........##....................",
    "..........##....................",
    "..........#^...########.........",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

const A4_ROWS: [&str; 18] = [
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^........^^^^........",
    "..........##....................",
    "..........##....................",
    "..........##.....====...........",
    "..........##...........=========",
    "..........##...##...............",
    "..........##...##...............",
    "..........#^...##...............",
    "..........##...##^^^^^^^^^^^^^^^",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

const A5_ROWS: [&str; 18] = [
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........##....................",
    "..........##.....====...........",
    "..........##....................",
    "..........##...........===......",
    "..........#^...^#====...........",
    "..........#^...##...............",
    "..........#^...##^^^^^^^^^^^####",
    "..........#######...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
];

fn room_from_rows(id: &str, title: &str, rows: &[&str; 18], spawn: Point, exit: Rect) -> Room {
    let tiles = rows
        .iter()
        .flat_map(|row| {
            row.bytes().map(|tile| match tile {
                b'.' => Tile::Empty,
                b'#' => Tile::Solid,
                b'^' => Tile::Hazard,
                b'=' => Tile::OneWay,
                _ => unreachable!("calibration gallery A contains an unsupported tile"),
            })
        })
        .collect();
    Room::new(
        id,
        title,
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        spawn,
        vec![Exit {
            id: CALIBRATION_GALLERY_A_TARGET.to_owned(),
            bounds: exit,
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("built-in calibration gallery A room must satisfy core invariants")
}

#[must_use]
pub fn calibration_gallery_a1_room() -> Room {
    room_from_rows(
        "calibration.wall_jump.a1_broad_ascent",
        "Open Chimney",
        &A1_ROWS,
        Point::new(120, 148),
        Rect::new(170, 0, 50, 60),
    )
}

#[must_use]
pub fn calibration_gallery_a1_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_a1_room(),
        CALIBRATION_GALLERY_A_ABILITIES,
    )
}

#[must_use]
pub fn calibration_gallery_a1_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-02")
}

#[must_use]
pub fn calibration_gallery_a2_room() -> Room {
    room_from_rows(
        "calibration.wall_jump.a2_needle_step",
        "Two-Tile Turn",
        &A2_ROWS,
        Point::new(120, 118),
        Rect::new(80, 20, 30, 60),
    )
}

#[must_use]
pub fn calibration_gallery_a2_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_a2_room(),
        CALIBRATION_GALLERY_A_ABILITIES,
    )
}

#[must_use]
pub fn calibration_gallery_a2_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-03")
}

#[must_use]
pub fn calibration_gallery_a3_room() -> Room {
    room_from_rows(
        "calibration.wall_jump.a3_even_tempo",
        "Even Tempo",
        &A3_ROWS,
        Point::new(120, 148),
        Rect::new(170, 0, 30, 70),
    )
}

#[must_use]
pub fn calibration_gallery_a3_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_a3_room(),
        CALIBRATION_GALLERY_A_ABILITIES,
    )
}

#[must_use]
pub fn calibration_gallery_a3_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-04")
}

#[must_use]
pub fn calibration_gallery_a4_room() -> Room {
    room_from_rows(
        "calibration.wall_jump.a4_low_clearance",
        "Low Clearance",
        &A4_ROWS,
        Point::new(120, 148),
        Rect::new(250, 40, 60, 50),
    )
}

#[must_use]
pub fn calibration_gallery_a4_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_a4_room(),
        CALIBRATION_GALLERY_A_ABILITIES,
    )
}

#[must_use]
pub fn calibration_gallery_a4_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-05")
}

#[must_use]
pub fn calibration_gallery_a5_room() -> Room {
    room_from_rows(
        "calibration.wall_jump.a5_safe_harbor",
        "Safe Harbor",
        &A5_ROWS,
        Point::new(142, 128),
        Rect::new(304, 90, 16, 40),
    )
}

#[must_use]
pub fn calibration_gallery_a5_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_a5_room(),
        CALIBRATION_GALLERY_A_ABILITIES,
    )
}

#[must_use]
pub fn calibration_gallery_a5_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-06")
}

/// Return cohort A in its stable native order. A later gallery seam may map
/// these native IDs to presentation-neutral public labels.
#[must_use]
pub fn calibration_gallery_a_cases() -> [CalibrationGalleryACase; 5] {
    [
        CalibrationGalleryACase {
            native_id: "calibration.wall_jump.a1_broad_ascent",
            title: "Open Chimney",
            mechanic_axis: "long broad-pad wall-jump climb",
            dimensions: CALIBRATION_GALLERY_A_DIMENSIONS[0],
            room_factory: calibration_gallery_a1_room,
            scenario_factory: calibration_gallery_a1_scenario,
            witness_factory: calibration_gallery_a1_witness_actions,
        },
        CalibrationGalleryACase {
            native_id: "calibration.wall_jump.a2_needle_step",
            title: "Two-Tile Turn",
            mechanic_axis: "short narrow-pad wall-jump precision",
            dimensions: CALIBRATION_GALLERY_A_DIMENSIONS[1],
            room_factory: calibration_gallery_a2_room,
            scenario_factory: calibration_gallery_a2_scenario,
            witness_factory: calibration_gallery_a2_witness_actions,
        },
        CalibrationGalleryACase {
            native_id: "calibration.wall_jump.a3_even_tempo",
            title: "Even Tempo",
            mechanic_axis: "moderate rhythmic alternating wall-jump climb",
            dimensions: CALIBRATION_GALLERY_A_DIMENSIONS[2],
            room_factory: calibration_gallery_a3_room,
            scenario_factory: calibration_gallery_a3_scenario,
            witness_factory: calibration_gallery_a3_witness_actions,
        },
        CalibrationGalleryACase {
            native_id: "calibration.wall_jump.a4_low_clearance",
            title: "Low Clearance",
            mechanic_axis: "low ceiling: release Jump early for a low hop",
            dimensions: CALIBRATION_GALLERY_A_DIMENSIONS[3],
            room_factory: calibration_gallery_a4_room,
            scenario_factory: calibration_gallery_a4_scenario,
            witness_factory: calibration_gallery_a4_witness_actions,
        },
        CalibrationGalleryACase {
            native_id: "calibration.wall_jump.a5_safe_harbor",
            title: "Safe Harbor",
            mechanic_axis: "short wall-jump climb with recovery landings",
            dimensions: CALIBRATION_GALLERY_A_DIMENSIONS[4],
            room_factory: calibration_gallery_a5_room,
            scenario_factory: calibration_gallery_a5_scenario,
            witness_factory: calibration_gallery_a5_witness_actions,
        },
    ]
}
