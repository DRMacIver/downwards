//! Render a descriptive behavior audit for every floor in the authored demo dungeon.
//!
//! This deliberately does not produce a difficulty score. It joins the mechanically generated
//! representative routes, strength-one shaky-hand observations, tile-layout similarity, and any
//! recorded human attempts so that a designer can inspect disagreement between those sources.
//!
//! Run from the workspace root with:
//! `cargo run -p downwards-content --example audit_demo_dungeon`
//! or pass a different human-history JSONL path as the sole argument.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use downwards_content::{
    DemoDungeonRoom, demo_dungeon_definition, demo_dungeon_room, demo_dungeon_route_specs,
};
use downwards_core::{PLAYER_MOVEMENT_POLICY_VERSION, Simulation, Tile};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;
use serde_json::Value;

const DEFAULT_HISTORY: &str = "playtest-history/human-attempts-v1.jsonl";
const WITNESS_ARTIFACT: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AiRouteEvidence {
    ticks: u64,
    action_spans: u64,
    jump_presses: u64,
    accepted_jumps: u64,
    wall_jumps: u64,
    dashes: u64,
    horizontal_reversals: u64,
    shaky: Vec<ShakyEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShakyEvidence {
    family: String,
    successes: u64,
    trials: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HumanEvidence {
    attempts: u64,
    stale_attempts: u64,
    successes: u64,
    deaths: u64,
    resets: u64,
    wrong_doors: u64,
    transition_events: u64,
    stale_transition_events: u64,
    successful_ticks: Vec<u64>,
}

#[derive(Clone, Debug)]
struct GeometryEvidence {
    tiles: Vec<Tile>,
    hazard_tiles: usize,
    one_way_tiles: usize,
}

fn parse_u64(token: Option<&str>, line: usize, field: &str) -> Result<u64, String> {
    token
        .ok_or_else(|| format!("line {line}: missing {field}"))?
        .parse::<u64>()
        .map_err(|error| format!("line {line}: invalid {field}: {error}"))
}

fn parse_witness_artifact(
    input: &str,
) -> Result<BTreeMap<DemoDungeonRoom, AiRouteEvidence>, String> {
    let mut routes = BTreeMap::new();
    let mut current_room = None;
    let mut current = None;
    let mut saw_schema = false;

    for (line_index, line) in input.lines().enumerate() {
        let line_number = line_index + 1;
        let mut fields = line.split_ascii_whitespace();
        let Some(kind) = fields.next() else {
            continue;
        };
        match kind {
            "schema" => {
                if fields.next() != Some("downwards-demo-dungeon-witnesses-v1") {
                    return Err(format!("line {line_number}: unsupported witness schema"));
                }
                saw_schema = true;
            }
            "route" => {
                if current.is_some() {
                    return Err(format!("line {line_number}: nested route"));
                }
                let id = fields
                    .next()
                    .ok_or_else(|| format!("line {line_number}: missing route id"))?;
                current_room =
                    Some(DemoDungeonRoom::from_id(id).ok_or_else(|| {
                        format!("line {line_number}: unknown dungeon route {id:?}")
                    })?);
                current = Some(AiRouteEvidence::default());
            }
            "room" => {
                let id = fields
                    .next()
                    .ok_or_else(|| format!("line {line_number}: missing room id"))?;
                let room = DemoDungeonRoom::from_id(id)
                    .ok_or_else(|| format!("line {line_number}: unknown room {id:?}"))?;
                if Some(room) != current_room {
                    return Err(format!(
                        "line {line_number}: route and room identities differ"
                    ));
                }
            }
            "ticks" => {
                let evidence = current
                    .as_mut()
                    .ok_or_else(|| format!("line {line_number}: ticks outside route"))?;
                evidence.ticks = parse_u64(fields.next(), line_number, "ticks")?;
            }
            "observation" => {
                let evidence = current
                    .as_mut()
                    .ok_or_else(|| format!("line {line_number}: observation outside route"))?;
                evidence.action_spans = parse_u64(fields.next(), line_number, "action spans")?;
                evidence.jump_presses = parse_u64(fields.next(), line_number, "jump presses")?;
                evidence.accepted_jumps = parse_u64(fields.next(), line_number, "accepted jumps")?;
                evidence.wall_jumps = parse_u64(fields.next(), line_number, "wall jumps")?;
                evidence.dashes = parse_u64(fields.next(), line_number, "dashes")?;
                evidence.horizontal_reversals =
                    parse_u64(fields.next(), line_number, "horizontal reversals")?;
            }
            "shaky" => {
                let evidence = current
                    .as_mut()
                    .ok_or_else(|| format!("line {line_number}: shaky row outside route"))?;
                let family = fields
                    .next()
                    .ok_or_else(|| format!("line {line_number}: missing shaky family"))?;
                evidence.shaky.push(ShakyEvidence {
                    family: family.to_owned(),
                    successes: parse_u64(fields.next(), line_number, "shaky successes")?,
                    trials: parse_u64(fields.next(), line_number, "shaky trials")?,
                });
            }
            "end" => {
                let room = current_room
                    .take()
                    .ok_or_else(|| format!("line {line_number}: end outside route"))?;
                let evidence = current
                    .take()
                    .ok_or_else(|| format!("line {line_number}: end outside route"))?;
                if evidence.ticks == 0 {
                    return Err(format!("line {line_number}: route has no ticks"));
                }
                if routes.insert(room, evidence).is_some() {
                    return Err(format!("line {line_number}: duplicate route {}", room.id()));
                }
            }
            "palette-generation"
            | "player-movement-policy"
            | "tuning"
            | "shaky-policy"
            | "entry"
            | "target"
            | "abilities"
            | "span" => {}
            other => return Err(format!("line {line_number}: unknown row {other:?}")),
        }
    }

    if !saw_schema {
        return Err("witness artifact has no schema".to_owned());
    }
    if current.is_some() {
        return Err("witness artifact ends inside a route".to_owned());
    }
    if routes.len() != DemoDungeonRoom::ALL.len() {
        return Err(format!(
            "witness artifact has {} routes; expected {}",
            routes.len(),
            DemoDungeonRoom::ALL.len()
        ));
    }
    Ok(routes)
}

fn json_string<'a>(value: &'a Value, key: &str, line: usize) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("history line {line}: missing string {key:?}"))
}

