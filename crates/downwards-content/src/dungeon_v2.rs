//! Dungeon v2: the redesigned dungeon, assembled data-driven from the
//! prototype room grids in `downwards-gen/rooms-v2` plus the layout file in
//! `docs/design/dungeon-v2-layout.txt` (both compiled in). Unlike the
//! hand-coded demo dungeon, rooms, doors, gates, and pickups here derive
//! entirely from those artifacts, so the design pipeline's verified layout is
//! exactly what ships.

use std::{collections::BTreeMap, sync::OnceLock};

use downwards_core::{BoundarySide, Door, Exit, Pickup, Point, Rect, Room, Tile, TimedHazard};
use downwards_gen::parse_room_grid;

use crate::{
    DEMO_DUNGEON_BOOT_PICKUP, DEMO_DUNGEON_CROWN_PICKUP, DEMO_DUNGEON_GLOVE_PICKUP,
    DEMO_DUNGEON_GOAL_EXIT,
};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

const LAYOUT: &str = include_str!("../../../docs/design/dungeon-v2-layout.txt");

/// (slug, grid, spec) for every distinct grid the layout uses.
const GRIDS: &[(&str, &str, &str)] = &[
    (
        "antiphase-airlock-a",
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/antiphase-airlock-a.spec.txt"),
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
        "keep-astral-seal",
        include_str!("../../downwards-gen/rooms-v2/keep-astral-seal.txt"),
        include_str!("../../downwards-gen/rooms-v2/keep-astral-seal.spec.txt"),
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
        "lantern-cross-a",
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/lantern-cross-a.spec.txt"),
    ),
    (
        "low-ceiling-arena-a",
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-a.txt"),
        include_str!("../../downwards-gen/rooms-v2/low-ceiling-arena-a.spec.txt"),
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
        "two-clock-fork-b",
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-b.txt"),
        include_str!("../../downwards-gen/rooms-v2/two-clock-fork-b.spec.txt"),
    ),
];

/// A door requirement in dungeon v2, mirroring the demo dungeon's gate model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DungeonV2Requirement {
    ClimbingGloves,
    WingedBoots,
    Coins(u16),
}

/// Parsed room `.spec.txt` payload, shared with the rooms-v2 catalogue so
/// hazard timing truth is only ever parsed in one place.
#[derive(Clone, Debug)]
pub(crate) struct SpecData {
    pub(crate) doors: Vec<(&'static str, BoundarySide)>,
    pub(crate) coins: Vec<Rect>,
    pub(crate) hazards: Vec<TimedHazard>,
}

#[derive(Clone, Debug)]
pub struct DungeonV2Instance {
    pub id: String,
    pub slug: String,
    /// door id -> (destination instance, destination door)
    pub connections: BTreeMap<String, (String, String)>,
}

#[derive(Debug)]
pub struct DungeonV2 {
    pub instances: BTreeMap<String, DungeonV2Instance>,
    pub spawn: String,
    pub goal: String,
    pub glove_room: String,
    pub boots_room: String,
    glove_bounds: Option<Rect>,
    boots_bounds: Option<Rect>,
    gates: BTreeMap<(String, String), DungeonV2Requirement>,
    specs: BTreeMap<&'static str, SpecData>,
    grids: BTreeMap<&'static str, Vec<Tile>>,
}

/// Total coins placed in dungeon v2 (the glove/boots rooms trade their coin
/// for the ability pickup, so those coins are excluded).
#[must_use]
pub fn dungeon_v2_total_coins() -> u16 {
    let dungeon = dungeon_v2_definition();
    dungeon
        .instances
        .values()
        .map(|instance| {
            let hosts_ability = (instance.id == dungeon.glove_room
                && dungeon.glove_bounds.is_none())
                || (instance.id == dungeon.boots_room && dungeon.boots_bounds.is_none());
            let coins = dungeon.specs[instance.slug.as_str()].coins.len() as u16;
            if hosts_ability { coins - 1 } else { coins }
        })
        .sum()
}

pub(crate) fn parse_spec(source: &str) -> SpecData {
    let mut spec = SpecData {
        doors: Vec::new(),
        coins: Vec::new(),
        hazards: Vec::new(),
    };
    for raw in source.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next().expect("non-empty spec line has a keyword") {
            "door" => spec.doors.push(match parts.next() {
                Some("west") => ("west", BoundarySide::Left),
                Some("east") => ("east", BoundarySide::Right),
                Some("ceiling") => ("ceiling", BoundarySide::Ceiling),
                Some("floor") => ("floor", BoundarySide::Floor),
                other => panic!("unknown door side {other:?}"),
            }),
            "coin" => {
                let mut number = || -> i32 {
                    parts
                        .next()
                        .expect("coin coordinate")
                        .parse()
                        .expect("number")
                };
                let (x, y) = (number(), number());
                spec.coins.push(Rect::new(x, y, 8, 10));
            }
            "hazard" => {
                let values = parts
                    .map(|part| part.parse::<i32>().expect("hazard number"))
                    .collect::<Vec<_>>();
                assert_eq!(values.len(), 7, "hazard takes 7 numbers");
                spec.hazards.push(
                    TimedHazard::new(
                        Rect::new(values[0], values[1], values[2], values[3]),
                        u32::try_from(values[4]).expect("period fits u32"),
                        u32::try_from(values[5]).expect("active fits u32"),
                        u32::try_from(values[6]).expect("phase fits u32"),
                    )
                    .expect("authored v2 hazards satisfy timed-hazard invariants"),
                );
            }
            other => panic!("unknown spec directive {other:?}"),
        }
    }
    spec
}

