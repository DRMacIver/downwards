use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

use downwards_ai::{ReachedTarget, Replay, SearchStats, SearchTarget, TargetSolution};
use downwards_core::{Action, Simulation, StateDigest};
use downwards_lab::{SimulationGeometryDescriptor, StaticVisualDescriptor};
use downwards_validation::{
    DoorReachabilityObjective, PickupFromDoorObjective, fingerprint_door_witness,
    fingerprint_pickup_from_door_witness,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    CandidateKeyRecord, ConstructionRecord, CorpusArtifactBundle, CorpusBuildConfigV1,
    EvaluationLoadout, GenerationAttemptRecord, GenerationBatchSummary, RoomId,
    artifact::{CORPUS_ACTION_ENCODING_VERSION, CORPUS_ARTIFACT_VERSION},
    fingerprints::fingerprint_static_visual,
    generate_seed_block,
};

/// Version of the immutable, single-seed checkpoint envelope.
pub const CORPUS_SEED_CHECKPOINT_VERSION: u32 = 1;

const ARTIFACT_FILE_NAMES: [&str; 6] = [
    "run.json",
    "candidates.jsonl",
    "rooms.jsonl",
    "routes.jsonl",
    "pickups.jsonl",
    "witnesses.jsonl",
];
const HASHED_STREAM_NAMES: [&str; 5] = [
    "candidates.jsonl",
    "rooms.jsonl",
    "routes.jsonl",
    "pickups.jsonl",
    "witnesses.jsonl",
];
// Current solver witnesses are bounded to hundreds of ticks. Keeping a much
// larger explicit ceiling prevents a corrupt compressed span from requesting
// an effectively unbounded allocation during verification. Raising it is an
// artifact-policy change and should accompany an encoding/version review.
const MAX_VERIFIABLE_WITNESS_TICKS: usize = 10_000;

/// Facts returned only after a complete artifact bundle has passed strict
/// parsing, regeneration, matrix, cross-reference, and replay verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedCorpusArtifact {
    pub config: CorpusBuildConfigV1,
    pub config_hash: String,
    pub generation: GenerationBatchSummary,
    pub rooms: usize,
    pub route_rows: usize,
    pub positive_route_rows: usize,
    pub inconclusive_route_rows: usize,
    pub pickup_rows: usize,
    pub positive_pickup_rows: usize,
    pub inconclusive_pickup_rows: usize,
    pub positive_witnesses: usize,
    /// Rooms whose complete target matrix is positive under both their
    /// construction loadout and the complete kit.
    pub construction_and_complete_kit_rooms: usize,
    pub construction_and_complete_kit_room_ids: Vec<RoomId>,
}