fn current_initial_digests() -> BTreeMap<DemoDungeonRoom, String> {
    demo_dungeon_route_specs()
        .into_iter()
        .map(|spec| {
            let room = demo_dungeon_room(spec.room, spec.inventory);
            let mut simulation = match spec.entry_door {
                Some(door) => Simulation::enter_via_door(room, spec.inventory.abilities(), door)
                    .expect("authored route entry is valid"),
                None => Simulation::with_abilities(room, spec.inventory.abilities()),
            };
            simulation.enable_current_player_movement();
            (spec.room, simulation.digest().to_string())
        })
        .collect()
}

fn parse_history(
    input: &str,
    initial_digests: &BTreeMap<DemoDungeonRoom, String>,
    dungeon_definition_id: &str,
) -> Result<BTreeMap<DemoDungeonRoom, HumanEvidence>, String> {
    let mut evidence = BTreeMap::<DemoDungeonRoom, HumanEvidence>::new();
    for (line_index, line) in input.lines().enumerate() {
        let line_number = line_index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(line)
            .map_err(|error| format!("history line {line_number}: invalid JSON: {error}"))?;
        match value["schema"].as_str() {
            Some("downwards-human-attempt-v1" | "downwards-human-attempt-v2") => {
                let room_id = json_string(&value, "room_id", line_number)?;
                let Some(room) = DemoDungeonRoom::from_id(room_id) else {
                    continue;
                };
                let row = evidence.entry(room).or_default();
                let current_policy = value["player_movement_policy_version"].as_u64()
                    == Some(u64::from(PLAYER_MOVEMENT_POLICY_VERSION));
                let current_content = if value["schema"] == "downwards-human-attempt-v2" {
                    value["dungeon_definition_id"].as_str() == Some(dungeon_definition_id)
                        && value["palette_generation_version"].as_u64()
                            == Some(u64::from(DUNGEON_PALETTE_GENERATION_VERSION))
                } else {
                    value["initial_state_digest"].as_str() == Some(initial_digests[&room].as_str())
                };
                let is_current = current_policy && current_content;
                if !is_current {
                    row.stale_attempts += 1;
                    continue;
                }
                row.attempts += 1;
                let outcome = value["outcome"]["kind"]
                    .as_str()
                    .ok_or_else(|| format!("history line {line_number}: missing outcome kind"))?;
                match outcome {
                    "success" => {
                        row.successes += 1;
                        let ticks = value["total_ticks"].as_u64().ok_or_else(|| {
                            format!("history line {line_number}: missing total_ticks")
                        })?;
                        row.successful_ticks.push(ticks);
                    }
                    "death" => row.deaths += 1,
                    "reset" => row.resets += 1,
                    "wrong_door" => row.wrong_doors += 1,
                    other => {
                        return Err(format!(
                            "history line {line_number}: unknown attempt outcome {other:?}"
                        ));
                    }
                }
            }
            Some("downwards-dungeon-progress-v1" | "downwards-dungeon-progress-v2") => {
                let room_id = json_string(&value, "room_id", line_number)?;
                let Some(room) = DemoDungeonRoom::from_id(room_id) else {
                    continue;
                };
                if value["event"].as_str() == Some("room transition") {
                    let row = evidence.entry(room).or_default();
                    let is_current = value["schema"] == "downwards-dungeon-progress-v2"
                        && value["dungeon_definition_id"].as_str() == Some(dungeon_definition_id)
                        && value["palette_generation_version"].as_u64()
                            == Some(u64::from(DUNGEON_PALETTE_GENERATION_VERSION));
                    if is_current {
                        row.transition_events += 1;
                    } else {
                        row.stale_transition_events += 1;
                    }
                }
            }
            Some(_) | None => {}
        }
    }
    for row in evidence.values_mut() {
        row.successful_ticks.sort_unstable();
    }
    Ok(evidence)
}

