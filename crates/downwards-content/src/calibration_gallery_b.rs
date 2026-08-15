//! Five WallJump-only rooms for the human-calibration gallery.
//!
//! Each room isolates a different authored movement burden. The labels are
//! design hypotheses for human playtesting, not difficulty measurements; the
//! stored replays establish only tractability under authoritative physics.

use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

use crate::CalibrationLevel;

/// Locked loadout shared by this calibration cohort.
pub const CALIBRATION_GALLERY_B_ABILITIES: AbilitySet = AbilitySet::new(true, false);

/// Stable target ID shared by these single-exit rooms.
pub const CALIBRATION_GALLERY_B_TARGET: &str = "finish";

/// Shared gallery descriptor type, named here for cohort-specific callers.
pub type CalibrationGalleryBCase = CalibrationLevel;

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

fn current_player_scenario(room: Room, abilities: AbilitySet) -> Simulation {
    let mut simulation = Simulation::with_abilities(room, abilities);
    simulation.enable_current_player_movement();
    simulation
}

fn authored_room(id: &str, title: &str, rows: &[&str; 18], spawn: Point, finish: Rect) -> Room {
    let tiles = rows
        .iter()
        .flat_map(|row| {
            row.bytes().map(|tile| match tile {
                b'.' => Tile::Empty,
                b'#' => Tile::Solid,
                b'^' => Tile::Hazard,
                b'=' => Tile::OneWay,
                _ => unreachable!("calibration gallery B contains an unsupported tile"),
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
            id: CALIBRATION_GALLERY_B_TARGET.to_owned(),
            bounds: finish,
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("built-in calibration gallery B room must satisfy core invariants")
}

// Revised after human calibration: a short route with broad contact bands and
// a stable staging shelf emphasizes three readable transfers without requiring
// an immediate midair opening or precision wall acquisition.
const B1_ROWS: [&str; 18] = [
    "..........##....................",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##===^#...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the short, narrow-contact wall climb.
#[must_use]
pub fn calibration_gallery_b1_room() -> Room {
    authored_room(
        "calibration.wall_jump.b1_needle_chimney",
        "Three Pins",
        &B1_ROWS,
        Point::new(120, 108),
        Rect::new(120, 0, 30, 28),
    )
}

/// Construct B1 with its exact WallJump-only loadout.
#[must_use]
pub fn calibration_gallery_b1_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_b1_room(),
        CALIBRATION_GALLERY_B_ABILITIES,
    )
}

/// Expand B1's stored exact witness.
#[must_use]
pub fn calibration_gallery_b1_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-07")
}

// Hypothesis: five broad alternating contact bands emphasize sustained input
// sequencing. The mid-shaft one-way is recovery geometry, not a shortcut from
// the spawn floor.
const B2_ROWS: [&str; 18] = [
    "..........#^...^#...............",
    "..........#^...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##===^#...............",
    "..........##...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the longer broad-contact endurance climb.
#[must_use]
pub fn calibration_gallery_b2_room() -> Room {
    authored_room(
        "calibration.wall_jump.b2_long_ascent",
        "Long Ascent",
        &B2_ROWS,
        Point::new(120, 148),
        Rect::new(120, 0, 30, 18),
    )
}

/// Construct B2 with its exact WallJump-only loadout.
#[must_use]
pub fn calibration_gallery_b2_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_b2_room(),
        CALIBRATION_GALLERY_B_ABILITIES,
    )
}

/// Expand B2's stored exact witness.
#[must_use]
pub fn calibration_gallery_b2_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-08")
}

// Hypothesis: a moderate-length climb becomes meaningfully less forgiving
// when every failed transfer falls to the start; there are no one-way shelves
// or intermediate horizontal supports in this shaft.
const B3_ROWS: [&str; 18] = [
    "..........#^...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#^...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the moderate climb with no intermediate recovery shelf.
#[must_use]
pub fn calibration_gallery_b3_room() -> Room {
    authored_room(
        "calibration.wall_jump.b3_open_shaft",
        "Open Shaft",
        &B3_ROWS,
        Point::new(120, 138),
        Rect::new(120, 0, 30, 28),
    )
}

/// Construct B3 with its exact WallJump-only loadout.
#[must_use]
pub fn calibration_gallery_b3_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_b3_room(),
        CALIBRATION_GALLERY_B_ABILITIES,
    )
}

