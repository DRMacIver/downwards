//! Dungeon v2: the redesigned dungeon, assembled data-driven from the
//! prototype room grids in `downwards-gen/rooms-v2` plus the layout file in
//! `docs/design/dungeon-v2-layout.txt` (both compiled in). Unlike the
//! hand-coded demo dungeon, rooms, doors, gates, and pickups here derive
//! entirely from those artifacts, so the design pipeline's verified layout is
//! exactly what ships.

use std::{collections::BTreeMap, sync::OnceLock};

use downwards_core::{BoundarySide, Door, Exit, Pickup, Point, Rect, Room, Tile, TimedHazard};
use downwards_gen::parse_room_grid;

/// Pickup ID for the climbing gloves.
pub const DUNGEON_V2_GLOVE_PICKUP: &str = "climbing-gloves";
/// Pickup ID for the winged boots.
pub const DUNGEON_V2_BOOT_PICKUP: &str = "winged-boots";
/// Pickup ID for the crown.
pub const DUNGEON_V2_CROWN_PICKUP: &str = "crown";
/// Exit ID for the crown goal exit.
pub const DUNGEON_V2_GOAL_EXIT: &str = "crown-goal";

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
    /// The crown itself: guards the exit room's ceiling escape door.
    Crown,
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
    /// The room whose ceiling door is the crown-locked escape out of the
    /// dungeon. Declared by the layout's `exit` directive; the layout tooling
    /// guarantees it sits on the top row with its ceiling door edge-free.
    pub exit: String,
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
    let mut exit = None;
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
            "exit" => exit = Some(parts[1].to_owned()),
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
    // The escape is the exit room's CEILING door: locked (crown-gated) on
    // the way in, and the winning exit once the crown is held. The exit room
    // sits on the top row with nothing above it, so that door leads out of
    // the dungeon and is deliberately NOT an edge. The gate is structural
    // (derived from the layout's `exit` directive rather than a `gate` line).
    let exit_id = exit.expect("layout declares an exit room");
    assert!(
        instances.contains_key(&exit_id),
        "exit room {exit_id} is not a declared room"
    );
    for instance in instances.values() {
        for &(door_id, _) in &specs[instance.slug.as_str()].doors {
            if instance.id == exit_id && door_id == "ceiling" {
                // The escape door leads out of the dungeon, not to a room.
                assert!(
                    !instance.connections.contains_key(door_id),
                    "the exit room's ceiling escape door must not be an edge"
                );
                continue;
            }
            assert!(
                instance.connections.contains_key(door_id),
                "door {}.{door_id} is not wired to anything",
                instance.id
            );
        }
    }
    assert!(
        specs[instances[&exit_id].slug.as_str()]
            .doors
            .iter()
            .any(|&(door_id, _)| door_id == "ceiling"),
        "exit room {exit_id} has no ceiling door to escape through"
    );
    assert!(
        gates
            .insert(
                (exit_id.clone(), "ceiling".to_owned()),
                DungeonV2Requirement::Crown
            )
            .is_none(),
        "the exit room's ceiling escape door must not carry another gate"
    );
    let (glove_room, glove_bounds) = glove_room.expect("layout places the glove");
    let (boots_room, boots_bounds) = boots_room.expect("layout places the boots");
    DungeonV2 {
        instances,
        spawn: spawn.expect("layout declares a spawn"),
        goal: goal.expect("layout declares a goal"),
        exit: exit_id,
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
            DungeonV2Requirement::Crown => self.crown,
        }
    }
}

/// Where the escape sits: the exit room's CEILING door mouth, at the top of
/// the dungeon (nothing above the exit room; the door leads outside).
/// Pre-crown the door is crown-locked; once the crown is held the mouth
/// carries the winning exit trigger (exits take priority over the coincident
/// door in the core, so touching the mouth ends the run). The client draws
/// the locked/open door treatment at this rectangle in every crown state.
#[must_use]
pub fn dungeon_v2_exit_gate_bounds() -> Rect {
    door_geometry(BoundarySide::Ceiling).0
}

