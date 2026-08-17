//! Build `RoomMusicInputs` for every rooms-v2 room from the checked-in
//! design data: hazards come from constructed `Room` values (via the
//! `downwards-content` catalogue — spec text is never re-parsed here),
//! difficulty/ability/coins/doors from `docs/design/rooms-v2-metadata.json`,
//! and the inventory-layer gates from `docs/design/rooms-v2-passability.json`
//! (§6.7 of the soundtrack spec).

use std::{collections::BTreeMap, error::Error, fs, path::PathBuf};

use downwards_audio::{
    AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs,
};
use downwards_content::{rooms_v2_room, rooms_v2_slugs};
use downwards_core::BoundarySide;
use serde_json::Value;

/// The repository root, resolved from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives two levels below the repository root")
        .to_path_buf()
}

/// Metadata defaults for rooms with no entry in the metadata JSON, matching
/// the client's runtime fallback: medium difficulty, no ability requirement.
const DEFAULT_DIFFICULTY: Difficulty = Difficulty::Medium;
const DEFAULT_ABILITY: AbilityReq = AbilityReq::None;

struct Metadata {
    difficulty: Difficulty,
    ability_requirement: AbilityReq,
    coin_count: Option<u32>,
    doors: Option<DoorSet>,
}

fn parse_difficulty(text: &str) -> Result<Difficulty, Box<dyn Error>> {
    match text {
        "easy" => Ok(Difficulty::Easy),
        "medium" => Ok(Difficulty::Medium),
        "hard" => Ok(Difficulty::Hard),
        other => Err(format!("unknown difficulty {other:?}").into()),
    }
}

fn parse_ability(text: &str) -> Result<AbilityReq, Box<dyn Error>> {
    match text {
        "none" => Ok(AbilityReq::None),
        "wall" => Ok(AbilityReq::Wall),
        "dash" => Ok(AbilityReq::Dash),
        "both" => Ok(AbilityReq::Both),
        other => Err(format!("unknown ability requirement {other:?}").into()),
    }
}

fn parse_doors(names: &[Value]) -> DoorSet {
    let mut doors = DoorSet::default();
    for name in names {
        match name.as_str() {
            Some("west") => doors.west = true,
            Some("east") => doors.east = true,
            Some("ceiling") => doors.ceiling = true,
            Some("floor") => doors.floor = true,
            _ => {}
        }
    }
    doors
}

fn load_metadata() -> Result<BTreeMap<String, Metadata>, Box<dyn Error>> {
    let path = repo_root().join("docs/design/rooms-v2-metadata.json");
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let mut map = BTreeMap::new();
    for entry in parsed.as_array().ok_or("metadata JSON must be an array")? {
        let slug = entry["slug"]
            .as_str()
            .ok_or("metadata entry needs a slug")?
            .to_owned();
        map.insert(
            slug,
            Metadata {
                difficulty: parse_difficulty(
                    entry["difficulty"].as_str().ok_or("difficulty missing")?,
                )?,
                ability_requirement: parse_ability(
                    entry["ability_requirement"]
                        .as_str()
                        .ok_or("ability_requirement missing")?,
                )?,
                coin_count: entry["coin_count"].as_u64().map(|count| count as u32),
                doors: entry["doors"].as_array().map(|names| parse_doors(names)),
            },
        );
    }
    Ok(map)
}

/// §6.7: per-ability layer gates from the passability data. An ability is
/// gated when some door pair or coin route is reachable only with it, or is
/// materially (< 0.75×) shorter with it.
fn layer_gates(passability: Option<&Value>) -> LayerGates {
    let Some(entry) = passability else {
        return LayerGates::default();
    };
    let mut gloves = false;
    let mut boots = false;
    for section in ["pairs", "coin_routes"] {
        let Some(routes) = entry[section].as_object() else {
            continue;
        };
        for costs in routes.values() {
            let cost = |key: &str| costs[key].as_u64();
            let best = |a: Option<u64>, b: Option<u64>| match (a, b) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (value, None) | (None, value) => value,
            };
            let with_wall = best(cost("wall"), cost("both"));
            let without_wall = best(cost("none"), cost("dash"));
            let with_dash = best(cost("dash"), cost("both"));
            let without_dash = best(cost("none"), cost("wall"));
            let materially_shorter = |with: Option<u64>, without: Option<u64>| match (with, without)
            {
                (Some(_), None) => true,
                (Some(with), Some(without)) => 4 * with < 3 * without,
                _ => false,
            };
            gloves |= materially_shorter(with_wall, without_wall);
            boots |= materially_shorter(with_dash, without_dash);
        }
    }
    LayerGates { gloves, boots }
}

fn door_set_from_room(room: &downwards_core::Room) -> DoorSet {
    let mut doors = DoorSet::default();
    for door in room.doors() {
        match door.side {
            BoundarySide::Left => doors.west = true,
            BoundarySide::Right => doors.east = true,
            BoundarySide::Ceiling => doors.ceiling = true,
            BoundarySide::Floor => doors.floor = true,
        }
    }
    doors
}

/// Inputs for one room plus its difficulty (needed again by the sync lint).
pub struct RoomRecord {
    pub inputs: RoomMusicInputs,
}

/// Build composer inputs for every rooms-v2 room, sorted by slug.
pub fn all_room_inputs() -> Result<Vec<RoomRecord>, Box<dyn Error>> {
    let metadata = load_metadata()?;
    let passability_path = repo_root().join("docs/design/rooms-v2-passability.json");
    let passability: Value = serde_json::from_str(&fs::read_to_string(&passability_path)?)?;

    let mut records = Vec::new();
    for slug in rooms_v2_slugs() {
        let room = rooms_v2_room(slug).expect("catalogue slugs build rooms");
        let hazards = room
            .timed_hazards()
            .iter()
            .map(|hazard| HazardTiming {
                period: hazard.period_ticks(),
                active: hazard.active_ticks(),
                phase: hazard.phase_ticks(),
            })
            .collect();
        let meta = metadata.get(slug);
        let inputs = RoomMusicInputs {
            slug: slug.to_owned(),
            difficulty: meta.map_or(DEFAULT_DIFFICULTY, |meta| meta.difficulty),
            ability_requirement: meta.map_or(DEFAULT_ABILITY, |meta| meta.ability_requirement),
            coin_count: meta
                .and_then(|meta| meta.coin_count)
                .unwrap_or(room.pickups().len() as u32),
            doors: meta
                .and_then(|meta| meta.doors)
                .unwrap_or_else(|| door_set_from_room(&room)),
            hazards,
            layer_gates: layer_gates(passability.get(slug)),
        };
        records.push(RoomRecord { inputs });
    }
    Ok(records)
}
