//! The full rooms-v2 catalogue: every prototype room grid + spec pair in
//! `downwards-gen/rooms-v2`, buildable as a standalone [`Room`] regardless of
//! whether the current dungeon layout uses it. The soundtrack pipeline
//! composes music for every one of these rooms, so newly invented rooms get
//! fitting music for free.
//!
//! Hazards, coins, and doors come from the same `parse_spec` used by the
//! dungeon v2 loader — spec text is never re-parsed independently.

use downwards_core::{Door, Pickup, Point, Room};
use downwards_gen::parse_room_grid;

use crate::dungeon_v2::{door_geometry, parse_spec};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

/// (slug, grid, spec) for every rooms-v2 room.
const ROOMS: &[(&str, &str, &str)] = &[
    (
        "antiphase-airlock-a",
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-a.spec.txt"),
    ),
    (
        "antiphase-airlock-b",
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-b.spec.txt"),
    ),
    (
        "antiphase-airlock-c",
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-c.spec.txt"),
    ),
    (
        "braided-crossing-a",
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-a.spec.txt"),
    ),
    (
        "braided-crossing-b",
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-b.spec.txt"),
    ),
    (
        "braided-crossing-c",
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/braided-crossing-c.spec.txt"),
    ),
    (
        "chimney-lock-a",
        include_str!("../../downwards-gen/rooms-v2/chimney-lock-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/chimney-lock-a.spec.txt"),
    ),
    (
        "eaves-walk-a",
        include_str!("../../downwards-gen/rooms-v2/eaves-walk-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/eaves-walk-a.spec.txt"),
    ),
    (
        "gable-run-a",
        include_str!("../../downwards-gen/rooms-v2/gable-run-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/gable-run-a.spec.txt"),
    ),
    (
        "greed-loop-a",
        include_str!("../../downwards-gen/rooms-v2/greed-loop-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/greed-loop-a.spec.txt"),
    ),
    (
        "greed-loop-b",
        include_str!("../../downwards-gen/rooms-v2/greed-loop-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/greed-loop-b.spec.txt"),
    ),
    (
        "greed-loop-c",
        include_str!("../../downwards-gen/rooms-v2/greed-loop-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/greed-loop-c.spec.txt"),
    ),
    (
        "greed-loop-d",
        include_str!("../../downwards-gen/rooms-v2/greed-loop-d.txt"),
        include_str!("../../downwards-gen/rooms-v2/greed-loop-d.spec.txt"),
    ),
    (
        "keep-astral-seal",
        include_str!("../../downwards-gen/rooms-v2/keep-astral-seal.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-astral-seal.spec.txt"),
    ),
    (
        "keep-aurora-spire",
        include_str!("../../downwards-gen/rooms-v2/keep-aurora-spire.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-aurora-spire.spec.txt"),
    ),
    (
        "keep-boots-vault",
        include_str!("../../downwards-gen/rooms-v2/keep-boots-vault.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-boots-vault.spec.txt"),
    ),
    (
        "keep-crown-sanctum",
        include_str!("../../downwards-gen/rooms-v2/keep-crown-sanctum.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-crown-sanctum.spec.txt"),
    ),
    (
        "keep-meteor-run",
        include_str!("../../downwards-gen/rooms-v2/keep-meteor-run.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-meteor-run.spec.txt"),
    ),
    (
        "keep-observatory",
        include_str!("../../downwards-gen/rooms-v2/keep-observatory.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-observatory.spec.txt"),
    ),
    (
        "keep-shard-vault",
        include_str!("../../downwards-gen/rooms-v2/keep-shard-vault.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-shard-vault.spec.txt"),
    ),
    (
        "keep-skybridge",
        include_str!("../../downwards-gen/rooms-v2/keep-skybridge.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-skybridge.spec.txt"),
    ),
    (
        "keep-void-pass",
        include_str!("../../downwards-gen/rooms-v2/keep-void-pass.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-void-pass.spec.txt"),
    ),
    (
        "keep-wall-gate",
        include_str!("../../downwards-gen/rooms-v2/keep-wall-gate.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-wall-gate.spec.txt"),
    ),
    (
        "keep-west-postern",
        include_str!("../../downwards-gen/rooms-v2/keep-west-postern.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-west-postern.spec.txt"),
    ),
    (
        "keep-zenith-shaft",
        include_str!("../../downwards-gen/rooms-v2/keep-zenith-shaft.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-zenith-shaft.spec.txt"),
    ),
    (
        "keyhole-vault-a",
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-a.spec.txt"),
    ),
    (
        "keyhole-vault-b",
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-b.spec.txt"),
    ),
    (
        "keyhole-vault-c",
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-c.spec.txt"),
    ),
    (
        "keyhole-vault-d",
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-d.txt"),
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-d.spec.txt"),
    ),
    (
        "keyhole-vault-e",
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-e.txt"),
        include_str!("../../downwards-gen/rooms-v2/keyhole-vault-e.spec.txt"),
    ),
    (
        "lantern-cross-a",
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-a.spec.txt"),
    ),
    (
        "lantern-cross-b",
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-b.spec.txt"),
    ),
    (
        "low-ceiling-arena-a",
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-a.spec.txt"),
    ),
    (
        "low-ceiling-arena-b",
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-b.spec.txt"),
    ),
    (
        "low-ceiling-arena-c",
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-c.spec.txt"),
    ),
    (
        "metronome-gallery-a",
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-a.spec.txt"),
    ),
    (
        "metronome-gallery-b",
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-b.spec.txt"),
    ),
    (
        "metronome-gallery-c",
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/metronome-gallery-c.spec.txt"),
    ),
    (
        "one-way-loop-a",
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-a.spec.txt"),
    ),
    (
        "one-way-loop-b",
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-b.spec.txt"),
    ),
    (
        "one-way-loop-c",
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-c.spec.txt"),
    ),
    (
        "one-way-loop-d",
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-d.txt"),
        include_str!("../../downwards-gen/rooms-v2/one-way-loop-d.spec.txt"),
    ),
    (
        "sandglass-drop-a",
        include_str!("../../downwards-gen/rooms-v2/sandglass-drop-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/sandglass-drop-a.spec.txt"),
    ),
    (
        "sandglass-drop-b",
        include_str!("../../downwards-gen/rooms-v2/sandglass-drop-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/sandglass-drop-b.spec.txt"),
    ),
    (
        "shutter-chute-a",
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-a.spec.txt"),
    ),
    (
        "shutter-chute-b",
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-b.spec.txt"),
    ),
    (
        "shutter-chute-c",
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/shutter-chute-c.spec.txt"),
    ),
    (
        "strata-sort-a",
        include_str!("../../downwards-gen/rooms-v2/strata-sort-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/strata-sort-a.spec.txt"),
    ),
    (
        "strata-sort-b",
        include_str!("../../downwards-gen/rooms-v2/strata-sort-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/strata-sort-b.spec.txt"),
    ),
    (
        "strata-sort-c",
        include_str!("../../downwards-gen/rooms-v2/strata-sort-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/strata-sort-c.spec.txt"),
    ),
    (
        "switchback-spine-a",
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-a.spec.txt"),
    ),
    (
        "switchback-spine-b",
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-b.spec.txt"),
    ),
    (
        "switchback-spine-c",
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/switchback-spine-c.spec.txt"),
    ),
    (
        "tide-shaft-a",
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-a.spec.txt"),
    ),
    (
        "tide-shaft-b",
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-b.spec.txt"),
    ),
    (
        "tide-shaft-c",
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/tide-shaft-c.spec.txt"),
    ),
    (
        "two-clock-fork-a",
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-a.spec.txt"),
    ),
    (
        "two-clock-fork-b",
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-b.spec.txt"),
    ),
    (
        "two-clock-fork-c",
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-c.txt"),
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-c.spec.txt"),
    ),
];