fn geometry_evidence() -> BTreeMap<DemoDungeonRoom, GeometryEvidence> {
    demo_dungeon_route_specs()
        .into_iter()
        .map(|spec| {
            let room = demo_dungeon_room(spec.room, spec.inventory);
            let tiles = room.tiles().to_vec();
            let hazard_tiles = tiles.iter().filter(|tile| tile.is_hazard()).count();
            let one_way_tiles = tiles
                .iter()
                .filter(|tile| matches!(tile, Tile::OneWay))
                .count();
            (
                spec.room,
                GeometryEvidence {
                    tiles,
                    hazard_tiles,
                    one_way_tiles,
                },
            )
        })
        .collect()
}

fn tile_distance(left: &GeometryEvidence, right: &GeometryEvidence) -> usize {
    if left.tiles.len() != right.tiles.len() {
        return usize::MAX;
    }
    left.tiles
        .iter()
        .zip(&right.tiles)
        .filter(|(left, right)| left != right)
        .count()
}

fn nearest_geometry(
    room: DemoDungeonRoom,
    all: &BTreeMap<DemoDungeonRoom, GeometryEvidence>,
) -> (DemoDungeonRoom, usize) {
    let geometry = &all[&room];
    all.iter()
        .filter(|(candidate, _)| **candidate != room)
        .map(|(candidate, candidate_geometry)| {
            (*candidate, tile_distance(geometry, candidate_geometry))
        })
        .min_by_key(|(candidate, distance)| (*distance, candidate.id()))
        .expect("the dungeon has more than one room")
}

fn median(values: &[u64]) -> Option<u64> {
    values.get(values.len() / 2).copied()
}

fn shaky_summary(evidence: &AiRouteEvidence) -> String {
    evidence
        .shaky
        .iter()
        .min_by_key(|row| {
            (
                row.successes.saturating_mul(10_000) / row.trials.max(1),
                &row.family,
            )
        })
        .map_or_else(
            || "n/a".to_owned(),
            |row| format!("{}/{} {}", row.successes, row.trials, row.family),
        )
}