/// Expand B3's stored exact witness.
#[must_use]
pub fn calibration_gallery_b3_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-09")
}

// Hypothesis: broad wall contacts keep the climb readable while the narrow
// post-climb one-way chain shifts the burden toward repeated landing control.
const B4_ROWS: [&str; 18] = [
    "..........##....................",
    "..........##....................",
    "..........##....................",
    "..........##....................",
    "..........##....................",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##.==...=...=..==",
    "..........##...^#...............",
    "..........##...^#^^^^^^^^^^^^^^^",
    "..........##...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the broad climb followed by a tight landing chain.
#[must_use]
pub fn calibration_gallery_b4_room() -> Room {
    authored_room(
        "calibration.wall_jump.b4_broken_causeway",
        "Broken Causeway",
        &B4_ROWS,
        Point::new(120, 148),
        Rect::new(304, 40, 16, 30),
    )
}

/// Construct B4 with its exact WallJump-only loadout.
#[must_use]
pub fn calibration_gallery_b4_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_b4_room(),
        CALIBRATION_GALLERY_B_ABILITIES,
    )
}

/// Expand B4's stored exact witness.
#[must_use]
pub fn calibration_gallery_b4_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-10")
}

// Hypothesis: the low hazard bank above the first post-climb gap requires an
// early jump release. Its placement relative to the first two islands is the
// already witnessed hard-challenge timing pattern; the climb below uses the
// broader contacts of this calibration cohort.
const B5_ROWS: [&str; 18] = [
    "..........##....................",
    "..........##....................",
    "..........##........^^^.........",
    "..........##........^^^.........",
    "..........##....................",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##.==..==...=..==",
    "..........##...^#...............",
    "..........##...^#^^^^^^^^^^^^^^^",
    "..........##...^#...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........#^...##...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the wall climb followed by a low-ceiling jump-cut check.
#[must_use]
pub fn calibration_gallery_b5_room() -> Room {
    authored_room(
        "calibration.wall_jump.b5_low_bridge",
        "Low Bridge",
        &B5_ROWS,
        Point::new(120, 148),
        Rect::new(304, 40, 16, 30),
    )
}

/// Construct B5 with its exact WallJump-only loadout.
#[must_use]
pub fn calibration_gallery_b5_scenario() -> Simulation {
    current_player_scenario(
        calibration_gallery_b5_room(),
        CALIBRATION_GALLERY_B_ABILITIES,
    )
}

/// Expand B5's stored exact witness.
#[must_use]
pub fn calibration_gallery_b5_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-11")
}

/// Construct the stable B1-B5 descriptors for gallery aggregation.
#[must_use]
pub fn calibration_gallery_b_cases() -> [CalibrationGalleryBCase; 5] {
    [
        CalibrationLevel::new(
            "cal-07",
            "Three Pins",
            "short wall climb / staged broad contact bands",
            CALIBRATION_GALLERY_B_TARGET,
            CALIBRATION_GALLERY_B_ABILITIES,
            calibration_gallery_b1_scenario,
            calibration_gallery_b1_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-08",
            "Long Ascent",
            "longer wall sequence / broad contacts / recovery shelf",
            CALIBRATION_GALLERY_B_TARGET,
            CALIBRATION_GALLERY_B_ABILITIES,
            calibration_gallery_b2_scenario,
            calibration_gallery_b2_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-09",
            "Open Shaft",
            "moderate wall climb / no intermediate recovery",
            CALIBRATION_GALLERY_B_TARGET,
            CALIBRATION_GALLERY_B_ABILITIES,
            calibration_gallery_b3_scenario,
            calibration_gallery_b3_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-10",
            "Broken Causeway",
            "wall climb / narrow repeated landing chain",
            CALIBRATION_GALLERY_B_TARGET,
            CALIBRATION_GALLERY_B_ABILITIES,
            calibration_gallery_b4_scenario,
            calibration_gallery_b4_witness_actions,
        ),
        CalibrationLevel::new(
            "cal-11",
            "Low Bridge",
            "wall climb / low ceiling: release Jump early",
            CALIBRATION_GALLERY_B_TARGET,
            CALIBRATION_GALLERY_B_ABILITIES,
            calibration_gallery_b5_scenario,
            calibration_gallery_b5_witness_actions,
        ),
    ]
}