/// Every rooms-v2 slug, sorted.
#[must_use]
pub fn rooms_v2_slugs() -> Vec<&'static str> {
    ROOMS.iter().map(|&(slug, _, _)| slug).collect()
}

/// Build a standalone room for a rooms-v2 slug: all coins present, doors
/// unwired (no destinations). Returns `None` for unknown slugs.
#[must_use]
pub fn rooms_v2_room(slug: &str) -> Option<Room> {
    let &(slug, grid, spec) = ROOMS.iter().find(|&&(known, _, _)| known == slug)?;
    let spec = parse_spec(spec);
    let doors = spec
        .doors
        .iter()
        .map(|&(door_id, side)| {
            let (trigger_bounds, arrival) = door_geometry(side);
            Door {
                id: door_id.to_owned(),
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
        .map(|(index, &bounds)| {
            Pickup::new(format!("v2-coin-{slug}-{index}"), bounds)
                .expect("authored v2 coin bounds are valid")
        })
        .collect();
    let room = Room::new(
        format!("rooms-v2.{slug}"),
        slug,
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        parse_room_grid(grid),
        Point::new(20, 148),
        vec![],
    )
    .expect("authored v2 grids build valid rooms")
    .with_objects(spec.hazards, pickups)
    .expect("authored v2 objects fit their rooms")
    .with_doors(doors)
    .expect("authored v2 doors are valid");
    Some(room)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_room_in_the_catalogue_builds() {
        let slugs = rooms_v2_slugs();
        assert!(slugs.len() >= 40);
        for slug in slugs {
            let room = rooms_v2_room(slug).expect("listed slug builds");
            assert_eq!(room.name(), slug);
        }
        assert!(rooms_v2_room("no-such-room").is_none());
    }
}
