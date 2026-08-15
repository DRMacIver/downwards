use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use downwards_ai::{InconclusiveReason, SearchStats};
use downwards_core::{Action, DoorSocket};
use downwards_validation::BoundedTargetEvidence;
use serde::Serialize;

use super::{
    CandidateKeyRecord, CorpusBuildConfigV1, EvaluatedCorpusBatch, EvaluationLoadout,
    GenerationBatchSummary, RoomId, fingerprints::fingerprint_static_visual,
};

pub const CORPUS_ARTIFACT_VERSION: u32 = 1;
pub const CORPUS_ACTION_ENCODING_VERSION: u32 = 1;

/// Canonical bytes for the first room-centric corpus artifact slice.
///
/// Every JSONL stream is already in stable key order and ends in one newline.
/// `run.json` binds the other byte streams by stable hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusArtifactBundle {
    pub run_json: Vec<u8>,
    pub candidates_jsonl: Vec<u8>,
    pub rooms_jsonl: Vec<u8>,
    pub routes_jsonl: Vec<u8>,
    pub pickups_jsonl: Vec<u8>,
    pub witnesses_jsonl: Vec<u8>,
}

impl CorpusArtifactBundle {
    const FILE_NAMES: [&'static str; 6] = [
        "run.json",
        "candidates.jsonl",
        "rooms.jsonl",
        "routes.jsonl",
        "pickups.jsonl",
        "witnesses.jsonl",
    ];