pub(crate) fn door_geometry(side: BoundarySide) -> (Rect, Point) {
    match side {
        BoundarySide::Left => (Rect::new(0, 130, 8, 40), Point::new(12, 148)),
        BoundarySide::Right => (Rect::new(312, 130, 8, 40), Point::new(300, 148)),
        BoundarySide::Ceiling => (Rect::new(140, 0, 40, 8), Point::new(150, 12)),
        BoundarySide::Floor => (Rect::new(140, 172, 40, 8), Point::new(150, 148)),
    }
}

fn build_definition() -> DungeonV2 {
    let specs: BTreeMap<_, _> = GRIDS
        .iter()
        .map(|&(slug, _, spec)| (slug, parse_spec(spec)))
        .collect();
    let grids: BTreeMap<_, _> = GRIDS
        .iter()
        .map(|&(slug, grid, _)| (slug, parse_room_grid(grid)))
        .collect();

    let mut instances = BTreeMap::new();
    let mut gates = BTreeMap::new();
    let mut spawn = None;
    let mut goal = None;
    let mut glove_room: Option<(String, Option<Rect>)> = None;
    let mut boots_room: Option<(String, Option<Rect>)> = None;
    let mut edges = Vec::new();
    for raw in LAYOUT.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        match parts[0] {
            "room" => {
                let slug = GRIDS
                    .iter()
                    .map(|&(slug, _, _)| slug)
                    .find(|&slug| slug == parts[2])
                    .unwrap_or_else(|| panic!("layout uses unknown grid {}", parts[2]));
                instances.insert(
                    parts[1].to_owned(),
                    DungeonV2Instance {
                        id: parts[1].to_owned(),
                        slug: slug.to_owned(),
                        connections: BTreeMap::new(),
                    },
                );
            }
            "edge" => edges.push((
                parts[1].to_owned(),
                parts[2].to_owned(),
                parts[3].to_owned(),
                parts[4].to_owned(),
            )),
            "gate" => {
                let requirement = match (parts[3], parts[4]) {
                    ("ability", "wall") => DungeonV2Requirement::ClimbingGloves,
                    ("ability", "dash") => DungeonV2Requirement::WingedBoots,
                    ("coins", n) => {
                        DungeonV2Requirement::Coins(n.parse().expect("coin gate count"))
                    }
                    other => panic!("unknown gate {other:?}"),
                };
                gates.insert((parts[1].to_owned(), parts[2].to_owned()), requirement);
            }
            "pickup" => {
                let placement = (
                    parts[2].to_owned(),
                    (parts.len() >= 5).then(|| {
                        Rect::new(
                            parts[3].parse().expect("pickup x"),
                            parts[4].parse().expect("pickup y"),
                            16,
                            16,
                        )
                    }),
                );
                match parts[1] {
                    "glove" => glove_room = Some(placement),
                    "boots" => boots_room = Some(placement),
                    other => panic!("unknown pickup {other:?}"),
                }
            }
            "spawn" => spawn = Some(parts[1].to_owned()),
            "goal" => goal = Some(parts[1].to_owned()),
            other => panic!("unknown layout directive {other:?}"),
        }
    }
    for (room_a, door_a, room_b, door_b) in edges {
        let entry_a = instances
            .get_mut(&room_a)
            .unwrap_or_else(|| panic!("edge references unknown room {room_a}"));
        assert!(
            entry_a
                .connections
                .insert(door_a.clone(), (room_b.clone(), door_b.clone()))
                .is_none(),
            "door {room_a}.{door_a} wired twice"
        );
        let entry_b = instances
            .get_mut(&room_b)
            .unwrap_or_else(|| panic!("edge references unknown room {room_b}"));
        assert!(
            entry_b
                .connections
                .insert(door_b.clone(), (room_a.clone(), door_a.clone()))
                .is_none(),
            "door {room_b}.{door_b} wired twice"
        );
    }
    for instance in instances.values() {
        for &(door_id, _) in &specs[instance.slug.as_str()].doors {
            assert!(
                instance.connections.contains_key(door_id),
                "door {}.{door_id} is not wired to anything",
                instance.id
            );
        }
    }
    let (glove_room, glove_bounds) = glove_room.expect("layout places the glove");
    let (boots_room, boots_bounds) = boots_room.expect("layout places the boots");
    DungeonV2 {
        instances,
        spawn: spawn.expect("layout declares a spawn"),
        goal: goal.expect("layout declares a goal"),
        glove_room,
        boots_room,
        glove_bounds,
        boots_bounds,
        gates,
        specs,
        grids,
    }
}