/// Where the player respawns after dying in the crown room while holding the
/// crown: the crown pickup point (standing atop the pedestal block under the
/// crown at `Rect(285, 14, 16, 16)`), not the room entry.
#[must_use]
pub fn dungeon_v2_crown_respawn_point() -> Point {
    Point::new(290, 18)
}

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
    // The run ends by climbing back OUT through the exit room's ceiling
    // door: the exit trigger only exists once the crown is held, and it
    // shares the door mouth's bounds (that door has no destination — it
    // leads out of the dungeon, and pre-crown the client rejects it as
    // crown-sealed). The locked-door marking itself is always drawn by the
    // client so the player learns the exit's location on the way in.
    let is_exit_room = instance_id == dungeon.exit;
    let exits = (is_exit_room && inventory.crown)
        .then(|| Exit {
            id: DUNGEON_V2_GOAL_EXIT.to_owned(),
            bounds: dungeon_v2_exit_gate_bounds(),
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
            // The exit room's ceiling escape door has no destination: it
            // leads out of the dungeon rather than to another room.
            let connection = instance.connections.get(door_id).cloned();
            let (destination_room, destination_door) = match connection {
                Some((room, door)) => (Some(room), Some(door)),
                None => (None, None),
            };
            Door {
                id: door_id.to_owned(),
                side,
                trigger_bounds,
                arrival,
                destination_room,
                destination_door,
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
            .push(Pickup::new(DUNGEON_V2_GLOVE_PICKUP, bounds).expect("glove bounds are valid"));
    }
    if instance_id == dungeon.boots_room && !inventory.winged_boots {
        let bounds = dungeon.boots_bounds.unwrap_or_else(|| {
            from_coin(*spec.coins.first().expect("boots room has a coin position"))
        });
        pickups
            .push(Pickup::new(DUNGEON_V2_BOOT_PICKUP, bounds).expect("boots bounds are valid"));
    }
    if is_goal && !inventory.crown {
        pickups.push(
            Pickup::new(DUNGEON_V2_CROWN_PICKUP, Rect::new(285, 14, 16, 16))
                .expect("crown bounds are valid"),
        );
    }

    // Post-crown, dying in the crown room respawns at the crown point (the
    // canonical spawn is only used when the client rebuilds without an entry
    // door, which it does exactly for that death case).
    let spawn_point = if is_goal && inventory.crown {
        dungeon_v2_crown_respawn_point()
    } else {
        Point::new(20, 148)
    };
    Room::new(
        format!("dungeon-v2.{instance_id}"),
        instance.slug.clone(),
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        dungeon.grids[instance.slug.as_str()].clone(),
        spawn_point,
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
                if *instance_id == dungeon.exit && door.id == "ceiling" {
                    // The escape door leads out of the dungeon.
                    assert!(door.destination_room.is_none());
                } else {
                    assert!(door.destination_room.is_some());
                }
            }
        }
        let glove_room = dungeon_v2_room(&dungeon.glove_room, &bare);
        assert!(
            glove_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DUNGEON_V2_GLOVE_PICKUP)
        );
        let boots_room = dungeon_v2_room(&dungeon.boots_room, &bare);
        assert!(
            boots_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DUNGEON_V2_BOOT_PICKUP)
        );
        let goal_room = dungeon_v2_room(&dungeon.goal, &bare);
        assert!(
            goal_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DUNGEON_V2_CROWN_PICKUP)
        );
    }

    #[test]
    fn the_escape_gate_opens_in_the_exit_room_only_with_the_crown() {
        let dungeon = dungeon_v2_definition();
        let bare = DungeonV2Inventory::default();
        for instance_id in dungeon.instances.keys() {
            assert!(
                dungeon_v2_room(instance_id, &bare).exits().is_empty(),
                "no exit anywhere before the crown ({instance_id})"
            );
        }
        let mut crowned = DungeonV2Inventory::default();
        crowned.climbing_gloves = true;
        crowned.winged_boots = true;
        crowned.crown = true;
        let exit_room = dungeon_v2_room(&dungeon.exit, &crowned);
        let exit = exit_room.exits().first().expect("exit room hosts the escape");
        assert_eq!(exit.id, DUNGEON_V2_GOAL_EXIT);
        assert_eq!(exit.bounds, dungeon_v2_exit_gate_bounds());
        assert!(
            dungeon_v2_room(&dungeon.spawn, &crowned).exits().is_empty(),
            "the spawn room no longer hosts the escape"
        );
        assert!(
            dungeon_v2_room(&dungeon.goal, &crowned).exits().is_empty(),
            "the crown room is no longer terminal"
        );
    }

    #[test]
    fn the_escape_is_the_exit_room_ceiling_door_and_it_is_crown_locked() {
        let dungeon = dungeon_v2_definition();
        // The exit trigger IS the ceiling door mouth of the exit room, so
        // the escape door sits on the ceiling at the top of the dungeon.
        let (ceiling_mouth, _) = door_geometry(BoundarySide::Ceiling);
        assert_eq!(dungeon_v2_exit_gate_bounds(), ceiling_mouth);
        let exit_room = dungeon_v2_room(&dungeon.exit, &DungeonV2Inventory::default());
        let ceiling = exit_room
            .doors()
            .iter()
            .find(|door| door.id == "ceiling")
            .expect("exit room has a ceiling door");
        assert_eq!(ceiling.trigger_bounds, dungeon_v2_exit_gate_bounds());
        // The escape door leads OUT of the dungeon: no destination room, and
        // no edge points back at it either.
        assert!(ceiling.destination_room.is_none());
        assert!(
            !dungeon.instances[&dungeon.exit]
                .connections
                .contains_key("ceiling"),
            "the exit room's ceiling door must not be a connection edge"
        );
        for instance in dungeon.instances.values() {
            for (destination, destination_door) in instance.connections.values() {
                assert!(
                    !(*destination == dungeon.exit && destination_door == "ceiling"),
                    "{}'s connections target the escape door",
                    instance.id
                );
            }
        }
        // The spawn room's ceiling is an ordinary edge again: it climbs into
        // the exit room above (the roof-cap the delver fell in through).
        let spawn = &dungeon.instances[&dungeon.spawn];
        assert_eq!(
            spawn.connections["ceiling"],
            (dungeon.exit.clone(), "floor".to_owned()),
            "spawn ceiling leads up into the exit room"
        );
        assert!(dungeon_v2_door_requirement(&dungeon.spawn, "ceiling").is_none());
        // Locked without the crown, open with it.
        let requirement = dungeon_v2_door_requirement(&dungeon.exit, "ceiling")
            .expect("the exit room's ceiling door is gated");
        assert_eq!(requirement, DungeonV2Requirement::Crown);
        assert!(!DungeonV2Inventory::default().satisfies(requirement));
        let mut crowned = DungeonV2Inventory::default();
        crowned.crown = true;
        assert!(crowned.satisfies(requirement));
    }

    #[test]
    fn dying_with_the_crown_respawns_at_the_crown_point_in_the_crown_room() {
        let dungeon = dungeon_v2_definition();
        let mut crowned = DungeonV2Inventory::default();
        crowned.climbing_gloves = true;
        crowned.winged_boots = true;
        crowned.crown = true;
        // The crowned crown room's canonical spawn is the crown point...
        let room = dungeon_v2_room(&dungeon.goal, &crowned);
        assert_eq!(room.spawn(), dungeon_v2_crown_respawn_point());
        // ...and it stands where the crown was collected.
        let crown_bounds = Rect::new(285, 14, 16, 16);
        let respawn = dungeon_v2_crown_respawn_point();
        assert!(
            Rect::new(
                respawn.x,
                respawn.y,
                downwards_core::PLAYER_WIDTH,
                downwards_core::PLAYER_HEIGHT
            )
            .intersects(crown_bounds)
        );
        // Everywhere else (and pre-crown) keeps the normal entry spawn.
        assert_eq!(
            dungeon_v2_room(&dungeon.goal, &DungeonV2Inventory::default()).spawn(),
            Point::new(20, 148)
        );
        assert_eq!(
            dungeon_v2_room(&dungeon.spawn, &crowned).spawn(),
            Point::new(20, 148)
        );
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
                .all(|pickup| pickup.id() != DUNGEON_V2_GLOVE_PICKUP)
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