fn human_summary(evidence: &HumanEvidence) -> String {
    let median = median(&evidence.successful_ticks)
        .map_or_else(|| "-".to_owned(), |ticks| ticks.to_string());
    format!(
        "{}a {}ok {}d {}r {}w med={}t trans={} stale={}/{}",
        evidence.attempts,
        evidence.successes,
        evidence.deaths,
        evidence.resets,
        evidence.wrong_doors,
        median,
        evidence.transition_events,
        evidence.stale_attempts,
        evidence.stale_transition_events,
    )
}

fn inspection_flags(
    ai: &AiRouteEvidence,
    human: &HumanEvidence,
    nearest_distance: usize,
) -> String {
    let mut flags = Vec::new();
    if human.stale_attempts > 0 || human.stale_transition_events > 0 {
        flags.push("STALE-HUMAN");
    }
    if human.attempts == 0 && human.transition_events == 0 {
        if human.stale_attempts == 0 && human.stale_transition_events == 0 {
            flags.push("NO-HUMAN");
        }
    } else if human.attempts > 0 && human.successes == 0 {
        flags.push("HUMAN-NO-SUCCESS");
    }
    if human.deaths + human.resets >= 3 {
        flags.push("HUMAN-RETRIES");
    }
    if ai
        .shaky
        .iter()
        .any(|row| row.trials > 0 && row.successes == 0)
    {
        flags.push("SHAKY-ZERO");
    }
    if ai.action_spans >= 32 {
        flags.push("AI-BUSY");
    }
    if ai.horizontal_reversals >= 8 {
        flags.push("AI-REVERSING");
    }
    if ai.jump_presses > ai.accepted_jumps.saturating_add(1) {
        flags.push("AI-REJECTED-JUMPS");
    }
    match nearest_distance {
        0 => flags.push("TILE-COPY"),
        1..=8 => flags.push("TILE-NEAR-COPY"),
        _ => {}
    }
    if flags.is_empty() {
        "-".to_owned()
    } else {
        flags.join(",")
    }
}

fn render_report(
    ai: &BTreeMap<DemoDungeonRoom, AiRouteEvidence>,
    human: &BTreeMap<DemoDungeonRoom, HumanEvidence>,
) -> String {
    let geometry = geometry_evidence();
    let mut output = String::from(
        "# Demo dungeon behavior audit\n\n\
This report is descriptive. It does not rank rooms or combine evidence into a difficulty score.\n\
`AI` is ticks/spans/jump presses/accepted wall jumps/dashes/horizontal reversals. `Shaky min` is the weakest recorded strength-one perturbation family. `Geometry` reports hazards/one-way tiles and the nearest tile layout. Flags are prompts for inspection, not failures.\n\n\
| # | Floor | AI | Shaky min | Human history | Geometry | Inspection flags |\n\
|---:|---|---|---|---|---|---|\n",
    );
    for (index, room) in DemoDungeonRoom::ALL.into_iter().enumerate() {
        let ai = &ai[&room];
        let human = human.get(&room).cloned().unwrap_or_default();
        let room_geometry = &geometry[&room];
        let (nearest, nearest_distance) = nearest_geometry(room, &geometry);
        let flags = inspection_flags(ai, &human, nearest_distance);
        output.push_str(&format!(
            "| {} | {} (`{}`) | {}/{}/{}/{}/{}/{} | {} | {} | H{} O{}; {} Δ{} | {} |\n",
            index + 1,
            room.title(),
            room.id(),
            ai.ticks,
            ai.action_spans,
            ai.jump_presses,
            ai.wall_jumps,
            ai.dashes,
            ai.horizontal_reversals,
            shaky_summary(ai),
            human_summary(&human),
            room_geometry.hazard_tiles,
            room_geometry.one_way_tiles,
            nearest.title(),
            nearest_distance,
            flags,
        ));
    }
    output
}

fn read_history(path: &Path) -> Result<String, String> {
    if path.exists() {
        fs::read_to_string(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))
    } else {
        eprintln!(
            "{} does not exist; reporting zero human attempts",
            path.display()
        );
        Ok(String::new())
    }
}