/// The static dungeon v2 topology.
#[must_use]
pub fn dungeon_v2_definition() -> &'static DungeonV2 {
    static DEFINITION: OnceLock<DungeonV2> = OnceLock::new();
    DEFINITION.get_or_init(build_definition)
}

/// The requirement guarding a door, if any.
#[must_use]
pub fn dungeon_v2_door_requirement(instance: &str, door: &str) -> Option<DungeonV2Requirement> {
    dungeon_v2_definition()
        .gates
        .get(&(instance.to_owned(), door.to_owned()))
        .copied()
}

/// The stable id of a dungeon v2 coin.
#[must_use]
pub fn dungeon_v2_coin_id(instance: &str, index: usize) -> String {
    format!("v2-coin-{instance}-{index}")
}

/// Persistent run inventory for dungeon v2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DungeonV2Inventory {
    pub climbing_gloves: bool,
    pub winged_boots: bool,
    pub crown: bool,
    pub coins: std::collections::BTreeSet<String>,
}

impl DungeonV2Inventory {
    #[must_use]
    pub const fn abilities(&self) -> downwards_core::AbilitySet {
        downwards_core::AbilitySet::new(self.climbing_gloves, self.winged_boots)
    }

    #[must_use]
    pub fn satisfies(&self, requirement: DungeonV2Requirement) -> bool {
        match requirement {
            DungeonV2Requirement::ClimbingGloves => self.climbing_gloves,
            DungeonV2Requirement::WingedBoots => self.winged_boots,
            DungeonV2Requirement::Coins(count) => self.coins.len() >= usize::from(count),
        }
    }
}

