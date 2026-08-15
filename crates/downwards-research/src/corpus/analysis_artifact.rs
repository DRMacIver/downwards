//! Canonical, replay-verifiable persistence for per-seed deep room analysis.
//!
//! This artifact deliberately keeps controller demand, canonical matrix-route
//! vectors, terrain evidence, aggregate room metrics, and operational cost in
//! separate fields.  In particular, canonical matrix witnesses are labelled
//! as not easiest-known, and bounded controller non-success is never encoded
//! as an unreachable claim.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::{error::Error, fmt};

use downwards_ai::{
    DIRECT_PROBE_AUDIT_VERSION, DirectProbeBudgetLimit, Replay, SOLVER_POLICY_VERSION, SearchStats,
    SolverConfig,
};
use downwards_core::{AbilitySet, Action, Simulation};
use downwards_gen::{
    CompositionalKey, CompositionalProfile, StagedCompositionalKey,
    experimental::{ChallengeIntent, EXPERIMENTAL_GENERATION_VERSION, GenerationStrategy},
    generate_staged_compositional,
};
use downwards_lab::{
    LANDING_PRECISION_DISCLAIMER, LANDING_PRECISION_VERSION, LandingPrecisionReport,
    LandingSupportKind, ROUTE_DIFFICULTY_VECTOR_VERSION, RoomAblationKind, RouteDifficultyVector,
    RouteDiversityReport, SimulationGeometryDescriptor, StaticVisualDescriptor,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{
    CONTROLLER_DEMAND_POLICY_VERSION, CORPUS_ROOM_ANALYSIS_VERSION, CORPUS_ROOM_ID_VERSION,
    CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, CandidateKeyRecord, ControllerDemand,
    ControllerDemandCoordinates, CorpusRoomAnalysis, CorpusRoomAnalysisConfig, EvaluationLoadout,
    FeatureStageRecord, LoadoutControllerAuditStatus, MetricEvidence, MissingMetricReason,
    NotApplicableMetricReason, ROOM_METRIC_SUMMARY_DISCLAIMER, ROOM_METRIC_SUMMARY_VERSION,
    ROUTE_CONTROLLER_ASSESSMENT_DISCLAIMER, ROUTE_CONTROLLER_TRACE_VERSION, RoomId,
    RoomMetricSummary, TERRAIN_AUDIT_EVIDENCE_DISCLAIMER, TERRAIN_AUDIT_VERSION,
    fingerprints::fingerprint_static_visual,
};

/// Version of the complete deep-analysis artifact schema and validation policy.
pub const CORPUS_ANALYSIS_ARTIFACT_VERSION: u32 = 3;

/// Version of the normalized run-length action encoding in this artifact.
pub const CORPUS_ANALYSIS_ACTION_ENCODING_VERSION: u32 = 1;

const ARTIFACT_SCHEMA: &str = "downwards-corpus-per-seed-deep-analysis";
const ARTIFACT_STATUS: &str = "deep_analysis";
const CANONICAL_ROUTE_CLAIM: &str = "canonical_matrix_witness_measurement_not_easiest_known";
const CONTROLLER_WITNESS_PREFIX: &str = "downwards-controller-witness-v1";

/// Canonical bytes for one seed's deep-analysis evidence.
///
/// Every non-empty stream ends in exactly one newline. `manifest_json` binds
/// both JSONL streams by deterministic FNV-1a hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusAnalysisArtifactBundle {
    pub manifest_json: Vec<u8>,
    pub rooms_jsonl: Vec<u8>,
    pub controller_witnesses_jsonl: Vec<u8>,
}

/// One keyed `(RoomId, CorpusRoomAnalysis, RoomMetricSummary)` input row.
///
/// The key is intentionally required rather than inferred from `RoomId`: it
/// is the exact regeneration contract used by replay verification.
#[derive(Clone, Copy, Debug)]
pub struct CorpusAnalysisArtifactRoomRef<'a> {
    pub generation_key: &'a CandidateKeyRecord,
    pub room_id: &'a RoomId,
    pub analysis: &'a CorpusRoomAnalysis,
    pub metrics: &'a RoomMetricSummary,
}

/// Successful verification summary. The parsed DTOs remain private so callers
/// cannot accidentally treat this persistence schema as the in-memory model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedCorpusAnalysisArtifact {
    pub seed: u64,
    pub rooms: usize,
    pub retained_controller_witnesses: usize,
    pub canonical_route_vectors: usize,
}

/// Render a deterministic deep-analysis artifact for one seed.
///
/// Input order is irrelevant. Rows and retained witnesses are sorted by their
/// stable identities before serialization.
pub fn render_seed_analysis_artifact<'a>(
    seed: u64,
    config: &CorpusRoomAnalysisConfig,
    rooms: impl IntoIterator<Item = CorpusAnalysisArtifactRoomRef<'a>>,
) -> Result<CorpusAnalysisArtifactBundle, CorpusAnalysisArtifactError> {
    let config = AnalysisConfigRecord::from_config(config)?;
    let config_fingerprint = fingerprint_json(&config)?;
    let mut witness_records = BTreeMap::<String, ControllerWitnessRecord>::new();
    let mut room_records = rooms
        .into_iter()
        .map(|room| room_record(seed, room, &mut witness_records))
        .collect::<Result<Vec<_>, _>>()?;
    room_records.sort_unstable_by(|left, right| left.room_id.cmp(&right.room_id));
    require_strict_order(
        "rendered room IDs",
        room_records.iter().map(|room| room.room_id.as_str()),
    )?;

    let controller_witnesses = witness_records.into_values().collect::<Vec<_>>();
    let rooms_jsonl = render_json_lines(room_records.iter())?;
    let controller_witnesses_jsonl = render_json_lines(controller_witnesses.iter())?;
    let canonical_route_vectors = room_records
        .iter()
        .map(|room| room.canonical_route_difficulty.routes.len())
        .sum();
    let manifest = ManifestRecord {
        artifact_schema: ARTIFACT_SCHEMA.to_owned(),
        artifact_version: CORPUS_ANALYSIS_ARTIFACT_VERSION,
        status: ARTIFACT_STATUS.to_owned(),
        seed,
        room_count: room_records.len(),
        retained_controller_witness_count: controller_witnesses.len(),
        canonical_route_vector_count: canonical_route_vectors,
        policies: PolicyIdentityRecord::current(),
        config,
        config_fingerprint,
        disclaimers: DisclaimerRecord::current(),
        stream_hashes: BTreeMap::from([
            (
                "controller_witnesses.jsonl".to_owned(),
                byte_hash(&controller_witnesses_jsonl),
            ),
            ("rooms.jsonl".to_owned(), byte_hash(&rooms_jsonl)),
        ]),
    };
    let manifest_json = render_single_json(&manifest)?;
    let bundle = CorpusAnalysisArtifactBundle {
        manifest_json,
        rooms_jsonl,
        controller_witnesses_jsonl,
    };

    // Rendering fails closed on the same structural contract used by parsing.
    verify_decoded(&manifest, &room_records, &controller_witnesses, false)?;
    Ok(bundle)
}