fn main() -> Result<(), String> {
    let history_path = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from(DEFAULT_HISTORY), PathBuf::from);
    if env::args_os().nth(2).is_some() {
        return Err("usage: audit_demo_dungeon [human-history.jsonl]".to_owned());
    }
    let ai = parse_witness_artifact(WITNESS_ARTIFACT)?;
    let history = read_history(&history_path)?;
    let human = parse_history(
        &history,
        &current_initial_digests(),
        &demo_dungeon_definition().id,
    )?;
    print!("{}", render_report(&ai, &human));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_artifact_has_exactly_one_route_per_floor() {
        let routes = parse_witness_artifact(WITNESS_ARTIFACT).unwrap();
        assert_eq!(routes.len(), 101);
        assert!(
            routes
                .values()
                .all(|route| route.ticks > 0 && !route.shaky.is_empty())
        );
    }

    #[test]
    fn history_joins_attempts_and_transitions_without_scalarizing_them() {
        let current_attempt = |outcome: &str, ticks: u64| {
            serde_json::json!({
                "schema": "downwards-human-attempt-v2",
                "dungeon_definition_id": "current-dungeon",
                "palette_generation_version": DUNGEON_PALETTE_GENERATION_VERSION,
                "room_id": DemoDungeonRoom::HollowLanding.id(),
                "player_movement_policy_version": PLAYER_MOVEMENT_POLICY_VERSION,
                "initial_state_digest": "current",
                "outcome": { "kind": outcome },
                "total_ticks": ticks,
            })
            .to_string()
        };
        let input = [
            current_attempt("death", 12),
            current_attempt("success", 42),
            serde_json::json!({
                "schema": "downwards-human-attempt-v1",
                "room_id": DemoDungeonRoom::HollowLanding.id(),
                "player_movement_policy_version": PLAYER_MOVEMENT_POLICY_VERSION - 1,
                "initial_state_digest": "old",
                "outcome": { "kind": "death" },
                "total_ticks": 9,
            })
            .to_string(),
            serde_json::json!({
                "schema": "downwards-dungeon-progress-v2",
                "dungeon_definition_id": "current-dungeon",
                "palette_generation_version": DUNGEON_PALETTE_GENERATION_VERSION,
                "room_id": DemoDungeonRoom::HollowLanding.id(),
                "event": "room transition",
            })
            .to_string(),
            serde_json::json!({
                "schema": "downwards-dungeon-progress-v1",
                "room_id": DemoDungeonRoom::HollowLanding.id(),
                "event": "room transition",
            })
            .to_string(),
            serde_json::json!({
                "schema": "downwards-human-attempt-v2",
                "dungeon_definition_id": "current-dungeon",
                "palette_generation_version": DUNGEON_PALETTE_GENERATION_VERSION,
                "room_id": "not-a-dungeon-room",
                "player_movement_policy_version": PLAYER_MOVEMENT_POLICY_VERSION,
                "initial_state_digest": "current",
                "outcome": { "kind": "success" },
                "total_ticks": 1,
            })
            .to_string(),
        ]
        .join("\n");
        let rows = parse_history(
            &input,
            &BTreeMap::from([(DemoDungeonRoom::HollowLanding, "current".to_owned())]),
            "current-dungeon",
        )
        .unwrap();
        assert_eq!(
            rows[&DemoDungeonRoom::HollowLanding],
            HumanEvidence {
                attempts: 2,
                stale_attempts: 1,
                successes: 1,
                deaths: 1,
                transition_events: 1,
                stale_transition_events: 1,
                successful_ticks: vec![42],
                ..HumanEvidence::default()
            }
        );
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn report_contains_every_floor_and_evidence_boundary() {
        let ai = parse_witness_artifact(WITNESS_ARTIFACT).unwrap();
        let report = render_report(&ai, &BTreeMap::new());
        assert_eq!(report.matches("| demo-dungeon.").count(), 0);
        assert_eq!(report.matches("(`demo-dungeon.").count(), 101);
        assert!(report.contains("does not rank rooms"));
        assert!(report.contains("NO-HUMAN"));
    }
}