/// Build one dungeon v2 room instance against the persistent inventory.
/// Collected coins and owned ability pickups are omitted, matching the demo
/// dungeon's reconstruction contract.
#[must_use]
pub fn dungeon_v2_room(instance_id: &str, inventory: &DungeonV2Inventory) -> Room {
    let dungeon = dungeon_v2_definition();
    let instance = dungeon
        .instances
        .get(instance_id)
        .unwrap_or_else(|| panic!("unknown dungeon v2 room {instance_id}"));
    let spec = &dungeon.specs[instance.slug.as_str()];
    let is_goal = instance_id == dungeon.goal;
    let exits = (is_goal && !inventory.crown)
        .then(|| Exit {
            id: DEMO_DUNGEON_GOAL_EXIT.to_owned(),
            bounds: Rect::new(302, 12, 8, 18),
            destination: None,
            destination_entrance: None,
        })
        .into_iter()
        .collect();
    let doors = spec
        .doors
        .iter()
        .map(|&(door_id, side)| {
            let (trigger_bounds, arrival) = door_geometry(side);
            let (destination_room, destination_door) = instance.connections[door_id].clone();
            Door {
                id: door_id.to_owned(),
                side,
                trigger_bounds,
                arrival,
                destination_room: Some(destination_room),
                destination_door: Some(destination_door),
            }
        })
        .collect::<Vec<_>>();

    // An ability room without an explicit pickup position trades its first
    // (verified reachable and bankable) coin position for the pickup.
    let coin_hosts_ability = (instance_id == dungeon.glove_room && dungeon.glove_bounds.is_none())
        || (instance_id == dungeon.boots_room && dungeon.boots_bounds.is_none());
    let mut pickups = Vec::new();
    for (index, &bounds) in spec.coins.iter().enumerate() {
        if coin_hosts_ability && index == 0 {
            continue;
        }
        let id = dungeon_v2_coin_id(instance_id, index);
        if !inventory.coins.contains(&id) {
            pickups.push(Pickup::new(id, bounds).expect("authored v2 coin bounds are valid"));
        }
    }
    let from_coin = |bounds: Rect| Rect::new(bounds.x - 4, bounds.y - 6, 16, 16);
    if instance_id == dungeon.glove_room && !inventory.climbing_gloves {
        let bounds = dungeon.glove_bounds.unwrap_or_else(|| {
            from_coin(*spec.coins.first().expect("glove room has a coin position"))
        });
        pickups
            .push(Pickup::new(DEMO_DUNGEON_GLOVE_PICKUP, bounds).expect("glove bounds are valid"));
    }
    if instance_id == dungeon.boots_room && !inventory.winged_boots {
        let bounds = dungeon.boots_bounds.unwrap_or_else(|| {
            from_coin(*spec.coins.first().expect("boots room has a coin position"))
        });
        pickups
            .push(Pickup::new(DEMO_DUNGEON_BOOT_PICKUP, bounds).expect("boots bounds are valid"));
    }
    if is_goal && !inventory.crown {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_CROWN_PICKUP, Rect::new(285, 14, 16, 16))
                .expect("crown bounds are valid"),
        );
    }

    Room::new(
        format!("dungeon-v2.{instance_id}"),
        instance.slug.clone(),
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        dungeon.grids[instance.slug.as_str()].clone(),
        Point::new(20, 148),
        exits,
    )
    .expect("authored v2 grids build valid rooms")
    .with_objects(spec.hazards.clone(), pickups)
    .expect("authored v2 objects fit their rooms")
    .with_doors(doors)
    .expect("authored v2 doors are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_parses_and_every_door_is_wired_both_ways() {
        let dungeon = dungeon_v2_definition();
        assert!(
            (26..=34).contains(&dungeon.instances.len()),
            "dungeon v2 should stay between 26 and 34 rooms, has {}",
            dungeon.instances.len()
        );
        for instance in dungeon.instances.values() {
            for (door, (other_id, other_door)) in &instance.connections {
                let other = &dungeon.instances[other_id];
                assert_eq!(
                    other.connections[other_door],
                    (instance.id.clone(), door.clone()),
                    "edge {}.{door} <-> {other_id}.{other_door} is not symmetric",
                    instance.id
                );
            }
        }
        assert!(dungeon.instances.contains_key(&dungeon.spawn));
        assert!(dungeon.instances.contains_key(&dungeon.goal));
    }

    #[test]
    fn every_room_builds_and_ability_rooms_host_their_pickups() {
        let dungeon = dungeon_v2_definition();
        let bare = DungeonV2Inventory::default();
        for instance_id in dungeon.instances.keys() {
            let room = dungeon_v2_room(instance_id, &bare);
            assert_eq!(room.width(), WIDTH);
            for door in room.doors() {
                assert!(door.destination_room.is_some());
            }
        }
        let glove_room = dungeon_v2_room(&dungeon.glove_room, &bare);
        assert!(
            glove_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_GLOVE_PICKUP)
        );
        let boots_room = dungeon_v2_room(&dungeon.boots_room, &bare);
        assert!(
            boots_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_BOOT_PICKUP)
        );
        let goal_room = dungeon_v2_room(&dungeon.goal, &bare);
        assert!(!goal_room.exits().is_empty());
    }

    #[test]
    fn the_crown_coin_gate_is_satisfiable_with_slack() {
        let total = dungeon_v2_total_coins();
        let DungeonV2Requirement::Coins(gate) =
            dungeon_v2_door_requirement("asl", "east").expect("crown door is coin gated")
        else {
            panic!("crown door gate is not a coin gate")
        };
        assert!(
            total >= gate + 4,
            "crown gate {gate} leaves too little slack against {total} total coins"
        );
    }

    #[test]
    fn collected_inventory_removes_pickups_on_rebuild() {
        let dungeon = dungeon_v2_definition();
        let mut inventory = DungeonV2Inventory::default();
        inventory.climbing_gloves = true;
        inventory
            .coins
            .insert(dungeon_v2_coin_id(&dungeon.spawn, 1));
        let room = dungeon_v2_room(&dungeon.glove_room, &inventory);
        assert!(
            room.pickups()
                .iter()
                .all(|pickup| pickup.id() != DEMO_DUNGEON_GLOVE_PICKUP)
        );
        let spawn_room = dungeon_v2_room(&dungeon.spawn, &inventory);
        assert!(
            spawn_room
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != dungeon_v2_coin_id(&dungeon.spawn, 1))
        );
    }
}