/// Parse canonical bytes, validate identities/order/cross-references/counts,
/// regenerate every room, and exactly replay every retained controller.
pub fn parse_and_verify_seed_analysis_artifact(
    bundle: &CorpusAnalysisArtifactBundle,
) -> Result<VerifiedCorpusAnalysisArtifact, CorpusAnalysisArtifactError> {
    let mut manifests: Vec<ManifestRecord> =
        parse_canonical_jsonl("manifest.json", &bundle.manifest_json)?;
    if manifests.len() != 1 {
        return Err(invalid(format!(
            "manifest.json must contain exactly one record, found {}",
            manifests.len()
        )));
    }
    let manifest = manifests.pop().expect("length checked");
    let rooms: Vec<RoomRecord> = parse_canonical_jsonl("rooms.jsonl", &bundle.rooms_jsonl)?;
    let witnesses: Vec<ControllerWitnessRecord> = parse_canonical_jsonl(
        "controller_witnesses.jsonl",
        &bundle.controller_witnesses_jsonl,
    )?;

    let expected_hashes = BTreeMap::from([
        (
            "controller_witnesses.jsonl".to_owned(),
            byte_hash(&bundle.controller_witnesses_jsonl),
        ),
        ("rooms.jsonl".to_owned(), byte_hash(&bundle.rooms_jsonl)),
    ]);
    if manifest.stream_hashes != expected_hashes {
        return Err(invalid(format!(
            "manifest stream hashes differ: recorded {:?}, actual {:?}",
            manifest.stream_hashes, expected_hashes
        )));
    }

    verify_decoded(&manifest, &rooms, &witnesses, true)?;
    Ok(VerifiedCorpusAnalysisArtifact {
        seed: manifest.seed,
        rooms: rooms.len(),
        retained_controller_witnesses: witnesses.len(),
        canonical_route_vectors: rooms
            .iter()
            .map(|room| room.canonical_route_difficulty.routes.len())
            .sum(),
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRecord {
    artifact_schema: String,
    artifact_version: u32,
    status: String,
    seed: u64,
    room_count: usize,
    retained_controller_witness_count: usize,
    canonical_route_vector_count: usize,
    policies: PolicyIdentityRecord,
    config: AnalysisConfigRecord,
    config_fingerprint: String,
    disclaimers: DisclaimerRecord,
    stream_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyIdentityRecord {
    experimental_generation_version: u32,
    corpus_room_id_version: u32,
    corpus_room_analysis_version: u32,
    room_metric_summary_version: u32,
    route_controller_assessment_version: u32,
    direct_probe_audit_version: u32,
    route_controller_trace_version: u32,
    controller_demand_policy_version: u32,
    terrain_audit_version: u32,
    landing_precision_version: u32,
    route_difficulty_vector_version: u32,
    solver_policy_version: u32,
    action_encoding_version: u32,
}

impl PolicyIdentityRecord {
    fn current() -> Self {
        Self {
            experimental_generation_version: EXPERIMENTAL_GENERATION_VERSION,
            corpus_room_id_version: CORPUS_ROOM_ID_VERSION,
            corpus_room_analysis_version: CORPUS_ROOM_ANALYSIS_VERSION,
            room_metric_summary_version: ROOM_METRIC_SUMMARY_VERSION,
            route_controller_assessment_version: CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY
                .assessment_version,
            direct_probe_audit_version: DIRECT_PROBE_AUDIT_VERSION,
            route_controller_trace_version: ROUTE_CONTROLLER_TRACE_VERSION,
            controller_demand_policy_version: CONTROLLER_DEMAND_POLICY_VERSION,
            terrain_audit_version: TERRAIN_AUDIT_VERSION,
            landing_precision_version: LANDING_PRECISION_VERSION,
            route_difficulty_vector_version: ROUTE_DIFFICULTY_VECTOR_VERSION,
            solver_policy_version: SOLVER_POLICY_VERSION,
            action_encoding_version: CORPUS_ANALYSIS_ACTION_ENCODING_VERSION,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisclaimerRecord {
    route_controller: String,
    terrain: String,
    room_metrics: String,
    landing_precision: String,
    canonical_route_claim: String,
}

impl DisclaimerRecord {
    fn current() -> Self {
        Self {
            route_controller: ROUTE_CONTROLLER_ASSESSMENT_DISCLAIMER.to_owned(),
            terrain: TERRAIN_AUDIT_EVIDENCE_DISCLAIMER.to_owned(),
            room_metrics: ROOM_METRIC_SUMMARY_DISCLAIMER.to_owned(),
            landing_precision: LANDING_PRECISION_DISCLAIMER.to_owned(),
            canonical_route_claim: CANONICAL_ROUTE_CLAIM.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisConfigRecord {
    direct_controller_solver: SolverConfigRecord,
    canonical_witness_difficulty: DifficultyConfigRecord,
}

impl AnalysisConfigRecord {
    fn from_config(config: &CorpusRoomAnalysisConfig) -> Result<Self, CorpusAnalysisArtifactError> {
        Ok(Self {
            direct_controller_solver: SolverConfigRecord::from_solver(
                &config.direct_controller_solver,
            )?,
            canonical_witness_difficulty: DifficultyConfigRecord {
                perturbation_grace_ticks: config
                    .canonical_witness_difficulty
                    .perturbation_grace_ticks,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SolverConfigRecord {
    max_expanded_nodes: usize,
    max_simulated_ticks: usize,
    max_ticks_per_path: usize,
    beam_width: usize,
    position_quantum: i32,
    velocity_quantum: i32,
    probe_direct_routes: bool,
    baseline_preview_max_expanded_nodes: usize,
    baseline_preview_max_simulated_ticks: usize,
    macros: Vec<SolverMacroRecord>,
}

impl SolverConfigRecord {
    fn from_solver(config: &SolverConfig) -> Result<Self, CorpusAnalysisArtifactError> {
        Ok(Self {
            max_expanded_nodes: config.max_expanded_nodes,
            max_simulated_ticks: config.max_simulated_ticks,
            max_ticks_per_path: config.max_ticks_per_path,
            beam_width: config.beam_width,
            position_quantum: config.position_quantum,
            velocity_quantum: config.velocity_quantum,
            probe_direct_routes: config.probe_direct_routes,
            baseline_preview_max_expanded_nodes: config.baseline_preview_max_expanded_nodes,
            baseline_preview_max_simulated_ticks: config.baseline_preview_max_simulated_ticks,
            macros: config
                .macros
                .iter()
                .map(|action_macro| {
                    Ok(SolverMacroRecord {
                        name: action_macro.name.clone(),
                        actions: encode_actions(action_macro.actions.iter().copied())?,
                    })
                })
                .collect::<Result<_, CorpusAnalysisArtifactError>>()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SolverMacroRecord {
    name: String,
    actions: Vec<ActionSpanRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DifficultyConfigRecord {
    perturbation_grace_ticks: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomRecord {
    row_version: u32,
    room_id: String,
    generation_key: CandidateKeyRecord,
    analysis_version: u32,
    metric_summary_version: u32,
    door_ids: Vec<String>,
    direct_controller_evidence: DirectControllerEvidenceRecord,
    canonical_route_difficulty: CanonicalRouteDifficultyRecord,
    terrain_positive_evidence: TerrainPositiveEvidenceRecord,
    metric_aggregates: MetricAggregatesRecord,
    operational_cost: OperationalCostSummaryRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectControllerEvidenceRecord {
    source_audits: Vec<SourceAuditRecord>,
    directed_routes: Vec<DirectedControllerRouteRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceAuditRecord {
    source_door_id: String,
    target_door_ids: Vec<String>,
    authoritative_loadout: EvaluationLoadout,
    expected_subset_loadouts: Vec<EvaluationLoadout>,
    shared_loadout_audits: Vec<SharedLoadoutAuditRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedLoadoutAuditRecord {
    loadout: EvaluationLoadout,
    status: AuditStatusRecord,
    target_count: usize,
    raw_positive_witnesses: usize,
    retained_semantic_witnesses: usize,
    operational_cost_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectedControllerRouteRecord {
    source_door_id: String,
    target_door_id: String,
    authoritative_loadout: EvaluationLoadout,
    expected_subset_loadouts: Vec<EvaluationLoadout>,
    completeness: RouteCompletenessRecord,
    overall_easiest_known_witness_id: Option<String>,
    overall_pareto_front_witness_ids: Vec<String>,
    loadouts: Vec<DirectedControllerLoadoutRecord>,
    positive_bypasses: Vec<PositiveBypassRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectedControllerLoadoutRecord {
    loadout: EvaluationLoadout,
    status: AuditStatusRecord,
    raw_positive_witnesses: usize,
    retained_semantic_witnesses: usize,
    easiest_known_witness_id: Option<String>,
    pareto_front_witness_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PositiveBypassRecord {
    loadout: EvaluationLoadout,
    retained_semantic_witnesses: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetLimitRecord {
    ExpandedNodes,
    SimulatedTicks,
}

impl From<DirectProbeBudgetLimit> for BudgetLimitRecord {
    fn from(limit: DirectProbeBudgetLimit) -> Self {
        match limit {
            DirectProbeBudgetLimit::ExpandedNodes => Self::ExpandedNodes,
            DirectProbeBudgetLimit::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum AuditStatusRecord {
    CompleteFiniteVocabulary,
    BoundedIncomplete { limit: BudgetLimitRecord },
}

impl From<LoadoutControllerAuditStatus> for AuditStatusRecord {
    fn from(status: LoadoutControllerAuditStatus) -> Self {
        match status {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                Self::CompleteFiniteVocabulary
            }
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => Self::BoundedIncomplete {
                limit: limit.into(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum RouteCompletenessRecord {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        incomplete_loadouts: Vec<BoundedLoadoutRecord>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedLoadoutRecord {
    loadout: EvaluationLoadout,
    limit: BudgetLimitRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerWitnessRecord {
    witness_id: String,
    room_id: String,
    source_door_id: String,
    target_door_id: String,
    loadout: EvaluationLoadout,
    initial_state_digest: String,
    terminal_state_digest: String,
    terminal_event_digest: Option<String>,
    total_ticks: usize,
    demand: ControllerDemandRecord,
    coordinates: ControllerDemandCoordinatesRecord,
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

impl ActionSpanRecord {
    const fn same_action(self, other: Self) -> bool {
        self.move_x == other.move_x
            && self.move_y == other.move_y
            && self.jump == other.jump
            && self.dash == other.dash
    }

    const fn action(self) -> Action {
        Action {
            move_x: self.move_x,
            move_y: self.move_y,
            jump: self.jump,
            dash: self.dash,
            restart: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerDemandRecord {
    run_only: bool,
    monotone_simple: bool,
    ordinary_jump_events: usize,
    wall_jump_events: usize,
    dash_events: usize,
    jump_press_edges: usize,
    dash_press_edges: usize,
    horizontal_reversals: usize,
    vertical_input_changes: usize,
    dash_direction_changes: usize,
    vertical_decisions: usize,
    semantic_spans: usize,
    semantic_transitions: usize,
    duration_ticks: usize,
}

impl From<ControllerDemand> for ControllerDemandRecord {
    fn from(demand: ControllerDemand) -> Self {
        Self {
            run_only: demand.run_only,
            monotone_simple: demand.monotone_simple,
            ordinary_jump_events: demand.ordinary_jump_events,
            wall_jump_events: demand.wall_jump_events,
            dash_events: demand.dash_events,
            jump_press_edges: demand.jump_press_edges,
            dash_press_edges: demand.dash_press_edges,
            horizontal_reversals: demand.horizontal_reversals,
            vertical_input_changes: demand.vertical_input_changes,
            dash_direction_changes: demand.dash_direction_changes,
            vertical_decisions: demand.vertical_decisions,
            semantic_spans: demand.semantic_spans,
            semantic_transitions: demand.semantic_transitions,
            duration_ticks: demand.duration_ticks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerDemandCoordinatesRecord {
    controller_class: u8,
    ability_events: usize,
    horizontal_reversals: usize,
    vertical_decisions: usize,
    semantic_spans: usize,
    semantic_transitions: usize,
    duration_ticks: usize,
}

impl From<ControllerDemandCoordinates> for ControllerDemandCoordinatesRecord {
    fn from(coordinates: ControllerDemandCoordinates) -> Self {
        Self {
            controller_class: coordinates.controller_class,
            ability_events: coordinates.ability_events,
            horizontal_reversals: coordinates.horizontal_reversals,
            vertical_decisions: coordinates.vertical_decisions,
            semantic_spans: coordinates.semantic_spans,
            semantic_transitions: coordinates.semantic_transitions,
            duration_ticks: coordinates.duration_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRouteDifficultyRecord {
    claim: String,
    routes: Vec<CanonicalRouteVectorRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRouteVectorRecord {
    measurement_version: u32,
    source_door_id: String,
    target_door_id: String,
    loadout: EvaluationLoadout,
    witness_fingerprint: String,
    landing_precision: LandingPrecisionRecord,
    vector: PlayerRouteDifficultyVectorRecord,
    operational_cost_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandingPrecisionRecord {
    version: u32,
    interpretation: String,
    inspected_ticks: usize,
    landing_event_count: usize,
    samples: Vec<LandingSampleRecord>,
    unmeasured_landing_ticks: Vec<usize>,
    minimum_footprint_overlap_pixels: Option<u32>,
    minimum_edge_margin_pixels: Option<i32>,
    narrowest_support_width_pixels: Option<u32>,
    edge_overhang_landings: usize,
    one_way_or_mixed_landings: usize,
}

impl From<&LandingPrecisionReport> for LandingPrecisionRecord {
    fn from(report: &LandingPrecisionReport) -> Self {
        Self {
            version: report.version,
            interpretation: LANDING_PRECISION_DISCLAIMER.to_owned(),
            inspected_ticks: report.inspected_ticks,
            landing_event_count: report.landing_event_count,
            samples: report.samples.iter().copied().map(Into::into).collect(),
            unmeasured_landing_ticks: report.unmeasured_landing_ticks.to_vec(),
            minimum_footprint_overlap_pixels: report.minimum_footprint_overlap_pixels,
            minimum_edge_margin_pixels: report.minimum_edge_margin_pixels,
            narrowest_support_width_pixels: report.narrowest_support_width_pixels,
            edge_overhang_landings: report.edge_overhang_landings,
            one_way_or_mixed_landings: report.one_way_or_mixed_landings,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandingSampleRecord {
    replay_tick: usize,
    player_x: i32,
    player_y: i32,
    player_width: i32,
    player_height: i32,
    surface_y: i32,
    support_left: i32,
    support_right: i32,
    support_kind: LandingSupportKindRecord,
    footprint_overlap_pixels: u32,
    left_edge_margin_pixels: i32,
    right_edge_margin_pixels: i32,
}

impl From<downwards_lab::LandingSample> for LandingSampleRecord {
    fn from(sample: downwards_lab::LandingSample) -> Self {
        Self {
            replay_tick: sample.replay_tick,
            player_x: sample.player_bounds.x,
            player_y: sample.player_bounds.y,
            player_width: sample.player_bounds.width,
            player_height: sample.player_bounds.height,
            surface_y: sample.surface_y,
            support_left: sample.support_left,
            support_right: sample.support_right,
            support_kind: sample.support_kind.into(),
            footprint_overlap_pixels: sample.footprint_overlap_pixels,
            left_edge_margin_pixels: sample.left_edge_margin_pixels,
            right_edge_margin_pixels: sample.right_edge_margin_pixels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LandingSupportKindRecord {
    Solid,
    OneWay,
    Mixed,
}

impl From<LandingSupportKind> for LandingSupportKindRecord {
    fn from(kind: LandingSupportKind) -> Self {
        match kind {
            LandingSupportKind::Solid => Self::Solid,
            LandingSupportKind::OneWay => Self::OneWay,
            LandingSupportKind::Mixed => Self::Mixed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerRouteDifficultyVectorRecord {
    version: u32,
    interpretation: String,
    target_id: String,
    traversal: TraversalDemandRecord,
    control: ControlDemandVectorRecord,
    hazards: HazardPressureRecord,
    timing: TimingRobustnessRecord,
}

impl PlayerRouteDifficultyVectorRecord {
    fn from_vector(vector: &RouteDifficultyVector) -> Result<Self, CorpusAnalysisArtifactError> {
        let shaky_hand = match &vector.timing.shaky_hand {
            downwards_lab::ShakyHandEvidence::Missing { reason } => {
                let reason = match reason {
                    downwards_lab::MissingEvidenceReason::ShakyHandStudyNotProvided => {
                        MissingTimingEvidenceRecord::ShakyHandStudyNotProvided
                    }
                    downwards_lab::MissingEvidenceReason::CurveNotRecorded => {
                        MissingTimingEvidenceRecord::CurveNotRecorded
                    }
                };
                ShakyHandStatusRecord::Missing { reason }
            }
            downwards_lab::ShakyHandEvidence::Observed { .. } => {
                return Err(invalid(
                    "canonical deep analysis unexpectedly contains shaky-hand evidence; schema v1 requires the configured no-shaky-hand policy",
                ));
            }
        };
        Ok(Self {
            version: vector.version,
            interpretation: vector.interpretation.to_owned(),
            target_id: vector.target_id.clone(),
            traversal: TraversalDemandRecord {
                grid_columns: vector.traversal.grid_columns,
                grid_rows: vector.traversal.grid_rows,
                completion_ticks: vector.traversal.completion_ticks,
                coarse_path_length_cells: vector.traversal.coarse_path_length_cells,
                horizontal_travel_cells: vector.traversal.horizontal_travel_cells,
                vertical_travel_cells: vector.traversal.vertical_travel_cells,
                horizontal_span_cells: vector.traversal.horizontal_span_cells,
                vertical_span_cells: vector.traversal.vertical_span_cells,
                spatial_direction_changes: vector.traversal.spatial_direction_changes,
                spatial_reversals: vector.traversal.spatial_reversals,
                visited_cell_count: vector.traversal.visited_cell_count,
            },
            control: ControlDemandVectorRecord {
                meaningful_input_transitions: vector.control.meaningful_input_transitions,
                movement_direction_changes: vector.control.movement_direction_changes,
                horizontal_input_reversals: vector.control.horizontal_input_reversals,
                vertical_input_reversals: vector.control.vertical_input_reversals,
                jump_presses: vector.control.jump_presses,
                dash_presses: vector.control.dash_presses,
                restart_presses: vector.control.restart_presses,
                active_control_ticks: vector.control.active_control_ticks,
                simultaneous_control_ticks: vector.control.simultaneous_control_ticks,
                accepted_movement: AcceptedMovementRecord {
                    grounded_jumps: vector.control.accepted_movement.grounded_jumps,
                    coyote_jumps: vector.control.accepted_movement.coyote_jumps,
                    buffered_jumps: vector.control.accepted_movement.buffered_jumps,
                    wall_jumps: vector.control.accepted_movement.wall_jumps,
                    dashes: vector.control.accepted_movement.dashes,
                },
            },
            hazards: HazardPressureRecord {
                deaths_before_completion: vector.hazards.deaths_before_completion,
                minimum_clearance: match vector.hazards.minimum_clearance {
                    downwards_lab::HazardClearanceEvidence::Observed {
                        pixels,
                        replay_tick,
                    } => HazardClearanceRecord::Observed {
                        pixels,
                        replay_tick,
                    },
                    downwards_lab::HazardClearanceEvidence::NotApplicableNoRelevantHazard => {
                        HazardClearanceRecord::NotApplicableNoRelevantHazard
                    }
                },
                pressure: vector.hazards.pressure,
            },
            timing: TimingRobustnessRecord {
                perfect_control: vector.timing.perfect_control.into(),
                shaky_hand,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraversalDemandRecord {
    grid_columns: u16,
    grid_rows: u16,
    completion_ticks: usize,
    #[serde(with = "canonical_f64")]
    coarse_path_length_cells: f64,
    #[serde(with = "canonical_f64")]
    horizontal_travel_cells: f64,
    #[serde(with = "canonical_f64")]
    vertical_travel_cells: f64,
    horizontal_span_cells: u16,
    vertical_span_cells: u16,
    spatial_direction_changes: usize,
    spatial_reversals: usize,
    visited_cell_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlDemandVectorRecord {
    meaningful_input_transitions: usize,
    movement_direction_changes: usize,
    horizontal_input_reversals: usize,
    vertical_input_reversals: usize,
    jump_presses: usize,
    dash_presses: usize,
    restart_presses: usize,
    active_control_ticks: usize,
    simultaneous_control_ticks: usize,
    accepted_movement: AcceptedMovementRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedMovementRecord {
    grounded_jumps: usize,
    coyote_jumps: usize,
    buffered_jumps: usize,
    wall_jumps: usize,
    dashes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HazardPressureRecord {
    deaths_before_completion: u32,
    minimum_clearance: HazardClearanceRecord,
    #[serde(with = "canonical_f64")]
    pressure: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
enum HazardClearanceRecord {
    Observed { pixels: u32, replay_tick: usize },
    NotApplicableNoRelevantHazard,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingRobustnessRecord {
    perfect_control: TimingOutcomeProbabilitiesRecord,
    shaky_hand: ShakyHandStatusRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingOutcomeProbabilitiesRecord {
    trials: usize,
    #[serde(with = "canonical_f64")]
    success_probability: f64,
    #[serde(with = "canonical_f64")]
    failure_probability: f64,
    #[serde(with = "canonical_f64")]
    death_probability: f64,
    #[serde(with = "canonical_f64")]
    wrong_target_probability: f64,
    #[serde(with = "canonical_f64")]
    timeout_probability: f64,
}

impl From<downwards_lab::TimingOutcomeProbabilities> for TimingOutcomeProbabilitiesRecord {
    fn from(value: downwards_lab::TimingOutcomeProbabilities) -> Self {
        Self {
            trials: value.trials,
            success_probability: value.success_probability,
            failure_probability: value.failure_probability,
            death_probability: value.death_probability,
            wrong_target_probability: value.wrong_target_probability,
            timeout_probability: value.timeout_probability,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
enum ShakyHandStatusRecord {
    Missing { reason: MissingTimingEvidenceRecord },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MissingTimingEvidenceRecord {
    ShakyHandStudyNotProvided,
    CurveNotRecorded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainPositiveEvidenceRecord {
    audit_version: u32,
    structural_descriptor_version: u32,
    room_ablation_version: u32,
    loadout: AbilitySetRecord,
    positive_door_controller_count: usize,
    positive_pickup_controller_count: usize,
    coverage: PositiveTerrainCoverageRecord,
    component_ablation_outcomes: Vec<AblationOutcomeRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilitySetRecord {
    wall_jump: bool,
    dash: bool,
}

impl From<AbilitySet> for AbilitySetRecord {
    fn from(value: AbilitySet) -> Self {
        Self {
            wall_jump: value.wall_jump,
            dash: value.dash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PositiveTerrainCoverageRecord {
    positive_controller_count: usize,
    interior_component_count: usize,
    interior_tile_count: usize,
    structurally_attributed_component_count: usize,
    structurally_attributed_tile_count: usize,
    traversal_near_component_count: usize,
    traversal_near_tile_count: usize,
    positively_corroborated_component_count: usize,
    positively_corroborated_tile_count: usize,
    uncorroborated_component_count: usize,
    uncorroborated_tile_count: usize,
}

impl From<super::PositiveTerrainCoverage> for PositiveTerrainCoverageRecord {
    fn from(value: super::PositiveTerrainCoverage) -> Self {
        Self {
            positive_controller_count: value.positive_controller_count,
            interior_component_count: value.interior_component_count,
            interior_tile_count: value.interior_tile_count,
            structurally_attributed_component_count: value.structurally_attributed_component_count,
            structurally_attributed_tile_count: value.structurally_attributed_tile_count,
            traversal_near_component_count: value.traversal_near_component_count,
            traversal_near_tile_count: value.traversal_near_tile_count,
            positively_corroborated_component_count: value.positively_corroborated_component_count,
            positively_corroborated_tile_count: value.positively_corroborated_tile_count,
            uncorroborated_component_count: value.uncorroborated_component_count,
            uncorroborated_tile_count: value.uncorroborated_tile_count,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AblationOutcomeRecord {
    kind: RoomAblationKindRecord,
    controller_count: usize,
    succeeded: usize,
    died: usize,
    wrong_target: usize,
    diverged: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RoomAblationKindRecord {
    InteriorTerrainComponent {
        component_index: usize,
        tile_count: usize,
    },
    StaticHazardComponent {
        component_index: usize,
        tile_count: usize,
    },
    TimedHazard {
        hazard_index: usize,
    },
}

impl From<RoomAblationKind> for RoomAblationKindRecord {
    fn from(value: RoomAblationKind) -> Self {
        match value {
            RoomAblationKind::InteriorTerrainComponent {
                component_index,
                tile_count,
            } => Self::InteriorTerrainComponent {
                component_index,
                tile_count,
            },
            RoomAblationKind::StaticHazardComponent {
                component_index,
                tile_count,
            } => Self::StaticHazardComponent {
                component_index,
                tile_count,
            },
            RoomAblationKind::TimedHazard { hazard_index } => Self::TimedHazard { hazard_index },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricAggregatesRecord {
    canonical_routes: CanonicalRouteAggregateRecord,
    landing_precision: LandingPrecisionMetricRecord,
    direct_controllers: DirectControllerAggregateRecord,
    directional_asymmetry: Vec<DirectionalAsymmetryRecord>,
    terrain: TerrainMetricAggregateRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandingPrecisionMetricRecord {
    source_landing_precision_versions: Vec<u32>,
    aggregate: LandingPrecisionAggregateRecord,
    by_loadout: Vec<LandingPrecisionLoadoutRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandingPrecisionLoadoutRecord {
    loadout: EvaluationLoadout,
    aggregate: LandingPrecisionAggregateRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandingPrecisionAggregateRecord {
    canonical_positive_route_count: usize,
    inspected_ticks: usize,
    routes_with_landing_events: usize,
    routes_without_landing_events: usize,
    routes_with_measured_landings: usize,
    routes_with_unmeasured_landings: usize,
    routes_with_only_unmeasured_landings: usize,
    landing_event_count: usize,
    measured_landing_count: usize,
    unmeasured_landing_count: usize,
    minimum_edge_margin_pixels:
        LandingCoordinateEvidenceRecord<SignedIntegerCoordinateDistributionRecord>,
    footprint_overlap_pixels: LandingCoordinateEvidenceRecord<IntegerCoordinateDistributionRecord>,
    support_width_pixels: LandingCoordinateEvidenceRecord<IntegerCoordinateDistributionRecord>,
    edge_overhang_landings: usize,
    one_way_or_mixed_landings: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
enum LandingCoordinateEvidenceRecord<T> {
    Observed {
        value: T,
    },
    NotApplicable {
        reason: LandingCoordinateNotApplicableReasonRecord,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
enum LandingCoordinateNotApplicableReasonRecord {
    NoCanonicalPositiveRoutes,
    NoLandingEvents,
    NoMeasuredLandings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedIntegerCoordinateDistributionRecord {
    sample_count: usize,
    minimum: i32,
    median_lower: i32,
    median_upper: i32,
    maximum: i32,
    spread: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRouteAggregateRecord {
    directed_route_count: usize,
    loadout_route_cell_count: usize,
    positive_route_count: usize,
    bounded_inconclusive_route_count: usize,
    by_loadout: Vec<CanonicalLoadoutAggregateRecord>,
    behavior_diversity: RouteDiversityRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalLoadoutAggregateRecord {
    loadout: EvaluationLoadout,
    route_cell_count: usize,
    positive_route_count: usize,
    bounded_inconclusive_route_count: usize,
    behavior_diversity: RouteDiversityRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteDiversityRecord {
    version: u32,
    route_count: usize,
    reached_target_count: usize,
    spatial_path_classes: usize,
    semantic_controller_classes: usize,
    joint_play_style_classes: usize,
    traversal_distance: PairwiseDistanceRecord,
    semantic_action_distance: PairwiseDistanceRecord,
    combined_behavior_distance: PairwiseDistanceRecord,
}

impl From<&RouteDiversityReport> for RouteDiversityRecord {
    fn from(value: &RouteDiversityReport) -> Self {
        Self {
            version: value.version,
            route_count: value.route_count,
            reached_target_count: value.reached_target_count,
            spatial_path_classes: value.spatial_path_classes,
            semantic_controller_classes: value.semantic_controller_classes,
            joint_play_style_classes: value.joint_play_style_classes,
            traversal_distance: value.traversal_distance.into(),
            semantic_action_distance: value.semantic_action_distance.into(),
            combined_behavior_distance: value.combined_behavior_distance.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairwiseDistanceRecord {
    comparisons: usize,
    #[serde(with = "canonical_option_f64")]
    minimum: Option<f64>,
    #[serde(with = "canonical_option_f64")]
    mean: Option<f64>,
    #[serde(with = "canonical_option_f64")]
    median: Option<f64>,
    #[serde(with = "canonical_option_f64")]
    maximum: Option<f64>,
    #[serde(with = "canonical_option_f64")]
    mean_nearest_neighbor: Option<f64>,
}

impl From<downwards_lab::PairwiseDistanceSummary> for PairwiseDistanceRecord {
    fn from(value: downwards_lab::PairwiseDistanceSummary) -> Self {
        Self {
            comparisons: value.comparisons,
            minimum: value.minimum,
            mean: value.mean,
            median: value.median,
            maximum: value.maximum,
            mean_nearest_neighbor: value.mean_nearest_neighbor,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectControllerAggregateRecord {
    directed_route_count: usize,
    by_loadout: Vec<DirectControllerLoadoutAggregateRecord>,
    ability_bypasses: AbilityBypassAggregateRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectControllerLoadoutAggregateRecord {
    loadout: EvaluationLoadout,
    source_audit_completeness: AuditCompletenessCountsRecord,
    route_audit_completeness: AuditCompletenessCountsRecord,
    known_positive_directed_routes: usize,
    ambiguous_nondominated_front_directed_routes: usize,
    no_positive_in_complete_finite_vocabulary: usize,
    inconclusive_without_positive: usize,
    missing_route_or_audit: usize,
    easiest_controller_fractions: MetricEvidenceRecord<EasiestControllerFractionRecord>,
    demand_coordinates: MetricEvidenceRecord<ControllerDemandCoordinateSummaryRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditCompletenessCountsRecord {
    expected: usize,
    complete_finite_vocabulary: usize,
    bounded_incomplete: usize,
    missing: usize,
    state: AggregateAuditCompletenessRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AggregateAuditCompletenessRecord {
    CompleteFiniteVocabulary,
    BoundedIncomplete,
    Missing,
    NotApplicableNoAuditsExpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EasiestControllerFractionRecord {
    successful_route_count: usize,
    run_only_class_fraction: ExactFractionRecord,
    monotone_simple_class_fraction: ExactFractionRecord,
    other_controller_class_fraction: ExactFractionRecord,
    run_only_fraction: ExactFractionRecord,
    monotone_simple_fraction: ExactFractionRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactFractionRecord {
    numerator: usize,
    denominator: usize,
}

impl From<super::ExactFraction> for ExactFractionRecord {
    fn from(value: super::ExactFraction) -> Self {
        Self {
            numerator: value.numerator,
            denominator: value.denominator,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerDemandCoordinateSummaryRecord {
    controller_class: IntegerCoordinateDistributionRecord,
    ability_events: IntegerCoordinateDistributionRecord,
    horizontal_reversals: IntegerCoordinateDistributionRecord,
    vertical_decisions: IntegerCoordinateDistributionRecord,
    semantic_spans: IntegerCoordinateDistributionRecord,
    semantic_transitions: IntegerCoordinateDistributionRecord,
    duration_ticks: IntegerCoordinateDistributionRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegerCoordinateDistributionRecord {
    sample_count: usize,
    minimum: usize,
    median_lower: usize,
    median_upper: usize,
    maximum: usize,
    spread: usize,
}

impl From<super::IntegerCoordinateDistribution> for IntegerCoordinateDistributionRecord {
    fn from(value: super::IntegerCoordinateDistribution) -> Self {
        Self {
            sample_count: value.sample_count,
            minimum: value.minimum,
            median_lower: value.median_lower,
            median_upper: value.median_upper,
            maximum: value.maximum,
            spread: value.spread,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityBypassAggregateRecord {
    directed_routes_with_any_bypass: usize,
    directed_routes_with_wall_jump_bypass: usize,
    directed_routes_with_dash_bypass: usize,
    directed_route_loadout_bypasses: usize,
    retained_semantic_bypass_witnesses: usize,
    by_successful_loadout: Vec<AbilityBypassLoadoutRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityBypassLoadoutRecord {
    successful_loadout: EvaluationLoadout,
    directed_route_bypasses: usize,
    retained_semantic_witnesses: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionalAsymmetryRecord {
    door_a: String,
    door_b: String,
    loadout: EvaluationLoadout,
    a_to_b: MetricEvidenceRecord<EasiestKnownControllerRecord>,
    b_to_a: MetricEvidenceRecord<EasiestKnownControllerRecord>,
    comparison: MetricEvidenceRecord<DirectionalAsymmetryValuesRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EasiestKnownControllerRecord {
    successful_loadout: EvaluationLoadout,
    demand: ControllerDemandRecord,
    coordinates: ControllerDemandCoordinatesRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionalAsymmetryValuesRecord {
    duration_ticks: DirectionalCountDifferenceRecord,
    semantic_spans: DirectionalCountDifferenceRecord,
    semantic_transitions: DirectionalCountDifferenceRecord,
    ability_events: DirectionalCountDifferenceRecord,
    ability_use: DirectionalAbilityUseComparisonRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionalCountDifferenceRecord {
    a_to_b: usize,
    b_to_a: usize,
    absolute_difference: usize,
    larger_direction: LargerDirectionRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LargerDirectionRecord {
    Equal,
    AToB,
    BToA,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionalAbilityUseComparisonRecord {
    a_to_b: AbilityUseRecord,
    b_to_a: AbilityUseRecord,
    wall_jump_use_differs: bool,
    dash_use_differs: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityUseRecord {
    wall_jump_events: usize,
    dash_events: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainMetricAggregateRecord {
    source_terrain_audit_version: u32,
    coverage: PositiveTerrainCoverageRecord,
    coverage_fractions: TerrainCoverageFractionRecord,
    ablations: Vec<AblationSurvivalRecord>,
    ablation_utility: AblationUtilityRecord,
    aggregate_ablation_survival_fraction: MetricEvidenceRecord<ExactFractionRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AblationUtilityRecord {
    variant_count: usize,
    all_stored_controllers_survived: usize,
    at_least_one_stored_controller_affected: usize,
    no_stored_controllers: usize,
    removed_tiles_all_survived: usize,
    removed_tiles_some_affected: usize,
    removed_tiles_no_controllers: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainCoverageFractionRecord {
    structurally_attributed_components: MetricEvidenceRecord<ExactFractionRecord>,
    structurally_attributed_tiles: MetricEvidenceRecord<ExactFractionRecord>,
    traversal_near_components: MetricEvidenceRecord<ExactFractionRecord>,
    traversal_near_tiles: MetricEvidenceRecord<ExactFractionRecord>,
    positively_corroborated_components: MetricEvidenceRecord<ExactFractionRecord>,
    positively_corroborated_tiles: MetricEvidenceRecord<ExactFractionRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AblationSurvivalRecord {
    kind: RoomAblationKindRecord,
    outcomes: AblationOutcomeRecord,
    survival_fraction: MetricEvidenceRecord<ExactFractionRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
enum MetricEvidenceRecord<T> {
    Observed {
        value: T,
    },
    Missing {
        reason: MissingMetricReasonRecord,
    },
    NotApplicable {
        reason: NotApplicableMetricReasonRecord,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MissingMetricReasonRecord {
    MissingDirectedRouteAssessment,
    MissingDirectControllerAudit,
    BoundedDirectControllerAuditWithoutPositive,
    NoPositiveTerrainController,
    ReverseDirectionsRequireTwoKnownPositiveControllers,
}

impl From<MissingMetricReason> for MissingMetricReasonRecord {
    fn from(value: MissingMetricReason) -> Self {
        match value {
            MissingMetricReason::MissingDirectedRouteAssessment => {
                Self::MissingDirectedRouteAssessment
            }
            MissingMetricReason::MissingDirectControllerAudit => Self::MissingDirectControllerAudit,
            MissingMetricReason::BoundedDirectControllerAuditWithoutPositive => {
                Self::BoundedDirectControllerAuditWithoutPositive
            }
            MissingMetricReason::NoPositiveTerrainController => Self::NoPositiveTerrainController,
            MissingMetricReason::ReverseDirectionsRequireTwoKnownPositiveControllers => {
                Self::ReverseDirectionsRequireTwoKnownPositiveControllers
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum NotApplicableMetricReasonRecord {
    #[serde(rename = "no_directed_routes")]
    DirectedRoutes,
    #[serde(rename = "no_known_positive_controller_in_complete_finite_vocabulary")]
    KnownPositiveControllerInCompleteFiniteVocabulary,
    #[serde(rename = "no_successful_routes_for_loadout")]
    SuccessfulRoutesForLoadout,
    #[serde(rename = "ambiguous_nondominated_front")]
    AmbiguousNondominatedFront,
    #[serde(rename = "no_interior_terrain_components")]
    InteriorTerrainComponents,
    #[serde(rename = "no_interior_terrain_tiles")]
    InteriorTerrainTiles,
    #[serde(rename = "no_ablation_variants")]
    AblationVariants,
    #[serde(rename = "no_exact_controllers_for_ablation")]
    ExactControllersForAblation,
}

impl From<NotApplicableMetricReason> for NotApplicableMetricReasonRecord {
    fn from(value: NotApplicableMetricReason) -> Self {
        match value {
            NotApplicableMetricReason::NoDirectedRoutes => Self::DirectedRoutes,
            NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary => {
                Self::KnownPositiveControllerInCompleteFiniteVocabulary
            }
            NotApplicableMetricReason::NoSuccessfulRoutesForLoadout => {
                Self::SuccessfulRoutesForLoadout
            }
            NotApplicableMetricReason::AmbiguousNondominatedFront => {
                Self::AmbiguousNondominatedFront
            }
            NotApplicableMetricReason::NoInteriorTerrainComponents => {
                Self::InteriorTerrainComponents
            }
            NotApplicableMetricReason::NoInteriorTerrainTiles => Self::InteriorTerrainTiles,
            NotApplicableMetricReason::NoAblationVariants => Self::AblationVariants,
            NotApplicableMetricReason::NoExactControllersForAblation => {
                Self::ExactControllersForAblation
            }
        }
    }
}

fn metric_evidence<T, U>(
    evidence: &MetricEvidence<T>,
    observed: impl FnOnce(&T) -> U,
) -> MetricEvidenceRecord<U> {
    match evidence {
        MetricEvidence::Observed(value) => MetricEvidenceRecord::Observed {
            value: observed(value),
        },
        MetricEvidence::Missing { reason } => MetricEvidenceRecord::Missing {
            reason: (*reason).into(),
        },
        MetricEvidence::NotApplicable { reason } => MetricEvidenceRecord::NotApplicable {
            reason: (*reason).into(),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationalCostSummaryRecord {
    interpretation: String,
    direct_controller_reported_total: OperationalCostRecord,
    direct_controller_recomputed_total: OperationalCostRecord,
    direct_controller_audit_units: Vec<DirectControllerCostUnitRecord>,
    canonical_positive_route_total: OperationalCostRecord,
    canonical_positive_routes: Vec<CanonicalRouteCostUnitRecord>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationalCostRecord {
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

impl From<super::OperationalCost> for OperationalCostRecord {
    fn from(value: super::OperationalCost) -> Self {
        Self {
            expanded_nodes: value.expanded_nodes,
            generated_nodes: value.generated_nodes,
            simulated_ticks: value.simulated_ticks,
            deepest_path_ticks: value.deepest_path_ticks,
        }
    }
}

impl From<SearchStats> for OperationalCostRecord {
    fn from(value: SearchStats) -> Self {
        Self {
            expanded_nodes: value.expanded_nodes,
            generated_nodes: value.generated_nodes,
            simulated_ticks: value.simulated_ticks,
            deepest_path_ticks: value.deepest_path_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectControllerCostUnitRecord {
    cost_id: String,
    source_door_id: String,
    loadout: EvaluationLoadout,
    status: AuditStatusRecord,
    cost: OperationalCostRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRouteCostUnitRecord {
    cost_id: String,
    source_door_id: String,
    target_door_id: String,
    loadout: EvaluationLoadout,
    cost: OperationalCostRecord,
}

fn room_record(
    seed: u64,
    input: CorpusAnalysisArtifactRoomRef<'_>,
    witnesses: &mut BTreeMap<String, ControllerWitnessRecord>,
) -> Result<RoomRecord, CorpusAnalysisArtifactError> {
    let CorpusAnalysisArtifactRoomRef {
        generation_key,
        room_id,
        analysis,
        metrics,
    } = input;
    if generation_key.seed != seed {
        return Err(invalid(format!(
            "room {:?} generation key seed {} differs from artifact seed {seed}",
            room_id.0, generation_key.seed
        )));
    }
    if &analysis.room_id != room_id || &metrics.room_id != room_id {
        return Err(invalid(format!(
            "room identity mismatch: tuple={:?}, analysis={:?}, metrics={:?}",
            room_id.0, analysis.room_id.0, metrics.room_id.0
        )));
    }
    if analysis.version != CORPUS_ROOM_ANALYSIS_VERSION
        || metrics.version != ROOM_METRIC_SUMMARY_VERSION
        || metrics.source_analysis_version != analysis.version
    {
        return Err(invalid(format!(
            "room {:?} has incompatible analysis/metric versions {}/{}/{}",
            room_id.0, analysis.version, metrics.version, metrics.source_analysis_version
        )));
    }

    let mut door_ids = analysis
        .source_route_assessments
        .iter()
        .map(|batch| batch.source_door_id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    require_strict_order(
        "analysis source door IDs",
        door_ids.iter().map(String::as_str),
    )?;

    let mut direct_cost_by_key = BTreeMap::new();
    let mut direct_cost_units = metrics
        .operational_cost
        .direct_controller_audit_units
        .iter()
        .map(|unit| {
            let cost_id = direct_cost_id(&unit.source_door_id, unit.loadout);
            let key = (unit.source_door_id.clone(), unit.loadout);
            if direct_cost_by_key.insert(key, cost_id.clone()).is_some() {
                return Err(invalid(format!(
                    "room {:?} repeats direct operational cost unit {} {}",
                    room_id.0,
                    unit.source_door_id,
                    unit.loadout.slug()
                )));
            }
            Ok(DirectControllerCostUnitRecord {
                cost_id,
                source_door_id: unit.source_door_id.clone(),
                loadout: unit.loadout,
                status: unit.status.into(),
                cost: unit.cost.into(),
            })
        })
        .collect::<Result<Vec<_>, CorpusAnalysisArtifactError>>()?;
    direct_cost_units.sort_unstable_by(|left, right| {
        (&left.source_door_id, left.loadout).cmp(&(&right.source_door_id, right.loadout))
    });

    let source_audits = analysis
        .source_route_assessments
        .iter()
        .map(|batch| {
            if batch.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY {
                return Err(invalid(format!(
                    "source {:?} uses a non-current controller policy",
                    batch.source_door_id
                )));
            }
            let shared_loadout_audits = batch
                .shared_audits
                .iter()
                .map(|audit| {
                    let key = (batch.source_door_id.clone(), audit.loadout);
                    let Some(cost_ref) = direct_cost_by_key.get(&key) else {
                        return Err(invalid(format!(
                            "source {:?} {} audit has no operational-cost unit",
                            batch.source_door_id,
                            audit.loadout.slug()
                        )));
                    };
                    let expected_cost: OperationalCostRecord = audit.operational_stats.into();
                    let recorded_cost = direct_cost_units
                        .iter()
                        .find(|unit| unit.cost_id == *cost_ref)
                        .expect("cost map and unit vector were built together");
                    if recorded_cost.cost != expected_cost
                        || recorded_cost.status != AuditStatusRecord::from(audit.status)
                    {
                        return Err(invalid(format!(
                            "source {:?} {} audit disagrees with segregated operational cost",
                            batch.source_door_id,
                            audit.loadout.slug()
                        )));
                    }
                    Ok(SharedLoadoutAuditRecord {
                        loadout: audit.loadout,
                        status: audit.status.into(),
                        target_count: audit.target_count,
                        raw_positive_witnesses: audit.raw_positive_witnesses,
                        retained_semantic_witnesses: audit.retained_semantic_witnesses,
                        operational_cost_ref: cost_ref.clone(),
                    })
                })
                .collect::<Result<Vec<_>, CorpusAnalysisArtifactError>>()?;
            Ok(SourceAuditRecord {
                source_door_id: batch.source_door_id.clone(),
                target_door_ids: batch.target_door_ids.clone(),
                authoritative_loadout: batch.authoritative_loadout,
                expected_subset_loadouts: batch.expected_subset_loadouts.clone(),
                shared_loadout_audits,
            })
        })
        .collect::<Result<Vec<_>, CorpusAnalysisArtifactError>>()?;

    let mut directed_routes = Vec::new();
    for batch in &analysis.source_route_assessments {
        for route in &batch.routes {
            if route.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY {
                return Err(invalid(format!(
                    "route {:?}->{:?} uses a non-current controller policy",
                    route.source_door_id, route.target_door_id
                )));
            }
            let mut witness_ids_by_index = HashMap::new();
            let retain_witness = |index: usize,
                                  witness_ids_by_index: &mut HashMap<usize, String>,
                                  witnesses: &mut BTreeMap<String, ControllerWitnessRecord>|
             -> Result<String, CorpusAnalysisArtifactError> {
                if let Some(id) = witness_ids_by_index.get(&index) {
                    return Ok(id.clone());
                }
                let Some(witness) = route.easiest_first_witnesses.get(index) else {
                    return Err(invalid(format!(
                        "route {:?}->{:?} references absent witness index {index}",
                        route.source_door_id, route.target_door_id
                    )));
                };
                let record = controller_witness_record(
                    room_id,
                    &route.source_door_id,
                    &route.target_door_id,
                    witness,
                )?;
                let id = record.witness_id.clone();
                if let Some(previous) = witnesses.insert(id.clone(), record.clone())
                    && previous != record
                {
                    return Err(invalid(format!(
                        "controller witness identity collision for {id:?}"
                    )));
                }
                witness_ids_by_index.insert(index, id.clone());
                Ok(id)
            };

            let overall_easiest_known_witness_id = if route.easiest_first_witnesses.is_empty() {
                None
            } else {
                Some(retain_witness(0, &mut witness_ids_by_index, witnesses)?)
            };
            let overall_pareto_front_witness_ids = route
                .easiest_known_front
                .iter()
                .map(|&index| retain_witness(index, &mut witness_ids_by_index, witnesses))
                .collect::<Result<Vec<_>, _>>()?;

            let mut loadouts = Vec::new();
            for audit in &route.audits {
                let indices = route
                    .easiest_first_witnesses
                    .iter()
                    .enumerate()
                    .filter_map(|(index, witness)| {
                        (witness.loadout == audit.loadout).then_some(index)
                    })
                    .collect::<Vec<_>>();
                if indices.len() != audit.retained_semantic_witnesses {
                    return Err(invalid(format!(
                        "route {:?}->{:?} {} retained-witness count differs: audit={}, witnesses={}",
                        route.source_door_id,
                        route.target_door_id,
                        audit.loadout.slug(),
                        audit.retained_semantic_witnesses,
                        indices.len()
                    )));
                }
                let easiest_known_witness_id = indices
                    .first()
                    .map(|&index| retain_witness(index, &mut witness_ids_by_index, witnesses))
                    .transpose()?;
                let pareto_front_witness_ids =
                    pareto_indices(&route.easiest_first_witnesses, &indices)
                        .into_iter()
                        .map(|index| retain_witness(index, &mut witness_ids_by_index, witnesses))
                        .collect::<Result<Vec<_>, _>>()?;
                loadouts.push(DirectedControllerLoadoutRecord {
                    loadout: audit.loadout,
                    status: audit.status.into(),
                    raw_positive_witnesses: audit.raw_positive_witnesses,
                    retained_semantic_witnesses: audit.retained_semantic_witnesses,
                    easiest_known_witness_id,
                    pareto_front_witness_ids,
                });
            }
            let completeness = match &route.completeness {
                super::RouteControllerAuditCompleteness::CompleteFiniteVocabulary => {
                    RouteCompletenessRecord::CompleteFiniteVocabulary
                }
                super::RouteControllerAuditCompleteness::BoundedIncomplete {
                    incomplete_loadouts,
                } => RouteCompletenessRecord::BoundedIncomplete {
                    incomplete_loadouts: incomplete_loadouts
                        .iter()
                        .map(|entry| BoundedLoadoutRecord {
                            loadout: entry.loadout,
                            limit: entry.limit.into(),
                        })
                        .collect(),
                },
            };
            directed_routes.push(DirectedControllerRouteRecord {
                source_door_id: route.source_door_id.clone(),
                target_door_id: route.target_door_id.clone(),
                authoritative_loadout: route.authoritative_loadout,
                expected_subset_loadouts: route.expected_subset_loadouts.clone(),
                completeness,
                overall_easiest_known_witness_id,
                overall_pareto_front_witness_ids,
                loadouts,
                positive_bypasses: route
                    .positive_bypasses
                    .iter()
                    .map(|bypass| PositiveBypassRecord {
                        loadout: bypass.loadout,
                        retained_semantic_witnesses: bypass.witness_indices.len(),
                    })
                    .collect(),
            });
        }
    }
    directed_routes.sort_unstable_by(|left, right| {
        (&left.source_door_id, &left.target_door_id)
            .cmp(&(&right.source_door_id, &right.target_door_id))
    });

    let mut canonical_cost_by_key = BTreeMap::new();
    let mut canonical_cost_units = metrics
        .operational_cost
        .canonical_positive_routes
        .iter()
        .map(|unit| {
            let key = (
                unit.source_door_id.clone(),
                unit.target_door_id.clone(),
                unit.loadout,
            );
            let cost_id = canonical_cost_id(&key.0, &key.1, key.2);
            if canonical_cost_by_key.insert(key, cost_id.clone()).is_some() {
                return Err(invalid("duplicate canonical route operational-cost unit"));
            }
            Ok(CanonicalRouteCostUnitRecord {
                cost_id,
                source_door_id: unit.source_door_id.clone(),
                target_door_id: unit.target_door_id.clone(),
                loadout: unit.loadout,
                cost: unit.cost.into(),
            })
        })
        .collect::<Result<Vec<_>, CorpusAnalysisArtifactError>>()?;
    canonical_cost_units.sort_unstable_by(|left, right| {
        (left.loadout, &left.source_door_id, &left.target_door_id).cmp(&(
            right.loadout,
            &right.source_door_id,
            &right.target_door_id,
        ))
    });
    let mut canonical_routes = analysis
        .canonical_route_measurements
        .iter()
        .map(|measurement| {
            let key = (
                measurement.source_door_id.clone(),
                measurement.target_door_id.clone(),
                measurement.loadout,
            );
            let Some(cost_ref) = canonical_cost_by_key.get(&key) else {
                return Err(invalid(format!(
                    "canonical route {:?}->{:?} {} has no segregated operational cost",
                    key.0,
                    key.1,
                    key.2.slug()
                )));
            };
            Ok(CanonicalRouteVectorRecord {
                measurement_version: measurement.version,
                source_door_id: key.0,
                target_door_id: key.1,
                loadout: key.2,
                witness_fingerprint: measurement.witness_fingerprint.to_string(),
                landing_precision: (&measurement.landing_precision).into(),
                vector: PlayerRouteDifficultyVectorRecord::from_vector(&measurement.vector)?,
                operational_cost_ref: cost_ref.clone(),
            })
        })
        .collect::<Result<Vec<_>, CorpusAnalysisArtifactError>>()?;
    canonical_routes.sort_unstable_by(|left, right| {
        (left.loadout, &left.source_door_id, &left.target_door_id).cmp(&(
            right.loadout,
            &right.source_door_id,
            &right.target_door_id,
        ))
    });

    let terrain_positive_evidence = TerrainPositiveEvidenceRecord {
        audit_version: analysis.terrain_audit.version,
        structural_descriptor_version: analysis.terrain_audit.structural_descriptor_version,
        room_ablation_version: analysis.terrain_audit.room_ablation_version,
        loadout: analysis.terrain_audit.loadout.into(),
        positive_door_controller_count: analysis.terrain_audit.positive_door_controller_count,
        positive_pickup_controller_count: analysis.terrain_audit.positive_pickup_controller_count,
        coverage: analysis.terrain_audit.coverage.into(),
        component_ablation_outcomes: analysis
            .terrain_audit
            .ablations
            .iter()
            .map(|ablation| AblationOutcomeRecord {
                kind: ablation.kind.into(),
                controller_count: ablation.summary.controller_count,
                succeeded: ablation.summary.succeeded,
                died: ablation.summary.died,
                wrong_target: ablation.summary.wrong_target,
                diverged: ablation.summary.diverged,
            })
            .collect(),
    };

    Ok(RoomRecord {
        row_version: CORPUS_ANALYSIS_ARTIFACT_VERSION,
        room_id: room_id.0.clone(),
        generation_key: generation_key.clone(),
        analysis_version: analysis.version,
        metric_summary_version: metrics.version,
        door_ids,
        direct_controller_evidence: DirectControllerEvidenceRecord {
            source_audits,
            directed_routes,
        },
        canonical_route_difficulty: CanonicalRouteDifficultyRecord {
            claim: CANONICAL_ROUTE_CLAIM.to_owned(),
            routes: canonical_routes,
        },
        terrain_positive_evidence,
        metric_aggregates: metric_aggregates(metrics),
        operational_cost: OperationalCostSummaryRecord {
            interpretation: "solver_capacity_evidence_not_player_difficulty".to_owned(),
            direct_controller_reported_total: metrics
                .operational_cost
                .direct_controller_reported_total
                .into(),
            direct_controller_recomputed_total: metrics
                .operational_cost
                .direct_controller_recomputed_total
                .into(),
            direct_controller_audit_units: direct_cost_units,
            canonical_positive_route_total: metrics
                .operational_cost
                .canonical_positive_route_total
                .into(),
            canonical_positive_routes: canonical_cost_units,
        },
    })
}

fn metric_aggregates(metrics: &RoomMetricSummary) -> MetricAggregatesRecord {
    let canonical_routes = &metrics.canonical_routes;
    let direct = &metrics.direct_controllers;
    MetricAggregatesRecord {
        canonical_routes: CanonicalRouteAggregateRecord {
            directed_route_count: canonical_routes.directed_route_count,
            loadout_route_cell_count: canonical_routes.loadout_route_cell_count,
            positive_route_count: canonical_routes.positive_route_count,
            bounded_inconclusive_route_count: canonical_routes.bounded_inconclusive_route_count,
            by_loadout: canonical_routes
                .by_loadout
                .iter()
                .map(|loadout| CanonicalLoadoutAggregateRecord {
                    loadout: loadout.loadout,
                    route_cell_count: loadout.route_cell_count,
                    positive_route_count: loadout.positive_route_count,
                    bounded_inconclusive_route_count: loadout.bounded_inconclusive_route_count,
                    behavior_diversity: (&loadout.behavior_diversity).into(),
                })
                .collect(),
            behavior_diversity: (&canonical_routes.behavior_diversity).into(),
        },
        landing_precision: landing_precision_metric_record(&metrics.landing_precision),
        direct_controllers: DirectControllerAggregateRecord {
            directed_route_count: direct.directed_route_count,
            by_loadout: direct
                .by_loadout
                .iter()
                .map(|loadout| DirectControllerLoadoutAggregateRecord {
                    loadout: loadout.loadout,
                    source_audit_completeness: audit_completeness_counts(
                        loadout.source_audit_completeness,
                    ),
                    route_audit_completeness: audit_completeness_counts(
                        loadout.route_audit_completeness,
                    ),
                    known_positive_directed_routes: loadout.known_positive_directed_routes,
                    ambiguous_nondominated_front_directed_routes: loadout
                        .ambiguous_nondominated_front_directed_routes,
                    no_positive_in_complete_finite_vocabulary: loadout
                        .no_positive_in_complete_finite_vocabulary,
                    inconclusive_without_positive: loadout.inconclusive_without_positive,
                    missing_route_or_audit: loadout.missing_route_or_audit,
                    easiest_controller_fractions: metric_evidence(
                        &loadout.easiest_controller_fractions,
                        |fractions| EasiestControllerFractionRecord {
                            successful_route_count: fractions.successful_route_count,
                            run_only_class_fraction: fractions.run_only_class_fraction.into(),
                            monotone_simple_class_fraction: fractions
                                .monotone_simple_class_fraction
                                .into(),
                            other_controller_class_fraction: fractions
                                .other_controller_class_fraction
                                .into(),
                            run_only_fraction: fractions.run_only_fraction.into(),
                            monotone_simple_fraction: fractions.monotone_simple_fraction.into(),
                        },
                    ),
                    demand_coordinates: metric_evidence(
                        &loadout.demand_coordinates,
                        controller_coordinate_summary,
                    ),
                })
                .collect(),
            ability_bypasses: AbilityBypassAggregateRecord {
                directed_routes_with_any_bypass: direct
                    .ability_bypasses
                    .directed_routes_with_any_bypass,
                directed_routes_with_wall_jump_bypass: direct
                    .ability_bypasses
                    .directed_routes_with_wall_jump_bypass,
                directed_routes_with_dash_bypass: direct
                    .ability_bypasses
                    .directed_routes_with_dash_bypass,
                directed_route_loadout_bypasses: direct
                    .ability_bypasses
                    .directed_route_loadout_bypasses,
                retained_semantic_bypass_witnesses: direct
                    .ability_bypasses
                    .retained_semantic_bypass_witnesses,
                by_successful_loadout: direct
                    .ability_bypasses
                    .by_successful_loadout
                    .iter()
                    .map(|loadout| AbilityBypassLoadoutRecord {
                        successful_loadout: loadout.successful_loadout,
                        directed_route_bypasses: loadout.directed_route_bypasses,
                        retained_semantic_witnesses: loadout.retained_semantic_witnesses,
                    })
                    .collect(),
            },
        },
        directional_asymmetry: metrics
            .directional_asymmetry
            .iter()
            .map(|metric| DirectionalAsymmetryRecord {
                door_a: metric.door_a.clone(),
                door_b: metric.door_b.clone(),
                loadout: metric.loadout,
                a_to_b: metric_evidence(&metric.a_to_b, easiest_known_controller),
                b_to_a: metric_evidence(&metric.b_to_a, easiest_known_controller),
                comparison: metric_evidence(&metric.comparison, asymmetry_values),
            })
            .collect(),
        terrain: TerrainMetricAggregateRecord {
            source_terrain_audit_version: metrics.terrain.source_terrain_audit_version,
            coverage: metrics.terrain.coverage.into(),
            coverage_fractions: TerrainCoverageFractionRecord {
                structurally_attributed_components: metric_evidence(
                    &metrics
                        .terrain
                        .coverage_fractions
                        .structurally_attributed_components,
                    |fraction| (*fraction).into(),
                ),
                structurally_attributed_tiles: metric_evidence(
                    &metrics
                        .terrain
                        .coverage_fractions
                        .structurally_attributed_tiles,
                    |fraction| (*fraction).into(),
                ),
                traversal_near_components: metric_evidence(
                    &metrics.terrain.coverage_fractions.traversal_near_components,
                    |fraction| (*fraction).into(),
                ),
                traversal_near_tiles: metric_evidence(
                    &metrics.terrain.coverage_fractions.traversal_near_tiles,
                    |fraction| (*fraction).into(),
                ),
                positively_corroborated_components: metric_evidence(
                    &metrics
                        .terrain
                        .coverage_fractions
                        .positively_corroborated_components,
                    |fraction| (*fraction).into(),
                ),
                positively_corroborated_tiles: metric_evidence(
                    &metrics
                        .terrain
                        .coverage_fractions
                        .positively_corroborated_tiles,
                    |fraction| (*fraction).into(),
                ),
            },
            ablations: metrics
                .terrain
                .ablations
                .iter()
                .map(|ablation| {
                    let outcomes = AblationOutcomeRecord {
                        kind: ablation.kind.into(),
                        controller_count: ablation.outcomes.controller_count,
                        succeeded: ablation.outcomes.succeeded,
                        died: ablation.outcomes.died,
                        wrong_target: ablation.outcomes.wrong_target,
                        diverged: ablation.outcomes.diverged,
                    };
                    AblationSurvivalRecord {
                        kind: ablation.kind.into(),
                        outcomes,
                        survival_fraction: metric_evidence(
                            &ablation.survival_fraction,
                            |fraction| (*fraction).into(),
                        ),
                    }
                })
                .collect(),
            ablation_utility: AblationUtilityRecord {
                variant_count: metrics.terrain.ablation_utility.variant_count,
                all_stored_controllers_survived: metrics
                    .terrain
                    .ablation_utility
                    .all_stored_controllers_survived,
                at_least_one_stored_controller_affected: metrics
                    .terrain
                    .ablation_utility
                    .at_least_one_stored_controller_affected,
                no_stored_controllers: metrics.terrain.ablation_utility.no_stored_controllers,
                removed_tiles_all_survived: metrics
                    .terrain
                    .ablation_utility
                    .removed_tiles_all_survived,
                removed_tiles_some_affected: metrics
                    .terrain
                    .ablation_utility
                    .removed_tiles_some_affected,
                removed_tiles_no_controllers: metrics
                    .terrain
                    .ablation_utility
                    .removed_tiles_no_controllers,
            },
            aggregate_ablation_survival_fraction: metric_evidence(
                &metrics.terrain.aggregate_ablation_survival_fraction,
                |fraction| (*fraction).into(),
            ),
        },
    }
}

fn landing_precision_metric_record(
    metric: &super::LandingPrecisionMetricSummary,
) -> LandingPrecisionMetricRecord {
    LandingPrecisionMetricRecord {
        source_landing_precision_versions: metric.source_landing_precision_versions.clone(),
        aggregate: landing_precision_aggregate_record(&metric.aggregate),
        by_loadout: metric
            .by_loadout
            .iter()
            .map(|loadout| LandingPrecisionLoadoutRecord {
                loadout: loadout.loadout,
                aggregate: landing_precision_aggregate_record(&loadout.aggregate),
            })
            .collect(),
    }
}

fn landing_precision_aggregate_record(
    aggregate: &super::LandingPrecisionAggregateMetric,
) -> LandingPrecisionAggregateRecord {
    LandingPrecisionAggregateRecord {
        canonical_positive_route_count: aggregate.canonical_positive_route_count,
        inspected_ticks: aggregate.inspected_ticks,
        routes_with_landing_events: aggregate.routes_with_landing_events,
        routes_without_landing_events: aggregate.routes_without_landing_events,
        routes_with_measured_landings: aggregate.routes_with_measured_landings,
        routes_with_unmeasured_landings: aggregate.routes_with_unmeasured_landings,
        routes_with_only_unmeasured_landings: aggregate.routes_with_only_unmeasured_landings,
        landing_event_count: aggregate.landing_event_count,
        measured_landing_count: aggregate.measured_landing_count,
        unmeasured_landing_count: aggregate.unmeasured_landing_count,
        minimum_edge_margin_pixels: landing_coordinate_evidence(
            &aggregate.minimum_edge_margin_pixels,
            |distribution| SignedIntegerCoordinateDistributionRecord {
                sample_count: distribution.sample_count,
                minimum: distribution.minimum,
                median_lower: distribution.median_lower,
                median_upper: distribution.median_upper,
                maximum: distribution.maximum,
                spread: distribution.spread,
            },
        ),
        footprint_overlap_pixels: landing_coordinate_evidence(
            &aggregate.footprint_overlap_pixels,
            |distribution| (*distribution).into(),
        ),
        support_width_pixels: landing_coordinate_evidence(
            &aggregate.support_width_pixels,
            |distribution| (*distribution).into(),
        ),
        edge_overhang_landings: aggregate.edge_overhang_landings,
        one_way_or_mixed_landings: aggregate.one_way_or_mixed_landings,
    }
}

fn landing_coordinate_evidence<T, U>(
    evidence: &super::LandingCoordinateEvidence<T>,
    observed: impl FnOnce(&T) -> U,
) -> LandingCoordinateEvidenceRecord<U> {
    match evidence {
        super::LandingCoordinateEvidence::Observed(value) => {
            LandingCoordinateEvidenceRecord::Observed {
                value: observed(value),
            }
        }
        super::LandingCoordinateEvidence::NotApplicable { reason } => {
            LandingCoordinateEvidenceRecord::NotApplicable {
                reason: match reason {
                    super::LandingCoordinateNotApplicableReason::NoCanonicalPositiveRoutes => {
                        LandingCoordinateNotApplicableReasonRecord::NoCanonicalPositiveRoutes
                    }
                    super::LandingCoordinateNotApplicableReason::NoLandingEvents => {
                        LandingCoordinateNotApplicableReasonRecord::NoLandingEvents
                    }
                    super::LandingCoordinateNotApplicableReason::NoMeasuredLandings => {
                        LandingCoordinateNotApplicableReasonRecord::NoMeasuredLandings
                    }
                },
            }
        }
    }
}

fn audit_completeness_counts(
    counts: super::AuditCompletenessCounts,
) -> AuditCompletenessCountsRecord {
    AuditCompletenessCountsRecord {
        expected: counts.expected,
        complete_finite_vocabulary: counts.complete_finite_vocabulary,
        bounded_incomplete: counts.bounded_incomplete,
        missing: counts.missing,
        state: match counts.state {
            super::AggregateAuditCompleteness::CompleteFiniteVocabulary => {
                AggregateAuditCompletenessRecord::CompleteFiniteVocabulary
            }
            super::AggregateAuditCompleteness::BoundedIncomplete => {
                AggregateAuditCompletenessRecord::BoundedIncomplete
            }
            super::AggregateAuditCompleteness::Missing => AggregateAuditCompletenessRecord::Missing,
            super::AggregateAuditCompleteness::NotApplicableNoAuditsExpected => {
                AggregateAuditCompletenessRecord::NotApplicableNoAuditsExpected
            }
        },
    }
}

fn controller_coordinate_summary(
    summary: &super::ControllerDemandCoordinateSummary,
) -> ControllerDemandCoordinateSummaryRecord {
    ControllerDemandCoordinateSummaryRecord {
        controller_class: summary.controller_class.into(),
        ability_events: summary.ability_events.into(),
        horizontal_reversals: summary.horizontal_reversals.into(),
        vertical_decisions: summary.vertical_decisions.into(),
        semantic_spans: summary.semantic_spans.into(),
        semantic_transitions: summary.semantic_transitions.into(),
        duration_ticks: summary.duration_ticks.into(),
    }
}

fn easiest_known_controller(
    metric: &super::EasiestKnownControllerMetric,
) -> EasiestKnownControllerRecord {
    EasiestKnownControllerRecord {
        successful_loadout: metric.successful_loadout,
        demand: metric.demand.into(),
        coordinates: metric.coordinates.into(),
    }
}

fn asymmetry_values(
    values: &super::DirectionalAsymmetryValues,
) -> DirectionalAsymmetryValuesRecord {
    DirectionalAsymmetryValuesRecord {
        duration_ticks: count_difference(values.duration_ticks),
        semantic_spans: count_difference(values.semantic_spans),
        semantic_transitions: count_difference(values.semantic_transitions),
        ability_events: count_difference(values.ability_events),
        ability_use: DirectionalAbilityUseComparisonRecord {
            a_to_b: AbilityUseRecord {
                wall_jump_events: values.ability_use.a_to_b.wall_jump_events,
                dash_events: values.ability_use.a_to_b.dash_events,
            },
            b_to_a: AbilityUseRecord {
                wall_jump_events: values.ability_use.b_to_a.wall_jump_events,
                dash_events: values.ability_use.b_to_a.dash_events,
            },
            wall_jump_use_differs: values.ability_use.wall_jump_use_differs,
            dash_use_differs: values.ability_use.dash_use_differs,
        },
    }
}

fn count_difference(
    difference: super::DirectionalCountDifference,
) -> DirectionalCountDifferenceRecord {
    DirectionalCountDifferenceRecord {
        a_to_b: difference.a_to_b,
        b_to_a: difference.b_to_a,
        absolute_difference: difference.absolute_difference,
        larger_direction: match difference.larger_direction {
            super::LargerDirection::Equal => LargerDirectionRecord::Equal,
            super::LargerDirection::AToB => LargerDirectionRecord::AToB,
            super::LargerDirection::BToA => LargerDirectionRecord::BToA,
        },
    }
}

fn controller_witness_record(
    room_id: &RoomId,
    source_door_id: &str,
    target_door_id: &str,
    witness: &super::RouteControllerWitness,
) -> Result<ControllerWitnessRecord, CorpusAnalysisArtifactError> {
    let actions = encode_actions(witness.replay.actions())?;
    let total_ticks = checked_action_ticks(&actions)?;
    if total_ticks != witness.replay.frames.len() || total_ticks != witness.demand.duration_ticks {
        return Err(invalid(format!(
            "controller {:?}->{:?} {} has replay/demand/action lengths {}/{}/{}",
            source_door_id,
            target_door_id,
            witness.loadout.slug(),
            witness.replay.frames.len(),
            witness.demand.duration_ticks,
            total_ticks
        )));
    }
    let terminal_state_digest = witness
        .replay
        .frames
        .last()
        .map_or(witness.replay.initial_digest.to_string(), |frame| {
            frame.expected_digest.to_string()
        });
    let terminal_event_digest = witness
        .replay
        .frames
        .last()
        .map(|frame| frame.expected_event_digest.to_string());
    let mut record = ControllerWitnessRecord {
        witness_id: String::new(),
        room_id: room_id.0.clone(),
        source_door_id: source_door_id.to_owned(),
        target_door_id: target_door_id.to_owned(),
        loadout: witness.loadout,
        initial_state_digest: witness.replay.initial_digest.to_string(),
        terminal_state_digest,
        terminal_event_digest,
        total_ticks,
        demand: witness.demand.into(),
        coordinates: witness.demand.coordinates().into(),
        actions,
    };
    record.witness_id = controller_witness_id(&record)?;
    Ok(record)
}

fn pareto_indices(
    witnesses: &[super::RouteControllerWitness],
    candidate_indices: &[usize],
) -> Vec<usize> {
    candidate_indices
        .iter()
        .copied()
        .filter(|&candidate| {
            !candidate_indices.iter().copied().any(|other| {
                other != candidate
                    && coordinates_dominate(
                        witnesses[other].demand.coordinates(),
                        witnesses[candidate].demand.coordinates(),
                    )
            })
        })
        .collect()
}

fn coordinates_dominate(
    left: ControllerDemandCoordinates,
    right: ControllerDemandCoordinates,
) -> bool {
    let no_harder = left.controller_class <= right.controller_class
        && left.ability_events <= right.ability_events
        && left.horizontal_reversals <= right.horizontal_reversals
        && left.vertical_decisions <= right.vertical_decisions
        && left.semantic_spans <= right.semantic_spans
        && left.semantic_transitions <= right.semantic_transitions
        && left.duration_ticks <= right.duration_ticks;
    let strictly_easier = left.controller_class < right.controller_class
        || left.ability_events < right.ability_events
        || left.horizontal_reversals < right.horizontal_reversals
        || left.vertical_decisions < right.vertical_decisions
        || left.semantic_spans < right.semantic_spans
        || left.semantic_transitions < right.semantic_transitions
        || left.duration_ticks < right.duration_ticks;
    no_harder && strictly_easier
}

fn encode_actions(
    actions: impl IntoIterator<Item = Action>,
) -> Result<Vec<ActionSpanRecord>, CorpusAnalysisArtifactError> {
    let mut spans = Vec::<ActionSpanRecord>::new();
    for action in actions {
        if action.restart {
            return Err(invalid(
                "retained controller/config action contains forbidden restart",
            ));
        }
        if !(-1..=1).contains(&action.move_x) || !(-1..=1).contains(&action.move_y) {
            return Err(invalid(format!(
                "action is not normalized: move_x={}, move_y={}",
                action.move_x, action.move_y
            )));
        }
        let span = ActionSpanRecord {
            ticks: 1,
            move_x: action.move_x,
            move_y: action.move_y,
            jump: action.jump,
            dash: action.dash,
        };
        if let Some(previous) = spans.last_mut()
            && previous.same_action(span)
        {
            previous.ticks = previous
                .ticks
                .checked_add(1)
                .ok_or_else(|| invalid("action span tick count overflow"))?;
        } else {
            spans.push(span);
        }
    }
    Ok(spans)
}

fn checked_action_ticks(spans: &[ActionSpanRecord]) -> Result<usize, CorpusAnalysisArtifactError> {
    let mut total = 0_usize;
    let mut previous = None;
    for (index, &span) in spans.iter().enumerate() {
        if span.ticks == 0 {
            return Err(invalid(format!("action span {index} has zero ticks")));
        }
        if !(-1..=1).contains(&span.move_x) || !(-1..=1).contains(&span.move_y) {
            return Err(invalid(format!(
                "action span {index} is not normalized: move_x={}, move_y={}",
                span.move_x, span.move_y
            )));
        }
        if previous.is_some_and(|previous| span.same_action(previous)) {
            return Err(invalid(format!(
                "adjacent action spans {} and {index} must be merged",
                index - 1
            )));
        }
        total = total
            .checked_add(span.ticks)
            .ok_or_else(|| invalid("action tick total overflow"))?;
        previous = Some(span);
    }
    Ok(total)
}

fn controller_witness_id(
    record: &ControllerWitnessRecord,
) -> Result<String, CorpusAnalysisArtifactError> {
    #[derive(Serialize)]
    struct Preimage<'a> {
        version: u32,
        room_id: &'a str,
        source_door_id: &'a str,
        target_door_id: &'a str,
        loadout: EvaluationLoadout,
        initial_state_digest: &'a str,
        terminal_state_digest: &'a str,
        terminal_event_digest: &'a Option<String>,
        total_ticks: usize,
        demand: ControllerDemandRecord,
        coordinates: ControllerDemandCoordinatesRecord,
        actions: &'a [ActionSpanRecord],
    }
    let bytes = serde_json::to_vec(&Preimage {
        version: CORPUS_ANALYSIS_ACTION_ENCODING_VERSION,
        room_id: &record.room_id,
        source_door_id: &record.source_door_id,
        target_door_id: &record.target_door_id,
        loadout: record.loadout,
        initial_state_digest: &record.initial_state_digest,
        terminal_state_digest: &record.terminal_state_digest,
        terminal_event_digest: &record.terminal_event_digest,
        total_ticks: record.total_ticks,
        demand: record.demand,
        coordinates: record.coordinates,
        actions: &record.actions,
    })?;
    Ok(format!(
        "{CONTROLLER_WITNESS_PREFIX}-{:016x}",
        fingerprint_bytes(&bytes)
    ))
}

fn direct_cost_id(source: &str, loadout: EvaluationLoadout) -> String {
    stable_binding_id("direct-cost-v1", &[source, loadout.slug()])
}

fn canonical_cost_id(source: &str, target: &str, loadout: EvaluationLoadout) -> String {
    stable_binding_id("canonical-cost-v1", &[source, target, loadout.slug()])
}

fn stable_binding_id(prefix: &str, fields: &[&str]) -> String {
    let mut bytes = Vec::new();
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u64).to_le_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    format!("{prefix}-{:016x}", fingerprint_bytes(&bytes))
}

fn render_json_lines<'a, T: Serialize + 'a>(
    values: impl IntoIterator<Item = &'a T>,
) -> Result<Vec<u8>, CorpusAnalysisArtifactError> {
    let mut result = Vec::new();
    for value in values {
        serde_json::to_writer(&mut result, value)?;
        result.push(b'\n');
    }
    Ok(result)
}

// `serde_json` without its optional `float_roundtrip` feature can parse a
// numeric token to an adjacent binary64 value, making decode/re-encode bytes
// non-canonical. Persisting the shortest Rust round-trip decimal as a JSON
// string gives exact, platform-stable binary64 identity without changing
// workspace-wide serde features.
mod canonical_f64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        let value = encoded.parse::<f64>().map_err(serde::de::Error::custom)?;
        if !value.is_finite() || value.to_string() != encoded {
            return Err(serde::de::Error::custom(
                "expected a canonical finite binary64 round-trip decimal",
            ));
        }
        Ok(value)
    }
}

mod canonical_option_f64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = Option::<String>::deserialize(deserializer)?;
        encoded
            .map(|encoded| {
                let value = encoded.parse::<f64>().map_err(serde::de::Error::custom)?;
                if !value.is_finite() || value.to_string() != encoded {
                    return Err(serde::de::Error::custom(
                        "expected a canonical finite binary64 round-trip decimal",
                    ));
                }
                Ok(value)
            })
            .transpose()
    }
}

fn render_single_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CorpusAnalysisArtifactError> {
    let mut result = serde_json::to_vec(value)?;
    result.push(b'\n');
    Ok(result)
}

fn parse_canonical_jsonl<T>(file: &str, bytes: &[u8]) -> Result<Vec<T>, CorpusAnalysisArtifactError>
where
    T: DeserializeOwned + Serialize,
{
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(invalid(format!("{file} does not end in a newline")));
    }
    if bytes.contains(&b'\r') {
        return Err(invalid(format!("{file} contains a carriage return")));
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (index, line) in bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .enumerate()
    {
        if line.is_empty() {
            return Err(invalid(format!(
                "{file} contains an empty line at {}",
                index + 1
            )));
        }
        let value: T =
            serde_json::from_slice(line).map_err(|source| CorpusAnalysisArtifactError::JsonAt {
                file: file.to_owned(),
                line: index + 1,
                source,
            })?;
        let canonical = serde_json::to_vec(&value)?;
        if canonical != line {
            let mismatch = canonical
                .iter()
                .zip(line)
                .position(|(canonical, original)| canonical != original)
                .unwrap_or_else(|| canonical.len().min(line.len()));
            let start = mismatch.saturating_sub(40);
            let canonical_end = canonical.len().min(mismatch.saturating_add(40));
            let original_end = line.len().min(mismatch.saturating_add(40));
            return Err(invalid(format!(
                "{file} line {} is not canonical JSON (first mismatch at byte {mismatch}, encoded/original lengths {}/{}, encoded {:?}, original {:?})",
                index + 1,
                canonical.len(),
                line.len(),
                String::from_utf8_lossy(&canonical[start..canonical_end]),
                String::from_utf8_lossy(&line[start..original_end]),
            )));
        }
        result.push(value);
    }
    Ok(result)
}

fn fingerprint_json<T: Serialize>(value: &T) -> Result<String, CorpusAnalysisArtifactError> {
    Ok(format!(
        "fnv1a64-{:016x}",
        fingerprint_bytes(&serde_json::to_vec(value)?)
    ))
}

fn byte_hash(bytes: &[u8]) -> String {
    format!("fnv1a64-{:016x}", fingerprint_bytes(bytes))
}

fn fingerprint_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn verify_decoded(
    manifest: &ManifestRecord,
    rooms: &[RoomRecord],
    witnesses: &[ControllerWitnessRecord],
    replay: bool,
) -> Result<(), CorpusAnalysisArtifactError> {
    if manifest.artifact_schema != ARTIFACT_SCHEMA
        || manifest.artifact_version != CORPUS_ANALYSIS_ARTIFACT_VERSION
        || manifest.status != ARTIFACT_STATUS
    {
        return Err(invalid(format!(
            "unsupported analysis artifact identity {:?}/{} with status {:?}",
            manifest.artifact_schema, manifest.artifact_version, manifest.status
        )));
    }
    if manifest.policies != PolicyIdentityRecord::current() {
        return Err(invalid(format!(
            "analysis artifact policy identity differs: recorded {:?}, current {:?}",
            manifest.policies,
            PolicyIdentityRecord::current()
        )));
    }
    if manifest.disclaimers != DisclaimerRecord::current() {
        return Err(invalid("analysis artifact disclaimers/claims differ"));
    }
    if manifest.config_fingerprint != fingerprint_json(&manifest.config)? {
        return Err(invalid("analysis config fingerprint mismatch"));
    }
    validate_solver_config(&manifest.config.direct_controller_solver)?;
    if manifest.room_count != rooms.len()
        || manifest.retained_controller_witness_count != witnesses.len()
    {
        return Err(invalid(format!(
            "manifest counts differ: rooms={}/{}, witnesses={}/{}",
            manifest.room_count,
            rooms.len(),
            manifest.retained_controller_witness_count,
            witnesses.len()
        )));
    }
    let actual_canonical_vectors = rooms
        .iter()
        .map(|room| room.canonical_route_difficulty.routes.len())
        .sum::<usize>();
    if manifest.canonical_route_vector_count != actual_canonical_vectors {
        return Err(invalid(format!(
            "manifest canonical vector count {}/{} differs",
            manifest.canonical_route_vector_count, actual_canonical_vectors
        )));
    }
    require_strict_order(
        "rooms.jsonl",
        rooms.iter().map(|room| room.room_id.as_str()),
    )?;
    require_strict_order(
        "controller_witnesses.jsonl",
        witnesses.iter().map(|witness| witness.witness_id.as_str()),
    )?;

    let witness_map = witnesses
        .iter()
        .map(|witness| (witness.witness_id.as_str(), witness))
        .collect::<HashMap<_, _>>();
    if witness_map.len() != witnesses.len() {
        return Err(invalid("duplicate controller witness ID"));
    }
    for witness in witnesses {
        validate_witness_record(witness, &manifest.config.direct_controller_solver)?;
    }

    let mut witness_references = HashMap::<&str, usize>::new();
    for room in rooms {
        validate_room_record(manifest.seed, room, &witness_map, &mut witness_references)?;
    }
    for witness in witnesses {
        if !witness_references.contains_key(witness.witness_id.as_str()) {
            return Err(invalid(format!(
                "orphan controller witness {:?}",
                witness.witness_id
            )));
        }
    }

    if replay {
        for room in rooms {
            regenerate_and_replay_room(room, witnesses)?;
        }
    }
    Ok(())
}

fn validate_solver_config(config: &SolverConfigRecord) -> Result<(), CorpusAnalysisArtifactError> {
    if config.max_ticks_per_path == 0
        || config.beam_width == 0
        || config.position_quantum <= 0
        || config.velocity_quantum <= 0
        || config.macros.is_empty()
    {
        return Err(invalid("analysis solver config is structurally invalid"));
    }
    for (index, action_macro) in config.macros.iter().enumerate() {
        if action_macro.name.is_empty() || action_macro.actions.is_empty() {
            return Err(invalid(format!(
                "analysis solver macro {index} has an empty name or action sequence"
            )));
        }
        checked_action_ticks(&action_macro.actions)?;
    }
    Ok(())
}

fn validate_witness_record(
    witness: &ControllerWitnessRecord,
    solver: &SolverConfigRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    if witness.witness_id != controller_witness_id(witness)? {
        return Err(invalid(format!(
            "controller witness {:?} has a content-identity mismatch",
            witness.witness_id
        )));
    }
    validate_digest("initial state", &witness.initial_state_digest)?;
    validate_digest("terminal state", &witness.terminal_state_digest)?;
    if let Some(digest) = &witness.terminal_event_digest {
        validate_digest("terminal event", digest)?;
    }
    let ticks = checked_action_ticks(&witness.actions)?;
    if ticks == 0
        || ticks != witness.total_ticks
        || ticks != witness.demand.duration_ticks
        || ticks > solver.max_ticks_per_path
    {
        return Err(invalid(format!(
            "controller witness {:?} has invalid action/replay/demand ticks {}/{}/{} (horizon {})",
            witness.witness_id,
            ticks,
            witness.total_ticks,
            witness.demand.duration_ticks,
            solver.max_ticks_per_path
        )));
    }
    let expected_coordinates = coordinates_from_demand(witness.demand);
    if witness.coordinates != expected_coordinates {
        return Err(invalid(format!(
            "controller witness {:?} demand coordinates do not match its transparent demand",
            witness.witness_id
        )));
    }
    Ok(())
}

fn coordinates_from_demand(demand: ControllerDemandRecord) -> ControllerDemandCoordinatesRecord {
    ControllerDemandCoordinatesRecord {
        controller_class: if demand.run_only {
            0
        } else if demand.monotone_simple {
            1
        } else {
            2
        },
        ability_events: demand.wall_jump_events.saturating_add(demand.dash_events),
        horizontal_reversals: demand.horizontal_reversals,
        vertical_decisions: demand.vertical_decisions,
        semantic_spans: demand.semantic_spans,
        semantic_transitions: demand.semantic_transitions,
        duration_ticks: demand.duration_ticks,
    }
}

fn validate_digest(label: &str, value: &str) -> Result<(), CorpusAnalysisArtifactError> {
    if value.len() != 16
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        || u64::from_str_radix(value, 16).is_err()
    {
        return Err(invalid(format!(
            "{label} digest {value:?} is not canonical 16-digit lowercase hex"
        )));
    }
    Ok(())
}

fn validate_room_record<'a>(
    seed: u64,
    room: &'a RoomRecord,
    witnesses: &HashMap<&'a str, &'a ControllerWitnessRecord>,
    witness_references: &mut HashMap<&'a str, usize>,
) -> Result<(), CorpusAnalysisArtifactError> {
    if room.row_version != CORPUS_ANALYSIS_ARTIFACT_VERSION
        || room.analysis_version != CORPUS_ROOM_ANALYSIS_VERSION
        || room.metric_summary_version != ROOM_METRIC_SUMMARY_VERSION
        || room.generation_key.seed != seed
    {
        return Err(invalid(format!(
            "room {:?} has incompatible row/analysis/metric/key identities",
            room.room_id
        )));
    }
    staged_key(&room.generation_key)?;
    require_strict_order("room door IDs", room.door_ids.iter().map(String::as_str))?;
    if room.door_ids.len() < 2 {
        return Err(invalid(format!(
            "room {:?} must retain at least two canonical doors",
            room.room_id
        )));
    }
    let directed_route_count = room
        .door_ids
        .len()
        .checked_mul(room.door_ids.len() - 1)
        .ok_or_else(|| invalid("directed route count overflow"))?;
    let expected_routes = room
        .door_ids
        .iter()
        .flat_map(|source| {
            room.door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.as_str(), target.as_str()))
        })
        .collect::<Vec<_>>();

    let source_audits = &room.direct_controller_evidence.source_audits;
    if source_audits.len() != room.door_ids.len() {
        return Err(invalid(format!(
            "room {:?} source-audit count {}/{} differs",
            room.room_id,
            source_audits.len(),
            room.door_ids.len()
        )));
    }
    for (source, expected_source) in source_audits.iter().zip(&room.door_ids) {
        if &source.source_door_id != expected_source
            || source.authoritative_loadout != EvaluationLoadout::Both
            || source.expected_subset_loadouts != EvaluationLoadout::ALL
        {
            return Err(invalid(format!(
                "room {:?} source audit {:?} has invalid identity/loadout policy",
                room.room_id, source.source_door_id
            )));
        }
        let expected_targets = room
            .door_ids
            .iter()
            .filter(|target| *target != expected_source)
            .cloned()
            .collect::<Vec<_>>();
        if source.target_door_ids != expected_targets {
            return Err(invalid(format!(
                "room {:?} source {:?} target order differs",
                room.room_id, source.source_door_id
            )));
        }
        require_exact_loadout_order(
            "shared source audits",
            source
                .shared_loadout_audits
                .iter()
                .map(|audit| audit.loadout),
        )?;
        for audit in &source.shared_loadout_audits {
            if audit.target_count != expected_targets.len()
                || audit.raw_positive_witnesses < audit.retained_semantic_witnesses
            {
                return Err(invalid(format!(
                    "room {:?} source {:?} {} shared-audit counts are invalid",
                    room.room_id,
                    source.source_door_id,
                    audit.loadout.slug()
                )));
            }
        }
    }

    let routes = &room.direct_controller_evidence.directed_routes;
    if routes.len() != directed_route_count {
        return Err(invalid(format!(
            "room {:?} direct-route count {}/{} differs",
            room.room_id,
            routes.len(),
            directed_route_count
        )));
    }
    for (route, &(expected_source, expected_target)) in routes.iter().zip(&expected_routes) {
        if route.source_door_id != expected_source
            || route.target_door_id != expected_target
            || route.authoritative_loadout != EvaluationLoadout::Both
            || route.expected_subset_loadouts != EvaluationLoadout::ALL
        {
            return Err(invalid(format!(
                "room {:?} direct route order/identity/loadout differs at {:?}->{:?}",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        }
        require_exact_loadout_order(
            "direct route loadouts",
            route.loadouts.iter().map(|loadout| loadout.loadout),
        )?;
        validate_route_completeness(route)?;
        let has_retained_witness = route
            .loadouts
            .iter()
            .any(|loadout| loadout.retained_semantic_witnesses > 0);
        if route.overall_easiest_known_witness_id.is_some() != has_retained_witness {
            return Err(invalid(format!(
                "room {:?} route {:?}->{:?} overall/loadout positive availability differs",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        }
        validate_witness_refs(
            room,
            route,
            None,
            route.overall_easiest_known_witness_id.as_deref(),
            &route.overall_pareto_front_witness_ids,
            witnesses,
            witness_references,
        )?;
        let source = source_audits
            .iter()
            .find(|source| source.source_door_id == route.source_door_id)
            .expect("source rows were checked against the canonical door universe");
        for loadout in &route.loadouts {
            if loadout.raw_positive_witnesses < loadout.retained_semantic_witnesses
                || (loadout.retained_semantic_witnesses == 0)
                    != loadout.easiest_known_witness_id.is_none()
            {
                return Err(invalid(format!(
                    "room {:?} route {:?}->{:?} {} witness counts/options differ",
                    room.room_id,
                    route.source_door_id,
                    route.target_door_id,
                    loadout.loadout.slug()
                )));
            }
            let shared = source
                .shared_loadout_audits
                .iter()
                .find(|shared| shared.loadout == loadout.loadout)
                .expect("source shared audits contain every loadout");
            if loadout.status != shared.status {
                return Err(invalid(format!(
                    "room {:?} route {:?}->{:?} {} status differs from its shared source audit",
                    room.room_id,
                    route.source_door_id,
                    route.target_door_id,
                    loadout.loadout.slug()
                )));
            }
            validate_witness_refs(
                room,
                route,
                Some(loadout.loadout),
                loadout.easiest_known_witness_id.as_deref(),
                &loadout.pareto_front_witness_ids,
                witnesses,
                witness_references,
            )?;
        }
        require_strict_order(
            "positive bypass loadouts",
            route.positive_bypasses.iter().map(|bypass| bypass.loadout),
        )?;
        let expected_bypasses = route
            .loadouts
            .iter()
            .filter(|loadout| {
                loadout.loadout != EvaluationLoadout::Both
                    && loadout.retained_semantic_witnesses > 0
            })
            .map(|loadout| PositiveBypassRecord {
                loadout: loadout.loadout,
                retained_semantic_witnesses: loadout.retained_semantic_witnesses,
            })
            .collect::<Vec<_>>();
        if route.positive_bypasses != expected_bypasses {
            return Err(invalid(format!(
                "room {:?} route {:?}->{:?} positive bypasses differ from exact loadout positives",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        }
        for bypass in &route.positive_bypasses {
            if bypass.loadout == EvaluationLoadout::Both
                || bypass.retained_semantic_witnesses == 0
                || route
                    .loadouts
                    .iter()
                    .find(|loadout| loadout.loadout == bypass.loadout)
                    .map(|loadout| loadout.retained_semantic_witnesses)
                    != Some(bypass.retained_semantic_witnesses)
            {
                return Err(invalid(format!(
                    "room {:?} route {:?}->{:?} has invalid positive bypass",
                    room.room_id, route.source_door_id, route.target_door_id
                )));
            }
        }
    }

    for source in source_audits {
        for shared in &source.shared_loadout_audits {
            let matching = routes
                .iter()
                .filter(|route| route.source_door_id == source.source_door_id)
                .map(|route| {
                    route
                        .loadouts
                        .iter()
                        .find(|loadout| loadout.loadout == shared.loadout)
                        .expect("every route contains every loadout")
                })
                .collect::<Vec<_>>();
            let raw_positive_witnesses = matching
                .iter()
                .map(|loadout| loadout.raw_positive_witnesses)
                .sum::<usize>();
            let retained_semantic_witnesses = matching
                .iter()
                .map(|loadout| loadout.retained_semantic_witnesses)
                .sum::<usize>();
            if shared.raw_positive_witnesses != raw_positive_witnesses
                || shared.retained_semantic_witnesses != retained_semantic_witnesses
            {
                return Err(invalid(format!(
                    "room {:?} source {:?} {} shared/per-target witness counts differ",
                    room.room_id,
                    source.source_door_id,
                    shared.loadout.slug()
                )));
            }
        }
    }

    validate_operational_cost(room)?;
    validate_canonical_routes(room, directed_route_count)?;
    validate_terrain(room)?;
    validate_metric_aggregates(room, directed_route_count)?;
    Ok(())
}

fn require_exact_loadout_order(
    label: &str,
    values: impl IntoIterator<Item = EvaluationLoadout>,
) -> Result<(), CorpusAnalysisArtifactError> {
    let values = values.into_iter().collect::<Vec<_>>();
    if values != EvaluationLoadout::ALL {
        return Err(invalid(format!(
            "{label} must contain baseline, wall-jump, dash, both exactly once; found {values:?}"
        )));
    }
    Ok(())
}

fn validate_route_completeness(
    route: &DirectedControllerRouteRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    let incomplete = route
        .loadouts
        .iter()
        .filter_map(|loadout| match loadout.status {
            AuditStatusRecord::CompleteFiniteVocabulary => None,
            AuditStatusRecord::BoundedIncomplete { limit } => Some(BoundedLoadoutRecord {
                loadout: loadout.loadout,
                limit,
            }),
        })
        .collect::<Vec<_>>();
    let expected = if incomplete.is_empty() {
        RouteCompletenessRecord::CompleteFiniteVocabulary
    } else {
        RouteCompletenessRecord::BoundedIncomplete {
            incomplete_loadouts: incomplete,
        }
    };
    if route.completeness != expected {
        return Err(invalid(format!(
            "route {:?}->{:?} aggregate completeness disagrees with its loadout audits",
            route.source_door_id, route.target_door_id
        )));
    }
    Ok(())
}

fn validate_witness_refs<'a>(
    room: &RoomRecord,
    route: &DirectedControllerRouteRecord,
    expected_loadout: Option<EvaluationLoadout>,
    easiest: Option<&'a str>,
    pareto: &'a [String],
    witnesses: &HashMap<&'a str, &'a ControllerWitnessRecord>,
    references: &mut HashMap<&'a str, usize>,
) -> Result<(), CorpusAnalysisArtifactError> {
    if easiest.is_none() != pareto.is_empty() {
        return Err(invalid(format!(
            "room {:?} route {:?}->{:?} easiest/Pareto availability differs",
            room.room_id, route.source_door_id, route.target_door_id
        )));
    }
    if let Some(easiest) = easiest
        && pareto.first().map(String::as_str) != Some(easiest)
    {
        return Err(invalid(format!(
            "room {:?} route {:?}->{:?} easiest witness is not first on its Pareto front",
            room.room_id, route.source_door_id, route.target_door_id
        )));
    }
    let mut pareto_unique = BTreeSet::new();
    for id in pareto {
        if !pareto_unique.insert(id.as_str()) {
            return Err(invalid(format!(
                "room {:?} route {:?}->{:?} repeats Pareto witness {id:?}",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        }
    }
    for id in easiest.into_iter().chain(pareto.iter().map(String::as_str)) {
        let Some(witness) = witnesses.get(id).copied() else {
            return Err(invalid(format!(
                "room {:?} route {:?}->{:?} references absent witness {id:?}",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        };
        if witness.room_id != room.room_id
            || witness.source_door_id != route.source_door_id
            || witness.target_door_id != route.target_door_id
            || expected_loadout.is_some_and(|loadout| witness.loadout != loadout)
        {
            return Err(invalid(format!(
                "witness {id:?} binding differs from room/route/loadout cross-reference"
            )));
        }
        *references.entry(id).or_default() += 1;
    }
    for left in 0..pareto.len() {
        for right in left + 1..pareto.len() {
            let left_witness = witnesses[pareto[left].as_str()];
            let right_witness = witnesses[pareto[right].as_str()];
            if record_coordinates_dominate(left_witness.coordinates, right_witness.coordinates)
                || record_coordinates_dominate(right_witness.coordinates, left_witness.coordinates)
            {
                return Err(invalid(format!(
                    "route {:?}->{:?} Pareto witnesses dominate one another",
                    route.source_door_id, route.target_door_id
                )));
            }
        }
    }
    Ok(())
}

fn record_coordinates_dominate(
    left: ControllerDemandCoordinatesRecord,
    right: ControllerDemandCoordinatesRecord,
) -> bool {
    let no_harder = left.controller_class <= right.controller_class
        && left.ability_events <= right.ability_events
        && left.horizontal_reversals <= right.horizontal_reversals
        && left.vertical_decisions <= right.vertical_decisions
        && left.semantic_spans <= right.semantic_spans
        && left.semantic_transitions <= right.semantic_transitions
        && left.duration_ticks <= right.duration_ticks;
    let strict = left != right;
    no_harder && strict
}

fn validate_operational_cost(room: &RoomRecord) -> Result<(), CorpusAnalysisArtifactError> {
    let operational = &room.operational_cost;
    if operational.interpretation != "solver_capacity_evidence_not_player_difficulty" {
        return Err(invalid(format!(
            "room {:?} operational-cost interpretation differs",
            room.room_id
        )));
    }
    require_strict_order(
        "direct operational cost units",
        operational
            .direct_controller_audit_units
            .iter()
            .map(|unit| (&unit.source_door_id, unit.loadout)),
    )?;
    require_strict_order(
        "canonical operational cost units",
        operational
            .canonical_positive_routes
            .iter()
            .map(|unit| (unit.loadout, &unit.source_door_id, &unit.target_door_id)),
    )?;
    let mut direct_units = HashMap::new();
    for unit in &operational.direct_controller_audit_units {
        if unit.cost_id != direct_cost_id(&unit.source_door_id, unit.loadout)
            || direct_units.insert(unit.cost_id.as_str(), unit).is_some()
        {
            return Err(invalid(format!(
                "room {:?} has invalid/duplicate direct cost identity {:?}",
                room.room_id, unit.cost_id
            )));
        }
    }
    let mut direct_references = BTreeSet::new();
    for source in &room.direct_controller_evidence.source_audits {
        for audit in &source.shared_loadout_audits {
            let Some(unit) = direct_units.get(audit.operational_cost_ref.as_str()) else {
                return Err(invalid(format!(
                    "room {:?} shared audit references absent direct cost {:?}",
                    room.room_id, audit.operational_cost_ref
                )));
            };
            if !direct_references.insert(audit.operational_cost_ref.as_str())
                || unit.source_door_id != source.source_door_id
                || unit.loadout != audit.loadout
                || unit.status != audit.status
            {
                return Err(invalid(format!(
                    "room {:?} shared audit/direct cost cross-reference differs",
                    room.room_id
                )));
            }
        }
    }
    if direct_references.len() != direct_units.len() {
        return Err(invalid(format!(
            "room {:?} has orphan direct operational-cost units",
            room.room_id
        )));
    }
    let recomputed_direct = operational
        .direct_controller_audit_units
        .iter()
        .map(|unit| unit.cost)
        .fold(OperationalCostRecord::default(), aggregate_cost);
    if recomputed_direct != operational.direct_controller_recomputed_total
        || recomputed_direct != operational.direct_controller_reported_total
    {
        return Err(invalid(format!(
            "room {:?} direct operational totals differ",
            room.room_id
        )));
    }

    let mut canonical_units = HashMap::new();
    for unit in &operational.canonical_positive_routes {
        if unit.cost_id
            != canonical_cost_id(&unit.source_door_id, &unit.target_door_id, unit.loadout)
            || canonical_units
                .insert(unit.cost_id.as_str(), unit)
                .is_some()
        {
            return Err(invalid(format!(
                "room {:?} has invalid/duplicate canonical cost identity {:?}",
                room.room_id, unit.cost_id
            )));
        }
    }
    let recomputed_canonical = operational
        .canonical_positive_routes
        .iter()
        .map(|unit| unit.cost)
        .fold(OperationalCostRecord::default(), aggregate_cost);
    if recomputed_canonical != operational.canonical_positive_route_total {
        return Err(invalid(format!(
            "room {:?} canonical operational total differs",
            room.room_id
        )));
    }
    Ok(())
}

fn aggregate_cost(
    mut total: OperationalCostRecord,
    additional: OperationalCostRecord,
) -> OperationalCostRecord {
    total.expanded_nodes = total
        .expanded_nodes
        .saturating_add(additional.expanded_nodes);
    total.generated_nodes = total
        .generated_nodes
        .saturating_add(additional.generated_nodes);
    total.simulated_ticks = total
        .simulated_ticks
        .saturating_add(additional.simulated_ticks);
    total.deepest_path_ticks = total.deepest_path_ticks.max(additional.deepest_path_ticks);
    total
}

fn validate_canonical_routes(
    room: &RoomRecord,
    directed_route_count: usize,
) -> Result<(), CorpusAnalysisArtifactError> {
    let canonical = &room.canonical_route_difficulty;
    if canonical.claim != CANONICAL_ROUTE_CLAIM {
        return Err(invalid(format!(
            "room {:?} canonical routes are not explicitly labelled not-easiest-known",
            room.room_id
        )));
    }
    require_strict_order(
        "canonical route vectors",
        canonical
            .routes
            .iter()
            .map(|route| (route.loadout, &route.source_door_id, &route.target_door_id)),
    )?;
    let cost_units = room
        .operational_cost
        .canonical_positive_routes
        .iter()
        .map(|unit| (unit.cost_id.as_str(), unit))
        .collect::<HashMap<_, _>>();
    let mut cost_references = BTreeSet::new();
    for route in &canonical.routes {
        if route.measurement_version != super::CORPUS_ROUTE_MEASUREMENT_VERSION
            || route.source_door_id == route.target_door_id
            || !room.door_ids.contains(&route.source_door_id)
            || !room.door_ids.contains(&route.target_door_id)
            || route.vector.version != ROUTE_DIFFICULTY_VECTOR_VERSION
            || route.vector.target_id != route.target_door_id
            || !route.witness_fingerprint.starts_with("downwards-witness-v")
        {
            return Err(invalid(format!(
                "room {:?} canonical route {:?}->{:?} has invalid identity/version",
                room.room_id, route.source_door_id, route.target_door_id
            )));
        }
        validate_route_vector(&route.vector)?;
        validate_landing_precision(
            &route.landing_precision,
            route.vector.traversal.completion_ticks,
        )?;
        let Some(unit) = cost_units.get(route.operational_cost_ref.as_str()) else {
            return Err(invalid(format!(
                "room {:?} canonical route references absent cost {:?}",
                room.room_id, route.operational_cost_ref
            )));
        };
        if !cost_references.insert(route.operational_cost_ref.as_str())
            || unit.source_door_id != route.source_door_id
            || unit.target_door_id != route.target_door_id
            || unit.loadout != route.loadout
        {
            return Err(invalid(format!(
                "room {:?} canonical route/cost cross-reference differs",
                room.room_id
            )));
        }
    }
    if cost_references.len() != cost_units.len() {
        return Err(invalid(format!(
            "room {:?} has orphan canonical operational costs",
            room.room_id
        )));
    }
    let aggregate = &room.metric_aggregates.canonical_routes;
    if aggregate.directed_route_count != directed_route_count
        || aggregate.positive_route_count != canonical.routes.len()
    {
        return Err(invalid(format!(
            "room {:?} canonical vector/metric counts differ",
            room.room_id
        )));
    }
    Ok(())
}

fn validate_landing_precision(
    landing: &LandingPrecisionRecord,
    completion_ticks: usize,
) -> Result<(), CorpusAnalysisArtifactError> {
    if landing.version != LANDING_PRECISION_VERSION
        || landing.interpretation != LANDING_PRECISION_DISCLAIMER
        || landing.inspected_ticks != completion_ticks
        || landing.landing_event_count
            != landing
                .samples
                .len()
                .saturating_add(landing.unmeasured_landing_ticks.len())
    {
        return Err(invalid(
            "canonical landing precision identity/counts differ",
        ));
    }

    require_strict_order(
        "canonical landing samples",
        landing.samples.iter().map(|sample| sample.replay_tick),
    )?;
    require_strict_order(
        "canonical unmeasured landing ticks",
        landing.unmeasured_landing_ticks.iter().copied(),
    )?;
    let mut all_ticks = BTreeSet::new();
    for tick in landing
        .samples
        .iter()
        .map(|sample| sample.replay_tick)
        .chain(landing.unmeasured_landing_ticks.iter().copied())
    {
        if tick == 0 || tick > landing.inspected_ticks || !all_ticks.insert(tick) {
            return Err(invalid("canonical landing ticks are invalid or duplicated"));
        }
    }

    for sample in &landing.samples {
        let player_right = sample.player_x.saturating_add(sample.player_width);
        let player_bottom = sample.player_y.saturating_add(sample.player_height);
        let overlap_left = sample.player_x.max(sample.support_left);
        let overlap_right = player_right.min(sample.support_right);
        let overlap = u32::try_from(overlap_right.saturating_sub(overlap_left)).unwrap_or_default();
        if sample.player_width <= 0
            || sample.player_height <= 0
            || sample.support_right <= sample.support_left
            || sample.surface_y != player_bottom
            || sample.footprint_overlap_pixels == 0
            || sample.footprint_overlap_pixels != overlap
            || sample.left_edge_margin_pixels != sample.player_x.saturating_sub(sample.support_left)
            || sample.right_edge_margin_pixels != sample.support_right.saturating_sub(player_right)
        {
            return Err(invalid("canonical landing sample geometry differs"));
        }
    }

    let minimum_overlap = landing
        .samples
        .iter()
        .map(|sample| sample.footprint_overlap_pixels)
        .min();
    let minimum_margin = landing
        .samples
        .iter()
        .map(|sample| {
            sample
                .left_edge_margin_pixels
                .min(sample.right_edge_margin_pixels)
        })
        .min();
    let narrowest_support = landing
        .samples
        .iter()
        .map(|sample| {
            u32::try_from(sample.support_right.saturating_sub(sample.support_left))
                .unwrap_or_default()
        })
        .min();
    let overhangs = landing
        .samples
        .iter()
        .filter(|sample| {
            sample
                .left_edge_margin_pixels
                .min(sample.right_edge_margin_pixels)
                < 0
        })
        .count();
    let one_way = landing
        .samples
        .iter()
        .filter(|sample| sample.support_kind != LandingSupportKindRecord::Solid)
        .count();
    if landing.minimum_footprint_overlap_pixels != minimum_overlap
        || landing.minimum_edge_margin_pixels != minimum_margin
        || landing.narrowest_support_width_pixels != narrowest_support
        || landing.edge_overhang_landings != overhangs
        || landing.one_way_or_mixed_landings != one_way
    {
        return Err(invalid("canonical landing precision aggregates differ"));
    }
    Ok(())
}

fn validate_route_vector(
    vector: &PlayerRouteDifficultyVectorRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    let finite_nonnegative = [
        vector.traversal.coarse_path_length_cells,
        vector.traversal.horizontal_travel_cells,
        vector.traversal.vertical_travel_cells,
        vector.hazards.pressure,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value >= 0.0);
    if !finite_nonnegative
        || vector.control.restart_presses != 0
        || vector.traversal.completion_ticks == 0
    {
        return Err(invalid(format!(
            "canonical route vector for {:?} has invalid numeric/control coordinates",
            vector.target_id
        )));
    }
    validate_probabilities(vector.timing.perfect_control)?;
    if vector.timing.perfect_control.trials != 1
        || vector.timing.perfect_control.success_probability != 1.0
        || vector.timing.perfect_control.failure_probability != 0.0
        || !matches!(
            vector.timing.shaky_hand,
            ShakyHandStatusRecord::Missing {
                reason: MissingTimingEvidenceRecord::ShakyHandStudyNotProvided
            }
        )
    {
        return Err(invalid(format!(
            "canonical route vector for {:?} violates the exact-control/no-shaky-hand policy",
            vector.target_id
        )));
    }
    Ok(())
}

fn validate_probabilities(
    probabilities: TimingOutcomeProbabilitiesRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    let values = [
        probabilities.success_probability,
        probabilities.failure_probability,
        probabilities.death_probability,
        probabilities.wrong_target_probability,
        probabilities.timeout_probability,
    ];
    if probabilities.trials == 0
        || values
            .into_iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || (probabilities.success_probability + probabilities.failure_probability - 1.0).abs()
            > f64::EPSILON
    {
        return Err(invalid("route timing probabilities are invalid"));
    }
    Ok(())
}

fn validate_terrain(room: &RoomRecord) -> Result<(), CorpusAnalysisArtifactError> {
    let terrain = &room.terrain_positive_evidence;
    if terrain.audit_version != TERRAIN_AUDIT_VERSION
        || terrain.loadout != AbilitySetRecord::from(AbilitySet::ALL)
        || terrain.coverage.positive_controller_count
            != terrain
                .positive_door_controller_count
                .saturating_add(terrain.positive_pickup_controller_count)
        || terrain.coverage.structurally_attributed_component_count
            > terrain.coverage.interior_component_count
        || terrain.coverage.traversal_near_component_count
            > terrain.coverage.interior_component_count
        || terrain.coverage.positively_corroborated_component_count
            > terrain.coverage.interior_component_count
        || terrain.coverage.uncorroborated_component_count
            > terrain.coverage.interior_component_count
        || terrain.coverage.structurally_attributed_tile_count
            > terrain.coverage.interior_tile_count
        || terrain.coverage.traversal_near_tile_count > terrain.coverage.interior_tile_count
        || terrain.coverage.positively_corroborated_tile_count
            > terrain.coverage.interior_tile_count
        || terrain.coverage.uncorroborated_tile_count > terrain.coverage.interior_tile_count
    {
        return Err(invalid(format!(
            "room {:?} terrain coverage/version/counts are invalid",
            room.room_id
        )));
    }
    require_strict_order(
        "terrain ablation outcomes",
        terrain
            .component_ablation_outcomes
            .iter()
            .map(|ablation| ablation.kind),
    )?;
    for ablation in &terrain.component_ablation_outcomes {
        validate_ablation_outcomes(ablation)?;
    }
    let metric = &room.metric_aggregates.terrain;
    if metric.source_terrain_audit_version != terrain.audit_version
        || metric.coverage != terrain.coverage
        || metric.ablations.len() != terrain.component_ablation_outcomes.len()
    {
        return Err(invalid(format!(
            "room {:?} terrain evidence/metric aggregates differ",
            room.room_id
        )));
    }
    for (metric_ablation, evidence_ablation) in metric
        .ablations
        .iter()
        .zip(&terrain.component_ablation_outcomes)
    {
        if metric_ablation.kind != evidence_ablation.kind
            || metric_ablation.outcomes != *evidence_ablation
        {
            return Err(invalid(format!(
                "room {:?} terrain ablation evidence/metric outcomes differ",
                room.room_id
            )));
        }
        validate_fraction_evidence(&metric_ablation.survival_fraction)?;
        match &metric_ablation.survival_fraction {
            MetricEvidenceRecord::Observed { value }
                if value.numerator == evidence_ablation.succeeded
                    && value.denominator == evidence_ablation.controller_count => {}
            MetricEvidenceRecord::NotApplicable {
                reason: NotApplicableMetricReasonRecord::ExactControllersForAblation,
            } if evidence_ablation.controller_count == 0 => {}
            _ => {
                return Err(invalid(format!(
                    "room {:?} ablation survival fraction differs from outcomes",
                    room.room_id
                )));
            }
        }
    }
    validate_ablation_utility(metric)?;
    validate_fraction_evidence(&metric.aggregate_ablation_survival_fraction)?;
    validate_coverage_fraction(
        &metric.coverage_fractions.structurally_attributed_components,
        terrain.coverage.structurally_attributed_component_count,
        terrain.coverage.interior_component_count,
        false,
    )?;
    validate_coverage_fraction(
        &metric.coverage_fractions.structurally_attributed_tiles,
        terrain.coverage.structurally_attributed_tile_count,
        terrain.coverage.interior_tile_count,
        false,
    )?;
    validate_coverage_fraction(
        &metric.coverage_fractions.traversal_near_components,
        terrain.coverage.traversal_near_component_count,
        terrain.coverage.interior_component_count,
        terrain.coverage.positive_controller_count == 0,
    )?;
    validate_coverage_fraction(
        &metric.coverage_fractions.traversal_near_tiles,
        terrain.coverage.traversal_near_tile_count,
        terrain.coverage.interior_tile_count,
        terrain.coverage.positive_controller_count == 0,
    )?;
    validate_coverage_fraction(
        &metric.coverage_fractions.positively_corroborated_components,
        terrain.coverage.positively_corroborated_component_count,
        terrain.coverage.interior_component_count,
        false,
    )?;
    validate_coverage_fraction(
        &metric.coverage_fractions.positively_corroborated_tiles,
        terrain.coverage.positively_corroborated_tile_count,
        terrain.coverage.interior_tile_count,
        false,
    )?;
    let total_controllers = metric
        .ablations
        .iter()
        .map(|ablation| ablation.outcomes.controller_count)
        .sum::<usize>();
    let total_succeeded = metric
        .ablations
        .iter()
        .map(|ablation| ablation.outcomes.succeeded)
        .sum::<usize>();
    match &metric.aggregate_ablation_survival_fraction {
        MetricEvidenceRecord::Observed { value }
            if !metric.ablations.is_empty()
                && total_controllers > 0
                && value.numerator == total_succeeded
                && value.denominator == total_controllers => {}
        MetricEvidenceRecord::NotApplicable {
            reason: NotApplicableMetricReasonRecord::AblationVariants,
        } if metric.ablations.is_empty() => {}
        MetricEvidenceRecord::NotApplicable {
            reason: NotApplicableMetricReasonRecord::ExactControllersForAblation,
        } if !metric.ablations.is_empty() && total_controllers == 0 => {}
        _ => {
            return Err(invalid(format!(
                "room {:?} aggregate ablation survival evidence differs from outcomes",
                room.room_id
            )));
        }
    }
    Ok(())
}

fn validate_coverage_fraction(
    evidence: &MetricEvidenceRecord<ExactFractionRecord>,
    numerator: usize,
    denominator: usize,
    missing_without_positive_controller: bool,
) -> Result<(), CorpusAnalysisArtifactError> {
    match evidence {
        MetricEvidenceRecord::Observed { value }
            if denominator > 0
                && !missing_without_positive_controller
                && value.numerator == numerator
                && value.denominator == denominator =>
        {
            Ok(())
        }
        MetricEvidenceRecord::Missing {
            reason: MissingMetricReasonRecord::NoPositiveTerrainController,
        } if denominator > 0 && missing_without_positive_controller => Ok(()),
        MetricEvidenceRecord::NotApplicable {
            reason:
                NotApplicableMetricReasonRecord::InteriorTerrainComponents
                | NotApplicableMetricReasonRecord::InteriorTerrainTiles,
        } if denominator == 0 => Ok(()),
        _ => Err(invalid(
            "terrain coverage fraction availability/value differs from coverage counts",
        )),
    }
}

fn validate_ablation_outcomes(
    ablation: &AblationOutcomeRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    if ablation
        .succeeded
        .saturating_add(ablation.died)
        .saturating_add(ablation.wrong_target)
        .saturating_add(ablation.diverged)
        != ablation.controller_count
    {
        return Err(invalid(format!(
            "ablation {:?} outcome counts do not sum to controller count",
            ablation.kind
        )));
    }
    Ok(())
}

fn validate_ablation_utility(
    terrain: &TerrainMetricAggregateRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    let mut expected = AblationUtilityRecord {
        variant_count: terrain.ablations.len(),
        all_stored_controllers_survived: 0,
        at_least_one_stored_controller_affected: 0,
        no_stored_controllers: 0,
        removed_tiles_all_survived: 0,
        removed_tiles_some_affected: 0,
        removed_tiles_no_controllers: 0,
    };
    for ablation in &terrain.ablations {
        let removed_tiles = match ablation.kind {
            RoomAblationKindRecord::InteriorTerrainComponent { tile_count, .. }
            | RoomAblationKindRecord::StaticHazardComponent { tile_count, .. } => tile_count,
            RoomAblationKindRecord::TimedHazard { .. } => 0,
        };
        if ablation.outcomes.controller_count == 0 {
            expected.no_stored_controllers += 1;
            expected.removed_tiles_no_controllers = expected
                .removed_tiles_no_controllers
                .saturating_add(removed_tiles);
        } else if ablation.outcomes.succeeded == ablation.outcomes.controller_count {
            expected.all_stored_controllers_survived += 1;
            expected.removed_tiles_all_survived = expected
                .removed_tiles_all_survived
                .saturating_add(removed_tiles);
        } else {
            expected.at_least_one_stored_controller_affected += 1;
            expected.removed_tiles_some_affected = expected
                .removed_tiles_some_affected
                .saturating_add(removed_tiles);
        }
    }
    if terrain.ablation_utility != expected {
        return Err(invalid(
            "terrain ablation utility counts/tile totals do not match per-variant outcomes",
        ));
    }
    Ok(())
}

fn validate_metric_aggregates(
    room: &RoomRecord,
    directed_route_count: usize,
) -> Result<(), CorpusAnalysisArtifactError> {
    let expected_landing =
        landing_metric_from_canonical_routes(&room.canonical_route_difficulty.routes);
    if room.metric_aggregates.landing_precision != expected_landing {
        return Err(invalid(format!(
            "room {:?} landing metric aggregate differs from canonical route samples",
            room.room_id
        )));
    }
    let canonical = &room.metric_aggregates.canonical_routes;
    let loadout_cells = directed_route_count
        .checked_mul(EvaluationLoadout::ALL.len())
        .ok_or_else(|| invalid("canonical loadout-cell count overflow"))?;
    if canonical.directed_route_count != directed_route_count
        || canonical.loadout_route_cell_count != loadout_cells
        || canonical
            .positive_route_count
            .saturating_add(canonical.bounded_inconclusive_route_count)
            != loadout_cells
        || canonical.behavior_diversity.route_count != canonical.positive_route_count
    {
        return Err(invalid(format!(
            "room {:?} canonical metric denominators/counts differ",
            room.room_id
        )));
    }
    require_exact_loadout_order(
        "canonical aggregate loadouts",
        canonical.by_loadout.iter().map(|loadout| loadout.loadout),
    )?;
    let mut positive_sum = 0_usize;
    let mut inconclusive_sum = 0_usize;
    for loadout in &canonical.by_loadout {
        let retained_vectors = room
            .canonical_route_difficulty
            .routes
            .iter()
            .filter(|route| route.loadout == loadout.loadout)
            .count();
        if loadout.route_cell_count != directed_route_count
            || loadout
                .positive_route_count
                .saturating_add(loadout.bounded_inconclusive_route_count)
                != directed_route_count
            || loadout.positive_route_count != retained_vectors
            || loadout.behavior_diversity.route_count != loadout.positive_route_count
        {
            return Err(invalid(format!(
                "room {:?} {} canonical aggregate counts differ",
                room.room_id,
                loadout.loadout.slug()
            )));
        }
        validate_route_diversity(&loadout.behavior_diversity)?;
        positive_sum = positive_sum.saturating_add(loadout.positive_route_count);
        inconclusive_sum =
            inconclusive_sum.saturating_add(loadout.bounded_inconclusive_route_count);
    }
    if positive_sum != canonical.positive_route_count
        || inconclusive_sum != canonical.bounded_inconclusive_route_count
    {
        return Err(invalid(format!(
            "room {:?} canonical per-loadout/overall counts differ",
            room.room_id
        )));
    }
    validate_route_diversity(&canonical.behavior_diversity)?;

    let direct = &room.metric_aggregates.direct_controllers;
    if direct.directed_route_count != directed_route_count {
        return Err(invalid(format!(
            "room {:?} direct metric directed-route denominator differs",
            room.room_id
        )));
    }
    require_exact_loadout_order(
        "direct aggregate loadouts",
        direct.by_loadout.iter().map(|loadout| loadout.loadout),
    )?;
    for loadout in &direct.by_loadout {
        let matching_sources = room
            .direct_controller_evidence
            .source_audits
            .iter()
            .map(|source| {
                source
                    .shared_loadout_audits
                    .iter()
                    .find(|audit| audit.loadout == loadout.loadout)
                    .expect("every source contains every loadout")
            })
            .collect::<Vec<_>>();
        let source_complete = matching_sources
            .iter()
            .filter(|audit| matches!(audit.status, AuditStatusRecord::CompleteFiniteVocabulary))
            .count();
        let source_bounded = matching_sources.len() - source_complete;
        let matching_routes = room
            .direct_controller_evidence
            .directed_routes
            .iter()
            .map(|route| {
                route
                    .loadouts
                    .iter()
                    .find(|cell| cell.loadout == loadout.loadout)
                    .expect("every route contains every loadout")
            })
            .collect::<Vec<_>>();
        let route_complete = matching_routes
            .iter()
            .filter(|cell| matches!(cell.status, AuditStatusRecord::CompleteFiniteVocabulary))
            .count();
        let route_bounded = matching_routes.len() - route_complete;
        let known_positive = matching_routes
            .iter()
            .filter(|cell| cell.retained_semantic_witnesses > 0)
            .count();
        let complete_without_positive = matching_routes
            .iter()
            .filter(|cell| {
                cell.retained_semantic_witnesses == 0
                    && matches!(cell.status, AuditStatusRecord::CompleteFiniteVocabulary)
            })
            .count();
        let bounded_without_positive = matching_routes
            .iter()
            .filter(|cell| {
                cell.retained_semantic_witnesses == 0
                    && matches!(cell.status, AuditStatusRecord::BoundedIncomplete { .. })
            })
            .count();
        if loadout.source_audit_completeness.complete_finite_vocabulary != source_complete
            || loadout.source_audit_completeness.bounded_incomplete != source_bounded
            || loadout.route_audit_completeness.complete_finite_vocabulary != route_complete
            || loadout.route_audit_completeness.bounded_incomplete != route_bounded
            || loadout.known_positive_directed_routes != known_positive
            || loadout.no_positive_in_complete_finite_vocabulary != complete_without_positive
            || loadout.inconclusive_without_positive != bounded_without_positive
            || loadout.missing_route_or_audit != 0
        {
            return Err(invalid(format!(
                "room {:?} {} direct aggregate differs from retained route/source detail",
                room.room_id,
                loadout.loadout.slug()
            )));
        }
        validate_audit_counts(loadout.source_audit_completeness, room.door_ids.len())?;
        validate_audit_counts(loadout.route_audit_completeness, directed_route_count)?;
        if loadout
            .known_positive_directed_routes
            .saturating_add(loadout.no_positive_in_complete_finite_vocabulary)
            .saturating_add(loadout.inconclusive_without_positive)
            .saturating_add(loadout.missing_route_or_audit)
            != directed_route_count
        {
            return Err(invalid(format!(
                "room {:?} {} direct outcome counts do not match route denominator",
                room.room_id,
                loadout.loadout.slug()
            )));
        }
        let ambiguity_is_explicit = matches!(
            (
                &loadout.easiest_controller_fractions,
                &loadout.demand_coordinates,
            ),
            (
                MetricEvidenceRecord::NotApplicable {
                    reason: NotApplicableMetricReasonRecord::AmbiguousNondominatedFront,
                },
                MetricEvidenceRecord::NotApplicable {
                    reason: NotApplicableMetricReasonRecord::AmbiguousNondominatedFront,
                },
            )
        );
        if loadout.ambiguous_nondominated_front_directed_routes
            > loadout.known_positive_directed_routes
            || (loadout.ambiguous_nondominated_front_directed_routes > 0) != ambiguity_is_explicit
        {
            return Err(invalid(
                "ambiguous fused fronts are not preserved in direct-controller aggregates",
            ));
        }
        match &loadout.easiest_controller_fractions {
            MetricEvidenceRecord::Observed { value } => {
                if value.successful_route_count != loadout.known_positive_directed_routes {
                    return Err(invalid("easiest-controller fraction sample count differs"));
                }
                for fraction in [
                    value.run_only_class_fraction,
                    value.monotone_simple_class_fraction,
                    value.other_controller_class_fraction,
                    value.run_only_fraction,
                    value.monotone_simple_fraction,
                ] {
                    validate_fraction(fraction)?;
                    if fraction.denominator != value.successful_route_count {
                        return Err(invalid(
                            "easiest-controller fraction denominator differs from sample count",
                        ));
                    }
                }
            }
            MetricEvidenceRecord::Missing { .. } | MetricEvidenceRecord::NotApplicable { .. } => {}
        }
        match &loadout.demand_coordinates {
            MetricEvidenceRecord::Observed { value } => {
                validate_coordinate_summary(value, loadout.known_positive_directed_routes)?;
            }
            MetricEvidenceRecord::Missing { .. } | MetricEvidenceRecord::NotApplicable { .. } => {}
        }
    }
    let bypass = &direct.ability_bypasses;
    if bypass.directed_routes_with_any_bypass > directed_route_count
        || bypass.directed_routes_with_wall_jump_bypass > directed_route_count
        || bypass.directed_routes_with_dash_bypass > directed_route_count
    {
        return Err(invalid(
            "ability-bypass aggregate exceeds route denominator",
        ));
    }
    require_strict_order(
        "ability bypass successful loadouts",
        bypass
            .by_successful_loadout
            .iter()
            .map(|loadout| loadout.successful_loadout),
    )?;

    let expected_asymmetry = room
        .door_ids
        .len()
        .saturating_mul(room.door_ids.len() - 1)
        .saturating_div(2)
        .saturating_mul(EvaluationLoadout::ALL.len());
    let asymmetry = &room.metric_aggregates.directional_asymmetry;
    if asymmetry.len() != expected_asymmetry {
        return Err(invalid(format!(
            "room {:?} asymmetry row count {}/{} differs",
            room.room_id,
            asymmetry.len(),
            expected_asymmetry
        )));
    }
    require_strict_order(
        "directional asymmetry rows",
        asymmetry
            .iter()
            .map(|row| (&row.door_a, &row.door_b, row.loadout)),
    )?;
    for row in asymmetry {
        if row.door_a >= row.door_b
            || !room.door_ids.contains(&row.door_a)
            || !room.door_ids.contains(&row.door_b)
        {
            return Err(invalid("directional asymmetry door identity is invalid"));
        }
        validate_easiest_metric_evidence(&row.a_to_b)?;
        validate_easiest_metric_evidence(&row.b_to_a)?;
        if let MetricEvidenceRecord::Observed { value } = &row.comparison {
            validate_asymmetry_values(value)?;
        }
    }

    let terrain = &room.metric_aggregates.terrain;
    for fraction in [
        &terrain
            .coverage_fractions
            .structurally_attributed_components,
        &terrain.coverage_fractions.structurally_attributed_tiles,
        &terrain.coverage_fractions.traversal_near_components,
        &terrain.coverage_fractions.traversal_near_tiles,
        &terrain
            .coverage_fractions
            .positively_corroborated_components,
        &terrain.coverage_fractions.positively_corroborated_tiles,
        &terrain.aggregate_ablation_survival_fraction,
    ] {
        validate_fraction_evidence(fraction)?;
    }
    Ok(())
}

fn landing_metric_from_canonical_routes(
    routes: &[CanonicalRouteVectorRecord],
) -> LandingPrecisionMetricRecord {
    let source_landing_precision_versions = routes
        .iter()
        .map(|route| route.landing_precision.version)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    LandingPrecisionMetricRecord {
        source_landing_precision_versions,
        aggregate: landing_aggregate_from_route_records(
            routes.iter().map(|route| &route.landing_precision),
        ),
        by_loadout: EvaluationLoadout::ALL
            .into_iter()
            .map(|loadout| LandingPrecisionLoadoutRecord {
                loadout,
                aggregate: landing_aggregate_from_route_records(
                    routes
                        .iter()
                        .filter(move |route| route.loadout == loadout)
                        .map(|route| &route.landing_precision),
                ),
            })
            .collect(),
    }
}

fn landing_aggregate_from_route_records<'a>(
    reports: impl IntoIterator<Item = &'a LandingPrecisionRecord>,
) -> LandingPrecisionAggregateRecord {
    let reports = reports.into_iter().collect::<Vec<_>>();
    let canonical_positive_route_count = reports.len();
    let inspected_ticks = reports.iter().fold(0_usize, |total, report| {
        total.saturating_add(report.inspected_ticks)
    });
    let routes_with_landing_events = reports
        .iter()
        .filter(|report| report.landing_event_count > 0)
        .count();
    let routes_without_landing_events =
        canonical_positive_route_count.saturating_sub(routes_with_landing_events);
    let routes_with_measured_landings = reports
        .iter()
        .filter(|report| !report.samples.is_empty())
        .count();
    let routes_with_unmeasured_landings = reports
        .iter()
        .filter(|report| !report.unmeasured_landing_ticks.is_empty())
        .count();
    let routes_with_only_unmeasured_landings = reports
        .iter()
        .filter(|report| report.samples.is_empty() && !report.unmeasured_landing_ticks.is_empty())
        .count();
    let landing_event_count = reports.iter().fold(0_usize, |total, report| {
        total.saturating_add(report.landing_event_count)
    });
    let measured_landing_count = reports.iter().fold(0_usize, |total, report| {
        total.saturating_add(report.samples.len())
    });
    let unmeasured_landing_count = reports.iter().fold(0_usize, |total, report| {
        total.saturating_add(report.unmeasured_landing_ticks.len())
    });
    let samples = reports
        .iter()
        .flat_map(|report| report.samples.iter())
        .collect::<Vec<_>>();
    let unavailable_reason = if canonical_positive_route_count == 0 {
        LandingCoordinateNotApplicableReasonRecord::NoCanonicalPositiveRoutes
    } else if landing_event_count == 0 {
        LandingCoordinateNotApplicableReasonRecord::NoLandingEvents
    } else {
        LandingCoordinateNotApplicableReasonRecord::NoMeasuredLandings
    };
    let minimum_edge_margin_pixels = if samples.is_empty() {
        LandingCoordinateEvidenceRecord::NotApplicable {
            reason: unavailable_reason,
        }
    } else {
        LandingCoordinateEvidenceRecord::Observed {
            value: signed_distribution_record(samples.iter().map(|sample| {
                sample
                    .left_edge_margin_pixels
                    .min(sample.right_edge_margin_pixels)
            })),
        }
    };
    let footprint_overlap_pixels = if samples.is_empty() {
        LandingCoordinateEvidenceRecord::NotApplicable {
            reason: unavailable_reason,
        }
    } else {
        LandingCoordinateEvidenceRecord::Observed {
            value: unsigned_distribution_record(
                samples
                    .iter()
                    .map(|sample| sample.footprint_overlap_pixels as usize),
            ),
        }
    };
    let support_width_pixels = if samples.is_empty() {
        LandingCoordinateEvidenceRecord::NotApplicable {
            reason: unavailable_reason,
        }
    } else {
        LandingCoordinateEvidenceRecord::Observed {
            value: unsigned_distribution_record(samples.iter().map(|sample| {
                usize::try_from(sample.support_right.saturating_sub(sample.support_left))
                    .unwrap_or(usize::MAX)
            })),
        }
    };
    LandingPrecisionAggregateRecord {
        canonical_positive_route_count,
        inspected_ticks,
        routes_with_landing_events,
        routes_without_landing_events,
        routes_with_measured_landings,
        routes_with_unmeasured_landings,
        routes_with_only_unmeasured_landings,
        landing_event_count,
        measured_landing_count,
        unmeasured_landing_count,
        minimum_edge_margin_pixels,
        footprint_overlap_pixels,
        support_width_pixels,
        edge_overhang_landings: samples
            .iter()
            .filter(|sample| {
                sample
                    .left_edge_margin_pixels
                    .min(sample.right_edge_margin_pixels)
                    < 0
            })
            .count(),
        one_way_or_mixed_landings: samples
            .iter()
            .filter(|sample| sample.support_kind != LandingSupportKindRecord::Solid)
            .count(),
    }
}

fn unsigned_distribution_record(
    values: impl IntoIterator<Item = usize>,
) -> IntegerCoordinateDistributionRecord {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    let sample_count = values.len();
    let lower = (sample_count - 1) / 2;
    let upper = sample_count / 2;
    let minimum = values[0];
    let maximum = values[sample_count - 1];
    IntegerCoordinateDistributionRecord {
        sample_count,
        minimum,
        median_lower: values[lower],
        median_upper: values[upper],
        maximum,
        spread: maximum.saturating_sub(minimum),
    }
}

fn signed_distribution_record(
    values: impl IntoIterator<Item = i32>,
) -> SignedIntegerCoordinateDistributionRecord {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    let sample_count = values.len();
    let lower = (sample_count - 1) / 2;
    let upper = sample_count / 2;
    let minimum = values[0];
    let maximum = values[sample_count - 1];
    SignedIntegerCoordinateDistributionRecord {
        sample_count,
        minimum,
        median_lower: values[lower],
        median_upper: values[upper],
        maximum,
        spread: minimum.abs_diff(maximum),
    }
}

fn validate_route_diversity(
    diversity: &RouteDiversityRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    let comparisons = diversity
        .route_count
        .saturating_mul(diversity.route_count.saturating_sub(1))
        / 2;
    if diversity.reached_target_count > diversity.route_count
        || diversity.spatial_path_classes > diversity.route_count
        || diversity.semantic_controller_classes > diversity.route_count
        || diversity.joint_play_style_classes > diversity.route_count
    {
        return Err(invalid("route-diversity class counts exceed route count"));
    }
    for distance in [
        diversity.traversal_distance,
        diversity.semantic_action_distance,
        diversity.combined_behavior_distance,
    ] {
        if distance.comparisons != comparisons {
            return Err(invalid(
                "route-diversity pairwise comparison denominator differs",
            ));
        }
        let values = [
            distance.minimum,
            distance.mean,
            distance.median,
            distance.maximum,
            distance.mean_nearest_neighbor,
        ];
        if comparisons == 0 {
            if values.into_iter().any(|value| value.is_some()) {
                return Err(invalid(
                    "route-diversity distance is present without a route pair",
                ));
            }
        } else if values.into_iter().any(|value| {
            value.is_none_or(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        }) {
            return Err(invalid(
                "route-diversity distance is missing, non-finite, or outside [0,1]",
            ));
        }
    }
    Ok(())
}

fn validate_audit_counts(
    counts: AuditCompletenessCountsRecord,
    expected: usize,
) -> Result<(), CorpusAnalysisArtifactError> {
    if counts.expected != expected
        || counts
            .complete_finite_vocabulary
            .saturating_add(counts.bounded_incomplete)
            .saturating_add(counts.missing)
            != expected
    {
        return Err(invalid("audit completeness counts/denominator differ"));
    }
    let expected_state = if expected == 0 {
        AggregateAuditCompletenessRecord::NotApplicableNoAuditsExpected
    } else if counts.missing > 0 {
        AggregateAuditCompletenessRecord::Missing
    } else if counts.bounded_incomplete > 0 {
        AggregateAuditCompletenessRecord::BoundedIncomplete
    } else {
        AggregateAuditCompletenessRecord::CompleteFiniteVocabulary
    };
    if counts.state != expected_state {
        return Err(invalid("audit completeness state/counts differ"));
    }
    Ok(())
}

fn validate_coordinate_summary(
    summary: &ControllerDemandCoordinateSummaryRecord,
    sample_count: usize,
) -> Result<(), CorpusAnalysisArtifactError> {
    for distribution in [
        summary.controller_class,
        summary.ability_events,
        summary.horizontal_reversals,
        summary.vertical_decisions,
        summary.semantic_spans,
        summary.semantic_transitions,
        summary.duration_ticks,
    ] {
        if distribution.sample_count != sample_count
            || distribution.minimum > distribution.median_lower
            || distribution.median_lower > distribution.median_upper
            || distribution.median_upper > distribution.maximum
            || distribution.spread != distribution.maximum - distribution.minimum
        {
            return Err(invalid(
                "controller-demand coordinate distribution is inconsistent",
            ));
        }
    }
    Ok(())
}

fn validate_easiest_metric_evidence(
    evidence: &MetricEvidenceRecord<EasiestKnownControllerRecord>,
) -> Result<(), CorpusAnalysisArtifactError> {
    if let MetricEvidenceRecord::Observed { value } = evidence
        && value.coordinates != coordinates_from_demand(value.demand)
    {
        return Err(invalid(
            "asymmetry easiest-known metric demand/coordinates differ",
        ));
    }
    Ok(())
}

fn validate_asymmetry_values(
    values: &DirectionalAsymmetryValuesRecord,
) -> Result<(), CorpusAnalysisArtifactError> {
    for difference in [
        values.duration_ticks,
        values.semantic_spans,
        values.semantic_transitions,
        values.ability_events,
    ] {
        let expected_direction = match difference.a_to_b.cmp(&difference.b_to_a) {
            std::cmp::Ordering::Less => LargerDirectionRecord::BToA,
            std::cmp::Ordering::Equal => LargerDirectionRecord::Equal,
            std::cmp::Ordering::Greater => LargerDirectionRecord::AToB,
        };
        if difference.absolute_difference != difference.a_to_b.abs_diff(difference.b_to_a)
            || difference.larger_direction != expected_direction
        {
            return Err(invalid("directional asymmetry difference is inconsistent"));
        }
    }
    let ability = values.ability_use;
    if ability.wall_jump_use_differs
        != ((ability.a_to_b.wall_jump_events > 0) != (ability.b_to_a.wall_jump_events > 0))
        || ability.dash_use_differs
            != ((ability.a_to_b.dash_events > 0) != (ability.b_to_a.dash_events > 0))
    {
        return Err(invalid(
            "directional asymmetry ability-use flags are inconsistent",
        ));
    }
    Ok(())
}

fn validate_fraction_evidence(
    evidence: &MetricEvidenceRecord<ExactFractionRecord>,
) -> Result<(), CorpusAnalysisArtifactError> {
    if let MetricEvidenceRecord::Observed { value } = evidence {
        validate_fraction(*value)?;
    }
    Ok(())
}

fn validate_fraction(fraction: ExactFractionRecord) -> Result<(), CorpusAnalysisArtifactError> {
    if fraction.denominator == 0 || fraction.numerator > fraction.denominator {
        return Err(invalid(format!(
            "invalid exact fraction {}/{}",
            fraction.numerator, fraction.denominator
        )));
    }
    Ok(())
}

fn regenerate_and_replay_room(
    room: &RoomRecord,
    witnesses: &[ControllerWitnessRecord],
) -> Result<(), CorpusAnalysisArtifactError> {
    let key = staged_key(&room.generation_key)?;
    let candidate = generate_staged_compositional(key).map_err(|source| {
        invalid(format!(
            "room {:?} generation key did not regenerate: {source}",
            room.room_id
        ))
    })?;
    if CandidateKeyRecord::from_staged_key(candidate.key) != room.generation_key {
        return Err(invalid(format!(
            "room {:?} regenerated candidate key differs",
            room.room_id
        )));
    }
    let static_visual = StaticVisualDescriptor::from_room(&candidate.generated.room);
    let simulation_geometry = SimulationGeometryDescriptor::from_room(&candidate.generated.room);
    let expected_room_id = format!(
        "room-v{CORPUS_ROOM_ID_VERSION}-{:016x}-{:016x}-{}",
        fingerprint_static_visual(&static_visual),
        simulation_geometry.stable_digest(),
        room.generation_key.stable_slug(),
    );
    if room.room_id != expected_room_id {
        return Err(invalid(format!(
            "room identity differs from regeneration: artifact {:?}, regenerated {:?}",
            room.room_id, expected_room_id
        )));
    }
    let mut regenerated_door_ids = candidate
        .generated
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    regenerated_door_ids.sort_unstable();
    if regenerated_door_ids != room.door_ids {
        return Err(invalid(format!(
            "room {:?} door identities differ from regeneration",
            room.room_id
        )));
    }

    for witness in witnesses
        .iter()
        .filter(|witness| witness.room_id == room.room_id)
    {
        let initial = Simulation::enter_via_door(
            candidate.generated.room.clone(),
            witness.loadout.abilities(),
            &witness.source_door_id,
        )
        .map_err(|source| {
            invalid(format!(
                "cannot recreate source arrival for witness {:?}: {source}",
                witness.witness_id
            ))
        })?;
        let actions = witness
            .actions
            .iter()
            .flat_map(|span| std::iter::repeat_n(span.action(), span.ticks));
        let replay = Replay::record(&initial, actions);
        if replay.initial_digest.to_string() != witness.initial_state_digest
            || replay.frames.len() != witness.total_ticks
        {
            return Err(invalid(format!(
                "witness {:?} initial digest/tick count differs on regeneration",
                witness.witness_id
            )));
        }
        let terminal_state = replay
            .frames
            .last()
            .map_or(replay.initial_digest.to_string(), |frame| {
                frame.expected_digest.to_string()
            });
        let terminal_event = replay
            .frames
            .last()
            .map(|frame| frame.expected_event_digest.to_string());
        if terminal_state != witness.terminal_state_digest
            || terminal_event != witness.terminal_event_digest
        {
            return Err(invalid(format!(
                "witness {:?} terminal state/event digest differs on regeneration",
                witness.witness_id
            )));
        }
        let verification = replay.verify(&initial).map_err(|source| {
            invalid(format!(
                "witness {:?} exact replay diverged: {source}",
                witness.witness_id
            ))
        })?;
        if verification.frames_verified != witness.total_ticks
            || verification.reached_exit.as_deref() != Some(witness.target_door_id.as_str())
        {
            return Err(invalid(format!(
                "witness {:?} replay reached {:?} after {}/{} frames, expected target {:?}",
                witness.witness_id,
                verification.reached_exit,
                verification.frames_verified,
                witness.total_ticks,
                witness.target_door_id
            )));
        }
    }
    Ok(())
}

fn staged_key(
    record: &CandidateKeyRecord,
) -> Result<StagedCompositionalKey, CorpusAnalysisArtifactError> {
    let Some(strategy) = GenerationStrategy::ALL
        .into_iter()
        .find(|strategy| strategy.slug() == record.strategy)
    else {
        return Err(invalid(format!(
            "unknown generation strategy {:?}",
            record.strategy
        )));
    };
    let Some(intent) = ChallengeIntent::ALL
        .into_iter()
        .find(|intent| intent.slug() == record.intent)
    else {
        return Err(invalid(format!(
            "unknown generation intent {:?}",
            record.intent
        )));
    };
    let key = StagedCompositionalKey::new(
        CompositionalKey::new(
            record.seed,
            CompositionalProfile::new(record.construction_loadout.abilities(), strategy, intent),
        ),
        match record.feature_stage {
            FeatureStageRecord::TerrainOnly => downwards_gen::CompositionalFeatureSet::TerrainOnly,
            FeatureStageRecord::StaticHazards => {
                downwards_gen::CompositionalFeatureSet::StaticHazards
            }
            FeatureStageRecord::TimedHazards => {
                downwards_gen::CompositionalFeatureSet::TimedHazards
            }
        },
    );
    if CandidateKeyRecord::from_staged_key(key) != *record {
        return Err(invalid("generation key is not in canonical form"));
    }
    Ok(key)
}

fn require_strict_order<T: Ord>(
    label: &str,
    values: impl IntoIterator<Item = T>,
) -> Result<(), CorpusAnalysisArtifactError> {
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

#[derive(Debug)]
pub enum CorpusAnalysisArtifactError {
    Json(serde_json::Error),
    JsonAt {
        file: String,
        line: usize,
        source: serde_json::Error,
    },
    Invalid(String),
}

fn invalid(detail: impl Into<String>) -> CorpusAnalysisArtifactError {
    CorpusAnalysisArtifactError::Invalid(detail.into())
}

impl fmt::Display for CorpusAnalysisArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(source) => write!(formatter, "analysis artifact JSON failed: {source}"),
            Self::JsonAt { file, line, source } => write!(
                formatter,
                "analysis artifact JSON failed in {file} line {line}: {source}"
            ),
            Self::Invalid(detail) => write!(formatter, "invalid analysis artifact: {detail}"),
        }
    }
}

impl Error for CorpusAnalysisArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(source) | Self::JsonAt { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<serde_json::Error> for CorpusAnalysisArtifactError {
    fn from(source: serde_json::Error) -> Self {
        Self::Json(source)
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{DifficultyConfig, SolverConfig};

    use super::*;
    use crate::corpus::{
        CorpusBuildConfigV1, analyze_corpus_room, evaluate_route_matrices, generate_seed_block,
        summarize_room_metrics,
    };

    fn real_room_artifact() -> CorpusAnalysisArtifactBundle {
        let mut generated =
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        generated.rooms.truncate(1);
        let generation_key =
            CandidateKeyRecord::from_staged_key(generated.rooms[0].variants[0].key);
        let evaluated = evaluate_route_matrices(generated)
            .unwrap()
            .rooms
            .pop()
            .unwrap();
        let config = CorpusRoomAnalysisConfig {
            direct_controller_solver: SolverConfig {
                max_expanded_nodes: 10_000,
                max_simulated_ticks: 2_000_000,
                max_ticks_per_path: 240,
                ..SolverConfig::default()
            },
            canonical_witness_difficulty: DifficultyConfig::default(),
        };
        let analysis = analyze_corpus_room(&evaluated, &config).unwrap();
        let metrics = summarize_room_metrics(&analysis).unwrap();
        let input = CorpusAnalysisArtifactRoomRef {
            generation_key: &generation_key,
            room_id: &analysis.room_id,
            analysis: &analysis,
            metrics: &metrics,
        };
        let first = render_seed_analysis_artifact(0, &config, [input]).unwrap();
        let second = render_seed_analysis_artifact(0, &config, [input]).unwrap();
        assert_eq!(first, second);
        first
    }

    fn decoded_bundle(
        bundle: &CorpusAnalysisArtifactBundle,
    ) -> (
        ManifestRecord,
        Vec<RoomRecord>,
        Vec<ControllerWitnessRecord>,
    ) {
        let manifest =
            parse_canonical_jsonl::<ManifestRecord>("manifest.json", &bundle.manifest_json)
                .unwrap()
                .pop()
                .unwrap();
        let rooms = parse_canonical_jsonl("rooms.jsonl", &bundle.rooms_jsonl).unwrap();
        let witnesses = parse_canonical_jsonl(
            "controller_witnesses.jsonl",
            &bundle.controller_witnesses_jsonl,
        )
        .unwrap();
        (manifest, rooms, witnesses)
    }

    fn rerender_decoded(
        mut manifest: ManifestRecord,
        rooms: &[RoomRecord],
        witnesses: &[ControllerWitnessRecord],
    ) -> CorpusAnalysisArtifactBundle {
        let rooms_jsonl = render_json_lines(rooms).unwrap();
        let controller_witnesses_jsonl = render_json_lines(witnesses).unwrap();
        manifest.stream_hashes = BTreeMap::from([
            (
                "controller_witnesses.jsonl".to_owned(),
                byte_hash(&controller_witnesses_jsonl),
            ),
            ("rooms.jsonl".to_owned(), byte_hash(&rooms_jsonl)),
        ]);
        CorpusAnalysisArtifactBundle {
            manifest_json: render_single_json(&manifest).unwrap(),
            rooms_jsonl,
            controller_witnesses_jsonl,
        }
    }

    fn replace_witness_id(rooms: &mut [RoomRecord], old: &str, new: &str) {
        for room in rooms {
            for route in &mut room.direct_controller_evidence.directed_routes {
                if route.overall_easiest_known_witness_id.as_deref() == Some(old) {
                    route.overall_easiest_known_witness_id = Some(new.to_owned());
                }
                for id in &mut route.overall_pareto_front_witness_ids {
                    if id == old {
                        *id = new.to_owned();
                    }
                }
                for loadout in &mut route.loadouts {
                    if loadout.easiest_known_witness_id.as_deref() == Some(old) {
                        loadout.easiest_known_witness_id = Some(new.to_owned());
                    }
                    for id in &mut loadout.pareto_front_witness_ids {
                        if id == old {
                            *id = new.to_owned();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn real_room_artifact_is_canonical_corruption_closed_and_replay_verified() {
        let artifact = real_room_artifact();
        let verified = parse_and_verify_seed_analysis_artifact(&artifact).unwrap();
        assert_eq!(verified.seed, 0);
        assert_eq!(verified.rooms, 1);
        assert!(verified.retained_controller_witnesses > 0);
        assert!(verified.canonical_route_vectors > 0);
        let (manifest, rooms, _) = decoded_bundle(&artifact);
        assert_eq!(manifest.artifact_version, 3);
        assert_eq!(rooms[0].row_version, 3);
        assert!(artifact.manifest_json.ends_with(b"\n"));
        assert!(artifact.rooms_jsonl.ends_with(b"\n"));
        assert!(artifact.controller_witnesses_jsonl.ends_with(b"\n"));

        let text = String::from_utf8(
            [
                artifact.manifest_json.as_slice(),
                artifact.rooms_jsonl.as_slice(),
                artifact.controller_witnesses_jsonl.as_slice(),
            ]
            .concat(),
        )
        .unwrap();
        assert!(text.contains(CANONICAL_ROUTE_CLAIM));
        assert!(text.contains(LANDING_PRECISION_DISCLAIMER));
        assert!(!text.contains("\"status\":\"unreachable\""));
        assert!(!text.contains("\"kind\":\"unreachable\""));
        assert!(!text.contains("\"availability\":\"unreachable\""));
        assert!(!text.contains("\"band\""));
        assert!(!text.contains("\"fun\""));

        let mut noncanonical = artifact.clone();
        noncanonical.manifest_json.insert(0, b' ');
        assert!(parse_and_verify_seed_analysis_artifact(&noncanonical).is_err());

        // Even after rebinding the stream hash and witness content identity,
        // non-normal RLE is rejected before any replay is attempted.
        let (manifest, mut rooms, mut witnesses) = decoded_bundle(&artifact);
        let old_id = witnesses[0].witness_id.clone();
        let duplicate = witnesses[0].actions[0];
        witnesses[0].actions.insert(1, duplicate);
        witnesses[0].total_ticks += duplicate.ticks;
        witnesses[0].demand.duration_ticks += duplicate.ticks;
        witnesses[0].coordinates.duration_ticks += duplicate.ticks;
        witnesses[0].witness_id = controller_witness_id(&witnesses[0]).unwrap();
        let new_id = witnesses[0].witness_id.clone();
        replace_witness_id(&mut rooms, &old_id, &new_id);
        witnesses.sort_unstable_by(|left, right| left.witness_id.cmp(&right.witness_id));
        let malformed_rle = rerender_decoded(manifest, &rooms, &witnesses);
        assert!(parse_and_verify_seed_analysis_artifact(&malformed_rle).is_err());

        // Landing coordinates are independently self-consistent evidence,
        // not unchecked decoration behind the stream checksum.
        let (manifest, mut rooms, witnesses) = decoded_bundle(&artifact);
        let landing = &mut rooms[0].canonical_route_difficulty.routes[0].landing_precision;
        landing.edge_overhang_landings = landing.edge_overhang_landings.saturating_add(1);
        let malformed_landing = rerender_decoded(manifest, &rooms, &witnesses);
        assert!(parse_and_verify_seed_analysis_artifact(&malformed_landing).is_err());

        // Room/loadout landing summaries are recomputed from the route rows;
        // rebinding the stream checksum cannot bless a forged aggregate.
        let (manifest, mut rooms, witnesses) = decoded_bundle(&artifact);
        rooms[0]
            .metric_aggregates
            .landing_precision
            .aggregate
            .landing_event_count = rooms[0]
            .metric_aggregates
            .landing_precision
            .aggregate
            .landing_event_count
            .saturating_add(1);
        let malformed_landing_aggregate = rerender_decoded(manifest, &rooms, &witnesses);
        assert!(parse_and_verify_seed_analysis_artifact(&malformed_landing_aggregate).is_err());

        // A normalized action corruption with internally updated identity and
        // cross-references passes structural checks but fails exact replay on
        // the regenerated room (terminal digest and/or target differs).
        let (manifest, mut rooms, mut witnesses) = decoded_bundle(&artifact);
        let old_id = witnesses[0].witness_id.clone();
        let first = &mut witnesses[0].actions[0];
        first.move_x = if first.move_x == 1 { -1 } else { 1 };
        if witnesses[0].actions.len() > 1
            && witnesses[0].actions[0].same_action(witnesses[0].actions[1])
        {
            let ticks = witnesses[0].actions.remove(1).ticks;
            witnesses[0].actions[0].ticks += ticks;
        }
        witnesses[0].witness_id = controller_witness_id(&witnesses[0]).unwrap();
        let new_id = witnesses[0].witness_id.clone();
        replace_witness_id(&mut rooms, &old_id, &new_id);
        witnesses.sort_unstable_by(|left, right| left.witness_id.cmp(&right.witness_id));
        let replay_corruption = rerender_decoded(manifest, &rooms, &witnesses);
        assert!(parse_and_verify_seed_analysis_artifact(&replay_corruption).is_err());
    }
}
