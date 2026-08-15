//! A deliberately hand-authored no-Dash challenge for human calibration.
//!
//! This room is a correctness probe for difficulty work, not generated corpus
//! material and not evidence that the current metrics measure human
//! difficulty.  Its exact replay proves only tractability under authoritative
//! physics. Human playtesting decides whether the intended precision is
//! actually hard, readable, and enjoyable.

use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

/// The challenge's locked traversal loadout: wall jumps are available and
/// Dash is deliberately unavailable.
pub const HARD_NO_DASH_ABILITIES: AbilitySet = AbilitySet::new(true, false);

/// Stable ID of the sole completion trigger.
pub const HARD_NO_DASH_TARGET: &str = "finish";

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

// The player begins inside the sealed shaft between columns 11 and 15. The
// front faces are spikes except for alternating two-tile contact pads; solid
// backing columns prevent passing through a spike gap into an easier route.
// At the top, the right wall opens onto a short lip and three small one-way
// islands over a spike floor. The low spike bank above the first gap punishes
// an uncut maximum-height jump.
const HARD_NO_DASH_ROWS: [&str; 18] = [
    "..........#>....................",
    "..........#>....................",
    "..........##........vvv.........",
    "..........##....................",
    "..........#>...<#...............",
    "..........#>...##...............",
    "..........#>...##...............",
    "..........#>...<#.==...=...=..==",
    "..........##...<#...............",
    "..........##...<#^^^^^^^^^^^^^^^",
    "..........#>...<#...............",
    "..........#>...##...............",
    "..........#>...##...............",
    "..........#>...<#...............",
    "..........#>...<#...............",
    "..........#>...<#...............",
    "..........#######...............",
    "..........#>...<#...............",
];

/// Construct the trusted hand-authored no-Dash challenge room.
///
/// The canonical spawn is the source; [`HARD_NO_DASH_TARGET`] is its sole
/// exit. Use [`hard_no_dash_scenario`] when the exact locked loadout matters.
#[must_use]
pub fn hard_no_dash_room() -> Room {
    let tiles = HARD_NO_DASH_ROWS
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
                _ => unreachable!("hard no-Dash challenge contains an unsupported tile"),
            })
        })
        .collect();
    Room::new(
        "dev.hard_no_dash",
        "No Dash: Needle's Eye",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(128, 148),
        vec![Exit {
            id: HARD_NO_DASH_TARGET.to_owned(),
            bounds: Rect::new(304, 40, 16, 30),
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("the built-in hard no-Dash room must satisfy core invariants")
}

/// Construct the challenge with its exact WallJump-only loadout.
#[must_use]
pub fn hard_no_dash_scenario() -> Simulation {
    crate::current_player_scenario(hard_no_dash_room(), HARD_NO_DASH_ABILITIES)
}

/// Expand the stored exact positive witness into authoritative per-tick
/// actions. The witness contains no Dash or restart input.
#[must_use]
pub fn hard_no_dash_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-12")
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_core::{JumpKind, SimulationEvent};

    #[test]
    fn authored_rows_and_locked_scenario_are_exact() {
        assert!(HARD_NO_DASH_ROWS.iter().all(|row| row.len() == 32));
        let scenario = hard_no_dash_scenario();
        assert_eq!(scenario.abilities(), HARD_NO_DASH_ABILITIES);
        assert!(scenario.abilities().wall_jump);
        assert!(!scenario.abilities().dash);
        assert_eq!(scenario.room().spawn(), Point::new(128, 148));
        assert_eq!(scenario.room().exits().len(), 1);
        assert_eq!(scenario.room().exits()[0].id, HARD_NO_DASH_TARGET);
        assert!(scenario.room().doors().is_empty());
    }

    #[test]
    fn spike_faces_have_backing_and_only_the_authored_safe_pads() {
        let room = hard_no_dash_room();
        for row in 0..18 {
            assert_eq!(room.tile(10, row), Some(Tile::Solid));
            let right_backing = if row <= 3 { Tile::Empty } else { Tile::Solid };
            assert_eq!(room.tile(16, row), Some(right_backing));
        }
        for row in 0..18 {
            let expected_left = if [2, 3, 8, 9, 16].contains(&row) {
                Tile::Solid
            } else {
                Tile::HazardRight
            };
            assert_eq!(room.tile(11, row), Some(expected_left));

            let expected_right = if row <= 3 {
                Tile::Empty
            } else if [5, 6, 11, 12, 16].contains(&row) {
                Tile::Solid
            } else {
                Tile::HazardLeft
            };
            assert_eq!(room.tile(15, row), Some(expected_right));
        }
    }

    #[test]
    fn stored_witness_is_an_exact_no_dash_positive() {
        let mut scenario = hard_no_dash_scenario();
        let actions = hard_no_dash_witness_actions();
        assert!(!actions.is_empty());
        assert!(actions.iter().all(|action| !action.dash && !action.restart));

        let mut wall_jumps = 0;
        let mut deaths = 0;
        for action in actions {
            for event in scenario.step(action).events {
                match event {
                    SimulationEvent::Jumped(JumpKind::Wall { .. }) => wall_jumps += 1,
                    SimulationEvent::Died(_) => deaths += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(deaths, 0);
        assert_eq!(scenario.deaths(), 0);
        assert!(wall_jumps >= 5);
        assert_eq!(scenario.reached_exit(), Some(HARD_NO_DASH_TARGET));
    }

    #[test]
    fn stored_trace_does_not_work_when_wall_jump_is_removed() {
        let mut baseline = Simulation::new(hard_no_dash_room());
        baseline.enable_current_player_movement();
        for action in hard_no_dash_witness_actions() {
            baseline.step(action);
        }
        assert_ne!(baseline.reached_exit(), Some(HARD_NO_DASH_TARGET));
    }
}