/// Immutable completion marker for one fully evaluated seed shard.
///
/// The configuration hash includes the complete canonical configuration, not
/// just the seed. The artifact hashes bind all six files, including
/// `run.json`. Loading a checkpoint with a different expected configuration
/// or a modified bundle therefore fails closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedShardCheckpoint {
    pub checkpoint_version: u32,
    pub status: String,
    pub config_hash: String,
    pub seed: u64,
    pub artifact_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRecord {
    artifact_version: u32,
    status: String,
    config: CorpusBuildConfigV1,
    generation: GenerationBatchSummary,
    rooms: usize,
    route_rows: usize,
    pickup_rows: usize,
    positive_routes: usize,
    inconclusive_routes: usize,
    positive_pickups: usize,
    inconclusive_pickups: usize,
    file_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateRecord {
    key: CandidateKeyRecord,
    construction: StrictConstructionRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum StrictConstructionRecord {
    Constructed {
        room_id: RoomId,
        static_visual_fingerprint: String,
        simulation_geometry_fingerprint: String,
        port_count: usize,
        pickup_count: usize,
        route_signature: String,
        cycle_rank: u16,
        canonical: bool,
    },
    Rejected {
        reason: String,
    },
}

impl From<CandidateRecord> for GenerationAttemptRecord {
    fn from(record: CandidateRecord) -> Self {
        let construction = match record.construction {
            StrictConstructionRecord::Constructed {
                room_id,
                static_visual_fingerprint,
                simulation_geometry_fingerprint,
                port_count,
                pickup_count,
                route_signature,
                cycle_rank,
                canonical,
            } => ConstructionRecord::Constructed {
                room_id,
                static_visual_fingerprint,
                simulation_geometry_fingerprint,
                port_count,
                pickup_count,
                route_signature,
                cycle_rank,
                canonical,
            },
            StrictConstructionRecord::Rejected { reason } => {
                ConstructionRecord::Rejected { reason }
            }
        };
        Self {
            key: record.key,
            construction,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SocketRecord {
    side: String,
    offset: i32,
    span: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetRowRecord {
    room_id: RoomId,
    source_door_id: String,
    target_kind: String,
    target_id: String,
    loadout: EvaluationLoadout,
    evidence: EvidenceRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum EvidenceRecord {
    PositiveReplay {
        witness_id: String,
    },
    BoundedInconclusive {
        reason: String,
        search_effort: SearchStatsRecord,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchStatsRecord {
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

impl From<SearchStatsRecord> for SearchStats {
    fn from(stats: SearchStatsRecord) -> Self {
        Self {
            expanded_nodes: stats.expanded_nodes,
            generated_nodes: stats.generated_nodes,
            simulated_ticks: stats.simulated_ticks,
            deepest_path_ticks: stats.deepest_path_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WitnessRecord {
    witness_id: String,
    room_id: RoomId,
    source_door_id: String,
    target_kind: String,
    target_id: String,
    loadout: EvaluationLoadout,
    initial_digest: String,
    total_ticks: usize,
    search_effort: SearchStatsRecord,
    action_encoding_version: u32,
    actions: Vec<ActionSpanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionSpanRecord {
    ticks: usize,
    move_x: i8,
    move_y: i8,
    jump: bool,
    dash: bool,
}

/// Read and strictly verify the six-file corpus bundle in `directory`.
///
/// Required entries must be regular files; symlinks and missing files are
/// rejected. Additional directory entries are ignored so immutable
/// checkpoints and later selection artifacts may coexist with the evidence
/// streams.
pub fn verify_artifact_directory(
    directory: &Path,
) -> Result<VerifiedCorpusArtifact, CorpusVerificationError> {
    let bundle = read_artifact_bundle(directory)?;
    verify_artifact_bundle(&bundle)
}

/// Read the six required files without interpreting their contents.
pub fn read_artifact_bundle(
    directory: &Path,
) -> Result<CorpusArtifactBundle, CorpusVerificationError> {
    let read = |name: &str| -> Result<Vec<u8>, CorpusVerificationError> {
        let path = directory.join(name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| CorpusVerificationError::Io {
                path: path.clone(),
                source,
            })?;
        if !metadata.file_type().is_file() {
            return Err(invalid(format!(
                "artifact entry is not a regular file: {}",
                path.display()
            )));
        }
        fs::read(&path).map_err(|source| CorpusVerificationError::Io { path, source })
    };

    Ok(CorpusArtifactBundle {
        run_json: read(ARTIFACT_FILE_NAMES[0])?,
        candidates_jsonl: read(ARTIFACT_FILE_NAMES[1])?,
        rooms_jsonl: read(ARTIFACT_FILE_NAMES[2])?,
        routes_jsonl: read(ARTIFACT_FILE_NAMES[3])?,
        pickups_jsonl: read(ARTIFACT_FILE_NAMES[4])?,
        witnesses_jsonl: read(ARTIFACT_FILE_NAMES[5])?,
    })
}

/// Strictly verify a complete in-memory corpus evidence bundle.
///
/// Bounded non-success remains inconclusive. Positive rows are stronger: the
/// generator is rerun, their exact source-door initial state is reconstructed,
/// their actions are executed by the authoritative simulation, their typed
/// target is checked, and their validation fingerprint is recomputed.
pub fn verify_artifact_bundle(
    bundle: &CorpusArtifactBundle,
) -> Result<VerifiedCorpusArtifact, CorpusVerificationError> {
    let mut run_rows: Vec<RunRecord> = parse_canonical_jsonl("run.json", &bundle.run_json)?;
    if run_rows.len() != 1 {
        return Err(invalid(format!(
            "run.json must contain exactly one record, found {}",
            run_rows.len()
        )));
    }
    let run = run_rows.pop().expect("length was checked");
    if run.artifact_version != CORPUS_ARTIFACT_VERSION {
        return Err(invalid(format!(
            "unsupported corpus artifact version {}, expected {}",
            run.artifact_version, CORPUS_ARTIFACT_VERSION
        )));
    }
    if run.status != "evaluated" {
        return Err(invalid(format!(
            "run status must be \"evaluated\", found {:?}",
            run.status
        )));
    }
    run.config
        .validate()
        .map_err(|error| invalid(format!("invalid corpus config: {error}")))?;
    verify_stream_hashes(bundle, &run.file_hashes)?;

    let candidate_records: Vec<CandidateRecord> =
        parse_canonical_jsonl("candidates.jsonl", &bundle.candidates_jsonl)?;
    let room_records: Vec<RoomRecord> = parse_canonical_jsonl("rooms.jsonl", &bundle.rooms_jsonl)?;
    let route_rows: Vec<TargetRowRecord> =
        parse_canonical_jsonl("routes.jsonl", &bundle.routes_jsonl)?;
    let pickup_rows: Vec<TargetRowRecord> =
        parse_canonical_jsonl("pickups.jsonl", &bundle.pickups_jsonl)?;
    let witness_records: Vec<WitnessRecord> =
        parse_canonical_jsonl("witnesses.jsonl", &bundle.witnesses_jsonl)?;

    require_strict_order(
        "candidates.jsonl",
        candidate_records.iter().map(|record| &record.key),
    )?;
    require_strict_order(
        "rooms.jsonl",
        room_records.iter().map(|record| &record.room_id),
    )?;
    require_strict_order("routes.jsonl", route_rows.iter().map(target_row_key))?;
    require_strict_order("pickups.jsonl", pickup_rows.iter().map(target_row_key))?;
    require_strict_order(
        "witnesses.jsonl",
        witness_records.iter().map(|record| &record.witness_id),
    )?;

    let regenerated = generate_seed_block(run.config.clone())
        .map_err(|error| invalid(format!("recorded config did not regenerate: {error}")))?;
    let decoded_candidates = candidate_records
        .into_iter()
        .map(GenerationAttemptRecord::from)
        .collect::<Vec<_>>();
    if decoded_candidates != regenerated.records {
        return Err(invalid(
            "candidates.jsonl differs from deterministic regeneration",
        ));
    }
    if run.generation != regenerated.summary {
        return Err(invalid(format!(
            "run generation summary {:?} differs from regenerated {:?}",
            run.generation, regenerated.summary
        )));
    }
    if run.rooms != regenerated.rooms.len() || room_records.len() != regenerated.rooms.len() {
        return Err(invalid(format!(
            "room count differs: run={}, rows={}, regenerated={}",
            run.rooms,
            room_records.len(),
            regenerated.rooms.len()
        )));
    }

    let mut regenerated_rooms = regenerated.rooms.iter().collect::<Vec<_>>();
    regenerated_rooms.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    let mut runtime_rooms = HashMap::with_capacity(regenerated.rooms.len());
    for (record, generated) in room_records.iter().zip(regenerated_rooms) {
        verify_room_record(record, generated)?;
        if runtime_rooms
            .insert(record.room_id.clone(), generated)
            .is_some()
        {
            return Err(invalid(format!(
                "duplicate room identity {:?}",
                record.room_id.0
            )));
        }
    }

    verify_run_counts(&run, &route_rows, &pickup_rows, &witness_records)?;
    let witness_map = witness_records
        .iter()
        .map(|witness| (witness.witness_id.as_str(), witness))
        .collect::<HashMap<_, _>>();
    let mut witness_references = HashMap::<String, usize>::new();
    verify_target_rows(
        "routes.jsonl",
        "door",
        &route_rows,
        &runtime_rooms,
        &witness_map,
        &mut witness_references,
    )?;
    verify_target_rows(
        "pickups.jsonl",
        "pickup",
        &pickup_rows,
        &runtime_rooms,
        &witness_map,
        &mut witness_references,
    )?;
    for witness in &witness_records {
        match witness_references.get(witness.witness_id.as_str()).copied() {
            Some(1) => {}
            Some(count) => {
                return Err(invalid(format!(
                    "witness {:?} is referenced {count} times",
                    witness.witness_id
                )));
            }
            None => {
                return Err(invalid(format!(
                    "orphan witness {:?} has no positive evidence row",
                    witness.witness_id
                )));
            }
        }
    }

    let positive_route_rows = route_rows
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    let positive_pickup_rows = pickup_rows
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    let construction_and_complete_kit_room_ids = room_records
        .iter()
        .filter(|room| {
            let construction = room.canonical_key.construction_loadout;
            target_matrix_is_all_positive(&route_rows, &pickup_rows, &room.room_id, construction)
                && target_matrix_is_all_positive(
                    &route_rows,
                    &pickup_rows,
                    &room.room_id,
                    EvaluationLoadout::Both,
                )
        })
        .map(|room| room.room_id.clone())
        .collect::<Vec<_>>();

    Ok(VerifiedCorpusArtifact {
        config_hash: corpus_config_hash(&run.config)?,
        config: run.config,
        generation: run.generation,
        rooms: room_records.len(),
        route_rows: route_rows.len(),
        positive_route_rows,
        inconclusive_route_rows: route_rows.len() - positive_route_rows,
        pickup_rows: pickup_rows.len(),
        positive_pickup_rows,
        inconclusive_pickup_rows: pickup_rows.len() - positive_pickup_rows,
        positive_witnesses: witness_records.len(),
        construction_and_complete_kit_rooms: construction_and_complete_kit_room_ids.len(),
        construction_and_complete_kit_room_ids,
    })
}

fn target_matrix_is_all_positive(
    route_rows: &[TargetRowRecord],
    pickup_rows: &[TargetRowRecord],
    room_id: &RoomId,
    loadout: EvaluationLoadout,
) -> bool {
    route_rows
        .iter()
        .chain(pickup_rows)
        .filter(|row| row.room_id == *room_id && row.loadout == loadout)
        .all(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
}

fn verify_stream_hashes(
    bundle: &CorpusArtifactBundle,
    recorded: &BTreeMap<String, String>,
) -> Result<(), CorpusVerificationError> {
    let expected_names = HASHED_STREAM_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual_names = recorded.keys().cloned().collect::<BTreeSet<_>>();
    if actual_names != expected_names {
        return Err(invalid(format!(
            "run file_hashes keys differ: expected {expected_names:?}, found {actual_names:?}"
        )));
    }
    for (name, bytes) in bundle_streams(bundle) {
        let expected = byte_hash(bytes);
        let actual = &recorded[name];
        if actual != &expected {
            return Err(invalid(format!(
                "hash mismatch for {name}: recorded {actual:?}, actual {expected:?}"
            )));
        }
    }
    Ok(())
}

fn verify_room_record(
    record: &RoomRecord,
    generated: &super::GeneratedCorpusRoom,
) -> Result<(), CorpusVerificationError> {
    if record.room_id != generated.id {
        return Err(invalid(format!(
            "room order/identity differs: artifact {:?}, regenerated {:?}",
            record.room_id.0, generated.id.0
        )));
    }
    let canonical = generated.variants.first().ok_or_else(|| {
        invalid(format!(
            "regenerated room {:?} has no variants",
            generated.id.0
        ))
    })?;
    let expected_canonical = CandidateKeyRecord::from_staged_key(canonical.key);
    if record.canonical_key != expected_canonical {
        return Err(invalid(format!(
            "room {:?} canonical key differs from regeneration",
            record.room_id.0
        )));
    }
    let mut expected_aliases = generated
        .variants
        .iter()
        .skip(1)
        .map(|variant| CandidateKeyRecord::from_staged_key(variant.key))
        .collect::<Vec<_>>();
    expected_aliases.sort_unstable();
    if record.alias_keys != expected_aliases {
        return Err(invalid(format!(
            "room {:?} alias keys differ from regeneration",
            record.room_id.0
        )));
    }
    require_strict_order("room alias_keys", record.alias_keys.iter())?;

    let regenerated_visual = StaticVisualDescriptor::from_room(&canonical.generated.room);
    if regenerated_visual != generated.static_visual {
        return Err(invalid(format!(
            "room {:?} regenerated static descriptor changed inside generation batch",
            record.room_id.0
        )));
    }
    let expected_static = format!(
        "downwards-corpus-static-v1-{:016x}",
        fingerprint_static_visual(&regenerated_visual)
    );
    if record.static_visual_fingerprint != expected_static {
        return Err(invalid(format!(
            "room {:?} static visual fingerprint differs: {:?} != {:?}",
            record.room_id.0, record.static_visual_fingerprint, expected_static
        )));
    }
    let regenerated_simulation = SimulationGeometryDescriptor::from_room(&canonical.generated.room);
    if regenerated_simulation != generated.simulation_geometry {
        return Err(invalid(format!(
            "room {:?} regenerated simulation descriptor changed inside generation batch",
            record.room_id.0
        )));
    }
    for alias in &generated.variants[1..] {
        let alias_visual = StaticVisualDescriptor::from_room(&alias.generated.room);
        let alias_simulation = SimulationGeometryDescriptor::from_room(&alias.generated.room);
        if alias_visual != regenerated_visual || alias_simulation != regenerated_simulation {
            return Err(invalid(format!(
                "room {:?} aliases candidates with different exact geometry",
                record.room_id.0
            )));
        }
    }
    let expected_simulation = format!(
        "downwards-simulation-geometry-v1-{:016x}",
        regenerated_simulation.stable_digest()
    );
    if record.simulation_geometry_fingerprint != expected_simulation {
        return Err(invalid(format!(
            "room {:?} simulation fingerprint differs: {:?} != {:?}",
            record.room_id.0, record.simulation_geometry_fingerprint, expected_simulation
        )));
    }

    let mut expected_sockets = canonical
        .generated
        .room
        .doors()
        .iter()
        .map(|door| SocketRecord {
            side: boundary_side_name(door.socket().side).to_owned(),
            offset: door.socket().offset,
            span: door.socket().span,
        })
        .collect::<Vec<_>>();
    expected_sockets.sort_unstable();
    if record.sockets != expected_sockets {
        return Err(invalid(format!(
            "room {:?} socket records differ from regeneration",
            record.room_id.0
        )));
    }
    let expected_route_signature = format!("{:016x}", canonical.route_summary.signature);
    if record.route_signature != expected_route_signature
        || record.cycle_rank != canonical.route_summary.cycle_rank
    {
        return Err(invalid(format!(
            "room {:?} route-plan summary differs from regeneration",
            record.room_id.0
        )));
    }
    Ok(())
}

fn verify_run_counts(
    run: &RunRecord,
    routes: &[TargetRowRecord],
    pickups: &[TargetRowRecord],
    witnesses: &[WitnessRecord],
) -> Result<(), CorpusVerificationError> {
    let positive_routes = routes
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    let positive_pickups = pickups
        .iter()
        .filter(|row| matches!(row.evidence, EvidenceRecord::PositiveReplay { .. }))
        .count();
    if run.route_rows != routes.len()
        || run.pickup_rows != pickups.len()
        || run.positive_routes != positive_routes
        || run.inconclusive_routes != routes.len().saturating_sub(positive_routes)
        || run.positive_pickups != positive_pickups
        || run.inconclusive_pickups != pickups.len().saturating_sub(positive_pickups)
    {
        return Err(invalid("run row/evidence counts do not match the streams"));
    }
    if positive_routes
        .checked_add(positive_pickups)
        .ok_or_else(|| invalid("positive evidence count overflow"))?
        != witnesses.len()
    {
        return Err(invalid(format!(
            "positive row count {} does not match witness count {}",
            positive_routes + positive_pickups,
            witnesses.len()
        )));
    }
    Ok(())
}

fn verify_target_rows(
    file: &str,
    required_kind: &str,
    rows: &[TargetRowRecord],
    rooms: &HashMap<RoomId, &super::GeneratedCorpusRoom>,
    witnesses: &HashMap<&str, &WitnessRecord>,
    witness_references: &mut HashMap<String, usize>,
) -> Result<(), CorpusVerificationError> {
    let mut actual = BTreeSet::new();
    for row in rows {
        if row.target_kind != required_kind {
            return Err(invalid(format!(
                "{file} row has target_kind {:?}, expected {required_kind:?}",
                row.target_kind
            )));
        }
        let room = rooms.get(&row.room_id).ok_or_else(|| {
            invalid(format!(
                "{file} references unknown room {:?}",
                row.room_id.0
            ))
        })?;
        let canonical = &room.variants[0];
        let door_ids = canonical
            .generated
            .room
            .doors()
            .iter()
            .map(|door| door.id.as_str())
            .collect::<BTreeSet<_>>();
        if !door_ids.contains(row.source_door_id.as_str()) {
            return Err(invalid(format!(
                "{file} references unknown source door {:?} in room {:?}",
                row.source_door_id, row.room_id.0
            )));
        }
        match required_kind {
            "door" => {
                if row.source_door_id == row.target_id || !door_ids.contains(row.target_id.as_str())
                {
                    return Err(invalid(format!(
                        "{file} has invalid directed door target {:?}->{:?} in room {:?}",
                        row.source_door_id, row.target_id, row.room_id.0
                    )));
                }
            }
            "pickup" => {
                if !canonical
                    .generated
                    .room
                    .pickups()
                    .iter()
                    .any(|pickup| pickup.id() == row.target_id)
                {
                    return Err(invalid(format!(
                        "{file} references unknown pickup {:?} in room {:?}",
                        row.target_id, row.room_id.0
                    )));
                }
            }
            _ => unreachable!("caller supplies a fixed target kind"),
        }
        let matrix_key = (
            row.room_id.clone(),
            row.source_door_id.clone(),
            row.target_id.clone(),
            row.loadout,
        );
        if !actual.insert(matrix_key) {
            return Err(invalid(format!("{file} contains a duplicate matrix cell")));
        }
        match &row.evidence {
            EvidenceRecord::PositiveReplay { witness_id } => {
                let witness = witnesses.get(witness_id.as_str()).ok_or_else(|| {
                    invalid(format!("{file} references missing witness {witness_id:?}"))
                })?;
                verify_positive_witness(row, witness, canonical)?;
                *witness_references
                    .entry(witness.witness_id.clone())
                    .or_default() += 1;
            }
            EvidenceRecord::BoundedInconclusive {
                reason,
                search_effort: _,
            } => verify_inconclusive_reason(reason)?,
        }
    }

    let mut expected = BTreeSet::new();
    for (room_id, room) in rooms {
        let canonical = &room.variants[0];
        let door_ids = canonical
            .generated
            .room
            .doors()
            .iter()
            .map(|door| door.id.clone())
            .collect::<Vec<_>>();
        for loadout in EvaluationLoadout::ALL {
            for source in &door_ids {
                match required_kind {
                    "door" => {
                        for target in &door_ids {
                            if source != target {
                                expected.insert((
                                    room_id.clone(),
                                    source.clone(),
                                    target.clone(),
                                    loadout,
                                ));
                            }
                        }
                    }
                    "pickup" => {
                        for pickup in canonical.generated.room.pickups() {
                            expected.insert((
                                room_id.clone(),
                                source.clone(),
                                pickup.id().to_owned(),
                                loadout,
                            ));
                        }
                    }
                    _ => unreachable!("caller supplies a fixed target kind"),
                }
            }
        }
    }
    if actual != expected {
        let missing = expected.difference(&actual).next();
        let unexpected = actual.difference(&expected).next();
        return Err(invalid(format!(
            "{file} is not an exact four-loadout route matrix; first missing={missing:?}, first unexpected={unexpected:?}"
        )));
    }
    Ok(())
}

fn verify_positive_witness(
    row: &TargetRowRecord,
    witness: &WitnessRecord,
    candidate: &downwards_gen::StagedCompositionalCandidate,
) -> Result<(), CorpusVerificationError> {
    if witness.room_id != row.room_id
        || witness.source_door_id != row.source_door_id
        || witness.target_kind != row.target_kind
        || witness.target_id != row.target_id
        || witness.loadout != row.loadout
    {
        return Err(invalid(format!(
            "witness {:?} identity does not match its positive row",
            witness.witness_id
        )));
    }
    if witness.action_encoding_version != CORPUS_ACTION_ENCODING_VERSION {
        return Err(invalid(format!(
            "witness {:?} uses action encoding version {}, expected {}",
            witness.witness_id, witness.action_encoding_version, CORPUS_ACTION_ENCODING_VERSION
        )));
    }
    if witness.total_ticks > MAX_VERIFIABLE_WITNESS_TICKS {
        return Err(invalid(format!(
            "witness {:?} records {} ticks, above verifier limit {}",
            witness.witness_id, witness.total_ticks, MAX_VERIFIABLE_WITNESS_TICKS
        )));
    }

    let mut total_ticks = 0usize;
    let mut previous_action = None;
    for (span_index, span) in witness.actions.iter().enumerate() {
        if span.ticks == 0 {
            return Err(invalid(format!(
                "witness {:?} action span {span_index} has zero ticks",
                witness.witness_id
            )));
        }
        if !(-1..=1).contains(&span.move_x) || !(-1..=1).contains(&span.move_y) {
            return Err(invalid(format!(
                "witness {:?} action span {span_index} has non-normalized movement",
                witness.witness_id
            )));
        }
        total_ticks = total_ticks.checked_add(span.ticks).ok_or_else(|| {
            invalid(format!(
                "witness {:?} tick total overflow",
                witness.witness_id
            ))
        })?;
        let action = Action {
            move_x: span.move_x,
            move_y: span.move_y,
            jump: span.jump,
            dash: span.dash,
            restart: false,
        };
        if previous_action == Some(action) {
            return Err(invalid(format!(
                "witness {:?} has adjacent equal action spans",
                witness.witness_id
            )));
        }
        previous_action = Some(action);
    }
    if total_ticks != witness.total_ticks {
        return Err(invalid(format!(
            "witness {:?} action spans total {total_ticks} ticks, recorded total is {}",
            witness.witness_id, witness.total_ticks
        )));
    }
    let mut actions = Vec::with_capacity(witness.total_ticks);
    for span in &witness.actions {
        let action = Action {
            move_x: span.move_x,
            move_y: span.move_y,
            jump: span.jump,
            dash: span.dash,
            restart: false,
        };
        actions.extend(std::iter::repeat_n(action, span.ticks));
    }

    let initial = Simulation::enter_via_door(
        candidate.generated.room.clone(),
        row.loadout.abilities(),
        &row.source_door_id,
    )
    .map_err(|error| {
        invalid(format!(
            "witness {:?} source-door entry failed: {error}",
            witness.witness_id
        ))
    })?;
    let expected_initial = parse_digest(&witness.initial_digest)?;
    if initial.digest() != expected_initial {
        return Err(invalid(format!(
            "witness {:?} initial digest differs: recorded {}, regenerated {}",
            witness.witness_id,
            expected_initial,
            initial.digest()
        )));
    }

    // Recording executes every action through the authoritative simulation
    // and restores the event/state digests omitted by the compact wire form.
    let replay = Replay::record(&initial, actions);
    let verification = replay.verify(&initial).map_err(|error| {
        invalid(format!(
            "witness {:?} authoritative replay diverged: {error}",
            witness.witness_id
        ))
    })?;
    let (target, reached) = match row.target_kind.as_str() {
        "door" => {
            if verification.reached_exit.as_deref() != Some(row.target_id.as_str()) {
                return Err(invalid(format!(
                    "witness {:?} did not reach target door {:?}; terminal={:?}",
                    witness.witness_id, row.target_id, verification.reached_exit
                )));
            }
            (
                SearchTarget::door(&row.target_id),
                ReachedTarget::Door(row.target_id.clone()),
            )
        }
        "pickup" => {
            if !verification
                .collected_pickup_ids
                .iter()
                .any(|id| id == &row.target_id)
            {
                return Err(invalid(format!(
                    "witness {:?} did not collect target pickup {:?}",
                    witness.witness_id, row.target_id
                )));
            }
            (
                SearchTarget::pickup(&row.target_id),
                ReachedTarget::Pickup(row.target_id.clone()),
            )
        }
        other => {
            return Err(invalid(format!(
                "witness {:?} has unsupported target kind {other:?}",
                witness.witness_id
            )));
        }
    };
    let solution = TargetSolution {
        target,
        reached,
        replay,
        stats: witness.search_effort.into(),
    };
    let actual_fingerprint = match row.target_kind.as_str() {
        "door" => {
            let objective = DoorReachabilityObjective::new(
                &row.source_door_id,
                &row.target_id,
                candidate.generated.metadata.clone(),
                row.loadout.abilities(),
            );
            fingerprint_door_witness(&candidate.generated, &objective, &solution).to_string()
        }
        "pickup" => {
            let objective = PickupFromDoorObjective::new(
                &row.source_door_id,
                &row.target_id,
                candidate.generated.metadata.clone(),
                row.loadout.abilities(),
            );
            fingerprint_pickup_from_door_witness(&candidate.generated, &objective, &solution)
                .to_string()
        }
        _ => unreachable!("target kind was checked above"),
    };
    if actual_fingerprint != witness.witness_id {
        return Err(invalid(format!(
            "witness fingerprint differs: recorded {:?}, recomputed {:?}",
            witness.witness_id, actual_fingerprint
        )));
    }
    Ok(())
}

fn verify_inconclusive_reason(reason: &str) -> Result<(), CorpusVerificationError> {
    match reason {
        "no-exits-defined"
        | "expanded-node-budget"
        | "simulated-tick-budget"
        | "path-horizon"
        | "frontier-exhausted" => Ok(()),
        other => Err(invalid(format!(
            "unsupported inconclusive reason {other:?}; unreachability is never inferred"
        ))),
    }
}

fn verify_stream_count_hashes(bundle: &CorpusArtifactBundle) -> BTreeMap<String, String> {
    artifact_files(bundle)
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), byte_hash(bytes)))
        .collect()
}

/// Canonical non-cryptographic identity of a complete corpus configuration.
pub fn corpus_config_hash(config: &CorpusBuildConfigV1) -> Result<String, CorpusVerificationError> {
    config
        .validate()
        .map_err(|error| invalid(format!("invalid corpus config: {error}")))?;
    let bytes = serde_json::to_vec(config).map_err(|source| CorpusVerificationError::Json {
        file: "corpus config".to_owned(),
        line: 1,
        source,
    })?;
    Ok(byte_hash(&bytes))
}

/// Verify `bundle` and render its immutable one-seed completion checkpoint.
pub fn render_seed_shard_checkpoint(
    bundle: &CorpusArtifactBundle,
) -> Result<Vec<u8>, CorpusVerificationError> {
    let verified = verify_artifact_bundle(bundle)?;
    if verified.config.seed_count != 1 {
        return Err(invalid(format!(
            "seed checkpoint requires seed_count=1, found {}",
            verified.config.seed_count
        )));
    }
    let checkpoint = SeedShardCheckpoint {
        checkpoint_version: CORPUS_SEED_CHECKPOINT_VERSION,
        status: "complete".to_owned(),
        config_hash: verified.config_hash,
        seed: verified.config.start_seed,
        artifact_hashes: verify_stream_count_hashes(bundle),
    };
    let mut bytes =
        serde_json::to_vec(&checkpoint).map_err(|source| CorpusVerificationError::Json {
            file: "seed checkpoint".to_owned(),
            line: 1,
            source,
        })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Check a checkpoint against both an expected configuration and its exact
/// artifact bundle. Any disagreement is rejected rather than treated as a
/// resumable shard.
pub fn verify_seed_shard_checkpoint(
    bytes: &[u8],
    expected_config: &CorpusBuildConfigV1,
    bundle: &CorpusArtifactBundle,
) -> Result<SeedShardCheckpoint, CorpusVerificationError> {
    let mut rows: Vec<SeedShardCheckpoint> = parse_canonical_jsonl("checkpoint.json", bytes)?;
    if rows.len() != 1 {
        return Err(invalid(format!(
            "checkpoint.json must contain exactly one record, found {}",
            rows.len()
        )));
    }
    let checkpoint = rows.pop().expect("length was checked");
    if checkpoint.checkpoint_version != CORPUS_SEED_CHECKPOINT_VERSION {
        return Err(invalid(format!(
            "unsupported checkpoint version {}, expected {}",
            checkpoint.checkpoint_version, CORPUS_SEED_CHECKPOINT_VERSION
        )));
    }
    if checkpoint.status != "complete" {
        return Err(invalid(format!(
            "checkpoint status must be \"complete\", found {:?}",
            checkpoint.status
        )));
    }
    if expected_config.seed_count != 1 {
        return Err(invalid(format!(
            "expected shard config must have seed_count=1, found {}",
            expected_config.seed_count
        )));
    }
    let expected_config_hash = corpus_config_hash(expected_config)?;
    if checkpoint.config_hash != expected_config_hash {
        return Err(invalid(format!(
            "checkpoint config hash mismatch: {:?} != {:?}",
            checkpoint.config_hash, expected_config_hash
        )));
    }
    if checkpoint.seed != expected_config.start_seed {
        return Err(invalid(format!(
            "checkpoint seed mismatch: {} != {}",
            checkpoint.seed, expected_config.start_seed
        )));
    }
    let verified = verify_artifact_bundle(bundle)?;
    if &verified.config != expected_config {
        return Err(invalid(
            "checkpoint expected config differs from the artifact run config",
        ));
    }
    let actual_hashes = verify_stream_count_hashes(bundle);
    if checkpoint.artifact_hashes != actual_hashes {
        return Err(invalid(
            "checkpoint artifact hashes differ from the supplied bundle",
        ));
    }
    Ok(checkpoint)
}

/// Atomically create a new checkpoint file. Existing evidence is never
/// overwritten, including under concurrent writers.
pub fn write_new_seed_shard_checkpoint(
    path: &Path,
    bundle: &CorpusArtifactBundle,
) -> Result<(), CorpusVerificationError> {
    let bytes = render_seed_shard_checkpoint(bundle)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CorpusVerificationError::Io {
            path: parent.to_owned(),
            source,
        })?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                CorpusVerificationError::AlreadyExists(path.to_owned())
            } else {
                CorpusVerificationError::Io {
                    path: path.to_owned(),
                    source,
                }
            }
        })?;
    file.write_all(&bytes)
        .map_err(|source| CorpusVerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    file.sync_all()
        .map_err(|source| CorpusVerificationError::Io {
            path: path.to_owned(),
            source,
        })?;
    Ok(())
}

fn parse_canonical_jsonl<T>(file: &str, bytes: &[u8]) -> Result<Vec<T>, CorpusVerificationError>
where
    T: DeserializeOwned + Serialize,
{
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(invalid(format!("{file} does not end in a newline")));
    }
    if bytes.contains(&b'\r') {
        return Err(invalid(format!("{file} contains a carriage return")));
    }
    let mut result = Vec::new();
    if bytes.is_empty() {
        return Ok(result);
    }
    for (index, line) in bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .enumerate()
    {
        let line_number = index + 1;
        if line.is_empty() {
            return Err(invalid(format!(
                "{file} contains an empty line at {line_number}"
            )));
        }
        let value: T =
            serde_json::from_slice(line).map_err(|source| CorpusVerificationError::Json {
                file: file.to_owned(),
                line: line_number,
                source,
            })?;
        let canonical =
            serde_json::to_vec(&value).map_err(|source| CorpusVerificationError::Json {
                file: file.to_owned(),
                line: line_number,
                source,
            })?;
        if canonical != line {
            return Err(CorpusVerificationError::NonCanonicalJson {
                file: file.to_owned(),
                line: line_number,
            });
        }
        result.push(value);
    }
    Ok(result)
}

fn require_strict_order<T: Ord>(
    label: &str,
    values: impl IntoIterator<Item = T>,
) -> Result<(), CorpusVerificationError> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|previous| previous >= &value) {
            return Err(invalid(format!(
                "{label} is not in strict canonical order or contains a duplicate"
            )));
        }
        previous = Some(value);
    }
    Ok(())
}

fn target_row_key(row: &TargetRowRecord) -> (&RoomId, &str, &str, EvaluationLoadout) {
    (
        &row.room_id,
        &row.source_door_id,
        &row.target_id,
        row.loadout,
    )
}

fn boundary_side_name(side: downwards_core::BoundarySide) -> &'static str {
    match side {
        downwards_core::BoundarySide::Left => "left",
        downwards_core::BoundarySide::Right => "right",
        downwards_core::BoundarySide::Ceiling => "ceiling",
        downwards_core::BoundarySide::Floor => "floor",
    }
}

fn parse_digest(text: &str) -> Result<StateDigest, CorpusVerificationError> {
    if text.len() != 16
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(format!(
            "state digest must be exactly 16 lowercase hexadecimal digits, found {text:?}"
        )));
    }
    u64::from_str_radix(text, 16)
        .map(StateDigest)
        .map_err(|error| invalid(format!("invalid state digest {text:?}: {error}")))
}

fn bundle_streams(bundle: &CorpusArtifactBundle) -> [(&'static str, &[u8]); 5] {
    [
        ("candidates.jsonl", &bundle.candidates_jsonl),
        ("rooms.jsonl", &bundle.rooms_jsonl),
        ("routes.jsonl", &bundle.routes_jsonl),
        ("pickups.jsonl", &bundle.pickups_jsonl),
        ("witnesses.jsonl", &bundle.witnesses_jsonl),
    ]
}

fn artifact_files(bundle: &CorpusArtifactBundle) -> [(&'static str, &[u8]); 6] {
    [
        ("run.json", &bundle.run_json),
        ("candidates.jsonl", &bundle.candidates_jsonl),
        ("rooms.jsonl", &bundle.rooms_jsonl),
        ("routes.jsonl", &bundle.routes_jsonl),
        ("pickups.jsonl", &bundle.pickups_jsonl),
        ("witnesses.jsonl", &bundle.witnesses_jsonl),
    ]
}

fn byte_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64-{hash:016x}")
}

fn invalid(message: impl Into<String>) -> CorpusVerificationError {
    CorpusVerificationError::Invalid(message.into())
}

#[derive(Debug)]
pub enum CorpusVerificationError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        file: String,
        line: usize,
        source: serde_json::Error,
    },
    NonCanonicalJson {
        file: String,
        line: usize,
    },
    AlreadyExists(PathBuf),
    Invalid(String),
}

impl fmt::Display for CorpusVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "could not read/write {}: {source}",
                    path.display()
                )
            }
            Self::Json { file, line, source } => {
                write!(formatter, "invalid JSON in {file} line {line}: {source}")
            }
            Self::NonCanonicalJson { file, line } => {
                write!(formatter, "non-canonical JSON in {file} line {line}")
            }
            Self::AlreadyExists(path) => {
                write!(formatter, "checkpoint already exists: {}", path.display())
            }
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for CorpusVerificationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::NonCanonicalJson { .. } | Self::AlreadyExists(_) | Self::Invalid(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use downwards_validation::ValidationConfig;

    use super::*;
    use crate::corpus::{
        evaluate_route_matrices_with, render_evaluated_artifacts, write_new_artifact_bundle,
    };

    fn verified_bundle() -> CorpusArtifactBundle {
        static BUNDLE: OnceLock<CorpusArtifactBundle> = OnceLock::new();
        BUNDLE
            .get_or_init(|| {
                let generated =
                    generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
                let evaluated = evaluate_route_matrices_with(generated, |loadout| {
                    let mut config = ValidationConfig::for_loadout(loadout.abilities());
                    config.solver.max_expanded_nodes = 1;
                    config.solver.max_simulated_ticks = 2_000;
                    config
                })
                .unwrap();
                render_evaluated_artifacts(&evaluated).unwrap()
            })
            .clone()
    }

    fn rehash_stream(bundle: &mut CorpusArtifactBundle, name: &str) {
        let bytes = artifact_files(bundle)
            .into_iter()
            .find_map(|(candidate, bytes)| (candidate == name).then_some(bytes))
            .unwrap();
        let hash = byte_hash(bytes);
        let mut run: RunRecord = parse_canonical_jsonl("run.json", &bundle.run_json)
            .unwrap()
            .pop()
            .unwrap();
        run.file_hashes.insert(name.to_owned(), hash);
        bundle.run_json = serde_json::to_vec(&run).unwrap();
        bundle.run_json.push(b'\n');
    }

    #[test]
    fn complete_bundle_regenerates_and_replays_byte_repeatably() {
        let bundle = verified_bundle();
        let first = verify_artifact_bundle(&bundle).unwrap();
        let second = verify_artifact_bundle(&bundle).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.config.seed_count, 1);
        assert!(first.rooms > 0);
        assert!(first.route_rows > 0);
        assert!(first.positive_witnesses > 0);

        let first_checkpoint = render_seed_shard_checkpoint(&bundle).unwrap();
        let second_checkpoint = render_seed_shard_checkpoint(&bundle).unwrap();
        assert_eq!(first_checkpoint, second_checkpoint);
        verify_seed_shard_checkpoint(&first_checkpoint, &first.config, &bundle).unwrap();
    }

    #[test]
    fn rejects_hash_mismatch_before_trusting_streams() {
        let mut bundle = verified_bundle();
        bundle.routes_jsonl[0] ^= 1;
        let error = verify_artifact_bundle(&bundle).unwrap_err().to_string();
        assert!(error.contains("hash mismatch for routes.jsonl"), "{error}");
    }

    #[test]
    fn rejects_unknown_json_fields_even_with_a_matching_file_hash() {
        let mut bundle = verified_bundle();
        let text = String::from_utf8(bundle.routes_jsonl).unwrap();
        bundle.routes_jsonl = text
            .replacen("}\n", ",\"unexpected\":true}\n", 1)
            .into_bytes();
        rehash_stream(&mut bundle, "routes.jsonl");
        let error = verify_artifact_bundle(&bundle).unwrap_err().to_string();
        assert!(error.contains("unknown field"), "{error}");
    }

    #[test]
    fn rejects_unreachable_claims_even_with_a_matching_file_hash() {
        let mut bundle = verified_bundle();
        let text = String::from_utf8(bundle.routes_jsonl).unwrap();
        assert!(text.contains("bounded_inconclusive"));
        bundle.routes_jsonl = text
            .replacen("bounded_inconclusive", "unreachable", 1)
            .into_bytes();
        rehash_stream(&mut bundle, "routes.jsonl");
        let error = verify_artifact_bundle(&bundle).unwrap_err().to_string();
        assert!(error.contains("unknown variant `unreachable`"), "{error}");
    }

    #[test]
    fn rejects_truncated_route_matrix_with_a_matching_file_hash() {
        let mut bundle = verified_bundle();
        let last_start = bundle.routes_jsonl[..bundle.routes_jsonl.len() - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        bundle.routes_jsonl.truncate(last_start);
        rehash_stream(&mut bundle, "routes.jsonl");
        let error = verify_artifact_bundle(&bundle).unwrap_err().to_string();
        assert!(
            error.contains("run row/evidence counts")
                || error.contains("exact four-loadout route matrix"),
            "{error}"
        );
    }

    #[test]
    fn rejects_corrupt_positive_witness_after_authoritative_replay() {
        let mut bundle = verified_bundle();
        let text = String::from_utf8(bundle.witnesses_jsonl).unwrap();
        let marker = "\"initial_digest\":\"";
        let start = text.find(marker).unwrap() + marker.len();
        let mut bytes = text.into_bytes();
        bytes[start] = if bytes[start] == b'0' { b'1' } else { b'0' };
        bundle.witnesses_jsonl = bytes;
        rehash_stream(&mut bundle, "witnesses.jsonl");
        let error = verify_artifact_bundle(&bundle).unwrap_err().to_string();
        assert!(error.contains("initial digest differs"), "{error}");
    }

    #[test]
    fn rejects_restart_injection_and_corrupt_geometry_fingerprints() {
        let mut restart_bundle = verified_bundle();
        let text = String::from_utf8(restart_bundle.witnesses_jsonl).unwrap();
        assert!(text.contains("\"actions\":[{"));
        restart_bundle.witnesses_jsonl = text
            .replacen("\"actions\":[{", "\"actions\":[{\"restart\":true,", 1)
            .into_bytes();
        rehash_stream(&mut restart_bundle, "witnesses.jsonl");
        let error = verify_artifact_bundle(&restart_bundle)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field `restart`"), "{error}");

        let mut geometry_bundle = verified_bundle();
        let text = String::from_utf8(geometry_bundle.rooms_jsonl).unwrap();
        let marker = "\"static_visual_fingerprint\":\"downwards-corpus-static-v1-";
        let start = text.find(marker).unwrap() + marker.len();
        let mut bytes = text.into_bytes();
        bytes[start] = if bytes[start] == b'0' { b'1' } else { b'0' };
        geometry_bundle.rooms_jsonl = bytes;
        rehash_stream(&mut geometry_bundle, "rooms.jsonl");
        let error = verify_artifact_bundle(&geometry_bundle)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("static visual fingerprint differs"),
            "{error}"
        );
    }

    #[test]
    fn checkpoint_mismatch_fails_closed_and_checkpoint_is_immutable() {
        let bundle = verified_bundle();
        let verified = verify_artifact_bundle(&bundle).unwrap();
        let checkpoint = render_seed_shard_checkpoint(&bundle).unwrap();
        let mut wrong_config = verified.config.clone();
        wrong_config.target_max_rooms += 1;
        let error = verify_seed_shard_checkpoint(&checkpoint, &wrong_config, &bundle)
            .unwrap_err()
            .to_string();
        assert!(error.contains("config hash mismatch"), "{error}");

        let unique = format!(
            "downwards-corpus-checkpoint-{}-{}",
            std::process::id(),
            byte_hash(&checkpoint)
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir(&directory).unwrap();
        write_new_artifact_bundle(&directory, &bundle).unwrap();
        assert_eq!(verify_artifact_directory(&directory).unwrap(), verified);
        let path = directory.join("seed.checkpoint.json");
        write_new_seed_shard_checkpoint(&path, &bundle).unwrap();
        assert!(matches!(
            write_new_seed_shard_checkpoint(&path, &bundle),
            Err(CorpusVerificationError::AlreadyExists(existing)) if existing == path
        ));
        let stored = fs::read(&path).unwrap();
        verify_seed_shard_checkpoint(&stored, &verified.config, &bundle).unwrap();
        fs::remove_dir_all(&directory).unwrap();
    }
}
