//! A deliberately hand-authored no-Dash tutorial challenge.
//!
//! This began as a proposed midpoint below the high-end human-calibration
//! room, but human playtesting found it much easier than that and identified
//! it as a good tutorial. It preserves a real alternating wall-jump climb and
//! a short precision traversal, while widening every recovery surface and
//! removing the hard room's long chain and jump-cut ceiling check. Its exact
//! replay establishes tractability only; human playtesting remains the
//! authority on difficulty.

use downwards_core::{AbilitySet, Action, Exit, Point, Rect, Room, Simulation, Tile};

/// The challenge's locked traversal loadout: WallJump is available and Dash
/// is deliberately unavailable.
pub const MEDIUM_NO_DASH_ABILITIES: AbilitySet = AbilitySet::new(true, false);

/// Stable ID of the sole completion trigger.
pub const MEDIUM_NO_DASH_TARGET: &str = "finish";

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

// The lower, middle, and upper contact bands are four to six tiles tall,
// versus the hard challenge's two-tile bands. Alternating spike-faced gaps
// still make wall jumps meaningful, but each successful transfer has room to
// settle.
// The opening leads to a four-tile recovery lip, then a three-tile island and
// a four-tile safe finish floor. There is no low ceiling demanding a jump cut.
const MEDIUM_NO_DASH_ROWS: [&str; 18] = [
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........#^....................",
    "..........##....................",
    "..........##....................",
    "..........##.....====...........",
    "..........##....................",
    "..........##...##......===......",
    "..........##...##...............",
    "..........#^...##...............",
    "..........##...##^^^^^^^^^^^####",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........##...^#...............",
    "..........#######...............",
    "..........#^...^#...............",
];

/// Construct the trusted hand-authored tutorial no-Dash room.
///
/// The canonical spawn is the source; [`MEDIUM_NO_DASH_TARGET`] is its sole
/// exit. Use [`medium_no_dash_scenario`] when the exact locked loadout matters.
#[must_use]
pub fn medium_no_dash_room() -> Room {
    let tiles = MEDIUM_NO_DASH_ROWS
        .iter()
        .flat_map(|row| {
            row.bytes().map(|tile| match tile {
                b'.' => Tile::Empty,
                b'#' => Tile::Solid,
                b'^' => Tile::Hazard,
                b'=' => Tile::OneWay,
                _ => unreachable!("medium no-Dash challenge contains an unsupported tile"),
            })
        })
        .collect();
    Room::new(
        "dev.medium_no_dash",
        "No Dash: Stepping Stones",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(120, 148),
        vec![Exit {
            id: MEDIUM_NO_DASH_TARGET.to_owned(),
            bounds: Rect::new(304, 80, 16, 40),
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("the built-in medium no-Dash room must satisfy core invariants")
}

/// Construct the challenge with its exact WallJump-only loadout.
#[must_use]
pub fn medium_no_dash_scenario() -> Simulation {
    crate::current_player_scenario(medium_no_dash_room(), MEDIUM_NO_DASH_ABILITIES)
}

/// Expand the stored exact positive witness into authoritative per-tick
/// actions. The witness contains no Dash or restart input.
#[must_use]
pub fn medium_no_dash_witness_actions() -> Vec<Action> {
    crate::generated_calibration_witnesses::generated_witness_actions("cal-01")
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_core::{JumpKind, SimulationEvent};

    #[test]
    fn authored_rows_and_locked_scenario_are_exact() {
        assert!(MEDIUM_NO_DASH_ROWS.iter().all(|row| row.len() == 32));
        let scenario = medium_no_dash_scenario();
        assert_eq!(scenario.abilities(), MEDIUM_NO_DASH_ABILITIES);
        assert!(scenario.abilities().wall_jump);
        assert!(!scenario.abilities().dash);
        assert_eq!(scenario.room().spawn(), Point::new(120, 148));
        assert_eq!(scenario.room().exits().len(), 1);
        assert_eq!(scenario.room().exits()[0].id, MEDIUM_NO_DASH_TARGET);
        assert!(scenario.room().doors().is_empty());
    }

    #[test]
    fn shaft_has_three_broad_safe_bands_with_backed_spike_gaps() {
        let room = medium_no_dash_room();

        // Left backing is sealed throughout. The lower-left and upper-left
        // bands contain five and six vertically contiguous solid tiles when
        // the shared floor is included.
        for row in 0..18 {
            assert_eq!(room.tile(10, row), Some(Tile::Solid));
        }
        for row in 5..=10 {
            assert_eq!(room.tile(11, row), Some(Tile::Solid));
        }
        for row in 12..=16 {
            assert_eq!(room.tile(11, row), Some(Tile::Solid));
        }
        for row in [0, 1, 2, 3, 4, 11, 17] {
            assert_eq!(room.tile(11, row), Some(Tile::Hazard));
        }

        // The middle-right band is four tiles tall. Its solid backing ends at
        // the lip opening, so the third wall jump has a clear route out.
        for row in 9..=12 {
            assert_eq!(room.tile(15, row), Some(Tile::Solid));
            assert_eq!(room.tile(16, row), Some(Tile::Solid));
        }
        for row in 13..=15 {
            assert_eq!(room.tile(15, row), Some(Tile::Hazard));
            assert_eq!(room.tile(16, row), Some(Tile::Solid));
        }
        for row in 0..=8 {
            assert_eq!(room.tile(15, row), Some(Tile::Empty));
            assert_eq!(room.tile(16, row), Some(Tile::Empty));
        }
    }

    #[test]
    fn traversal_has_one_wide_island_recovery_floor_and_no_ceiling_trap() {
        let room = medium_no_dash_room();

        for col in 17..=20 {
            assert_eq!(room.tile(col, 7), Some(Tile::OneWay));
        }
        for col in 23..=25 {
            assert_eq!(room.tile(col, 9), Some(Tile::OneWay));
        }
        for col in 17..=27 {
            assert_eq!(room.tile(col, 12), Some(Tile::Hazard));
        }
        for col in 28..=31 {
            assert_eq!(room.tile(col, 12), Some(Tile::Solid));
        }
        for row in 0..=6 {
            for col in 17..32 {
                assert_ne!(room.tile(col, row), Some(Tile::Hazard));
            }
        }
    }

    #[test]
    fn stored_witness_is_an_exact_clean_three_wall_jump_positive() {
        let mut scenario = medium_no_dash_scenario();
        let actions = medium_no_dash_witness_actions();
        assert!(!actions.is_empty());
        assert!(actions.iter().all(|action| !action.dash && !action.restart));

        let mut wall_jumps = 0;
        let mut ordinary_jumps = 0;
        let mut deaths = 0;
        for action in actions {
            for event in scenario.step(action).events {
                match event {
                    SimulationEvent::Jumped(JumpKind::Wall { .. }) => wall_jumps += 1,
                    SimulationEvent::Jumped(_) => ordinary_jumps += 1,
                    SimulationEvent::Died(_) => deaths += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(deaths, 0);
        assert_eq!(scenario.deaths(), 0);
        assert!(wall_jumps >= 3);
        assert_eq!(ordinary_jumps, 0);
        assert_eq!(scenario.reached_exit(), Some(MEDIUM_NO_DASH_TARGET));
    }

    #[test]
    fn stored_trace_does_not_work_when_wall_jump_is_removed() {
        let mut baseline = Simulation::new(medium_no_dash_room());
        baseline.enable_current_player_movement();
        for action in medium_no_dash_witness_actions() {
            baseline.step(action);
        }
        assert_ne!(baseline.reached_exit(), Some(MEDIUM_NO_DASH_TARGET));
    }
}