    fn files(&self) -> [(&'static str, &[u8]); 6] {
        [
            (Self::FILE_NAMES[0], &self.run_json),
            (Self::FILE_NAMES[1], &self.candidates_jsonl),
            (Self::FILE_NAMES[2], &self.rooms_jsonl),
            (Self::FILE_NAMES[3], &self.routes_jsonl),
            (Self::FILE_NAMES[4], &self.pickups_jsonl),
            (Self::FILE_NAMES[5], &self.witnesses_jsonl),
        ]
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RunRecord<'a> {
    artifact_version: u32,
    status: &'static str,
    config: &'a CorpusBuildConfigV1,
    generation: GenerationBatchSummary,
    rooms: usize,
    route_rows: usize,
    pickup_rows: usize,
    positive_routes: usize,
    inconclusive_routes: usize,
    positive_pickups: usize,
    inconclusive_pickups: usize,
    file_hashes: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RoomRecord {
    room_id: RoomId,
    canonical_key: CandidateKeyRecord,
    alias_keys: Vec<CandidateKeyRecord>,
    static_visual_fingerprint: String,
    simulation_geometry_fingerprint: String,
    sockets: Vec<SocketRecord>,
    route_signature: String,
    cycle_rank: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketRecord {
    side: &'static str,
    offset: i32,
    span: i32,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct TargetRowRecord<'a> {
    room_id: &'a RoomId,
    source_door_id: &'a str,
    target_kind: &'static str,
    target_id: &'a str,
    loadout: EvaluationLoadout,
    evidence: EvidenceRecord,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EvidenceRecord {
    PositiveReplay {
        witness_id: String,
    },
    BoundedInconclusive {
        reason: &'static str,
        search_effort: SearchStatsRecord,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SearchStatsRecord {
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

impl From<SearchStats> for SearchStatsRecord {
    fn from(stats: SearchStats) -> Self {
        Self {
            expanded_nodes: stats.expanded_nodes,
            generated_nodes: stats.generated_nodes,
            simulated_ticks: stats.simulated_ticks,
            deepest_path_ticks: stats.deepest_path_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct WitnessRecord {
    witness_id: String,
    room_id: RoomId,
    source_door_id: String,
    target_kind: &'static str,
    target_id: String,
    loadout: EvaluationLoadout,
    initial_digest: String,
    total_ticks: usize,
    search_effort: SearchStatsRecord,
    action_encoding_version: u32,
    actions: Vec<ActionSpanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ActionSpanRecord {
    ticks: usize,
    move_x: i8,
    move_y: i8,
    jump: bool,
    dash: bool,
}

pub fn render_evaluated_artifacts(
    evaluated: &EvaluatedCorpusBatch,
) -> Result<CorpusArtifactBundle, CorpusArtifactError> {
    let candidates_jsonl = render_json_lines(evaluated.generated.records.iter())?;
    let mut room_records = Vec::with_capacity(evaluated.rooms.len());
    let mut route_rows = Vec::new();
    let mut pickup_rows = Vec::new();
    let mut witnesses = BTreeMap::<String, WitnessRecord>::new();

    for evaluated_room in &evaluated.rooms {
        let room = &evaluated_room.generated;
        let canonical = &room.variants[0];
        let mut alias_keys = room
            .variants
            .iter()
            .skip(1)
            .map(|candidate| CandidateKeyRecord::from_staged_key(candidate.key))
            .collect::<Vec<_>>();
        alias_keys.sort_unstable();
        let mut sockets = canonical
            .generated
            .room
            .doors()
            .iter()
            .map(|door| socket_record(door.socket()))
            .collect::<Vec<_>>();
        sockets.sort_unstable_by_key(|socket| (socket.side, socket.offset, socket.span));
        room_records.push(RoomRecord {
            room_id: room.id.clone(),
            canonical_key: CandidateKeyRecord::from_staged_key(canonical.key),
            alias_keys,
            static_visual_fingerprint: format!(
                "downwards-corpus-static-v1-{:016x}",
                fingerprint_static_visual(&room.static_visual)
            ),
            simulation_geometry_fingerprint: format!(
                "downwards-simulation-geometry-v1-{:016x}",
                room.simulation_geometry.stable_digest()
            ),
            sockets,
            route_signature: format!("{:016x}", canonical.route_summary.signature),
            cycle_rank: canonical.route_summary.cycle_rank,
        });

        for matrix in &evaluated_room.matrices {
            for row in matrix.evidence.door_routes() {
                let evidence = evidence_record(
                    &room.id,
                    &row.source_door_id,
                    "door",
                    &row.target_door_id,
                    matrix.loadout,
                    &row.evidence,
                    &mut witnesses,
                )?;
                route_rows.push(TargetRowRecord {
                    room_id: &room.id,
                    source_door_id: &row.source_door_id,
                    target_kind: "door",
                    target_id: &row.target_door_id,
                    loadout: matrix.loadout,
                    evidence,
                });
            }
            for row in matrix.evidence.pickup_routes() {
                let evidence = evidence_record(
                    &room.id,
                    &row.source_door_id,
                    "pickup",
                    &row.required_pickup_id,
                    matrix.loadout,
                    &row.evidence,
                    &mut witnesses,
                )?;
                pickup_rows.push(TargetRowRecord {
                    room_id: &room.id,
                    source_door_id: &row.source_door_id,
                    target_kind: "pickup",
                    target_id: &row.required_pickup_id,
                    loadout: matrix.loadout,
                    evidence,
                });
            }
        }
    }

    room_records.sort_unstable_by(|left, right| left.room_id.cmp(&right.room_id));
    route_rows.sort_unstable_by(|left, right| {
        (
            left.room_id,
            left.source_door_id,
            left.target_id,
            left.loadout,
        )
            .cmp(&(
                right.room_id,
                right.source_door_id,
                right.target_id,
                right.loadout,
            ))
    });
    pickup_rows.sort_unstable_by(|left, right| {
        (
            left.room_id,
            left.source_door_id,
            left.target_id,
            left.loadout,
        )
            .cmp(&(
                right.room_id,
                right.source_door_id,
                right.target_id,
                right.loadout,
            ))
    });

    let rooms_jsonl = render_json_lines(room_records.iter())?;
    let routes_jsonl = render_json_lines(route_rows.iter())?;
    let pickups_jsonl = render_json_lines(pickup_rows.iter())?;
    let witnesses_jsonl = render_json_lines(witnesses.values())?;
    let mut file_hashes = BTreeMap::new();
    for (name, bytes) in [
        ("candidates.jsonl", &candidates_jsonl),
        ("rooms.jsonl", &rooms_jsonl),
        ("routes.jsonl", &routes_jsonl),
        ("pickups.jsonl", &pickups_jsonl),
        ("witnesses.jsonl", &witnesses_jsonl),
    ] {
        file_hashes.insert(name, format!("fnv1a64-{:016x}", fingerprint_bytes(bytes)));
    }
    let positive_routes = route_rows
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    let positive_pickups = pickup_rows
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    let run = RunRecord {
        artifact_version: CORPUS_ARTIFACT_VERSION,
        status: "evaluated",
        config: &evaluated.generated.config,
        generation: evaluated.generated.summary,
        rooms: room_records.len(),
        route_rows: route_rows.len(),
        pickup_rows: pickup_rows.len(),
        positive_routes,
        inconclusive_routes: route_rows.len() - positive_routes,
        positive_pickups,
        inconclusive_pickups: pickup_rows.len() - positive_pickups,
        file_hashes,
    };
    let mut run_json = serde_json::to_vec(&run)?;
    run_json.push(b'\n');

    Ok(CorpusArtifactBundle {
        run_json,
        candidates_jsonl,
        rooms_jsonl,
        routes_jsonl,
        pickups_jsonl,
        witnesses_jsonl,
    })
}

fn evidence_record(
    room_id: &RoomId,
    source_door_id: &str,
    target_kind: &'static str,
    target_id: &str,
    loadout: EvaluationLoadout,
    evidence: &BoundedTargetEvidence,
    witnesses: &mut BTreeMap<String, WitnessRecord>,
) -> Result<EvidenceRecord, CorpusArtifactError> {
    match evidence {
        BoundedTargetEvidence::Positive(positive) => {
            let witness_id = positive.witness_fingerprint().to_string();
            let solution = positive.solution();
            let actions = encode_actions(solution.replay.actions())?;
            let record = WitnessRecord {
                witness_id: witness_id.clone(),
                room_id: room_id.clone(),
                source_door_id: source_door_id.to_owned(),
                target_kind,
                target_id: target_id.to_owned(),
                loadout,
                initial_digest: solution.replay.initial_digest.to_string(),
                total_ticks: solution.replay.frames.len(),
                search_effort: solution.stats.into(),
                action_encoding_version: CORPUS_ACTION_ENCODING_VERSION,
                actions,
            };
            if let Some(previous) = witnesses.insert(witness_id.clone(), record.clone())
                && previous != record
            {
                return Err(CorpusArtifactError::WitnessIdentityCollision(witness_id));
            }
            Ok(EvidenceRecord::PositiveReplay { witness_id })
        }
        BoundedTargetEvidence::Inconclusive(inconclusive) => {
            Ok(EvidenceRecord::BoundedInconclusive {
                reason: inconclusive_reason(inconclusive.reason),
                search_effort: inconclusive.search_effort.into(),
            })
        }
    }
}

fn encode_actions(
    actions: impl IntoIterator<Item = Action>,
) -> Result<Vec<ActionSpanRecord>, CorpusArtifactError> {
    let mut spans = Vec::<ActionSpanRecord>::new();
    for action in actions {
        if action.restart {
            return Err(CorpusArtifactError::RestartAction);
        }
        let normalized = ActionSpanRecord {
            ticks: 1,
            move_x: action.move_x.clamp(-1, 1),
            move_y: action.move_y.clamp(-1, 1),
            jump: action.jump,
            dash: action.dash,
        };
        if let Some(previous) = spans.last_mut()
            && previous.move_x == normalized.move_x
            && previous.move_y == normalized.move_y
            && previous.jump == normalized.jump
            && previous.dash == normalized.dash
        {
            previous.ticks += 1;
        } else {
            spans.push(normalized);
        }
    }
    Ok(spans)
}

fn render_json_lines<'a, T: Serialize + 'a>(
    values: impl IntoIterator<Item = &'a T>,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut result = Vec::new();
    for value in values {
        serde_json::to_writer(&mut result, value)?;
        result.push(b'\n');
    }
    Ok(result)
}

fn socket_record(socket: DoorSocket) -> SocketRecord {
    SocketRecord {
        side: match socket.side {
            downwards_core::BoundarySide::Left => "left",
            downwards_core::BoundarySide::Right => "right",
            downwards_core::BoundarySide::Ceiling => "ceiling",
            downwards_core::BoundarySide::Floor => "floor",
        },
        offset: socket.offset,
        span: socket.span,
    }
}

const fn inconclusive_reason(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::NoExitsDefined => "no-exits-defined",
        InconclusiveReason::ExpandedNodeBudget => "expanded-node-budget",
        InconclusiveReason::SimulatedTickBudget => "simulated-tick-budget",
        InconclusiveReason::PathHorizon => "path-horizon",
        InconclusiveReason::FrontierExhausted => "frontier-exhausted",
    }
}

fn fingerprint_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Write a bundle only into paths that do not already exist.
///
/// Each file is first written to a process-specific sibling and then renamed.
/// Existing corpus evidence is never overwritten implicitly.
pub fn write_new_artifact_bundle(
    directory: &Path,
    bundle: &CorpusArtifactBundle,
) -> Result<(), CorpusArtifactError> {
    fs::create_dir_all(directory).map_err(CorpusArtifactError::Io)?;
    for name in CorpusArtifactBundle::FILE_NAMES {
        let path = directory.join(name);
        if path.exists() {
            return Err(CorpusArtifactError::AlreadyExists(path));
        }
    }

    let mut temporary_paths = HashSet::new();
    for (name, bytes) in bundle.files() {
        let final_path = directory.join(name);
        let temporary_path = temporary_path(directory, name);
        if !temporary_paths.insert(temporary_path.clone()) || temporary_path.exists() {
            return Err(CorpusArtifactError::TemporaryPathExists(temporary_path));
        }
        fs::write(&temporary_path, bytes).map_err(CorpusArtifactError::Io)?;
        fs::rename(&temporary_path, &final_path).map_err(CorpusArtifactError::Io)?;
    }
    Ok(())
}

fn temporary_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(format!(".{name}.tmp-{}", std::process::id()))
}

#[derive(Debug)]
pub enum CorpusArtifactError {
    Json(serde_json::Error),
    Io(std::io::Error),
    AlreadyExists(PathBuf),
    TemporaryPathExists(PathBuf),
    RestartAction,
    WitnessIdentityCollision(String),
}

impl fmt::Display for CorpusArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "could not encode corpus JSON: {error}"),
            Self::Io(error) => write!(formatter, "could not write corpus artifact: {error}"),
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "corpus artifact already exists: {}",
                    path.display()
                )
            }
            Self::TemporaryPathExists(path) => write!(
                formatter,
                "corpus temporary path already exists: {}",
                path.display()
            ),
            Self::RestartAction => write!(formatter, "positive corpus witness contains restart"),
            Self::WitnessIdentityCollision(id) => {
                write!(formatter, "distinct corpus witnesses share identity {id:?}")
            }
        }
    }
}

impl Error for CorpusArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::AlreadyExists(_)
            | Self::TemporaryPathExists(_)
            | Self::RestartAction
            | Self::WitnessIdentityCollision(_) => None,
        }
    }
}

impl From<serde_json::Error> for CorpusArtifactError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use downwards_validation::ValidationConfig;

    use super::*;
    use crate::corpus::{CorpusBuildConfigV1, evaluate_route_matrices_with, generate_seed_block};

    #[test]
    fn evaluated_artifact_bytes_are_repeatable_and_explicit_about_inconclusive_rows() {
        fn build() -> CorpusArtifactBundle {
            let generated =
                generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
            let mut one_room = generated;
            one_room.rooms.truncate(1);
            let evaluated = evaluate_route_matrices_with(one_room, |loadout| {
                let mut config = ValidationConfig::for_loadout(loadout.abilities());
                config.solver.max_expanded_nodes = 1;
                config.solver.max_simulated_ticks = 2_000;
                config
            })
            .unwrap();
            render_evaluated_artifacts(&evaluated).unwrap()
        }

        let first = build();
        let second = build();
        assert_eq!(first, second);
        let run = String::from_utf8(first.run_json).unwrap();
        let routes = String::from_utf8(first.routes_jsonl).unwrap();
        assert!(run.contains("\"status\":\"evaluated\""));
        assert!(run.contains("\"file_hashes\""));
        assert!(routes.contains("\"kind\":\"bounded_inconclusive\""));
        assert!(!routes.contains("unreachable"));
    }
}
