//! Immutable, independently verifiable per-seed artifacts for the
//! generator-neutral corpus path.
//!
//! This schema is intentionally additive. It neither reads nor writes the
//! historical staged-v6 corpus artifact/checkpoint formats.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::cell::{Cell, RefCell};

use downwards_ai::{
    DirectProbeBudgetLimit, DirectProbePolicy, DirectProbeProvenance, InconclusiveReason,
    ReachedTarget, Replay, SearchStats, SearchTarget, TargetSolution,
};
use downwards_core::{Action, BoundarySide, DashDirection, Simulation, StateDigest, WallSide};
use downwards_gen::experimental::{
    AbilityEdgeRewriteProvenance, AbilityGateEmbeddingContract, AbilityGateEmbeddingPendingReason,
    AbilityGateEmbeddingState, AbilityGateGeometry, AbilityGateRealization, AbilityGateTileCell,
    AbilityRewrittenMission, AbilityRewrittenMissionPlan, BoundaryPort,
    CompositionalAbilityEdgeRewriteKey, CompositionalAbilityEmbeddingSeam,
    CompositionalAbilityEmbeddingSummary, CompositionalRouteCutEmbeddingSummary, CutAnchor,
    DerivedMission, DirectedAbilityGate, DirectedMissionEdge, DirectedTraversalRequirement,
    GateAbility, GateUnavoidabilityCertificate, MissionCut, MissionEdge, MissionEdgeKind,
    MissionFork, MissionNode, MissionNodeKind, MissionPlan, MissionPort, MissionProvenance,
    MissionRewrite, MissionRouteNodeMapping, NodeRole, PartitionDerivation,
    PartitionDerivationBeat, PartitionDerivationSplit, PartitionRouteSummary, PartitionSplitAxis,
    RouteCutRealization, RouteEdge, RouteNode, RoutePlan, RoutePlanSummary, RouteVerb, SupportKind,
    SupportSpec,
};
use downwards_lab::{
    ActionSpan as SemanticActionSpan, DescriptorDoor, DescriptorPoint, DescriptorRect,
    SemanticAction, SemanticActionTrace, SemanticEvent, SemanticEventAt, SimulationDoorGeometry,
    SimulationGeometryDescriptor, SimulationTimedHazardGeometry, StaticVisualDescriptor,
    VisualTile,
};
use downwards_validation::{
    BoundedTargetEvidence, DoorReachabilityObjective, PickupFromDoorObjective,
    RecordedSearchObservation, fingerprint_door_witness, fingerprint_pickup_from_door_witness,
    validate_recorded_source_search_observations,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    AbilityPromotionBudgetLimitV2, AbilityPromotionClaimV2, AbilityPromotionDecisionV2,
    AbilityPromotionDirectAuditStateV2, AbilityPromotionIntendedEvidenceSourceV2,
    AbilityPromotionIntendedStateV2, AbilityPromotionMatrixStateV2,
    AbilityPromotionMissingLoadoutEvidenceV2, BoundedIncompleteLoadout,
    CORPUS_ABILITY_PROMOTION_GATE_VERSION, CORPUS_CANDIDATE_KEY_RECORD_VERSION,
    CORPUS_CANONICAL_REGENERATION_POLICY_VERSION, CORPUS_FEASIBILITY_GATE_VERSION,
    CORPUS_PHYSICAL_ROOM_ID_VERSION, CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION,
    CanonicalRegenerationPolicy, CompositionalAbilityGateProfileRecord,
    CompositionalRouteCutKeyRecord, ControllerDemand, CorpusBuildConfigV2, CorpusCandidate,
    CorpusCandidateKeyRecord, CorpusConstructionAttemptV2, CorpusConstructionFailureClassV2,
    CorpusConstructionOutcomeV2, CorpusFeasibilityGateState, CorpusRoomAnalysisConfig,
    CorpusRoomAnalysisConfigRecord, EvaluatedCorpusBatchV2, EvaluationLoadout,
    GenerationBatchSummaryV2, LoadoutControllerAudit, LoadoutControllerAuditStatus,
    LoadoutRouteMatrix, PositiveBypassEvidence, RoomId, RouteControllerAssessment,
    RouteControllerAssessmentPolicy, RouteControllerAuditCompleteness, RouteControllerWitness,
    RouteEvaluationConfigV2, VariantAbilityPromotionEvidenceV2, VariantAbilityPromotionGateV2,
    VariantConstructionGateV2, generate_seed_block_v2,
    rerun_validate_variant_ability_promotion_gate_v2, select_canonical_regeneration_v2,
    validate_evaluated_corpus_room_v2, validate_route_evaluation_configs_v2,
};

/// Version of this final-path evidence schema. It is unrelated to the
/// historical `CORPUS_ARTIFACT_VERSION`.
pub const CORPUS_ARTIFACT_V3_SCHEMA_VERSION: u32 = 3;
pub const CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION: u32 = 1;
pub const CORPUS_ARTIFACT_V3_SEARCH_OBSERVATION_POLICY_VERSION: u32 = 1;
pub const CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION: u32 = 1;
pub const CORPUS_ARTIFACT_V3_CHECKPOINT_FILE: &str = "corpus-v3-checkpoint.json";

const MAX_VERIFIABLE_WITNESS_TICKS_V3: usize = 10_000;
const STREAM_NAMES: [&str; 5] = [
    "corpus-v3-attempts.jsonl",
    "corpus-v3-rooms.jsonl",
    "corpus-v3-routes.jsonl",
    "corpus-v3-pickups.jsonl",
    "corpus-v3-witnesses.jsonl",
];
const FILE_NAMES: [&str; 6] = [
    "corpus-v3-run.json",
    STREAM_NAMES[0],
    STREAM_NAMES[1],
    STREAM_NAMES[2],
    STREAM_NAMES[3],
    STREAM_NAMES[4],
];

/// Route-plan waypoint rescue evidence is deliberately absent from schema
/// v3. Its producer API is not yet stable; a future stream can add it without
/// changing the meaning of these construction and cheap-matrix records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaypointRescueEvidencePolicyV3 {
    OmittedUntilProducerApiIsStable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusArtifactBundleV3 {
    pub run_json: Vec<u8>,
    pub attempts_jsonl: Vec<u8>,
    pub rooms_jsonl: Vec<u8>,
    pub routes_jsonl: Vec<u8>,
    pub pickups_jsonl: Vec<u8>,
    pub witnesses_jsonl: Vec<u8>,
}

impl CorpusArtifactBundleV3 {
    fn streams(&self) -> [(&'static str, &[u8]); 5] {
        [
            (STREAM_NAMES[0], &self.attempts_jsonl),
            (STREAM_NAMES[1], &self.rooms_jsonl),
            (STREAM_NAMES[2], &self.routes_jsonl),
            (STREAM_NAMES[3], &self.pickups_jsonl),
            (STREAM_NAMES[4], &self.witnesses_jsonl),
        ]
    }

    fn files(&self) -> [(&'static str, &[u8]); 6] {
        [
            (FILE_NAMES[0], &self.run_json),
            (FILE_NAMES[1], &self.attempts_jsonl),
            (FILE_NAMES[2], &self.rooms_jsonl),
            (FILE_NAMES[3], &self.routes_jsonl),
            (FILE_NAMES[4], &self.pickups_jsonl),
            (FILE_NAMES[5], &self.witnesses_jsonl),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactPolicyRecordV3 {
    candidate_key_record_version: u32,
    physical_room_id_version: u32,
    feasibility_gate_version: u32,
    canonical_regeneration_policy_version: u32,
    route_evaluation_config_id_version: u32,
    ability_promotion_gate_version: u32,
    ability_promotion_audit_config_id: String,
    action_encoding_version: u32,
    search_observation_policy_version: u32,
    waypoint_rescue_evidence: WaypointRescueEvidencePolicyV3,
}

impl ArtifactPolicyRecordV3 {
    fn current(ability_promotion_audit_config: &CorpusRoomAnalysisConfigRecord) -> Self {
        Self {
            candidate_key_record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            physical_room_id_version: CORPUS_PHYSICAL_ROOM_ID_VERSION,
            feasibility_gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
            canonical_regeneration_policy_version: CORPUS_CANONICAL_REGENERATION_POLICY_VERSION,
            route_evaluation_config_id_version: CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION,
            ability_promotion_gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
            ability_promotion_audit_config_id: ability_promotion_audit_config.config_id.clone(),
            action_encoding_version: CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION,
            search_observation_policy_version: CORPUS_ARTIFACT_V3_SEARCH_OBSERVATION_POLICY_VERSION,
            waypoint_rescue_evidence:
                WaypointRescueEvidencePolicyV3::OmittedUntilProducerApiIsStable,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusArtifactSummaryV3 {
    pub attempts: usize,
    pub constructed_attempts: usize,
    pub rejected_attempts: usize,
    pub physical_rooms: usize,
    pub native_variants: usize,
    pub alias_candidates: usize,
    pub route_rows: usize,
    pub positive_route_rows: usize,
    pub inconclusive_route_rows: usize,
    pub pickup_rows: usize,
    pub positive_pickup_rows: usize,
    pub inconclusive_pickup_rows: usize,
    pub positive_witnesses: usize,
    pub passing_variant_construction_gates: usize,
    pub ability_aliases: usize,
    pub promoted_ability_aliases: usize,
    pub unpromoted_ability_aliases: usize,
    pub passing_complete_kit_rooms: usize,
    pub rooms_with_canonical_regeneration: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRecordV3 {
    artifact_schema: String,
    artifact_version: u32,
    status: String,
    config: CorpusBuildConfigV2,
    config_id: String,
    evaluation_configs: Vec<RouteEvaluationConfigV2>,
    ability_promotion_audit_config: CorpusRoomAnalysisConfigRecord,
    policies: ArtifactPolicyRecordV3,
    generation: GenerationBatchSummaryV2,
    summary: CorpusArtifactSummaryV3,
    stream_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum StrictConstructionOutcomeV3 {
    Constructed {
        room_id: RoomId,
        static_visual_fingerprint: String,
        simulation_geometry_fingerprint: String,
        port_count: usize,
        pickup_count: usize,
    },
    Rejected {
        generator: super::CorpusCandidateGenerator,
        failure_class: CorpusConstructionFailureClassV2,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptRecordV3 {
    key: CorpusCandidateKeyRecord,
    outcome: StrictConstructionOutcomeV3,
}

impl From<&CorpusConstructionAttemptV2> for AttemptRecordV3 {
    fn from(attempt: &CorpusConstructionAttemptV2) -> Self {
        let outcome = match &attempt.outcome {
            CorpusConstructionOutcomeV2::Constructed {
                room_id,
                static_visual_fingerprint,
                simulation_geometry_fingerprint,
                port_count,
                pickup_count,
            } => StrictConstructionOutcomeV3::Constructed {
                room_id: room_id.clone(),
                static_visual_fingerprint: static_visual_fingerprint.clone(),
                simulation_geometry_fingerprint: simulation_geometry_fingerprint.clone(),
                port_count: *port_count,
                pickup_count: *pickup_count,
            },
            CorpusConstructionOutcomeV2::Rejected {
                generator,
                failure_class,
                detail,
            } => StrictConstructionOutcomeV3::Rejected {
                generator: *generator,
                failure_class: *failure_class,
                detail: detail.clone(),
            },
        };
        Self {
            key: attempt.key.clone(),
            outcome,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BoundarySideRecordV3 {
    Left,
    Right,
    Ceiling,
    Floor,
}

impl From<BoundarySide> for BoundarySideRecordV3 {
    fn from(side: BoundarySide) -> Self {
        match side {
            BoundarySide::Left => Self::Left,
            BoundarySide::Right => Self::Right,
            BoundarySide::Ceiling => Self::Ceiling,
            BoundarySide::Floor => Self::Floor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PointRecordV3 {
    x: i32,
    y: i32,
}

impl From<DescriptorPoint> for PointRecordV3 {
    fn from(point: DescriptorPoint) -> Self {
        Self {
            x: point.x,
            y: point.y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RectRecordV3 {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl From<DescriptorRect> for RectRecordV3 {
    fn from(bounds: DescriptorRect) -> Self {
        Self {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
    }
}

impl From<downwards_core::Point> for PointRecordV3 {
    fn from(point: downwards_core::Point) -> Self {
        Self {
            x: point.x,
            y: point.y,
        }
    }
}

impl From<downwards_core::Rect> for RectRecordV3 {
    fn from(bounds: downwards_core::Rect) -> Self {
        Self {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum VisualTileRecordV3 {
    Empty,
    Solid,
    Hazard,
    OneWay,
}

impl From<VisualTile> for VisualTileRecordV3 {
    fn from(tile: VisualTile) -> Self {
        match tile {
            VisualTile::Empty => Self::Empty,
            VisualTile::Solid => Self::Solid,
            VisualTile::Hazard => Self::Hazard,
            VisualTile::OneWay => Self::OneWay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorDoorRecordV3 {
    side: BoundarySideRecordV3,
    trigger_bounds: RectRecordV3,
}

impl From<DescriptorDoor> for DescriptorDoorRecordV3 {
    fn from(door: DescriptorDoor) -> Self {
        Self {
            side: door.side.into(),
            trigger_bounds: door.trigger_bounds.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticVisualDescriptorRecordV3 {
    version: u32,
    width: u16,
    height: u16,
    tile_size: i32,
    spawn: PointRecordV3,
    tiles: Vec<VisualTileRecordV3>,
    exits: Vec<RectRecordV3>,
    doors: Vec<DescriptorDoorRecordV3>,
    pickups: Vec<RectRecordV3>,
    timed_hazards: Vec<RectRecordV3>,
}

impl From<&StaticVisualDescriptor> for StaticVisualDescriptorRecordV3 {
    fn from(descriptor: &StaticVisualDescriptor) -> Self {
        Self {
            version: descriptor.version,
            width: descriptor.width,
            height: descriptor.height,
            tile_size: descriptor.tile_size,
            spawn: descriptor.spawn.into(),
            tiles: descriptor.tiles.iter().copied().map(Into::into).collect(),
            exits: descriptor.exits.iter().copied().map(Into::into).collect(),
            doors: descriptor.doors.iter().copied().map(Into::into).collect(),
            pickups: descriptor.pickups.iter().copied().map(Into::into).collect(),
            timed_hazards: descriptor
                .timed_hazards
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimulationDoorRecordV3 {
    side: BoundarySideRecordV3,
    trigger_bounds: RectRecordV3,
    arrival: PointRecordV3,
}

impl From<SimulationDoorGeometry> for SimulationDoorRecordV3 {
    fn from(door: SimulationDoorGeometry) -> Self {
        Self {
            side: door.side.into(),
            trigger_bounds: door.trigger_bounds.into(),
            arrival: door.arrival.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimulationTimedHazardRecordV3 {
    bounds: RectRecordV3,
    period_ticks: u32,
    active_ticks: u32,
    phase_ticks: u32,
}

impl From<SimulationTimedHazardGeometry> for SimulationTimedHazardRecordV3 {
    fn from(hazard: SimulationTimedHazardGeometry) -> Self {
        Self {
            bounds: hazard.bounds.into(),
            period_ticks: hazard.period_ticks,
            active_ticks: hazard.active_ticks,
            phase_ticks: hazard.phase_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimulationGeometryDescriptorRecordV3 {
    version: u32,
    width: u16,
    height: u16,
    tile_size: i32,
    spawn: PointRecordV3,
    tiles: Vec<VisualTileRecordV3>,
    exits: Vec<RectRecordV3>,
    doors: Vec<SimulationDoorRecordV3>,
    pickups: Vec<RectRecordV3>,
    timed_hazards: Vec<SimulationTimedHazardRecordV3>,
}

impl From<&SimulationGeometryDescriptor> for SimulationGeometryDescriptorRecordV3 {
    fn from(descriptor: &SimulationGeometryDescriptor) -> Self {
        Self {
            version: descriptor.version,
            width: descriptor.width,
            height: descriptor.height,
            tile_size: descriptor.tile_size,
            spawn: descriptor.spawn.into(),
            tiles: descriptor.tiles.iter().copied().map(Into::into).collect(),
            exits: descriptor.exits.iter().copied().map(Into::into).collect(),
            doors: descriptor.doors.iter().copied().map(Into::into).collect(),
            pickups: descriptor.pickups.iter().copied().map(Into::into).collect(),
            timed_hazards: descriptor
                .timed_hazards
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicalRoomDescriptorRecordV3 {
    static_visual: StaticVisualDescriptorRecordV3,
    simulation_geometry: SimulationGeometryDescriptorRecordV3,
}

impl From<&super::CorpusPhysicalRoomDescriptorV3> for PhysicalRoomDescriptorRecordV3 {
    fn from(descriptor: &super::CorpusPhysicalRoomDescriptorV3) -> Self {
        Self {
            static_visual: (&descriptor.static_visual).into(),
            simulation_geometry: (&descriptor.simulation_geometry).into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum NodeRoleRecordV3 {
    Port,
    Start,
    Exit,
    Landing,
    Junction,
    Pickup,
    Recovery,
}

impl From<NodeRole> for NodeRoleRecordV3 {
    fn from(role: NodeRole) -> Self {
        match role {
            NodeRole::Port => Self::Port,
            NodeRole::Start => Self::Start,
            NodeRole::Exit => Self::Exit,
            NodeRole::Landing => Self::Landing,
            NodeRole::Junction => Self::Junction,
            NodeRole::Pickup => Self::Pickup,
            NodeRole::Recovery => Self::Recovery,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RouteVerbRecordV3 {
    Run,
    Jump,
    Drop,
    WallClimb,
    DashAcross,
    DashUp,
}

impl From<RouteVerb> for RouteVerbRecordV3 {
    fn from(verb: RouteVerb) -> Self {
        match verb {
            RouteVerb::Run => Self::Run,
            RouteVerb::Jump => Self::Jump,
            RouteVerb::Drop => Self::Drop,
            RouteVerb::WallClimb => Self::WallClimb,
            RouteVerb::DashAcross => Self::DashAcross,
            RouteVerb::DashUp => Self::DashUp,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SupportKindRecordV3 {
    Solid,
    OneWay,
}

impl From<SupportKind> for SupportKindRecordV3 {
    fn from(kind: SupportKind) -> Self {
        match kind {
            SupportKind::Solid => Self::Solid,
            SupportKind::OneWay => Self::OneWay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportRecordV3 {
    start_x: u16,
    end_x: u16,
    row: u16,
    kind: SupportKindRecordV3,
}

impl From<SupportSpec> for SupportRecordV3 {
    fn from(support: SupportSpec) -> Self {
        Self {
            start_x: support.start_x,
            end_x: support.end_x,
            row: support.row,
            kind: support.kind.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteNodeRecordV3 {
    id: u16,
    role: NodeRoleRecordV3,
    support: SupportRecordV3,
}

impl From<&RouteNode> for RouteNodeRecordV3 {
    fn from(node: &RouteNode) -> Self {
        Self {
            id: node.id,
            role: node.role.into(),
            support: node.support.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteEdgeRecordV3 {
    from: u16,
    to: u16,
    verb: RouteVerbRecordV3,
    critical: bool,
}

impl From<&RouteEdge> for RouteEdgeRecordV3 {
    fn from(edge: &RouteEdge) -> Self {
        Self {
            from: edge.from,
            to: edge.to,
            verb: edge.verb.into(),
            critical: edge.critical,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutePlanRecordV3 {
    nodes: Vec<RouteNodeRecordV3>,
    edges: Vec<RouteEdgeRecordV3>,
}

impl From<&RoutePlan> for RoutePlanRecordV3 {
    fn from(plan: &RoutePlan) -> Self {
        Self {
            nodes: plan.nodes.iter().map(Into::into).collect(),
            edges: plan.edges.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutePlanSummaryRecordV3 {
    node_count: u16,
    edge_count: u16,
    port_count: u16,
    cycle_rank: u16,
    branch_nodes: u16,
    vertical_span_rows: u16,
    wall_edges: u16,
    dash_edges: u16,
    signature: u64,
}

impl From<RoutePlanSummary> for RoutePlanSummaryRecordV3 {
    fn from(summary: RoutePlanSummary) -> Self {
        Self {
            node_count: summary.node_count,
            edge_count: summary.edge_count,
            port_count: summary.port_count,
            cycle_rank: summary.cycle_rank,
            branch_nodes: summary.branch_nodes,
            vertical_span_rows: summary.vertical_span_rows,
            wall_edges: summary.wall_edges,
            dash_edges: summary.dash_edges,
            signature: summary.signature,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryPortRecordV3 {
    node_id: u16,
    door_id: String,
    side: BoundarySideRecordV3,
    trigger_bounds: RectRecordV3,
    arrival: PointRecordV3,
    destination_room: Option<String>,
    destination_door: Option<String>,
    socket_offset: i32,
    socket_span: i32,
}

impl From<&BoundaryPort> for BoundaryPortRecordV3 {
    fn from(port: &BoundaryPort) -> Self {
        let socket = port.door.socket();
        Self {
            node_id: port.node_id,
            door_id: port.door.id.clone(),
            side: port.door.side.into(),
            trigger_bounds: port.door.trigger_bounds.into(),
            arrival: port.door.arrival.into(),
            destination_room: port.door.destination_room.clone(),
            destination_door: port.door.destination_door.clone(),
            socket_offset: socket.offset,
            socket_span: socket.span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PartitionSplitAxisRecordV3 {
    Vertical,
    Horizontal,
}

impl From<PartitionSplitAxis> for PartitionSplitAxisRecordV3 {
    fn from(axis: PartitionSplitAxis) -> Self {
        match axis {
            PartitionSplitAxis::Vertical => Self::Vertical,
            PartitionSplitAxis::Horizontal => Self::Horizontal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionDerivationSplitRecordV3 {
    parent_path: u32,
    target_spine_rank: u8,
    axis: PartitionSplitAxisRecordV3,
    ratio_slot: u8,
    gate_band: u8,
    cadence: u8,
    reverses_before_gate: bool,
    branch_runs_forward: bool,
}

impl From<PartitionDerivationSplit> for PartitionDerivationSplitRecordV3 {
    fn from(split: PartitionDerivationSplit) -> Self {
        Self {
            parent_path: split.parent_path,
            target_spine_rank: split.target_spine_rank,
            axis: split.axis.into(),
            ratio_slot: split.ratio_slot,
            gate_band: split.gate_band,
            cadence: split.cadence,
            reverses_before_gate: split.reverses_before_gate,
            branch_runs_forward: split.branch_runs_forward,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionDerivationBeatRecordV3 {
    relative_vertical_delta: i8,
    cadence: u8,
    horizontal_reversal: bool,
}

impl From<PartitionDerivationBeat> for PartitionDerivationBeatRecordV3 {
    fn from(beat: PartitionDerivationBeat) -> Self {
        Self {
            relative_vertical_delta: beat.relative_vertical_delta,
            cadence: beat.cadence,
            horizontal_reversal: beat.horizontal_reversal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionDerivationRecordV3 {
    chamber_count: u8,
    splits: Vec<PartitionDerivationSplitRecordV3>,
    primary_beats: Vec<PartitionDerivationBeatRecordV3>,
    fork_rejoin_cycles: u8,
    boundary_sides: Vec<BoundarySideRecordV3>,
    pickup_attachment: Option<u32>,
    vertical_socket_slot: u8,
    floor_socket_slot: Option<u8>,
    graph_topology_signature: u64,
    route_derivation_signature: u64,
    derivation_fingerprint: u64,
}

impl From<&PartitionDerivation> for PartitionDerivationRecordV3 {
    fn from(derivation: &PartitionDerivation) -> Self {
        Self {
            chamber_count: derivation.chamber_count,
            splits: derivation.splits.iter().copied().map(Into::into).collect(),
            primary_beats: derivation
                .primary_beats
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            fork_rejoin_cycles: derivation.fork_rejoin_cycles,
            boundary_sides: derivation
                .boundary_sides
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            pickup_attachment: derivation.pickup_attachment,
            vertical_socket_slot: derivation.vertical_socket_slot,
            floor_socket_slot: derivation.floor_socket_slot,
            graph_topology_signature: derivation.graph_topology_signature,
            route_derivation_signature: derivation.route_derivation_signature,
            derivation_fingerprint: derivation.derivation_fingerprint,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionRouteSummaryRecordV3 {
    generation_version: u32,
    chambers: u8,
    partition_cuts: u8,
    fork_rejoin_cycles: u8,
    boundary_ports: u8,
    authored_rises: u8,
    authored_falls: u8,
    authored_horizontal_reversals: u8,
    pickup_node_id: Option<u16>,
    interior_terrain_tiles: u16,
    graph_topology_signature: u64,
    route_derivation_signature: u64,
    derivation_fingerprint: u64,
    coordinate_route_signature: u64,
}

impl From<PartitionRouteSummary> for PartitionRouteSummaryRecordV3 {
    fn from(summary: PartitionRouteSummary) -> Self {
        Self {
            generation_version: summary.generation_version,
            chambers: summary.chambers,
            partition_cuts: summary.partition_cuts,
            fork_rejoin_cycles: summary.fork_rejoin_cycles,
            boundary_ports: summary.boundary_ports,
            authored_rises: summary.authored_rises,
            authored_falls: summary.authored_falls,
            authored_horizontal_reversals: summary.authored_horizontal_reversals,
            pickup_node_id: summary.pickup_node_id,
            interior_terrain_tiles: summary.interior_terrain_tiles,
            graph_topology_signature: summary.graph_topology_signature,
            route_derivation_signature: summary.route_derivation_signature,
            derivation_fingerprint: summary.derivation_fingerprint,
            coordinate_route_signature: summary.coordinate_route_signature,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MissionNodeKindRecordV3 {
    Port,
    Transit,
    Junction,
    Cut,
    ForkBranch,
    Pickup,
}

impl From<MissionNodeKind> for MissionNodeKindRecordV3 {
    fn from(kind: MissionNodeKind) -> Self {
        match kind {
            MissionNodeKind::Port => Self::Port,
            MissionNodeKind::Transit => Self::Transit,
            MissionNodeKind::Junction => Self::Junction,
            MissionNodeKind::Cut => Self::Cut,
            MissionNodeKind::ForkBranch => Self::ForkBranch,
            MissionNodeKind::Pickup => Self::Pickup,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MissionEdgeKindRecordV3 {
    Spine,
    ForkBranch,
}

impl From<MissionEdgeKind> for MissionEdgeKindRecordV3 {
    fn from(kind: MissionEdgeKind) -> Self {
        match kind {
            MissionEdgeKind::Spine => Self::Spine,
            MissionEdgeKind::ForkBranch => Self::ForkBranch,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CutAnchorRecordV3 {
    West,
    East,
}

impl From<CutAnchor> for CutAnchorRecordV3 {
    fn from(anchor: CutAnchor) -> Self {
        match anchor {
            CutAnchor::West => Self::West,
            CutAnchor::East => Self::East,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionNodeRecordV3 {
    id: u16,
    kind: MissionNodeKindRecordV3,
}

impl From<MissionNode> for MissionNodeRecordV3 {
    fn from(node: MissionNode) -> Self {
        Self {
            id: node.id,
            kind: node.kind.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionEdgeRecordV3 {
    from: u16,
    to: u16,
    kind: MissionEdgeKindRecordV3,
    critical: bool,
}

impl From<MissionEdge> for MissionEdgeRecordV3 {
    fn from(edge: MissionEdge) -> Self {
        Self {
            from: edge.from,
            to: edge.to,
            kind: edge.kind.into(),
            critical: edge.critical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionCutRecordV3 {
    order: u16,
    node_id: u16,
    spine_index: u16,
    anchor: CutAnchorRecordV3,
}

impl From<MissionCut> for MissionCutRecordV3 {
    fn from(cut: MissionCut) -> Self {
        Self {
            order: cut.order,
            node_id: cut.node_id,
            spine_index: cut.spine_index,
            anchor: cut.anchor.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionForkRecordV3 {
    from: u16,
    to: u16,
    branch_nodes: Vec<u16>,
}

impl From<&MissionFork> for MissionForkRecordV3 {
    fn from(fork: &MissionFork) -> Self {
        Self {
            from: fork.from,
            to: fork.to,
            branch_nodes: fork.branch_nodes.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionPortRecordV3 {
    ordinal: u16,
    node_id: u16,
    side: BoundarySideRecordV3,
}

impl From<MissionPort> for MissionPortRecordV3 {
    fn from(port: MissionPort) -> Self {
        Self {
            ordinal: port.ordinal,
            node_id: port.node_id,
            side: port.side.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rewrite", rename_all = "kebab-case", deny_unknown_fields)]
enum MissionRewriteRecordV3 {
    SubdivideEdge {
        removed_from: u16,
        removed_to: u16,
        inserted_node: u16,
    },
    InsertRouteCut {
        node_id: u16,
        spine_index: u16,
        order: u16,
        anchor: CutAnchorRecordV3,
    },
    InsertForkRejoin {
        from: u16,
        to: u16,
        branch_nodes: Vec<u16>,
    },
    AttachPort {
        node_id: u16,
        ordinal: u16,
        side: BoundarySideRecordV3,
    },
    MarkPickup {
        node_id: u16,
    },
}

impl From<&MissionRewrite> for MissionRewriteRecordV3 {
    fn from(rewrite: &MissionRewrite) -> Self {
        match rewrite {
            MissionRewrite::SubdivideEdge {
                removed_from,
                removed_to,
                inserted_node,
            } => Self::SubdivideEdge {
                removed_from: *removed_from,
                removed_to: *removed_to,
                inserted_node: *inserted_node,
            },
            MissionRewrite::InsertRouteCut {
                node_id,
                spine_index,
                order,
                anchor,
            } => Self::InsertRouteCut {
                node_id: *node_id,
                spine_index: *spine_index,
                order: *order,
                anchor: (*anchor).into(),
            },
            MissionRewrite::InsertForkRejoin {
                from,
                to,
                branch_nodes,
            } => Self::InsertForkRejoin {
                from: *from,
                to: *to,
                branch_nodes: branch_nodes.clone(),
            },
            MissionRewrite::AttachPort {
                node_id,
                ordinal,
                side,
            } => Self::AttachPort {
                node_id: *node_id,
                ordinal: *ordinal,
                side: (*side).into(),
            },
            MissionRewrite::MarkPickup { node_id } => Self::MarkPickup { node_id: *node_id },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionPlanRecordV3 {
    nodes: Vec<MissionNodeRecordV3>,
    edges: Vec<MissionEdgeRecordV3>,
    spine: Vec<u16>,
    cuts: Vec<MissionCutRecordV3>,
    forks: Vec<MissionForkRecordV3>,
    ports: Vec<MissionPortRecordV3>,
    pickup_node_id: u16,
}

impl From<&MissionPlan> for MissionPlanRecordV3 {
    fn from(plan: &MissionPlan) -> Self {
        Self {
            nodes: plan.nodes.iter().copied().map(Into::into).collect(),
            edges: plan.edges.iter().copied().map(Into::into).collect(),
            spine: plan.spine.clone(),
            cuts: plan.cuts.iter().copied().map(Into::into).collect(),
            forks: plan.forks.iter().map(Into::into).collect(),
            ports: plan.ports.iter().copied().map(Into::into).collect(),
            pickup_node_id: plan.pickup_node_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionProvenanceRecordV3 {
    derivation_version: u32,
    initial_node_count: u16,
    initial_edge_count: u16,
    subdivision_rewrites: u16,
    cut_rewrites: u16,
    fork_rewrites: u16,
    port_rewrites: u16,
    pickup_rewrites: u16,
    topology_signature: u64,
    derivation_signature: u64,
    rewrites: Vec<MissionRewriteRecordV3>,
}

impl From<&MissionProvenance> for MissionProvenanceRecordV3 {
    fn from(provenance: &MissionProvenance) -> Self {
        Self {
            derivation_version: provenance.derivation_version,
            initial_node_count: provenance.initial_node_count,
            initial_edge_count: provenance.initial_edge_count,
            subdivision_rewrites: provenance.subdivision_rewrites,
            cut_rewrites: provenance.cut_rewrites,
            fork_rewrites: provenance.fork_rewrites,
            port_rewrites: provenance.port_rewrites,
            pickup_rewrites: provenance.pickup_rewrites,
            topology_signature: provenance.topology_signature,
            derivation_signature: provenance.derivation_signature,
            rewrites: provenance.rewrites.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivedMissionRecordV3 {
    key: CorpusCandidateKeyRecord,
    plan: MissionPlanRecordV3,
    provenance: MissionProvenanceRecordV3,
}

impl From<&DerivedMission> for DerivedMissionRecordV3 {
    fn from(mission: &DerivedMission) -> Self {
        Self {
            key: CorpusCandidateKeyRecord::CompositionalRouteCut(
                super::CompositionalRouteCutKeyRecord::from(mission.key),
            ),
            plan: (&mission.plan).into(),
            provenance: (&mission.provenance).into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionRouteNodeMappingRecordV3 {
    mission_node_id: u16,
    route_node_id: u16,
}

impl From<MissionRouteNodeMapping> for MissionRouteNodeMappingRecordV3 {
    fn from(mapping: MissionRouteNodeMapping) -> Self {
        Self {
            mission_node_id: mapping.mission_node_id,
            route_node_id: mapping.route_node_id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteCutRealizationRecordV3 {
    mission_node_id: u16,
    route_node_id: u16,
    order: u16,
    anchor: CutAnchorRecordV3,
    row: u16,
    shelf_start_x: u16,
    shelf_end_x: u16,
    opening_start_x: u16,
    opening_end_x: u16,
    predecessor_route_node_id: u16,
    successor_route_node_id: u16,
}

impl From<RouteCutRealization> for RouteCutRealizationRecordV3 {
    fn from(realization: RouteCutRealization) -> Self {
        Self {
            mission_node_id: realization.mission_node_id,
            route_node_id: realization.route_node_id,
            order: realization.order,
            anchor: realization.anchor.into(),
            row: realization.row,
            shelf_start_x: realization.shelf_start_x,
            shelf_end_x: realization.shelf_end_x,
            opening_start_x: realization.opening_start_x,
            opening_end_x: realization.opening_end_x,
            predecessor_route_node_id: realization.predecessor_route_node_id,
            successor_route_node_id: realization.successor_route_node_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompositionalEmbeddingRecordV3 {
    generation_version: u32,
    embedding_attempt: u8,
    ascent_edges: u16,
    descent_edges: u16,
    level_edges: u16,
    horizontal_direction_reversals: u16,
    cut_realizations: Vec<RouteCutRealizationRecordV3>,
    socket_columns: Vec<u16>,
    claims_wall_jump_requirement: bool,
    claims_dash_requirement: bool,
}

impl From<&CompositionalRouteCutEmbeddingSummary> for CompositionalEmbeddingRecordV3 {
    fn from(embedding: &CompositionalRouteCutEmbeddingSummary) -> Self {
        Self {
            generation_version: embedding.generation_version,
            embedding_attempt: embedding.embedding_attempt,
            ascent_edges: embedding.ascent_edges,
            descent_edges: embedding.descent_edges,
            level_edges: embedding.level_edges,
            horizontal_direction_reversals: embedding.horizontal_direction_reversals,
            cut_realizations: embedding
                .cut_realizations
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            socket_columns: embedding.socket_columns.clone(),
            claims_wall_jump_requirement: embedding.claims_wall_jump_requirement,
            claims_dash_requirement: embedding.claims_dash_requirement,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum GateAbilityRecordV3 {
    WallJump,
    Dash,
}

impl From<GateAbility> for GateAbilityRecordV3 {
    fn from(ability: GateAbility) -> Self {
        match ability {
            GateAbility::WallJump => Self::WallJump,
            GateAbility::Dash => Self::Dash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "requirement",
    content = "ability",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
enum DirectedTraversalRequirementRecordV3 {
    Baseline,
    Ability(GateAbilityRecordV3),
}

impl From<DirectedTraversalRequirement> for DirectedTraversalRequirementRecordV3 {
    fn from(requirement: DirectedTraversalRequirement) -> Self {
        match requirement {
            DirectedTraversalRequirement::Baseline => Self::Baseline,
            DirectedTraversalRequirement::Ability(ability) => Self::Ability(ability.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectedMissionEdgeRecordV3 {
    edge: MissionEdgeRecordV3,
    forward_requirement: DirectedTraversalRequirementRecordV3,
    reverse_requirement: DirectedTraversalRequirementRecordV3,
}

impl From<DirectedMissionEdge> for DirectedMissionEdgeRecordV3 {
    fn from(edge: DirectedMissionEdge) -> Self {
        Self {
            edge: edge.edge.into(),
            forward_requirement: edge.forward_requirement.into(),
            reverse_requirement: edge.reverse_requirement.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityGateGeometryRecordV3 {
    PairedWallShaft,
    DashRiseTransfer,
}

impl From<AbilityGateGeometry> for AbilityGateGeometryRecordV3 {
    fn from(geometry: AbilityGateGeometry) -> Self {
        match geometry {
            AbilityGateGeometry::PairedWallShaft => Self::PairedWallShaft,
            AbilityGateGeometry::DashRiseTransfer => Self::DashRiseTransfer,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CompositionalAbilityEmbeddingSeamRecordV3 {
    SpineRowsAndPairedSupportDomainsBeforeRasterization,
}

impl From<CompositionalAbilityEmbeddingSeam> for CompositionalAbilityEmbeddingSeamRecordV3 {
    fn from(seam: CompositionalAbilityEmbeddingSeam) -> Self {
        match seam {
            CompositionalAbilityEmbeddingSeam::SpineRowsAndPairedSupportDomainsBeforeRasterization => {
                Self::SpineRowsAndPairedSupportDomainsBeforeRasterization
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityGateEmbeddingContractRecordV3 {
    version: u32,
    seam: CompositionalAbilityEmbeddingSeamRecordV3,
    geometry: AbilityGateGeometryRecordV3,
    minimum_ascent_rows: u16,
    minimum_clear_interior_width_tiles: u16,
    reserve_empty_transfer_volume: bool,
    forbid_intermediate_supports: bool,
    forbid_wall_contact_in_ascent: bool,
    preserve_all_incident_route_edges: bool,
    require_baseline_reverse_descent: bool,
}

impl From<AbilityGateEmbeddingContract> for AbilityGateEmbeddingContractRecordV3 {
    fn from(contract: AbilityGateEmbeddingContract) -> Self {
        Self {
            version: contract.version,
            seam: contract.seam.into(),
            geometry: contract.geometry.into(),
            minimum_ascent_rows: contract.minimum_ascent_rows,
            minimum_clear_interior_width_tiles: contract.minimum_clear_interior_width_tiles,
            reserve_empty_transfer_volume: contract.reserve_empty_transfer_volume,
            forbid_intermediate_supports: contract.forbid_intermediate_supports,
            forbid_wall_contact_in_ascent: contract.forbid_wall_contact_in_ascent,
            preserve_all_incident_route_edges: contract.preserve_all_incident_route_edges,
            require_baseline_reverse_descent: contract.require_baseline_reverse_descent,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectedAbilityGateRecordV3 {
    ordinal: u16,
    spine_edge_index: u16,
    mission_edge_index: u16,
    ascent_from: u16,
    ascent_to: u16,
    required_ability: GateAbilityRecordV3,
    embedding_contract: AbilityGateEmbeddingContractRecordV3,
}

impl From<&DirectedAbilityGate> for DirectedAbilityGateRecordV3 {
    fn from(gate: &DirectedAbilityGate) -> Self {
        Self {
            ordinal: gate.ordinal,
            spine_edge_index: gate.spine_edge_index,
            mission_edge_index: gate.mission_edge_index,
            ascent_from: gate.ascent_from,
            ascent_to: gate.ascent_to,
            required_ability: gate.required_ability.into(),
            embedding_contract: gate.embedding_contract.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateUnavoidabilityCertificateRecordV3 {
    gate_ordinal: u16,
    source_reachable_after_edge_deletion: Vec<u16>,
    source_reachable_without_required_ability: Vec<u16>,
    sink_reachable_with_baseline_reverse_traversal: Vec<u16>,
}

impl From<&GateUnavoidabilityCertificate> for GateUnavoidabilityCertificateRecordV3 {
    fn from(certificate: &GateUnavoidabilityCertificate) -> Self {
        Self {
            gate_ordinal: certificate.gate_ordinal,
            source_reachable_after_edge_deletion: certificate
                .source_reachable_after_edge_deletion
                .clone(),
            source_reachable_without_required_ability: certificate
                .source_reachable_without_required_ability
                .clone(),
            sink_reachable_with_baseline_reverse_traversal: certificate
                .sink_reachable_with_baseline_reverse_traversal
                .clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityRewrittenMissionPlanRecordV3 {
    base: MissionPlanRecordV3,
    edges: Vec<DirectedMissionEdgeRecordV3>,
    gates: Vec<DirectedAbilityGateRecordV3>,
}

impl From<&AbilityRewrittenMissionPlan> for AbilityRewrittenMissionPlanRecordV3 {
    fn from(plan: &AbilityRewrittenMissionPlan) -> Self {
        Self {
            base: (&plan.base).into(),
            edges: plan.edges.iter().copied().map(Into::into).collect(),
            gates: plan.gates.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityEdgeRewriteKeyRecordV3 {
    base_key: CompositionalRouteCutKeyRecord,
    profile: CompositionalAbilityGateProfileRecord,
    rewrite_attempt: u16,
}

impl From<CompositionalAbilityEdgeRewriteKey> for AbilityEdgeRewriteKeyRecordV3 {
    fn from(key: CompositionalAbilityEdgeRewriteKey) -> Self {
        Self {
            base_key: key.base_key.into(),
            profile: key
                .profile
                .try_into()
                .expect("promoted corpus ability candidates exclude the combined profile"),
            rewrite_attempt: key.rewrite_attempt,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityEdgeRewriteProvenanceRecordV3 {
    rewrite_version: u32,
    base_topology_signature: u64,
    base_derivation_signature: u64,
    eligible_bridge_count: u16,
    arrangement_count: u16,
    rewritten_topology_signature: u64,
    rewrite_signature: u64,
}

impl From<&AbilityEdgeRewriteProvenance> for AbilityEdgeRewriteProvenanceRecordV3 {
    fn from(provenance: &AbilityEdgeRewriteProvenance) -> Self {
        Self {
            rewrite_version: provenance.rewrite_version,
            base_topology_signature: provenance.base_topology_signature,
            base_derivation_signature: provenance.base_derivation_signature,
            eligible_bridge_count: provenance.eligible_bridge_count,
            arrangement_count: provenance.arrangement_count,
            rewritten_topology_signature: provenance.rewritten_topology_signature,
            rewrite_signature: provenance.rewrite_signature,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityGateEmbeddingPendingReasonRecordV3 {
    ReservedGeometryNotEmbedded,
    AuthoritativePositiveReplayNotObserved,
    ReducedLoadoutBypassAuditNotObserved,
}

impl From<AbilityGateEmbeddingPendingReason> for AbilityGateEmbeddingPendingReasonRecordV3 {
    fn from(reason: AbilityGateEmbeddingPendingReason) -> Self {
        match reason {
            AbilityGateEmbeddingPendingReason::ReservedGeometryNotEmbedded => {
                Self::ReservedGeometryNotEmbedded
            }
            AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved => {
                Self::AuthoritativePositiveReplayNotObserved
            }
            AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved => {
                Self::ReducedLoadoutBypassAuditNotObserved
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum AbilityGateEmbeddingStateRecordV3 {
    Pending {
        reasons: Vec<AbilityGateEmbeddingPendingReasonRecordV3>,
    },
}

impl From<&AbilityGateEmbeddingState> for AbilityGateEmbeddingStateRecordV3 {
    fn from(state: &AbilityGateEmbeddingState) -> Self {
        match state {
            AbilityGateEmbeddingState::Pending { reasons } => Self::Pending {
                reasons: reasons.iter().copied().map(Into::into).collect(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityRewrittenMissionRecordV3 {
    key: AbilityEdgeRewriteKeyRecordV3,
    base_mission: DerivedMissionRecordV3,
    plan: AbilityRewrittenMissionPlanRecordV3,
    provenance: AbilityEdgeRewriteProvenanceRecordV3,
    certificates: Vec<GateUnavoidabilityCertificateRecordV3>,
    embedding_state: AbilityGateEmbeddingStateRecordV3,
}

impl From<&AbilityRewrittenMission> for AbilityRewrittenMissionRecordV3 {
    fn from(mission: &AbilityRewrittenMission) -> Self {
        Self {
            key: mission.key.into(),
            base_mission: (&mission.base_mission).into(),
            plan: (&mission.plan).into(),
            provenance: (&mission.provenance).into(),
            certificates: mission.certificates.iter().map(Into::into).collect(),
            embedding_state: (&mission.embedding_state).into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityGateTileCellRecordV3 {
    x: u16,
    row: u16,
}

impl From<AbilityGateTileCell> for AbilityGateTileCellRecordV3 {
    fn from(cell: AbilityGateTileCell) -> Self {
        Self {
            x: cell.x,
            row: cell.row,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityGateRealizationRecordV3 {
    gate: DirectedAbilityGateRecordV3,
    from_route_node_id: u16,
    to_route_node_id: u16,
    lower_support: SupportRecordV3,
    upper_support: SupportRecordV3,
    ascent_bounds: RectRecordV3,
    lower_standing_bounds: RectRecordV3,
    upper_standing_bounds: RectRecordV3,
    required_solid_tiles: Vec<AbilityGateTileCellRecordV3>,
    required_empty_tiles: Vec<AbilityGateTileCellRecordV3>,
}

impl From<&AbilityGateRealization> for AbilityGateRealizationRecordV3 {
    fn from(realization: &AbilityGateRealization) -> Self {
        Self {
            gate: (&realization.gate).into(),
            from_route_node_id: realization.from_route_node_id,
            to_route_node_id: realization.to_route_node_id,
            lower_support: realization.lower_support.into(),
            upper_support: realization.upper_support.into(),
            ascent_bounds: realization.ascent_bounds.into(),
            lower_standing_bounds: realization.lower_standing_bounds.into(),
            upper_standing_bounds: realization.upper_standing_bounds.into(),
            required_solid_tiles: realization
                .required_solid_tiles
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            required_empty_tiles: realization
                .required_empty_tiles
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompositionalAbilityEmbeddingRecordV3 {
    generation_version: u32,
    graph_rewrite_version: u32,
    embedding_attempt: u8,
    rewrite_attempt: u16,
    ascent_edges: u16,
    descent_edges: u16,
    level_edges: u16,
    horizontal_direction_reversals: u16,
    cut_realizations: Vec<RouteCutRealizationRecordV3>,
    socket_columns: Vec<u16>,
    gate_realizations: Vec<AbilityGateRealizationRecordV3>,
}

impl From<&CompositionalAbilityEmbeddingSummary> for CompositionalAbilityEmbeddingRecordV3 {
    fn from(embedding: &CompositionalAbilityEmbeddingSummary) -> Self {
        Self {
            generation_version: embedding.generation_version,
            graph_rewrite_version: embedding.graph_rewrite_version,
            embedding_attempt: embedding.embedding_attempt,
            rewrite_attempt: embedding.rewrite_attempt,
            ascent_edges: embedding.ascent_edges,
            descent_edges: embedding.descent_edges,
            level_edges: embedding.level_edges,
            horizontal_direction_reversals: embedding.horizontal_direction_reversals,
            cut_realizations: embedding
                .cut_realizations
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            socket_columns: embedding.socket_columns.clone(),
            gate_realizations: embedding.gate_realizations.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "generator", rename_all = "kebab-case", deny_unknown_fields)]
enum NativeProvenanceRecordV3 {
    PartitionRoute {
        derivation: PartitionDerivationRecordV3,
        generator_summary: PartitionRouteSummaryRecordV3,
    },
    CompositionalRouteCut {
        mission: DerivedMissionRecordV3,
        mission_route_nodes: Vec<MissionRouteNodeMappingRecordV3>,
        embedding: CompositionalEmbeddingRecordV3,
    },
    CompositionalAbility {
        rewritten_mission: Box<AbilityRewrittenMissionRecordV3>,
        mission_route_nodes: Vec<MissionRouteNodeMappingRecordV3>,
        embedding: CompositionalAbilityEmbeddingRecordV3,
        evidence_state: AbilityGateEmbeddingStateRecordV3,
    },
}

impl From<&CorpusCandidate> for NativeProvenanceRecordV3 {
    fn from(candidate: &CorpusCandidate) -> Self {
        match candidate {
            CorpusCandidate::PartitionRoute(candidate) => Self::PartitionRoute {
                derivation: (&candidate.derivation).into(),
                generator_summary: candidate.summary.into(),
            },
            CorpusCandidate::CompositionalRouteCut(candidate) => Self::CompositionalRouteCut {
                mission: (&candidate.mission).into(),
                mission_route_nodes: candidate
                    .mission_route_nodes
                    .iter()
                    .copied()
                    .map(Into::into)
                    .collect(),
                embedding: (&candidate.embedding).into(),
            },
            CorpusCandidate::CompositionalAbility(candidate) => Self::CompositionalAbility {
                rewritten_mission: Box::new((&candidate.rewritten_mission).into()),
                mission_route_nodes: candidate
                    .mission_route_nodes
                    .iter()
                    .copied()
                    .map(Into::into)
                    .collect(),
                embedding: (&candidate.embedding).into(),
                evidence_state: (&candidate.evidence_state).into(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeVariantRecordV3 {
    key: CorpusCandidateKeyRecord,
    construction_loadout: EvaluationLoadout,
    route_plan: RoutePlanRecordV3,
    route_plan_summary: RoutePlanSummaryRecordV3,
    boundary_ports: Vec<BoundaryPortRecordV3>,
    provenance: NativeProvenanceRecordV3,
}

impl From<&CorpusCandidate> for NativeVariantRecordV3 {
    fn from(candidate: &CorpusCandidate) -> Self {
        Self {
            key: candidate.exact_key(),
            construction_loadout: EvaluationLoadout::ALL
                .into_iter()
                .find(|loadout| loadout.abilities() == candidate.construction_abilities())
                .expect("every AbilitySet has an EvaluationLoadout"),
            route_plan: candidate.route_plan().into(),
            route_plan_summary: (*candidate.route_plan_summary()).into(),
            boundary_ports: candidate.boundary_ports().iter().map(Into::into).collect(),
            provenance: candidate.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum FeasibilityGateRecordV3 {
    ReplayCertifiedAllTargets,
    BoundedInconclusive {
        door_rows: usize,
        positive_door_rows: usize,
        pickup_rows: usize,
        positive_pickup_rows: usize,
    },
}

impl FeasibilityGateRecordV3 {
    const fn passes(self) -> bool {
        matches!(self, Self::ReplayCertifiedAllTargets)
    }
}

impl From<CorpusFeasibilityGateState> for FeasibilityGateRecordV3 {
    fn from(state: CorpusFeasibilityGateState) -> Self {
        match state {
            CorpusFeasibilityGateState::ReplayCertifiedAllTargets => {
                Self::ReplayCertifiedAllTargets
            }
            CorpusFeasibilityGateState::BoundedInconclusive {
                door_rows,
                positive_door_rows,
                pickup_rows,
                positive_pickup_rows,
            } => Self::BoundedInconclusive {
                door_rows,
                positive_door_rows,
                pickup_rows,
                positive_pickup_rows,
            },
        }
    }
}

impl From<FeasibilityGateRecordV3> for CorpusFeasibilityGateState {
    fn from(state: FeasibilityGateRecordV3) -> Self {
        match state {
            FeasibilityGateRecordV3::ReplayCertifiedAllTargets => Self::ReplayCertifiedAllTargets,
            FeasibilityGateRecordV3::BoundedInconclusive {
                door_rows,
                positive_door_rows,
                pickup_rows,
                positive_pickup_rows,
            } => Self::BoundedInconclusive {
                door_rows,
                positive_door_rows,
                pickup_rows,
                positive_pickup_rows,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantConstructionGateRecordV3 {
    gate_version: u32,
    key: CorpusCandidateKeyRecord,
    construction_loadout: EvaluationLoadout,
    state: FeasibilityGateRecordV3,
}

impl From<&VariantConstructionGateV2> for VariantConstructionGateRecordV3 {
    fn from(gate: &VariantConstructionGateV2) -> Self {
        Self {
            gate_version: gate.gate_version,
            key: gate.key.clone(),
            construction_loadout: gate.construction_loadout,
            state: gate.state.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityPromotionClaimRecordV3 {
    WallJump,
    Dash,
}

impl From<AbilityPromotionClaimV2> for AbilityPromotionClaimRecordV3 {
    fn from(claim: AbilityPromotionClaimV2) -> Self {
        match claim {
            AbilityPromotionClaimV2::WallJump => Self::WallJump,
            AbilityPromotionClaimV2::Dash => Self::Dash,
        }
    }
}

impl From<AbilityPromotionClaimRecordV3> for AbilityPromotionClaimV2 {
    fn from(claim: AbilityPromotionClaimRecordV3) -> Self {
        match claim {
            AbilityPromotionClaimRecordV3::WallJump => Self::WallJump,
            AbilityPromotionClaimRecordV3::Dash => Self::Dash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityPromotionMatrixStateRecordV3 {
    ReplayCertifiedPositive,
    BoundedInconclusive,
}

impl From<AbilityPromotionMatrixStateV2> for AbilityPromotionMatrixStateRecordV3 {
    fn from(state: AbilityPromotionMatrixStateV2) -> Self {
        match state {
            AbilityPromotionMatrixStateV2::ReplayCertifiedPositive => Self::ReplayCertifiedPositive,
            AbilityPromotionMatrixStateV2::BoundedInconclusive => Self::BoundedInconclusive,
        }
    }
}

impl From<AbilityPromotionMatrixStateRecordV3> for AbilityPromotionMatrixStateV2 {
    fn from(state: AbilityPromotionMatrixStateRecordV3) -> Self {
        match state {
            AbilityPromotionMatrixStateRecordV3::ReplayCertifiedPositive => {
                Self::ReplayCertifiedPositive
            }
            AbilityPromotionMatrixStateRecordV3::BoundedInconclusive => Self::BoundedInconclusive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityPromotionIntendedStateRecordV3 {
    ReplayCertifiedRequiredEventAccepted,
    ReplayCertifiedRequiredEventMissing,
    BoundedInconclusive,
}

impl From<AbilityPromotionIntendedStateV2> for AbilityPromotionIntendedStateRecordV3 {
    fn from(state: AbilityPromotionIntendedStateV2) -> Self {
        match state {
            AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted => {
                Self::ReplayCertifiedRequiredEventAccepted
            }
            AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventMissing => {
                Self::ReplayCertifiedRequiredEventMissing
            }
            AbilityPromotionIntendedStateV2::BoundedInconclusive => Self::BoundedInconclusive,
        }
    }
}

impl From<AbilityPromotionIntendedStateRecordV3> for AbilityPromotionIntendedStateV2 {
    fn from(state: AbilityPromotionIntendedStateRecordV3) -> Self {
        match state {
            AbilityPromotionIntendedStateRecordV3::ReplayCertifiedRequiredEventAccepted => {
                Self::ReplayCertifiedRequiredEventAccepted
            }
            AbilityPromotionIntendedStateRecordV3::ReplayCertifiedRequiredEventMissing => {
                Self::ReplayCertifiedRequiredEventMissing
            }
            AbilityPromotionIntendedStateRecordV3::BoundedInconclusive => Self::BoundedInconclusive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
enum AbilityPromotionIntendedEvidenceSourceRecordV3 {
    DirectController { witness_index: usize },
    CanonicalMatrix,
}

impl From<AbilityPromotionIntendedEvidenceSourceV2>
    for AbilityPromotionIntendedEvidenceSourceRecordV3
{
    fn from(source: AbilityPromotionIntendedEvidenceSourceV2) -> Self {
        match source {
            AbilityPromotionIntendedEvidenceSourceV2::DirectController { witness_index } => {
                Self::DirectController { witness_index }
            }
            AbilityPromotionIntendedEvidenceSourceV2::CanonicalMatrix => Self::CanonicalMatrix,
        }
    }
}

impl From<AbilityPromotionIntendedEvidenceSourceRecordV3>
    for AbilityPromotionIntendedEvidenceSourceV2
{
    fn from(source: AbilityPromotionIntendedEvidenceSourceRecordV3) -> Self {
        match source {
            AbilityPromotionIntendedEvidenceSourceRecordV3::DirectController { witness_index } => {
                Self::DirectController { witness_index }
            }
            AbilityPromotionIntendedEvidenceSourceRecordV3::CanonicalMatrix => {
                Self::CanonicalMatrix
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AbilityPromotionBudgetLimitRecordV3 {
    ExpandedNodes,
    SimulatedTicks,
}

impl From<AbilityPromotionBudgetLimitV2> for AbilityPromotionBudgetLimitRecordV3 {
    fn from(limit: AbilityPromotionBudgetLimitV2) -> Self {
        match limit {
            AbilityPromotionBudgetLimitV2::ExpandedNodes => Self::ExpandedNodes,
            AbilityPromotionBudgetLimitV2::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

impl From<AbilityPromotionBudgetLimitRecordV3> for AbilityPromotionBudgetLimitV2 {
    fn from(limit: AbilityPromotionBudgetLimitRecordV3) -> Self {
        match limit {
            AbilityPromotionBudgetLimitRecordV3::ExpandedNodes => Self::ExpandedNodes,
            AbilityPromotionBudgetLimitRecordV3::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum AbilityPromotionDirectAuditStateRecordV3 {
    CompleteFiniteVocabularyNoPositive,
    PositiveBypass {
        raw_positive_witnesses: usize,
        retained_semantic_witnesses: usize,
    },
    BoundedInconclusive {
        limit: AbilityPromotionBudgetLimitRecordV3,
    },
}

impl From<AbilityPromotionDirectAuditStateV2> for AbilityPromotionDirectAuditStateRecordV3 {
    fn from(state: AbilityPromotionDirectAuditStateV2) -> Self {
        match state {
            AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive => {
                Self::CompleteFiniteVocabularyNoPositive
            }
            AbilityPromotionDirectAuditStateV2::PositiveBypass {
                raw_positive_witnesses,
                retained_semantic_witnesses,
            } => Self::PositiveBypass {
                raw_positive_witnesses,
                retained_semantic_witnesses,
            },
            AbilityPromotionDirectAuditStateV2::BoundedInconclusive { limit } => {
                Self::BoundedInconclusive {
                    limit: limit.into(),
                }
            }
        }
    }
}

impl From<AbilityPromotionDirectAuditStateRecordV3> for AbilityPromotionDirectAuditStateV2 {
    fn from(state: AbilityPromotionDirectAuditStateRecordV3) -> Self {
        match state {
            AbilityPromotionDirectAuditStateRecordV3::CompleteFiniteVocabularyNoPositive => {
                Self::CompleteFiniteVocabularyNoPositive
            }
            AbilityPromotionDirectAuditStateRecordV3::PositiveBypass {
                raw_positive_witnesses,
                retained_semantic_witnesses,
            } => Self::PositiveBypass {
                raw_positive_witnesses,
                retained_semantic_witnesses,
            },
            AbilityPromotionDirectAuditStateRecordV3::BoundedInconclusive { limit } => {
                Self::BoundedInconclusive {
                    limit: limit.into(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityPromotionMissingLoadoutEvidenceRecordV3 {
    loadout: EvaluationLoadout,
    matrix: AbilityPromotionMatrixStateRecordV3,
    direct_audit: AbilityPromotionDirectAuditStateRecordV3,
}

impl From<&AbilityPromotionMissingLoadoutEvidenceV2>
    for AbilityPromotionMissingLoadoutEvidenceRecordV3
{
    fn from(evidence: &AbilityPromotionMissingLoadoutEvidenceV2) -> Self {
        Self {
            loadout: evidence.loadout,
            matrix: evidence.matrix.into(),
            direct_audit: evidence.direct_audit.into(),
        }
    }
}

impl From<AbilityPromotionMissingLoadoutEvidenceRecordV3>
    for AbilityPromotionMissingLoadoutEvidenceV2
{
    fn from(evidence: AbilityPromotionMissingLoadoutEvidenceRecordV3) -> Self {
        Self {
            loadout: evidence.loadout,
            matrix: evidence.matrix.into(),
            direct_audit: evidence.direct_audit.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum AbilityPromotionDecisionRecordV3 {
    NotApplicable,
    PromotedStructuralNoKnownBypass,
    RefusedAliasRoomMismatch,
    BoundedIntendedRoute,
    IntendedRouteMissingRequiredEvent,
    BoundedBaselineReverse,
    PositiveMatrixBypass {
        loadout: EvaluationLoadout,
    },
    PositiveDirectBypass {
        loadout: EvaluationLoadout,
    },
    BoundedDirectAudit {
        loadout: EvaluationLoadout,
        limit: AbilityPromotionBudgetLimitRecordV3,
    },
}

impl From<AbilityPromotionDecisionV2> for AbilityPromotionDecisionRecordV3 {
    fn from(decision: AbilityPromotionDecisionV2) -> Self {
        match decision {
            AbilityPromotionDecisionV2::NotApplicable => Self::NotApplicable,
            AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass => {
                Self::PromotedStructuralNoKnownBypass
            }
            AbilityPromotionDecisionV2::RefusedAliasRoomMismatch => Self::RefusedAliasRoomMismatch,
            AbilityPromotionDecisionV2::BoundedIntendedRoute => Self::BoundedIntendedRoute,
            AbilityPromotionDecisionV2::IntendedRouteMissingRequiredEvent => {
                Self::IntendedRouteMissingRequiredEvent
            }
            AbilityPromotionDecisionV2::BoundedBaselineReverse => Self::BoundedBaselineReverse,
            AbilityPromotionDecisionV2::PositiveMatrixBypass { loadout } => {
                Self::PositiveMatrixBypass { loadout }
            }
            AbilityPromotionDecisionV2::PositiveDirectBypass { loadout } => {
                Self::PositiveDirectBypass { loadout }
            }
            AbilityPromotionDecisionV2::BoundedDirectAudit { loadout, limit } => {
                Self::BoundedDirectAudit {
                    loadout,
                    limit: limit.into(),
                }
            }
        }
    }
}

impl From<AbilityPromotionDecisionRecordV3> for AbilityPromotionDecisionV2 {
    fn from(decision: AbilityPromotionDecisionRecordV3) -> Self {
        match decision {
            AbilityPromotionDecisionRecordV3::NotApplicable => Self::NotApplicable,
            AbilityPromotionDecisionRecordV3::PromotedStructuralNoKnownBypass => {
                Self::PromotedStructuralNoKnownBypass
            }
            AbilityPromotionDecisionRecordV3::RefusedAliasRoomMismatch => {
                Self::RefusedAliasRoomMismatch
            }
            AbilityPromotionDecisionRecordV3::BoundedIntendedRoute => Self::BoundedIntendedRoute,
            AbilityPromotionDecisionRecordV3::IntendedRouteMissingRequiredEvent => {
                Self::IntendedRouteMissingRequiredEvent
            }
            AbilityPromotionDecisionRecordV3::BoundedBaselineReverse => {
                Self::BoundedBaselineReverse
            }
            AbilityPromotionDecisionRecordV3::PositiveMatrixBypass { loadout } => {
                Self::PositiveMatrixBypass { loadout }
            }
            AbilityPromotionDecisionRecordV3::PositiveDirectBypass { loadout } => {
                Self::PositiveDirectBypass { loadout }
            }
            AbilityPromotionDecisionRecordV3::BoundedDirectAudit { loadout, limit } => {
                Self::BoundedDirectAudit {
                    loadout,
                    limit: limit.into(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum VariantAbilityPromotionEvidenceRecordV3 {
    NotApplicable,
    Ability {
        claim: AbilityPromotionClaimRecordV3,
        source_door_id: String,
        sink_door_id: String,
        edge_rewrite_version: u32,
        gate_embedding_contract_version: u32,
        generation_version: u32,
        alias_room_compatible: bool,
        intended_route: Option<AbilityPromotionIntendedStateRecordV3>,
        intended_evidence_source: Option<AbilityPromotionIntendedEvidenceSourceRecordV3>,
        baseline_reverse: Option<AbilityPromotionMatrixStateRecordV3>,
        missing_ability_loadouts: Vec<AbilityPromotionMissingLoadoutEvidenceRecordV3>,
        direct_route_assessment: Option<Box<RouteControllerAssessmentRecordV3>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VariantAbilityPromotionGateRecordV3 {
    gate_version: u32,
    pub(super) key: CorpusCandidateKeyRecord,
    evidence: VariantAbilityPromotionEvidenceRecordV3,
    decision: AbilityPromotionDecisionRecordV3,
}

fn variant_ability_promotion_gate_record_v3(
    gate: &VariantAbilityPromotionGateV2,
) -> Result<VariantAbilityPromotionGateRecordV3, CorpusArtifactV3Error> {
    let evidence = match &gate.evidence {
        VariantAbilityPromotionEvidenceV2::NotApplicable => {
            VariantAbilityPromotionEvidenceRecordV3::NotApplicable
        }
        VariantAbilityPromotionEvidenceV2::Ability {
            claim,
            source_door_id,
            sink_door_id,
            edge_rewrite_version,
            gate_embedding_contract_version,
            generation_version,
            alias_room_compatible,
            intended_route,
            intended_evidence_source,
            baseline_reverse,
            missing_ability_loadouts,
            direct_route_assessment,
        } => VariantAbilityPromotionEvidenceRecordV3::Ability {
            claim: (*claim).into(),
            source_door_id: source_door_id.clone(),
            sink_door_id: sink_door_id.clone(),
            edge_rewrite_version: *edge_rewrite_version,
            gate_embedding_contract_version: *gate_embedding_contract_version,
            generation_version: *generation_version,
            alias_room_compatible: *alias_room_compatible,
            intended_route: intended_route.map(Into::into),
            intended_evidence_source: intended_evidence_source.map(Into::into),
            baseline_reverse: baseline_reverse.map(Into::into),
            missing_ability_loadouts: missing_ability_loadouts.iter().map(Into::into).collect(),
            direct_route_assessment: direct_route_assessment
                .as_deref()
                .map(route_controller_assessment_record_v3)
                .transpose()?
                .map(Box::new),
        },
    };
    Ok(VariantAbilityPromotionGateRecordV3 {
        gate_version: gate.gate_version,
        key: gate.key.clone(),
        evidence,
        decision: gate.decision.into(),
    })
}

pub(super) fn rehydrate_variant_ability_promotion_gate_record_v3(
    record: VariantAbilityPromotionGateRecordV3,
    candidate: &CorpusCandidate,
) -> Result<VariantAbilityPromotionGateV2, CorpusArtifactV3Error> {
    let expected_record = record.clone();
    let evidence = match record.evidence {
        VariantAbilityPromotionEvidenceRecordV3::NotApplicable => {
            VariantAbilityPromotionEvidenceV2::NotApplicable
        }
        VariantAbilityPromotionEvidenceRecordV3::Ability {
            claim,
            source_door_id,
            sink_door_id,
            edge_rewrite_version,
            gate_embedding_contract_version,
            generation_version,
            alias_room_compatible,
            intended_route,
            intended_evidence_source,
            baseline_reverse,
            missing_ability_loadouts,
            direct_route_assessment,
        } => VariantAbilityPromotionEvidenceV2::Ability {
            claim: claim.into(),
            source_door_id,
            sink_door_id,
            edge_rewrite_version,
            gate_embedding_contract_version,
            generation_version,
            alias_room_compatible,
            intended_route: intended_route.map(Into::into),
            intended_evidence_source: intended_evidence_source.map(Into::into),
            baseline_reverse: baseline_reverse.map(Into::into),
            missing_ability_loadouts: missing_ability_loadouts
                .into_iter()
                .map(Into::into)
                .collect(),
            direct_route_assessment: direct_route_assessment
                .map(|assessment| {
                    rehydrate_route_controller_assessment_record_v3(*assessment, candidate)
                        .map(Box::new)
                })
                .transpose()?,
        },
    };
    let gate = VariantAbilityPromotionGateV2 {
        gate_version: record.gate_version,
        key: record.key,
        evidence,
        decision: record.decision.into(),
    };
    if variant_ability_promotion_gate_record_v3(&gate)? != expected_record {
        return Err(invalid(format!(
            "ability-promotion gate {} did not round-trip exactly",
            gate.key.stable_slug()
        )));
    }
    Ok(gate)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CanonicalRegenerationPolicyRecordV3 {
    LexicographicallySmallestExactKeyPassingAllGates,
}

impl From<CanonicalRegenerationPolicy> for CanonicalRegenerationPolicyRecordV3 {
    fn from(policy: CanonicalRegenerationPolicy) -> Self {
        match policy {
            CanonicalRegenerationPolicy::LexicographicallySmallestExactKeyPassingAllGates => {
                Self::LexicographicallySmallestExactKeyPassingAllGates
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRegenerationRecordV3 {
    policy_version: u32,
    policy: CanonicalRegenerationPolicyRecordV3,
    selected_key: Option<CorpusCandidateKeyRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomRecordV3 {
    room_id: RoomId,
    physical_descriptor: PhysicalRoomDescriptorRecordV3,
    physical_evidence_source_key: CorpusCandidateKeyRecord,
    variants: Vec<NativeVariantRecordV3>,
    matrix_search_effort: Vec<MatrixSearchEffortRecordV3>,
    variant_construction_gates: Vec<VariantConstructionGateRecordV3>,
    ability_promotion_audit_config: CorpusRoomAnalysisConfigRecord,
    variant_ability_promotion_gates: Vec<VariantAbilityPromotionGateRecordV3>,
    complete_kit_gate: FeasibilityGateRecordV3,
    canonical_regeneration: CanonicalRegenerationRecordV3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InconclusiveReasonRecordV3 {
    NoExitsDefined,
    ExpandedNodeBudget,
    SimulatedTickBudget,
    PathHorizon,
    FrontierExhausted,
}

impl From<InconclusiveReason> for InconclusiveReasonRecordV3 {
    fn from(reason: InconclusiveReason) -> Self {
        match reason {
            InconclusiveReason::NoExitsDefined => Self::NoExitsDefined,
            InconclusiveReason::ExpandedNodeBudget => Self::ExpandedNodeBudget,
            InconclusiveReason::SimulatedTickBudget => Self::SimulatedTickBudget,
            InconclusiveReason::PathHorizon => Self::PathHorizon,
            InconclusiveReason::FrontierExhausted => Self::FrontierExhausted,
        }
    }
}

impl From<InconclusiveReasonRecordV3> for InconclusiveReason {
    fn from(reason: InconclusiveReasonRecordV3) -> Self {
        match reason {
            InconclusiveReasonRecordV3::NoExitsDefined => Self::NoExitsDefined,
            InconclusiveReasonRecordV3::ExpandedNodeBudget => Self::ExpandedNodeBudget,
            InconclusiveReasonRecordV3::SimulatedTickBudget => Self::SimulatedTickBudget,
            InconclusiveReasonRecordV3::PathHorizon => Self::PathHorizon,
            InconclusiveReasonRecordV3::FrontierExhausted => Self::FrontierExhausted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchStatsRecordV3 {
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSearchEffortRecordV3 {
    source_door_id: String,
    search_effort: SearchStatsRecordV3,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MatrixSearchEffortRecordV3 {
    loadout: EvaluationLoadout,
    source_search_effort: Vec<SourceSearchEffortRecordV3>,
    aggregate_search_effort: SearchStatsRecordV3,
}

impl From<SearchStats> for SearchStatsRecordV3 {
    fn from(stats: SearchStats) -> Self {
        Self {
            expanded_nodes: stats.expanded_nodes,
            generated_nodes: stats.generated_nodes,
            simulated_ticks: stats.simulated_ticks,
            deepest_path_ticks: stats.deepest_path_ticks,
        }
    }
}

impl From<SearchStatsRecordV3> for SearchStats {
    fn from(stats: SearchStatsRecordV3) -> Self {
        Self {
            expanded_nodes: stats.expanded_nodes,
            generated_nodes: stats.generated_nodes,
            simulated_ticks: stats.simulated_ticks,
            deepest_path_ticks: stats.deepest_path_ticks,
        }
    }
}

/// Lossless strict DTO for the small finite direct-controller audit embedded
/// in an ability-promotion gate. Ordinary route matrices never use this
/// record: their positives are replay-rehydrated and their bounded cells stay
/// bounded without rerunning search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteControllerAssessmentRecordV3 {
    policy: RouteControllerAssessmentPolicy,
    source_door_id: String,
    target_door_id: String,
    authoritative_loadout: EvaluationLoadout,
    expected_subset_loadouts: Vec<EvaluationLoadout>,
    audits: Vec<LoadoutControllerAuditRecordV3>,
    completeness: RouteControllerAuditCompletenessRecordV3,
    easiest_first_witnesses: Vec<RouteControllerWitnessRecordV3>,
    easiest_known_front: Vec<usize>,
    positive_bypasses: Vec<PositiveBypassEvidenceRecordV3>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadoutControllerAuditRecordV3 {
    loadout: EvaluationLoadout,
    status: LoadoutControllerAuditStatusRecordV3,
    operational_stats: SearchStatsRecordV3,
    raw_positive_witnesses: usize,
    retained_semantic_witnesses: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum LoadoutControllerAuditStatusRecordV3 {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        limit: DirectProbeBudgetLimitRecordV3,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DirectProbeBudgetLimitRecordV3 {
    ExpandedNodes,
    SimulatedTicks,
}

impl From<DirectProbeBudgetLimit> for DirectProbeBudgetLimitRecordV3 {
    fn from(limit: DirectProbeBudgetLimit) -> Self {
        match limit {
            DirectProbeBudgetLimit::ExpandedNodes => Self::ExpandedNodes,
            DirectProbeBudgetLimit::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

impl From<DirectProbeBudgetLimitRecordV3> for DirectProbeBudgetLimit {
    fn from(limit: DirectProbeBudgetLimitRecordV3) -> Self {
        match limit {
            DirectProbeBudgetLimitRecordV3::ExpandedNodes => Self::ExpandedNodes,
            DirectProbeBudgetLimitRecordV3::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

impl From<LoadoutControllerAuditStatus> for LoadoutControllerAuditStatusRecordV3 {
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

impl From<LoadoutControllerAuditStatusRecordV3> for LoadoutControllerAuditStatus {
    fn from(status: LoadoutControllerAuditStatusRecordV3) -> Self {
        match status {
            LoadoutControllerAuditStatusRecordV3::CompleteFiniteVocabulary => {
                Self::CompleteFiniteVocabulary
            }
            LoadoutControllerAuditStatusRecordV3::BoundedIncomplete { limit } => {
                Self::BoundedIncomplete {
                    limit: limit.into(),
                }
            }
        }
    }
}

impl From<&LoadoutControllerAudit> for LoadoutControllerAuditRecordV3 {
    fn from(audit: &LoadoutControllerAudit) -> Self {
        Self {
            loadout: audit.loadout,
            status: audit.status.into(),
            operational_stats: audit.operational_stats.into(),
            raw_positive_witnesses: audit.raw_positive_witnesses,
            retained_semantic_witnesses: audit.retained_semantic_witnesses,
        }
    }
}

impl From<LoadoutControllerAuditRecordV3> for LoadoutControllerAudit {
    fn from(audit: LoadoutControllerAuditRecordV3) -> Self {
        Self {
            loadout: audit.loadout,
            status: audit.status.into(),
            operational_stats: audit.operational_stats.into(),
            raw_positive_witnesses: audit.raw_positive_witnesses,
            retained_semantic_witnesses: audit.retained_semantic_witnesses,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum RouteControllerAuditCompletenessRecordV3 {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        incomplete_loadouts: Vec<BoundedIncompleteLoadoutRecordV3>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundedIncompleteLoadoutRecordV3 {
    loadout: EvaluationLoadout,
    limit: DirectProbeBudgetLimitRecordV3,
}

impl From<&RouteControllerAuditCompleteness> for RouteControllerAuditCompletenessRecordV3 {
    fn from(completeness: &RouteControllerAuditCompleteness) -> Self {
        match completeness {
            RouteControllerAuditCompleteness::CompleteFiniteVocabulary => {
                Self::CompleteFiniteVocabulary
            }
            RouteControllerAuditCompleteness::BoundedIncomplete {
                incomplete_loadouts,
            } => Self::BoundedIncomplete {
                incomplete_loadouts: incomplete_loadouts
                    .iter()
                    .map(|incomplete| BoundedIncompleteLoadoutRecordV3 {
                        loadout: incomplete.loadout,
                        limit: incomplete.limit.into(),
                    })
                    .collect(),
            },
        }
    }
}

impl From<RouteControllerAuditCompletenessRecordV3> for RouteControllerAuditCompleteness {
    fn from(completeness: RouteControllerAuditCompletenessRecordV3) -> Self {
        match completeness {
            RouteControllerAuditCompletenessRecordV3::CompleteFiniteVocabulary => {
                Self::CompleteFiniteVocabulary
            }
            RouteControllerAuditCompletenessRecordV3::BoundedIncomplete {
                incomplete_loadouts,
            } => Self::BoundedIncomplete {
                incomplete_loadouts: incomplete_loadouts
                    .into_iter()
                    .map(|incomplete| BoundedIncompleteLoadout {
                        loadout: incomplete.loadout,
                        limit: incomplete.limit.into(),
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteControllerWitnessRecordV3 {
    loadout: EvaluationLoadout,
    initial_digest: String,
    total_ticks: usize,
    action_encoding_version: u32,
    actions: Vec<ActionSpanRecordV3>,
    semantic_trace: SemanticActionTraceRecordV3,
    demand: ControllerDemandRecordV3,
    probes: Vec<DirectProbeProvenanceRecordV3>,
    operational_discovery_stats: SearchStatsRecordV3,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticActionTraceRecordV3 {
    total_ticks: usize,
    spans: Vec<SemanticActionSpanRecordV3>,
    events: Vec<SemanticEventAtRecordV3>,
    jump_presses: usize,
    dash_presses: usize,
    restart_presses: usize,
    successful_jumps: usize,
    successful_wall_jumps: usize,
    successful_dashes: usize,
    deaths: usize,
    pickups_collected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticActionSpanRecordV3 {
    action: SemanticActionRecordV3,
    ticks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticActionRecordV3 {
    move_x: i8,
    move_y: i8,
    jump_held: bool,
    dash_held: bool,
    restart: bool,
}

impl From<SemanticAction> for SemanticActionRecordV3 {
    fn from(action: SemanticAction) -> Self {
        Self {
            move_x: action.move_x,
            move_y: action.move_y,
            jump_held: action.jump_held,
            dash_held: action.dash_held,
            restart: action.restart,
        }
    }
}

impl From<SemanticActionRecordV3> for SemanticAction {
    fn from(action: SemanticActionRecordV3) -> Self {
        Self {
            move_x: action.move_x,
            move_y: action.move_y,
            jump_held: action.jump_held,
            dash_held: action.dash_held,
            restart: action.restart,
        }
    }
}

impl From<SemanticActionSpan> for SemanticActionSpanRecordV3 {
    fn from(span: SemanticActionSpan) -> Self {
        Self {
            action: span.action.into(),
            ticks: span.ticks,
        }
    }
}

impl From<SemanticActionSpanRecordV3> for SemanticActionSpan {
    fn from(span: SemanticActionSpanRecordV3) -> Self {
        Self {
            action: span.action.into(),
            ticks: span.ticks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticEventAtRecordV3 {
    tick: usize,
    event: SemanticEventRecordV3,
}

impl From<SemanticEventAt> for SemanticEventAtRecordV3 {
    fn from(event: SemanticEventAt) -> Self {
        Self {
            tick: event.tick,
            event: event.event.into(),
        }
    }
}

impl From<SemanticEventAtRecordV3> for SemanticEventAt {
    fn from(event: SemanticEventAtRecordV3) -> Self {
        Self {
            tick: event.tick,
            event: event.event.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
enum SemanticEventRecordV3 {
    GroundJump,
    CoyoteJump,
    BufferedJump,
    WallJump { side: WallSideRecordV3 },
    Dash { direction: DashDirectionRecordV3 },
    Land,
    DeathFromStaticHazard,
    DeathFromTimedHazard,
    Pickup,
    Reset,
    Exit,
}

impl From<SemanticEvent> for SemanticEventRecordV3 {
    fn from(event: SemanticEvent) -> Self {
        match event {
            SemanticEvent::GroundJump => Self::GroundJump,
            SemanticEvent::CoyoteJump => Self::CoyoteJump,
            SemanticEvent::BufferedJump => Self::BufferedJump,
            SemanticEvent::WallJump(side) => Self::WallJump { side: side.into() },
            SemanticEvent::Dash(direction) => Self::Dash {
                direction: direction.into(),
            },
            SemanticEvent::Land => Self::Land,
            SemanticEvent::DeathFromStaticHazard => Self::DeathFromStaticHazard,
            SemanticEvent::DeathFromTimedHazard => Self::DeathFromTimedHazard,
            SemanticEvent::Pickup => Self::Pickup,
            SemanticEvent::Reset => Self::Reset,
            SemanticEvent::Exit => Self::Exit,
        }
    }
}

impl From<SemanticEventRecordV3> for SemanticEvent {
    fn from(event: SemanticEventRecordV3) -> Self {
        match event {
            SemanticEventRecordV3::GroundJump => Self::GroundJump,
            SemanticEventRecordV3::CoyoteJump => Self::CoyoteJump,
            SemanticEventRecordV3::BufferedJump => Self::BufferedJump,
            SemanticEventRecordV3::WallJump { side } => Self::WallJump(side.into()),
            SemanticEventRecordV3::Dash { direction } => Self::Dash(direction.into()),
            SemanticEventRecordV3::Land => Self::Land,
            SemanticEventRecordV3::DeathFromStaticHazard => Self::DeathFromStaticHazard,
            SemanticEventRecordV3::DeathFromTimedHazard => Self::DeathFromTimedHazard,
            SemanticEventRecordV3::Pickup => Self::Pickup,
            SemanticEventRecordV3::Reset => Self::Reset,
            SemanticEventRecordV3::Exit => Self::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WallSideRecordV3 {
    Left,
    Right,
}

impl From<WallSide> for WallSideRecordV3 {
    fn from(side: WallSide) -> Self {
        match side {
            WallSide::Left => Self::Left,
            WallSide::Right => Self::Right,
        }
    }
}

impl From<WallSideRecordV3> for WallSide {
    fn from(side: WallSideRecordV3) -> Self {
        match side {
            WallSideRecordV3::Left => Self::Left,
            WallSideRecordV3::Right => Self::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DashDirectionRecordV3 {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl From<DashDirection> for DashDirectionRecordV3 {
    fn from(direction: DashDirection) -> Self {
        match direction {
            DashDirection::Up => Self::Up,
            DashDirection::UpRight => Self::UpRight,
            DashDirection::Right => Self::Right,
            DashDirection::DownRight => Self::DownRight,
            DashDirection::Down => Self::Down,
            DashDirection::DownLeft => Self::DownLeft,
            DashDirection::Left => Self::Left,
            DashDirection::UpLeft => Self::UpLeft,
        }
    }
}

impl From<DashDirectionRecordV3> for DashDirection {
    fn from(direction: DashDirectionRecordV3) -> Self {
        match direction {
            DashDirectionRecordV3::Up => Self::Up,
            DashDirectionRecordV3::UpRight => Self::UpRight,
            DashDirectionRecordV3::Right => Self::Right,
            DashDirectionRecordV3::DownRight => Self::DownRight,
            DashDirectionRecordV3::Down => Self::Down,
            DashDirectionRecordV3::DownLeft => Self::DownLeft,
            DashDirectionRecordV3::Left => Self::Left,
            DashDirectionRecordV3::UpLeft => Self::UpLeft,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerDemandRecordV3 {
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

impl From<ControllerDemand> for ControllerDemandRecordV3 {
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

impl From<ControllerDemandRecordV3> for ControllerDemand {
    fn from(demand: ControllerDemandRecordV3) -> Self {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectProbeProvenanceRecordV3 {
    ordinal: usize,
    move_x: i8,
    policy: DirectProbePolicyRecordV3,
}

impl From<DirectProbeProvenance> for DirectProbeProvenanceRecordV3 {
    fn from(provenance: DirectProbeProvenance) -> Self {
        Self {
            ordinal: provenance.ordinal,
            move_x: provenance.move_x,
            policy: provenance.policy.into(),
        }
    }
}

impl From<DirectProbeProvenanceRecordV3> for DirectProbeProvenance {
    fn from(provenance: DirectProbeProvenanceRecordV3) -> Self {
        Self {
            ordinal: provenance.ordinal,
            move_x: provenance.move_x,
            policy: provenance.policy.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "kebab-case", deny_unknown_fields)]
enum DirectProbePolicyRecordV3 {
    DropThrough,
    Run,
    PeriodicJump {
        period: usize,
        hold_ticks: usize,
        phase: usize,
    },
    ReactiveJump {
        lookahead_pixels: i32,
        hold_ticks: u8,
    },
    AutoJump,
    WallClimb,
    BufferedWallClimb,
    Dash {
        move_y: i8,
        jump_period: usize,
        jump_hold_ticks: usize,
        dash_delay_ticks: usize,
    },
    StagedDash {
        period: usize,
        jump_start: usize,
        jump_hold_ticks: usize,
        dash_tick: usize,
    },
    ReactiveDash {
        lookahead_pixels: i32,
        move_y: i8,
    },
    DetourJump {
        turn_tick: usize,
        period: usize,
        hold_ticks: usize,
        phase: usize,
    },
    DetourClimb {
        turn_tick: usize,
        lookahead_pixels: i32,
        hold_ticks: u8,
    },
    DetourHomingClimb {
        turn_tick: usize,
        target_center_x: i32,
        deadzone_pixels: i32,
        hold_ticks: u8,
    },
}

impl From<DirectProbePolicy> for DirectProbePolicyRecordV3 {
    fn from(policy: DirectProbePolicy) -> Self {
        match policy {
            DirectProbePolicy::DropThrough => Self::DropThrough,
            DirectProbePolicy::Run => Self::Run,
            DirectProbePolicy::PeriodicJump {
                period,
                hold_ticks,
                phase,
            } => Self::PeriodicJump {
                period,
                hold_ticks,
                phase,
            },
            DirectProbePolicy::ReactiveJump {
                lookahead_pixels,
                hold_ticks,
            } => Self::ReactiveJump {
                lookahead_pixels,
                hold_ticks,
            },
            DirectProbePolicy::AutoJump => Self::AutoJump,
            DirectProbePolicy::WallClimb => Self::WallClimb,
            DirectProbePolicy::BufferedWallClimb => Self::BufferedWallClimb,
            DirectProbePolicy::Dash {
                move_y,
                jump_period,
                jump_hold_ticks,
                dash_delay_ticks,
            } => Self::Dash {
                move_y,
                jump_period,
                jump_hold_ticks,
                dash_delay_ticks,
            },
            DirectProbePolicy::StagedDash {
                period,
                jump_start,
                jump_hold_ticks,
                dash_tick,
            } => Self::StagedDash {
                period,
                jump_start,
                jump_hold_ticks,
                dash_tick,
            },
            DirectProbePolicy::ReactiveDash {
                lookahead_pixels,
                move_y,
            } => Self::ReactiveDash {
                lookahead_pixels,
                move_y,
            },
            DirectProbePolicy::DetourJump {
                turn_tick,
                period,
                hold_ticks,
                phase,
            } => Self::DetourJump {
                turn_tick,
                period,
                hold_ticks,
                phase,
            },
            DirectProbePolicy::DetourClimb {
                turn_tick,
                lookahead_pixels,
                hold_ticks,
            } => Self::DetourClimb {
                turn_tick,
                lookahead_pixels,
                hold_ticks,
            },
            DirectProbePolicy::DetourHomingClimb {
                turn_tick,
                target_center_x,
                deadzone_pixels,
                hold_ticks,
            } => Self::DetourHomingClimb {
                turn_tick,
                target_center_x,
                deadzone_pixels,
                hold_ticks,
            },
        }
    }
}

impl From<DirectProbePolicyRecordV3> for DirectProbePolicy {
    fn from(policy: DirectProbePolicyRecordV3) -> Self {
        match policy {
            DirectProbePolicyRecordV3::DropThrough => Self::DropThrough,
            DirectProbePolicyRecordV3::Run => Self::Run,
            DirectProbePolicyRecordV3::PeriodicJump {
                period,
                hold_ticks,
                phase,
            } => Self::PeriodicJump {
                period,
                hold_ticks,
                phase,
            },
            DirectProbePolicyRecordV3::ReactiveJump {
                lookahead_pixels,
                hold_ticks,
            } => Self::ReactiveJump {
                lookahead_pixels,
                hold_ticks,
            },
            DirectProbePolicyRecordV3::AutoJump => Self::AutoJump,
            DirectProbePolicyRecordV3::WallClimb => Self::WallClimb,
            DirectProbePolicyRecordV3::BufferedWallClimb => Self::BufferedWallClimb,
            DirectProbePolicyRecordV3::Dash {
                move_y,
                jump_period,
                jump_hold_ticks,
                dash_delay_ticks,
            } => Self::Dash {
                move_y,
                jump_period,
                jump_hold_ticks,
                dash_delay_ticks,
            },
            DirectProbePolicyRecordV3::StagedDash {
                period,
                jump_start,
                jump_hold_ticks,
                dash_tick,
            } => Self::StagedDash {
                period,
                jump_start,
                jump_hold_ticks,
                dash_tick,
            },
            DirectProbePolicyRecordV3::ReactiveDash {
                lookahead_pixels,
                move_y,
            } => Self::ReactiveDash {
                lookahead_pixels,
                move_y,
            },
            DirectProbePolicyRecordV3::DetourJump {
                turn_tick,
                period,
                hold_ticks,
                phase,
            } => Self::DetourJump {
                turn_tick,
                period,
                hold_ticks,
                phase,
            },
            DirectProbePolicyRecordV3::DetourClimb {
                turn_tick,
                lookahead_pixels,
                hold_ticks,
            } => Self::DetourClimb {
                turn_tick,
                lookahead_pixels,
                hold_ticks,
            },
            DirectProbePolicyRecordV3::DetourHomingClimb {
                turn_tick,
                target_center_x,
                deadzone_pixels,
                hold_ticks,
            } => Self::DetourHomingClimb {
                turn_tick,
                target_center_x,
                deadzone_pixels,
                hold_ticks,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PositiveBypassEvidenceRecordV3 {
    loadout: EvaluationLoadout,
    witness_indices: Vec<usize>,
}

impl From<&PositiveBypassEvidence> for PositiveBypassEvidenceRecordV3 {
    fn from(evidence: &PositiveBypassEvidence) -> Self {
        Self {
            loadout: evidence.loadout,
            witness_indices: evidence.witness_indices.clone(),
        }
    }
}

impl From<PositiveBypassEvidenceRecordV3> for PositiveBypassEvidence {
    fn from(evidence: PositiveBypassEvidenceRecordV3) -> Self {
        Self {
            loadout: evidence.loadout,
            witness_indices: evidence.witness_indices,
        }
    }
}

fn route_controller_witness_record_v3(
    witness: &RouteControllerWitness,
) -> Result<RouteControllerWitnessRecordV3, CorpusArtifactV3Error> {
    Ok(RouteControllerWitnessRecordV3 {
        loadout: witness.loadout,
        initial_digest: witness.replay.initial_digest.to_string(),
        total_ticks: witness.replay.frames.len(),
        action_encoding_version: CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION,
        actions: encode_actions_v3(witness.replay.actions())?,
        semantic_trace: SemanticActionTraceRecordV3::from(&witness.semantic_trace),
        demand: witness.demand.into(),
        probes: witness.probes.iter().copied().map(Into::into).collect(),
        operational_discovery_stats: witness.operational_discovery_stats.into(),
    })
}

impl From<&SemanticActionTrace> for SemanticActionTraceRecordV3 {
    fn from(trace: &SemanticActionTrace) -> Self {
        Self {
            total_ticks: trace.total_ticks,
            spans: trace.spans.iter().copied().map(Into::into).collect(),
            events: trace.events.iter().copied().map(Into::into).collect(),
            jump_presses: trace.jump_presses,
            dash_presses: trace.dash_presses,
            restart_presses: trace.restart_presses,
            successful_jumps: trace.successful_jumps,
            successful_wall_jumps: trace.successful_wall_jumps,
            successful_dashes: trace.successful_dashes,
            deaths: trace.deaths,
            pickups_collected: trace.pickups_collected,
        }
    }
}

impl From<SemanticActionTraceRecordV3> for SemanticActionTrace {
    fn from(trace: SemanticActionTraceRecordV3) -> Self {
        Self {
            total_ticks: trace.total_ticks,
            spans: trace
                .spans
                .into_iter()
                .map(Into::into)
                .collect::<Vec<SemanticActionSpan>>()
                .into_boxed_slice(),
            events: trace
                .events
                .into_iter()
                .map(Into::into)
                .collect::<Vec<SemanticEventAt>>()
                .into_boxed_slice(),
            jump_presses: trace.jump_presses,
            dash_presses: trace.dash_presses,
            restart_presses: trace.restart_presses,
            successful_jumps: trace.successful_jumps,
            successful_wall_jumps: trace.successful_wall_jumps,
            successful_dashes: trace.successful_dashes,
            deaths: trace.deaths,
            pickups_collected: trace.pickups_collected,
        }
    }
}

fn route_controller_assessment_record_v3(
    assessment: &RouteControllerAssessment,
) -> Result<RouteControllerAssessmentRecordV3, CorpusArtifactV3Error> {
    Ok(RouteControllerAssessmentRecordV3 {
        policy: assessment.policy,
        source_door_id: assessment.source_door_id.clone(),
        target_door_id: assessment.target_door_id.clone(),
        authoritative_loadout: assessment.authoritative_loadout,
        expected_subset_loadouts: assessment.expected_subset_loadouts.clone(),
        audits: assessment.audits.iter().map(Into::into).collect(),
        completeness: (&assessment.completeness).into(),
        easiest_first_witnesses: assessment
            .easiest_first_witnesses
            .iter()
            .map(route_controller_witness_record_v3)
            .collect::<Result<_, _>>()?,
        easiest_known_front: assessment.easiest_known_front.clone(),
        positive_bypasses: assessment
            .positive_bypasses
            .iter()
            .map(Into::into)
            .collect(),
    })
}

fn rehydrate_route_controller_assessment_record_v3(
    record: RouteControllerAssessmentRecordV3,
    candidate: &CorpusCandidate,
) -> Result<RouteControllerAssessment, CorpusArtifactV3Error> {
    let expected_record = record.clone();
    let source_door_id = record.source_door_id.clone();
    let target_door_id = record.target_door_id.clone();
    let mut witnesses = Vec::with_capacity(record.easiest_first_witnesses.len());
    for (index, witness) in record.easiest_first_witnesses.into_iter().enumerate() {
        if witness.action_encoding_version != CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION {
            return Err(invalid(format!(
                "ability-promotion direct witness {index} uses unsupported action encoding {}",
                witness.action_encoding_version
            )));
        }
        let actions = decode_action_spans_v3(
            &format!("ability-promotion direct witness {index}"),
            witness.total_ticks,
            &witness.actions,
        )?;
        let initial = Simulation::enter_via_door(
            candidate.generated().room.clone(),
            witness.loadout.abilities(),
            &source_door_id,
        )
        .map_err(|error| {
            invalid(format!(
                "ability-promotion direct witness {index} cannot enter source door {source_door_id:?}: {error}"
            ))
        })?;
        let expected_initial = parse_digest_v3(&witness.initial_digest)?;
        if initial.digest() != expected_initial {
            return Err(invalid(format!(
                "ability-promotion direct witness {index} initial digest differs from exact alias"
            )));
        }
        let replay = Replay::record(&initial, actions);
        let verification = replay.verify(&initial).map_err(|error| {
            invalid(format!(
                "ability-promotion direct witness {index} replay diverged: {error}"
            ))
        })?;
        if verification.reached_exit.as_deref() != Some(target_door_id.as_str()) {
            return Err(invalid(format!(
                "ability-promotion direct witness {index} did not reach advertised sink {target_door_id:?}"
            )));
        }
        let semantic_trace = SemanticActionTrace::from(witness.semantic_trace);
        if semantic_trace.total_ticks != witness.total_ticks {
            return Err(invalid(format!(
                "ability-promotion direct witness {index} semantic/action tick totals differ"
            )));
        }
        witnesses.push(RouteControllerWitness {
            loadout: witness.loadout,
            replay,
            semantic_trace,
            demand: witness.demand.into(),
            probes: witness.probes.into_iter().map(Into::into).collect(),
            operational_discovery_stats: witness.operational_discovery_stats.into(),
        });
    }
    let assessment = RouteControllerAssessment {
        policy: record.policy,
        source_door_id,
        target_door_id,
        authoritative_loadout: record.authoritative_loadout,
        expected_subset_loadouts: record.expected_subset_loadouts,
        audits: record.audits.into_iter().map(Into::into).collect(),
        completeness: record.completeness.into(),
        easiest_first_witnesses: witnesses,
        easiest_known_front: record.easiest_known_front,
        positive_bypasses: record
            .positive_bypasses
            .into_iter()
            .map(Into::into)
            .collect(),
    };
    if route_controller_assessment_record_v3(&assessment)? != expected_record {
        return Err(invalid(
            "ability-promotion direct assessment did not round-trip exactly",
        ));
    }
    Ok(assessment)
}

fn decode_action_spans_v3(
    label: &str,
    expected_ticks: usize,
    spans: &[ActionSpanRecordV3],
) -> Result<Vec<Action>, CorpusArtifactV3Error> {
    if expected_ticks > MAX_VERIFIABLE_WITNESS_TICKS_V3 {
        return Err(invalid(format!(
            "{label} has {expected_ticks} ticks above verifier limit {MAX_VERIFIABLE_WITNESS_TICKS_V3}"
        )));
    }
    let mut total_ticks = 0usize;
    let mut previous = None;
    let mut actions = Vec::with_capacity(expected_ticks);
    for (span_index, span) in spans.iter().copied().enumerate() {
        if span.ticks == 0 {
            return Err(invalid(format!(
                "{label} action span {span_index} has zero ticks"
            )));
        }
        if !(-1..=1).contains(&span.move_x) || !(-1..=1).contains(&span.move_y) {
            return Err(invalid(format!(
                "{label} action span {span_index} is not normalized"
            )));
        }
        let action = span.action();
        if previous == Some(action) {
            return Err(invalid(format!("{label} has adjacent equal RLE spans")));
        }
        previous = Some(action);
        total_ticks = total_ticks
            .checked_add(span.ticks)
            .ok_or_else(|| invalid(format!("{label} tick total overflow")))?;
        actions.extend(std::iter::repeat_n(action, span.ticks));
    }
    if total_ticks != expected_ticks {
        return Err(invalid(format!(
            "{label} RLE ticks {total_ticks} differ from recorded {expected_ticks}"
        )));
    }
    Ok(actions)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum TargetEvidenceRecordV3 {
    PositiveReplay {
        witness_id: String,
    },
    BoundedInconclusive {
        reason: InconclusiveReasonRecordV3,
        search_effort: SearchStatsRecordV3,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetRowRecordV3 {
    room_id: RoomId,
    source_door_id: String,
    target_id: String,
    loadout: EvaluationLoadout,
    evidence: TargetEvidenceRecordV3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionSpanRecordV3 {
    ticks: usize,
    move_x: i8,
    move_y: i8,
    jump: bool,
    dash: bool,
}

impl ActionSpanRecordV3 {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WitnessRecordV3 {
    witness_id: String,
    room_id: RoomId,
    source_door_id: String,
    target_id: String,
    loadout: EvaluationLoadout,
    initial_digest: String,
    total_ticks: usize,
    search_effort: SearchStatsRecordV3,
    action_encoding_version: u32,
    actions: Vec<ActionSpanRecordV3>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedCorpusArtifactV3 {
    pub config: CorpusBuildConfigV2,
    pub config_id: String,
    pub evaluation_configs: Vec<RouteEvaluationConfigV2>,
    pub ability_promotion_audit_config: CorpusRoomAnalysisConfigRecord,
    pub generation: GenerationBatchSummaryV2,
    pub summary: CorpusArtifactSummaryV3,
    pub room_ids: Vec<RoomId>,
}

/// Process-local result of one complete semantic verification pass.  This is
/// deliberately not serialized: callers may reuse the exact replay-certified
/// batch only while they retain the verified input bytes in this process.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VerifiedRehydratedArtifactBundleV3 {
    pub verified: VerifiedCorpusArtifactV3,
    pub evaluated: EvaluatedCorpusBatchV2,
}

#[cfg(test)]
thread_local! {
    static SEMANTIC_VERIFIER_PASSES_V3: Cell<usize> = const { Cell::new(0) };
    static BEFORE_SHARD_STABILITY_CHECK_V3: RefCell<Option<Box<dyn FnOnce()>>> =
        const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn reset_semantic_verifier_passes_v3() {
    SEMANTIC_VERIFIER_PASSES_V3.set(0);
}

#[cfg(test)]
pub(super) fn semantic_verifier_passes_v3() -> usize {
    SEMANTIC_VERIFIER_PASSES_V3.get()
}

fn record_semantic_verifier_pass_v3() {
    #[cfg(test)]
    SEMANTIC_VERIFIER_PASSES_V3.set(SEMANTIC_VERIFIER_PASSES_V3.get().saturating_add(1));
}

#[cfg(test)]
pub(super) fn set_before_shard_stability_check_v3(hook: impl FnOnce() + 'static) {
    BEFORE_SHARD_STABILITY_CHECK_V3.with(|slot| {
        let previous = slot.replace(Some(Box::new(hook)));
        assert!(
            previous.is_none(),
            "a shard-stability test hook is already set"
        );
    });
}

fn run_before_shard_stability_check_v3() {
    #[cfg(test)]
    BEFORE_SHARD_STABILITY_CHECK_V3.with(|slot| {
        if let Some(hook) = slot.take() {
            hook();
        }
    });
}

pub(super) struct ArtifactRehydrationMatrixV3 {
    pub loadout: EvaluationLoadout,
    pub door_routes: Vec<ArtifactRehydrationDoorRowV3>,
    pub pickup_routes: Vec<ArtifactRehydrationPickupRowV3>,
    pub source_search_effort: Vec<downwards_validation::DoorSourceSearchEffort>,
    pub aggregate_search_effort: SearchStats,
}

pub(super) struct ArtifactRehydrationDoorRowV3 {
    pub source_door_id: String,
    pub target_door_id: String,
    pub evidence: ArtifactRehydrationEvidenceV3,
}

pub(super) struct ArtifactRehydrationPickupRowV3 {
    pub source_door_id: String,
    pub required_pickup_id: String,
    pub evidence: ArtifactRehydrationEvidenceV3,
}

pub(super) enum ArtifactRehydrationEvidenceV3 {
    PositiveReplay {
        witness_id: String,
        initial_digest: StateDigest,
        actions: Vec<Action>,
        search_effort: SearchStats,
    },
    BoundedInconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
}

/// Render one complete single-seed construction/evaluation shard.
pub fn render_evaluated_artifact_v3(
    evaluated: &EvaluatedCorpusBatchV2,
) -> Result<CorpusArtifactBundleV3, CorpusArtifactV3Error> {
    evaluated
        .config
        .validate()
        .map_err(|error| invalid(format!("invalid corpus-v2 build config: {error}")))?;
    if evaluated.config.seed_count != 1 {
        return Err(invalid(format!(
            "corpus artifact v3 requires seed_count=1, found {}",
            evaluated.config.seed_count
        )));
    }
    validate_route_evaluation_configs_v2(&evaluated.evaluation_configs)
        .map_err(|error| invalid(format!("invalid route-evaluation config table: {error}")))?;
    let default_ability_promotion_audit_config = CorpusRoomAnalysisConfig::default()
        .identity_record()
        .map_err(|error| invalid(format!("invalid default promotion-audit config: {error}")))?;
    let ability_promotion_audit_config = evaluated
        .rooms
        .first()
        .map(|room| room.ability_promotion_audit_config.clone())
        .unwrap_or(default_ability_promotion_audit_config);
    ability_promotion_audit_config
        .validate()
        .map_err(|error| invalid(format!("invalid artifact promotion-audit config: {error}")))?;
    if evaluated
        .rooms
        .iter()
        .any(|room| room.ability_promotion_audit_config != ability_promotion_audit_config)
    {
        return Err(invalid(
            "evaluated rooms do not share one exact promotion-audit config",
        ));
    }

    let mut attempts = evaluated
        .construction_records
        .iter()
        .map(AttemptRecordV3::from)
        .collect::<Vec<_>>();
    attempts.sort_unstable_by_key(|record| record.key.stable_slug());

    let mut rooms = Vec::with_capacity(evaluated.rooms.len());
    let mut routes = Vec::new();
    let mut pickups = Vec::new();
    let mut witnesses = BTreeMap::<String, WitnessRecordV3>::new();
    for evaluated_room in &evaluated.rooms {
        validate_evaluated_corpus_room_v2(evaluated_room).map_err(|error| {
            invalid(format!(
                "evaluated room {:?} is invalid before artifact rendering: {error}",
                evaluated_room.generated.id.0
            ))
        })?;
        let mut variants = evaluated_room
            .generated
            .variants
            .iter()
            .map(NativeVariantRecordV3::from)
            .collect::<Vec<_>>();
        variants.sort_unstable_by_key(|variant| variant.key.stable_slug());
        let mut gates = evaluated_room
            .variant_construction_gates
            .iter()
            .map(VariantConstructionGateRecordV3::from)
            .collect::<Vec<_>>();
        gates.sort_unstable_by_key(|gate| gate.key.stable_slug());
        let mut promotion_gates = evaluated_room
            .variant_ability_promotion_gates
            .iter()
            .map(variant_ability_promotion_gate_record_v3)
            .collect::<Result<Vec<_>, _>>()?;
        promotion_gates.sort_unstable_by_key(|gate| gate.key.stable_slug());
        rooms.push(RoomRecordV3 {
            room_id: evaluated_room.generated.id.clone(),
            physical_descriptor: (&evaluated_room.generated.physical_descriptor).into(),
            physical_evidence_source_key: evaluated_room.physical_evidence_source_key.clone(),
            variants,
            matrix_search_effort: evaluated_room
                .matrices
                .iter()
                .map(|matrix| MatrixSearchEffortRecordV3 {
                    loadout: matrix.loadout,
                    source_search_effort: matrix
                        .evidence
                        .source_search_effort()
                        .iter()
                        .map(|source| SourceSearchEffortRecordV3 {
                            source_door_id: source.source_door_id.clone(),
                            search_effort: source.stats.into(),
                        })
                        .collect(),
                    aggregate_search_effort: matrix.evidence.aggregate_search_effort().into(),
                })
                .collect(),
            variant_construction_gates: gates,
            ability_promotion_audit_config: evaluated_room.ability_promotion_audit_config.clone(),
            variant_ability_promotion_gates: promotion_gates,
            complete_kit_gate: evaluated_room.complete_kit_gate.into(),
            canonical_regeneration: CanonicalRegenerationRecordV3 {
                policy_version: evaluated_room.canonical_regeneration.policy_version,
                policy: evaluated_room.canonical_regeneration.policy.into(),
                selected_key: evaluated_room.canonical_regeneration.selected_key.clone(),
            },
        });

        for matrix in &evaluated_room.matrices {
            for row in matrix.evidence.door_routes() {
                let evidence = encode_evidence_v3(
                    &evaluated_room.generated.id,
                    &row.source_door_id,
                    &row.target_door_id,
                    matrix.loadout,
                    &row.evidence,
                    &mut witnesses,
                )?;
                routes.push(TargetRowRecordV3 {
                    room_id: evaluated_room.generated.id.clone(),
                    source_door_id: row.source_door_id.clone(),
                    target_id: row.target_door_id.clone(),
                    loadout: matrix.loadout,
                    evidence,
                });
            }
            for row in matrix.evidence.pickup_routes() {
                let evidence = encode_evidence_v3(
                    &evaluated_room.generated.id,
                    &row.source_door_id,
                    &row.required_pickup_id,
                    matrix.loadout,
                    &row.evidence,
                    &mut witnesses,
                )?;
                pickups.push(TargetRowRecordV3 {
                    room_id: evaluated_room.generated.id.clone(),
                    source_door_id: row.source_door_id.clone(),
                    target_id: row.required_pickup_id.clone(),
                    loadout: matrix.loadout,
                    evidence,
                });
            }
        }
    }

    rooms.sort_unstable_by(|left, right| left.room_id.cmp(&right.room_id));
    routes.sort_unstable_by(target_row_order_v3);
    pickups.sort_unstable_by(target_row_order_v3);
    let attempts_jsonl = render_json_lines_v3(attempts.iter())?;
    let rooms_jsonl = render_json_lines_v3(rooms.iter())?;
    let routes_jsonl = render_json_lines_v3(routes.iter())?;
    let pickups_jsonl = render_json_lines_v3(pickups.iter())?;
    let witnesses_jsonl = render_json_lines_v3(witnesses.values())?;

    let mut bundle = CorpusArtifactBundleV3 {
        run_json: Vec::new(),
        attempts_jsonl,
        rooms_jsonl,
        routes_jsonl,
        pickups_jsonl,
        witnesses_jsonl,
    };
    let stream_hashes = bundle
        .streams()
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), byte_hash_v3(bytes)))
        .collect();
    let summary = summarize_artifact_v3(&attempts, &rooms, &routes, &pickups, witnesses.len())?;
    let run = RunRecordV3 {
        artifact_schema: "downwards-corpus-generator-neutral-v3".to_owned(),
        artifact_version: CORPUS_ARTIFACT_V3_SCHEMA_VERSION,
        status: "complete".to_owned(),
        config: evaluated.config.clone(),
        config_id: corpus_config_id_v3(&evaluated.config)?,
        evaluation_configs: evaluated.evaluation_configs.clone(),
        ability_promotion_audit_config: ability_promotion_audit_config.clone(),
        policies: ArtifactPolicyRecordV3::current(&ability_promotion_audit_config),
        generation: evaluated.generation_summary,
        summary,
        stream_hashes,
    };
    bundle.run_json = serde_json::to_vec(&run)?;
    bundle.run_json.push(b'\n');
    Ok(bundle)
}

fn encode_evidence_v3(
    room_id: &RoomId,
    source_door_id: &str,
    target_id: &str,
    loadout: EvaluationLoadout,
    evidence: &BoundedTargetEvidence,
    witnesses: &mut BTreeMap<String, WitnessRecordV3>,
) -> Result<TargetEvidenceRecordV3, CorpusArtifactV3Error> {
    match evidence {
        BoundedTargetEvidence::Positive(positive) => {
            let witness_id = positive.witness_fingerprint().to_string();
            let solution = positive.solution();
            let record = WitnessRecordV3 {
                witness_id: witness_id.clone(),
                room_id: room_id.clone(),
                source_door_id: source_door_id.to_owned(),
                target_id: target_id.to_owned(),
                loadout,
                initial_digest: solution.replay.initial_digest.to_string(),
                total_ticks: solution.replay.frames.len(),
                search_effort: solution.stats.into(),
                action_encoding_version: CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION,
                actions: encode_actions_v3(solution.replay.actions())?,
            };
            if let Some(previous) = witnesses.insert(witness_id.clone(), record.clone())
                && previous != record
            {
                return Err(CorpusArtifactV3Error::WitnessIdentityCollision(witness_id));
            }
            Ok(TargetEvidenceRecordV3::PositiveReplay { witness_id })
        }
        BoundedTargetEvidence::Inconclusive(inconclusive) => {
            Ok(TargetEvidenceRecordV3::BoundedInconclusive {
                reason: inconclusive.reason.into(),
                search_effort: inconclusive.search_effort.into(),
            })
        }
    }
}

fn encode_actions_v3(
    actions: impl IntoIterator<Item = Action>,
) -> Result<Vec<ActionSpanRecordV3>, CorpusArtifactV3Error> {
    let mut spans = Vec::<ActionSpanRecordV3>::new();
    for action in actions {
        if action.restart {
            return Err(invalid("positive replay contains restart"));
        }
        if !(-1..=1).contains(&action.move_x) || !(-1..=1).contains(&action.move_y) {
            return Err(invalid("positive replay contains non-normalized movement"));
        }
        let normalized = ActionSpanRecordV3 {
            ticks: 1,
            move_x: action.move_x,
            move_y: action.move_y,
            jump: action.jump,
            dash: action.dash,
        };
        if let Some(previous) = spans.last_mut()
            && previous.action() == normalized.action()
        {
            previous.ticks += 1;
        } else {
            spans.push(normalized);
        }
    }
    Ok(spans)
}

fn target_row_order_v3(left: &TargetRowRecordV3, right: &TargetRowRecordV3) -> std::cmp::Ordering {
    target_row_key_v3(left).cmp(&target_row_key_v3(right))
}

fn target_row_key_v3(row: &TargetRowRecordV3) -> (&RoomId, &str, &str, EvaluationLoadout) {
    (
        &row.room_id,
        &row.source_door_id,
        &row.target_id,
        row.loadout,
    )
}

fn render_json_lines_v3<'a, T: Serialize + 'a>(
    values: impl IntoIterator<Item = &'a T>,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut result = Vec::new();
    for value in values {
        serde_json::to_writer(&mut result, value)?;
        result.push(b'\n');
    }
    Ok(result)
}

fn summarize_artifact_v3(
    attempts: &[AttemptRecordV3],
    rooms: &[RoomRecordV3],
    routes: &[TargetRowRecordV3],
    pickups: &[TargetRowRecordV3],
    witness_count: usize,
) -> Result<CorpusArtifactSummaryV3, CorpusArtifactV3Error> {
    let constructed_attempts = attempts
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.outcome,
                StrictConstructionOutcomeV3::Constructed { .. }
            )
        })
        .count();
    let positive_route_rows = routes
        .iter()
        .filter(|row| matches!(row.evidence, TargetEvidenceRecordV3::PositiveReplay { .. }))
        .count();
    let positive_pickup_rows = pickups
        .iter()
        .filter(|row| matches!(row.evidence, TargetEvidenceRecordV3::PositiveReplay { .. }))
        .count();
    let positive_rows = positive_route_rows
        .checked_add(positive_pickup_rows)
        .ok_or_else(|| invalid("positive evidence count overflow"))?;
    if positive_rows != witness_count {
        return Err(invalid(format!(
            "positive row count {positive_rows} differs from witness count {witness_count}"
        )));
    }
    let native_variants = rooms.iter().map(|room| room.variants.len()).sum::<usize>();
    Ok(CorpusArtifactSummaryV3 {
        attempts: attempts.len(),
        constructed_attempts,
        rejected_attempts: attempts.len().saturating_sub(constructed_attempts),
        physical_rooms: rooms.len(),
        native_variants,
        alias_candidates: native_variants.saturating_sub(rooms.len()),
        route_rows: routes.len(),
        positive_route_rows,
        inconclusive_route_rows: routes.len().saturating_sub(positive_route_rows),
        pickup_rows: pickups.len(),
        positive_pickup_rows,
        inconclusive_pickup_rows: pickups.len().saturating_sub(positive_pickup_rows),
        positive_witnesses: witness_count,
        passing_variant_construction_gates: rooms
            .iter()
            .flat_map(|room| &room.variant_construction_gates)
            .filter(|gate| gate.state.passes())
            .count(),
        ability_aliases: rooms
            .iter()
            .flat_map(|room| &room.variant_ability_promotion_gates)
            .filter(|gate| {
                matches!(
                    gate.evidence,
                    VariantAbilityPromotionEvidenceRecordV3::Ability { .. }
                )
            })
            .count(),
        promoted_ability_aliases: rooms
            .iter()
            .flat_map(|room| &room.variant_ability_promotion_gates)
            .filter(|gate| {
                gate.decision == AbilityPromotionDecisionRecordV3::PromotedStructuralNoKnownBypass
            })
            .count(),
        unpromoted_ability_aliases: rooms
            .iter()
            .flat_map(|room| &room.variant_ability_promotion_gates)
            .filter(|gate| {
                matches!(
                    gate.evidence,
                    VariantAbilityPromotionEvidenceRecordV3::Ability { .. }
                ) && gate.decision
                    != AbilityPromotionDecisionRecordV3::PromotedStructuralNoKnownBypass
            })
            .count(),
        passing_complete_kit_rooms: rooms
            .iter()
            .filter(|room| room.complete_kit_gate.passes())
            .count(),
        rooms_with_canonical_regeneration: rooms
            .iter()
            .filter(|room| room.canonical_regeneration.selected_key.is_some())
            .count(),
    })
}

pub fn corpus_config_id_v3(config: &CorpusBuildConfigV2) -> Result<String, CorpusArtifactV3Error> {
    config
        .validate()
        .map_err(|error| invalid(format!("invalid corpus-v2 build config: {error}")))?;
    let domain = format!("downwards-corpus-build-config-v{}", config.schema_version);
    let mut bytes = domain.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(serde_json::to_vec(config)?);
    Ok(format!("{domain}-{:016x}", fingerprint_bytes_v3(&bytes)))
}

fn byte_hash_v3(bytes: &[u8]) -> String {
    format!("fnv1a64-{:016x}", fingerprint_bytes_v3(bytes))
}

fn fingerprint_bytes_v3(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Strictly parse, regenerate, reconstruct, and replay an in-memory v3 shard.
pub fn verify_artifact_bundle_v3(
    bundle: &CorpusArtifactBundleV3,
) -> Result<VerifiedCorpusArtifactV3, CorpusArtifactV3Error> {
    Ok(verify_rehydrated_artifact_bundle_v3(bundle)?.verified)
}

/// One complete semantic verifier pass whose rich replay-certified result can
/// be consumed by sibling rehydration code without parsing, regenerating, or
/// replaying the same bundle again.
pub(super) fn verify_rehydrated_artifact_bundle_v3(
    bundle: &CorpusArtifactBundleV3,
) -> Result<VerifiedRehydratedArtifactBundleV3, CorpusArtifactV3Error> {
    record_semantic_verifier_pass_v3();
    let mut run_rows: Vec<RunRecordV3> = parse_canonical_jsonl_v3(FILE_NAMES[0], &bundle.run_json)?;
    if run_rows.len() != 1 {
        return Err(invalid(format!(
            "{} must contain exactly one record, found {}",
            FILE_NAMES[0],
            run_rows.len()
        )));
    }
    let run = run_rows.pop().expect("length was checked");
    if run.artifact_schema != "downwards-corpus-generator-neutral-v3"
        || run.artifact_version != CORPUS_ARTIFACT_V3_SCHEMA_VERSION
        || run.status != "complete"
    {
        return Err(invalid(
            "unsupported or incomplete corpus artifact v3 run record",
        ));
    }
    if run.policies != ArtifactPolicyRecordV3::current(&run.ability_promotion_audit_config) {
        return Err(invalid(
            "corpus artifact v3 policy identity differs from this build",
        ));
    }
    run.config
        .validate()
        .map_err(|error| invalid(format!("invalid corpus-v2 build config: {error}")))?;
    if run.config.seed_count != 1 {
        return Err(invalid(format!(
            "corpus artifact v3 requires seed_count=1, found {}",
            run.config.seed_count
        )));
    }
    let expected_config_id = corpus_config_id_v3(&run.config)?;
    if run.config_id != expected_config_id {
        return Err(invalid(format!(
            "corpus artifact config ID mismatch: {:?} != {:?}",
            run.config_id, expected_config_id
        )));
    }
    validate_route_evaluation_configs_v2(&run.evaluation_configs)
        .map_err(|error| invalid(format!("invalid route-evaluation config table: {error}")))?;
    run.ability_promotion_audit_config
        .validate()
        .map_err(|error| invalid(format!("invalid promotion-audit config: {error}")))?;
    verify_stream_hashes_v3(bundle, &run.stream_hashes)?;

    let attempts: Vec<AttemptRecordV3> =
        parse_canonical_jsonl_v3(STREAM_NAMES[0], &bundle.attempts_jsonl)?;
    let rooms: Vec<RoomRecordV3> = parse_canonical_jsonl_v3(STREAM_NAMES[1], &bundle.rooms_jsonl)?;
    let routes: Vec<TargetRowRecordV3> =
        parse_canonical_jsonl_v3(STREAM_NAMES[2], &bundle.routes_jsonl)?;
    let pickups: Vec<TargetRowRecordV3> =
        parse_canonical_jsonl_v3(STREAM_NAMES[3], &bundle.pickups_jsonl)?;
    let witnesses: Vec<WitnessRecordV3> =
        parse_canonical_jsonl_v3(STREAM_NAMES[4], &bundle.witnesses_jsonl)?;

    require_strict_order_v3(
        STREAM_NAMES[0],
        attempts.iter().map(|attempt| attempt.key.stable_slug()),
    )?;
    require_strict_order_v3(STREAM_NAMES[1], rooms.iter().map(|room| &room.room_id))?;
    require_strict_order_v3(STREAM_NAMES[2], routes.iter().map(target_row_key_v3))?;
    require_strict_order_v3(STREAM_NAMES[3], pickups.iter().map(target_row_key_v3))?;
    require_strict_order_v3(
        STREAM_NAMES[4],
        witnesses.iter().map(|witness| &witness.witness_id),
    )?;

    let regenerated = generate_seed_block_v2(run.config.clone())
        .map_err(|error| invalid(format!("recorded seed config did not regenerate: {error}")))?;
    let mut expected_attempts = regenerated
        .construction_records
        .iter()
        .map(AttemptRecordV3::from)
        .collect::<Vec<_>>();
    expected_attempts.sort_unstable_by_key(|record| record.key.stable_slug());
    if attempts != expected_attempts {
        return Err(invalid(
            "attempt stream differs from exact regeneration, including failures",
        ));
    }
    if run.generation != regenerated.summary {
        return Err(invalid(format!(
            "generation summary differs from exact regeneration: {:?} != {:?}",
            run.generation, regenerated.summary
        )));
    }
    if rooms.len() != regenerated.rooms.len() {
        return Err(invalid(format!(
            "room count differs from exact regeneration: {} != {}",
            rooms.len(),
            regenerated.rooms.len()
        )));
    }

    let mut runtime_rooms = HashMap::with_capacity(regenerated.rooms.len());
    for (record, generated) in rooms.iter().zip(&regenerated.rooms) {
        verify_room_construction_v3(record, generated, &run.ability_promotion_audit_config)?;
        if runtime_rooms
            .insert(record.room_id.clone(), generated)
            .is_some()
        {
            return Err(invalid(format!(
                "duplicate physical room ID {:?}",
                record.room_id.0
            )));
        }
    }

    let witness_map = witnesses
        .iter()
        .map(|witness| (witness.witness_id.as_str(), witness))
        .collect::<HashMap<_, _>>();
    let mut witness_references = HashMap::<String, usize>::new();
    verify_target_rows_v3(
        TargetKindV3::Door,
        &routes,
        &runtime_rooms,
        &witness_map,
        &mut witness_references,
    )?;
    verify_target_rows_v3(
        TargetKindV3::Pickup,
        &pickups,
        &runtime_rooms,
        &witness_map,
        &mut witness_references,
    )?;
    for witness in &witnesses {
        match witness_references.get(&witness.witness_id).copied() {
            Some(1) => {}
            Some(count) => {
                return Err(invalid(format!(
                    "positive witness {:?} is referenced {count} times",
                    witness.witness_id
                )));
            }
            None => {
                return Err(invalid(format!(
                    "orphan positive witness {:?}",
                    witness.witness_id
                )));
            }
        }
    }

    verify_recorded_search_observations_v3(
        &rooms,
        &routes,
        &pickups,
        &witness_map,
        &run.evaluation_configs,
    )?;
    let mut runtime_evaluated_rooms = verify_ability_promotion_gates_v3(
        &rooms,
        &routes,
        &pickups,
        &witness_map,
        &runtime_rooms,
        &run.evaluation_configs,
    )?;
    verify_room_gates_v3(&rooms, &routes, &pickups, &runtime_evaluated_rooms)?;
    let summary = summarize_artifact_v3(&attempts, &rooms, &routes, &pickups, witnesses.len())?;
    if run.summary != summary {
        return Err(invalid(format!(
            "artifact summary differs from recomputed streams: {:?} != {:?}",
            run.summary, summary
        )));
    }
    if summary.attempts != run.generation.attempted
        || summary.constructed_attempts != run.generation.constructed
        || summary.rejected_attempts != run.generation.rejected
        || summary.physical_rooms != run.generation.physical_rooms
        || summary.alias_candidates != run.generation.alias_candidates
    {
        return Err(invalid(
            "artifact summary does not agree with regenerated generation accounting",
        ));
    }

    let room_ids = rooms
        .iter()
        .map(|room| room.room_id.clone())
        .collect::<Vec<_>>();
    let evaluated_rooms = room_ids
        .iter()
        .map(|room_id| {
            runtime_evaluated_rooms.remove(room_id).ok_or_else(|| {
                invalid(format!(
                    "room {:?} has no replay-rehydrated runtime value",
                    room_id.0
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !runtime_evaluated_rooms.is_empty() {
        return Err(invalid(
            "semantic verifier produced unmatched replay-rehydrated rooms",
        ));
    }
    let evaluated = EvaluatedCorpusBatchV2 {
        config: run.config.clone(),
        evaluation_configs: run.evaluation_configs.clone(),
        construction_records: regenerated.construction_records.clone(),
        generation_summary: run.generation,
        rooms: evaluated_rooms,
    };
    let verified = VerifiedCorpusArtifactV3 {
        config: run.config,
        config_id: run.config_id,
        evaluation_configs: run.evaluation_configs,
        ability_promotion_audit_config: run.ability_promotion_audit_config,
        generation: run.generation,
        summary,
        room_ids,
    };
    Ok(VerifiedRehydratedArtifactBundleV3 {
        verified,
        evaluated,
    })
}

pub(super) fn verified_rehydration_data_v3(
    bundle: &CorpusArtifactBundleV3,
) -> Result<VerifiedRehydratedArtifactBundleV3, CorpusArtifactV3Error> {
    verify_rehydrated_artifact_bundle_v3(bundle)
}

fn artifact_rehydration_matrices_v3(
    room: &RoomRecordV3,
    route_rows: &[TargetRowRecordV3],
    pickup_rows: &[TargetRowRecordV3],
    witness_map: &HashMap<&str, &WitnessRecordV3>,
) -> Result<Vec<ArtifactRehydrationMatrixV3>, CorpusArtifactV3Error> {
    let mut matrices = Vec::with_capacity(EvaluationLoadout::ALL.len());
    for loadout in EvaluationLoadout::ALL {
        let effort = room
            .matrix_search_effort
            .iter()
            .find(|effort| effort.loadout == loadout)
            .expect("verified room has all four effort records");
        let door_routes = route_rows
            .iter()
            .filter(|row| row.room_id == room.room_id && row.loadout == loadout)
            .map(|row| {
                Ok(ArtifactRehydrationDoorRowV3 {
                    source_door_id: row.source_door_id.clone(),
                    target_door_id: row.target_id.clone(),
                    evidence: rehydration_evidence_v3(row, witness_map)?,
                })
            })
            .collect::<Result<Vec<_>, CorpusArtifactV3Error>>()?;
        let pickup_routes = pickup_rows
            .iter()
            .filter(|row| row.room_id == room.room_id && row.loadout == loadout)
            .map(|row| {
                Ok(ArtifactRehydrationPickupRowV3 {
                    source_door_id: row.source_door_id.clone(),
                    required_pickup_id: row.target_id.clone(),
                    evidence: rehydration_evidence_v3(row, witness_map)?,
                })
            })
            .collect::<Result<Vec<_>, CorpusArtifactV3Error>>()?;
        matrices.push(ArtifactRehydrationMatrixV3 {
            loadout,
            door_routes,
            pickup_routes,
            source_search_effort: effort
                .source_search_effort
                .iter()
                .map(|source| downwards_validation::DoorSourceSearchEffort {
                    source_door_id: source.source_door_id.clone(),
                    stats: source.search_effort.into(),
                })
                .collect(),
            aggregate_search_effort: effort.aggregate_search_effort.into(),
        });
    }
    Ok(matrices)
}

fn rehydrate_runtime_matrices_v3(
    room: &RoomRecordV3,
    generated: &super::GeneratedCorpusRoomV2,
    route_rows: &[TargetRowRecordV3],
    pickup_rows: &[TargetRowRecordV3],
    witness_map: &HashMap<&str, &WitnessRecordV3>,
    evaluation_configs: &[RouteEvaluationConfigV2],
) -> Result<Vec<LoadoutRouteMatrix>, CorpusArtifactV3Error> {
    let source = generated
        .variants
        .iter()
        .find(|candidate| candidate.exact_key() == room.physical_evidence_source_key)
        .expect("room construction verification checked the evidence-source alias");
    let serialized = artifact_rehydration_matrices_v3(room, route_rows, pickup_rows, witness_map)?;
    serialized
        .into_iter()
        .map(|matrix| {
            let loadout = matrix.loadout;
            let config = evaluation_configs
                .iter()
                .find(|config| config.loadout == loadout)
                .expect("the canonical evaluation config table was verified");
            let evidence = super::artifact_v3_rehydrate::rehydrate_matrix_v3(
                &room.room_id,
                source.generated(),
                config,
                matrix,
            )
            .map_err(|error| {
                invalid(format!(
                    "room {:?} {} matrix could not be replay-rehydrated for promotion verification: {error}",
                    room.room_id.0,
                    loadout.slug()
                ))
            })?;
            let summary =
                super::artifact_v3_rehydrate::summarize_rehydrated_evidence(&evidence);
            Ok(LoadoutRouteMatrix {
                loadout,
                evidence,
                summary,
            })
        })
        .collect()
}

fn rehydration_evidence_v3(
    row: &TargetRowRecordV3,
    witnesses: &HashMap<&str, &WitnessRecordV3>,
) -> Result<ArtifactRehydrationEvidenceV3, CorpusArtifactV3Error> {
    match &row.evidence {
        TargetEvidenceRecordV3::BoundedInconclusive {
            reason,
            search_effort,
        } => Ok(ArtifactRehydrationEvidenceV3::BoundedInconclusive {
            reason: (*reason).into(),
            search_effort: (*search_effort).into(),
        }),
        TargetEvidenceRecordV3::PositiveReplay { witness_id } => {
            let witness = witnesses
                .get(witness_id.as_str())
                .expect("verified positive row references one witness");
            let actions = witness
                .actions
                .iter()
                .copied()
                .flat_map(|span| std::iter::repeat_n(span.action(), span.ticks))
                .collect();
            Ok(ArtifactRehydrationEvidenceV3::PositiveReplay {
                witness_id: witness_id.clone(),
                initial_digest: parse_digest_v3(&witness.initial_digest)?,
                actions,
                search_effort: witness.search_effort.into(),
            })
        }
    }
}

fn verify_room_construction_v3(
    record: &RoomRecordV3,
    generated: &super::GeneratedCorpusRoomV2,
    shard_ability_promotion_audit_config: &CorpusRoomAnalysisConfigRecord,
) -> Result<(), CorpusArtifactV3Error> {
    if record.room_id != generated.id {
        return Err(invalid(format!(
            "room order/identity differs: artifact {:?}, regenerated {:?}",
            record.room_id.0, generated.id.0
        )));
    }
    if generated.id != generated.physical_descriptor.room_id() {
        return Err(invalid(format!(
            "regenerated room {:?} has a compact ID inconsistent with its exact descriptor",
            generated.id.0
        )));
    }
    let expected_descriptor = PhysicalRoomDescriptorRecordV3::from(&generated.physical_descriptor);
    if record.physical_descriptor != expected_descriptor {
        return Err(invalid(format!(
            "room {:?} exact physical descriptor differs from regeneration",
            record.room_id.0
        )));
    }
    if record.variants.is_empty() {
        return Err(invalid(format!(
            "room {:?} has no native variants",
            record.room_id.0
        )));
    }
    require_strict_order_v3(
        "room native variants",
        record
            .variants
            .iter()
            .map(|variant| variant.key.stable_slug()),
    )?;
    let mut expected_variants = generated
        .variants
        .iter()
        .map(NativeVariantRecordV3::from)
        .collect::<Vec<_>>();
    expected_variants.sort_unstable_by_key(|variant| variant.key.stable_slug());
    if record.variants != expected_variants {
        return Err(invalid(format!(
            "room {:?} native keys, route graphs, boundary ports, or provenance differ from exact regeneration",
            record.room_id.0
        )));
    }
    for candidate in &generated.variants {
        if candidate.physical_room_descriptor_v3() != generated.physical_descriptor {
            return Err(invalid(format!(
                "room {:?} contains a regenerated alias outside its exact descriptor group",
                record.room_id.0
            )));
        }
    }
    let expected_source = generated
        .variants
        .iter()
        .min_by_key(|candidate| candidate.exact_key().stable_slug())
        .expect("nonempty variants were checked")
        .exact_key();
    if record.physical_evidence_source_key != expected_source {
        return Err(invalid(format!(
            "room {:?} physical evidence source differs from the frozen lexical-key policy",
            record.room_id.0
        )));
    }
    if !record
        .variants
        .iter()
        .any(|variant| variant.key == record.physical_evidence_source_key)
    {
        return Err(invalid(format!(
            "room {:?} physical evidence source is not a native alias",
            record.room_id.0
        )));
    }
    if record
        .matrix_search_effort
        .iter()
        .map(|matrix| matrix.loadout)
        .ne(EvaluationLoadout::ALL)
    {
        return Err(invalid(format!(
            "room {:?} matrix search-effort records are not the exact four canonical loadouts",
            record.room_id.0
        )));
    }
    let source_candidate = generated
        .variants
        .iter()
        .find(|candidate| candidate.exact_key() == record.physical_evidence_source_key)
        .expect("the evidence-source alias membership was checked");
    let mut expected_source_ids = source_candidate
        .generated()
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    expected_source_ids.sort_unstable();
    for matrix in &record.matrix_search_effort {
        if matrix
            .source_search_effort
            .iter()
            .map(|source| &source.source_door_id)
            .ne(expected_source_ids.iter())
        {
            return Err(invalid(format!(
                "room {:?} {} source-search effort is not in exact canonical door order",
                record.room_id.0,
                matrix.loadout.slug()
            )));
        }
        let mut aggregate = SearchStats::default();
        for source in &matrix.source_search_effort {
            accumulate_search_stats_v3(&mut aggregate, source.search_effort.into());
        }
        if SearchStats::from(matrix.aggregate_search_effort) != aggregate {
            return Err(invalid(format!(
                "room {:?} {} aggregate search effort differs from its source records",
                record.room_id.0,
                matrix.loadout.slug()
            )));
        }
    }
    require_strict_order_v3(
        "room variant construction gates",
        record
            .variant_construction_gates
            .iter()
            .map(|gate| gate.key.stable_slug()),
    )?;
    require_strict_order_v3(
        "room variant ability-promotion gates",
        record
            .variant_ability_promotion_gates
            .iter()
            .map(|gate| gate.key.stable_slug()),
    )?;
    record
        .ability_promotion_audit_config
        .validate()
        .map_err(|error| {
            invalid(format!(
                "room {:?} has an invalid ability-promotion audit config: {error}",
                record.room_id.0
            ))
        })?;
    if &record.ability_promotion_audit_config != shard_ability_promotion_audit_config {
        return Err(invalid(format!(
            "room {:?} promotion-audit config differs from the exact shard config",
            record.room_id.0
        )));
    }
    if record.variant_ability_promotion_gates.len() != record.variants.len()
        || record
            .variant_ability_promotion_gates
            .iter()
            .zip(&record.variants)
            .any(|(gate, variant)| gate.key != variant.key)
    {
        return Err(invalid(format!(
            "room {:?} ability-promotion gates are not exactly aligned with native aliases",
            record.room_id.0
        )));
    }
    if record
        .variant_construction_gates
        .iter()
        .any(|gate| gate.gate_version != CORPUS_FEASIBILITY_GATE_VERSION)
        || record
            .variant_ability_promotion_gates
            .iter()
            .any(|gate| gate.gate_version != CORPUS_ABILITY_PROMOTION_GATE_VERSION)
        || record.canonical_regeneration.policy_version
            != CORPUS_CANONICAL_REGENERATION_POLICY_VERSION
        || record.canonical_regeneration.policy
            != CanonicalRegenerationPolicyRecordV3::LexicographicallySmallestExactKeyPassingAllGates
    {
        return Err(invalid(format!(
            "room {:?} uses unsupported gate/canonical policy versions",
            record.room_id.0
        )));
    }
    Ok(())
}

fn accumulate_search_stats_v3(aggregate: &mut SearchStats, source: SearchStats) {
    aggregate.expanded_nodes = aggregate
        .expanded_nodes
        .saturating_add(source.expanded_nodes);
    aggregate.generated_nodes = aggregate
        .generated_nodes
        .saturating_add(source.generated_nodes);
    aggregate.simulated_ticks = aggregate
        .simulated_ticks
        .saturating_add(source.simulated_ticks);
    aggregate.deepest_path_ticks = aggregate.deepest_path_ticks.max(source.deepest_path_ticks);
}

#[derive(Clone, Copy)]
enum TargetKindV3 {
    Door,
    Pickup,
}

impl TargetKindV3 {
    const fn stream_name(self) -> &'static str {
        match self {
            Self::Door => STREAM_NAMES[2],
            Self::Pickup => STREAM_NAMES[3],
        }
    }
}

fn verify_target_rows_v3(
    kind: TargetKindV3,
    rows: &[TargetRowRecordV3],
    rooms: &HashMap<RoomId, &super::GeneratedCorpusRoomV2>,
    witnesses: &HashMap<&str, &WitnessRecordV3>,
    witness_references: &mut HashMap<String, usize>,
) -> Result<(), CorpusArtifactV3Error> {
    let file = kind.stream_name();
    let mut actual = BTreeSet::new();
    for row in rows {
        let room = rooms.get(&row.room_id).ok_or_else(|| {
            invalid(format!(
                "{file} references unknown room {:?}",
                row.room_id.0
            ))
        })?;
        let candidate = evidence_source_candidate_v3(room, &row.room_id)?;
        let door_ids = candidate
            .generated()
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
        match kind {
            TargetKindV3::Door => {
                if row.source_door_id == row.target_id || !door_ids.contains(row.target_id.as_str())
                {
                    return Err(invalid(format!(
                        "{file} references invalid target door {:?} in room {:?}",
                        row.target_id, row.room_id.0
                    )));
                }
            }
            TargetKindV3::Pickup => {
                if !candidate
                    .generated()
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
        }
        if !actual.insert((
            row.room_id.clone(),
            row.source_door_id.clone(),
            row.target_id.clone(),
            row.loadout,
        )) {
            return Err(invalid(format!("{file} contains a duplicate matrix cell")));
        }
        if let TargetEvidenceRecordV3::PositiveReplay { witness_id } = &row.evidence {
            let witness = witnesses.get(witness_id.as_str()).ok_or_else(|| {
                invalid(format!("{file} references missing witness {witness_id:?}"))
            })?;
            verify_positive_witness_v3(kind, row, witness, candidate)?;
            *witness_references.entry(witness_id.clone()).or_default() += 1;
        }
    }

    let mut expected = BTreeSet::new();
    for (room_id, room) in rooms {
        let candidate = evidence_source_candidate_v3(room, room_id)?;
        let doors = candidate
            .generated()
            .room
            .doors()
            .iter()
            .map(|door| door.id.clone())
            .collect::<Vec<_>>();
        for loadout in EvaluationLoadout::ALL {
            for source in &doors {
                match kind {
                    TargetKindV3::Door => {
                        for target in &doors {
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
                    TargetKindV3::Pickup => {
                        for pickup in candidate.generated().room.pickups() {
                            expected.insert((
                                room_id.clone(),
                                source.clone(),
                                pickup.id().to_owned(),
                                loadout,
                            ));
                        }
                    }
                }
            }
        }
    }
    if actual != expected {
        return Err(invalid(format!(
            "{file} is not an exact four-loadout matrix; first missing={:?}, first unexpected={:?}",
            expected.difference(&actual).next(),
            actual.difference(&expected).next()
        )));
    }
    Ok(())
}

fn evidence_source_candidate_v3<'a>(
    room: &'a super::GeneratedCorpusRoomV2,
    room_id: &RoomId,
) -> Result<&'a CorpusCandidate, CorpusArtifactV3Error> {
    room.variants
        .iter()
        .min_by_key(|candidate| candidate.exact_key().stable_slug())
        .ok_or_else(|| invalid(format!("room {:?} has no native variants", room_id.0)))
}

fn verify_positive_witness_v3(
    kind: TargetKindV3,
    row: &TargetRowRecordV3,
    witness: &WitnessRecordV3,
    candidate: &CorpusCandidate,
) -> Result<(), CorpusArtifactV3Error> {
    if witness.room_id != row.room_id
        || witness.source_door_id != row.source_door_id
        || witness.target_id != row.target_id
        || witness.loadout != row.loadout
    {
        return Err(invalid(format!(
            "positive witness {:?} identity differs from its matrix row",
            witness.witness_id
        )));
    }
    if witness.action_encoding_version != CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION {
        return Err(invalid(format!(
            "positive witness {:?} uses unsupported action encoding {}",
            witness.witness_id, witness.action_encoding_version
        )));
    }
    if witness.total_ticks > MAX_VERIFIABLE_WITNESS_TICKS_V3 {
        return Err(invalid(format!(
            "positive witness {:?} has {} ticks above verifier limit {}",
            witness.witness_id, witness.total_ticks, MAX_VERIFIABLE_WITNESS_TICKS_V3
        )));
    }
    let mut total_ticks = 0usize;
    let mut previous = None;
    for (span_index, span) in witness.actions.iter().copied().enumerate() {
        if span.ticks == 0 {
            return Err(invalid(format!(
                "positive witness {:?} action span {span_index} has zero ticks",
                witness.witness_id
            )));
        }
        if !(-1..=1).contains(&span.move_x) || !(-1..=1).contains(&span.move_y) {
            return Err(invalid(format!(
                "positive witness {:?} action span {span_index} is not normalized",
                witness.witness_id
            )));
        }
        total_ticks = total_ticks
            .checked_add(span.ticks)
            .ok_or_else(|| invalid("positive witness tick total overflow"))?;
        let action = span.action();
        if previous == Some(action) {
            return Err(invalid(format!(
                "positive witness {:?} has adjacent equal RLE spans",
                witness.witness_id
            )));
        }
        previous = Some(action);
    }
    if total_ticks != witness.total_ticks {
        return Err(invalid(format!(
            "positive witness {:?} RLE ticks {total_ticks} differ from recorded {}",
            witness.witness_id, witness.total_ticks
        )));
    }
    let actions = witness
        .actions
        .iter()
        .copied()
        .flat_map(|span| std::iter::repeat_n(span.action(), span.ticks))
        .collect::<Vec<_>>();
    let initial = Simulation::enter_via_door(
        candidate.generated().room.clone(),
        row.loadout.abilities(),
        &row.source_door_id,
    )
    .map_err(|error| {
        invalid(format!(
            "positive witness {:?} source-door entry failed: {error}",
            witness.witness_id
        ))
    })?;
    let expected_initial = parse_digest_v3(&witness.initial_digest)?;
    if initial.digest() != expected_initial {
        return Err(invalid(format!(
            "positive witness {:?} initial digest differs: {} != {}",
            witness.witness_id,
            expected_initial,
            initial.digest()
        )));
    }
    let replay = Replay::record(&initial, actions);
    let verification = replay.verify(&initial).map_err(|error| {
        invalid(format!(
            "positive witness {:?} authoritative replay diverged: {error}",
            witness.witness_id
        ))
    })?;
    let (target, reached) = match kind {
        TargetKindV3::Door => {
            if verification.reached_exit.as_deref() != Some(row.target_id.as_str()) {
                return Err(invalid(format!(
                    "positive witness {:?} did not reach door {:?}; terminal={:?}",
                    witness.witness_id, row.target_id, verification.reached_exit
                )));
            }
            (
                SearchTarget::door(&row.target_id),
                ReachedTarget::Door(row.target_id.clone()),
            )
        }
        TargetKindV3::Pickup => {
            if !verification
                .collected_pickup_ids
                .iter()
                .any(|pickup| pickup == &row.target_id)
            {
                return Err(invalid(format!(
                    "positive witness {:?} did not collect pickup {:?}",
                    witness.witness_id, row.target_id
                )));
            }
            (
                SearchTarget::pickup(&row.target_id),
                ReachedTarget::Pickup(row.target_id.clone()),
            )
        }
    };
    let solution = TargetSolution {
        target,
        reached,
        replay,
        stats: witness.search_effort.into(),
    };
    let actual_id = match kind {
        TargetKindV3::Door => {
            let objective = DoorReachabilityObjective::new(
                &row.source_door_id,
                &row.target_id,
                candidate.generated().metadata.clone(),
                row.loadout.abilities(),
            );
            fingerprint_door_witness(candidate.generated(), &objective, &solution).to_string()
        }
        TargetKindV3::Pickup => {
            let objective = PickupFromDoorObjective::new(
                &row.source_door_id,
                &row.target_id,
                candidate.generated().metadata.clone(),
                row.loadout.abilities(),
            );
            fingerprint_pickup_from_door_witness(candidate.generated(), &objective, &solution)
                .to_string()
        }
    };
    if witness.witness_id != actual_id {
        return Err(invalid(format!(
            "positive witness ID differs after authoritative replay: {:?} != {:?}",
            witness.witness_id, actual_id
        )));
    }
    Ok(())
}

fn verify_recorded_search_observations_v3(
    rooms: &[RoomRecordV3],
    routes: &[TargetRowRecordV3],
    pickups: &[TargetRowRecordV3],
    witnesses: &HashMap<&str, &WitnessRecordV3>,
    evaluation_configs: &[RouteEvaluationConfigV2],
) -> Result<(), CorpusArtifactV3Error> {
    for room in rooms {
        for loadout in EvaluationLoadout::ALL {
            let config = evaluation_configs
                .iter()
                .find(|config| config.loadout == loadout)
                .expect("the canonical evaluation-config table was verified");
            let solver_config = config.solver.to_solver_config();
            let matrix_effort = room
                .matrix_search_effort
                .iter()
                .find(|matrix| matrix.loadout == loadout)
                .expect("the canonical matrix-effort table was verified");
            for source in &matrix_effort.source_search_effort {
                let observations = routes
                    .iter()
                    .chain(pickups)
                    .filter(|row| {
                        row.room_id == room.room_id
                            && row.loadout == loadout
                            && row.source_door_id == source.source_door_id
                    })
                    .map(|row| match &row.evidence {
                        TargetEvidenceRecordV3::PositiveReplay { witness_id } => {
                            let witness = witnesses
                                .get(witness_id.as_str())
                                .expect("positive witness references were verified");
                            RecordedSearchObservation::Positive {
                                search_effort: witness.search_effort.into(),
                                replay_ticks: witness.total_ticks,
                            }
                        }
                        TargetEvidenceRecordV3::BoundedInconclusive {
                            reason,
                            search_effort,
                        } => RecordedSearchObservation::BoundedInconclusive {
                            reason: (*reason).into(),
                            search_effort: (*search_effort).into(),
                        },
                    })
                    .collect::<Vec<_>>();
                validate_recorded_source_search_observations(
                    &solver_config,
                    source.search_effort.into(),
                    &observations,
                )
                .map_err(|error| {
                    invalid(format!(
                        "room {:?} {} source {:?} has inconsistent recorded search observations: {error}",
                        room.room_id.0,
                        loadout.slug(),
                        source.source_door_id
                    ))
                })?;
            }
        }
    }
    Ok(())
}

fn verify_ability_promotion_gates_v3(
    rooms: &[RoomRecordV3],
    routes: &[TargetRowRecordV3],
    pickups: &[TargetRowRecordV3],
    witnesses: &HashMap<&str, &WitnessRecordV3>,
    runtime_rooms: &HashMap<RoomId, &super::GeneratedCorpusRoomV2>,
    evaluation_configs: &[RouteEvaluationConfigV2],
) -> Result<BTreeMap<RoomId, super::EvaluatedCorpusRoomV2>, CorpusArtifactV3Error> {
    let mut verified_rooms = BTreeMap::new();
    for room in rooms {
        let generated = runtime_rooms
            .get(&room.room_id)
            .expect("exact room regeneration was verified");
        let matrices = rehydrate_runtime_matrices_v3(
            room,
            generated,
            routes,
            pickups,
            witnesses,
            evaluation_configs,
        )?;
        let evidence_source = generated
            .variants
            .iter()
            .find(|candidate| candidate.exact_key() == room.physical_evidence_source_key)
            .expect("room construction verification checked the evidence-source alias");
        let mut runtime_gates = Vec::with_capacity(generated.variants.len());
        for candidate in &generated.variants {
            let record = room
                .variant_ability_promotion_gates
                .iter()
                .find(|gate| gate.key == candidate.exact_key())
                .expect("ability-promotion gate/alias alignment was verified");
            let gate =
                rehydrate_variant_ability_promotion_gate_record_v3(record.clone(), candidate)?;
            rerun_validate_variant_ability_promotion_gate_v2(
                candidate,
                evidence_source,
                &matrices,
                &room.ability_promotion_audit_config,
                &gate,
            )
            .map_err(|error| {
                invalid(format!(
                    "room {:?} ability-promotion gate {} differs from an exact advertised-pair audit rerun: {error}",
                    room.room_id.0,
                    candidate.exact_key().stable_slug()
                ))
            })?;
            runtime_gates.push(gate);
        }

        let evaluated = EvaluatedCorpusBatchRoomV3::from_verified_records(
            room,
            (*generated).clone(),
            matrices,
            runtime_gates.clone(),
        );
        validate_evaluated_corpus_room_v2(&evaluated).map_err(|error| {
            invalid(format!(
                "room {:?} fails full replay-rehydrated room validation: {error}",
                room.room_id.0
            ))
        })?;
        if verified_rooms
            .insert(room.room_id.clone(), evaluated)
            .is_some()
        {
            return Err(invalid(format!(
                "duplicate replay-rehydrated promotion gate room {:?}",
                room.room_id.0
            )));
        }
    }
    Ok(verified_rooms)
}

/// Tiny constructor namespace keeps the strict verifier's reconstruction
/// visibly separate from the public batch rehydrator while producing the
/// exact same rich room value and invoking the shared validator.
struct EvaluatedCorpusBatchRoomV3;

impl EvaluatedCorpusBatchRoomV3 {
    fn from_verified_records(
        record: &RoomRecordV3,
        generated: super::GeneratedCorpusRoomV2,
        matrices: Vec<LoadoutRouteMatrix>,
        variant_ability_promotion_gates: Vec<VariantAbilityPromotionGateV2>,
    ) -> super::EvaluatedCorpusRoomV2 {
        super::EvaluatedCorpusRoomV2 {
            generated,
            physical_evidence_source_key: record.physical_evidence_source_key.clone(),
            matrices,
            variant_construction_gates: record
                .variant_construction_gates
                .iter()
                .map(|gate| VariantConstructionGateV2 {
                    gate_version: gate.gate_version,
                    key: gate.key.clone(),
                    construction_loadout: gate.construction_loadout,
                    state: gate.state.into(),
                })
                .collect(),
            ability_promotion_audit_config: record.ability_promotion_audit_config.clone(),
            variant_ability_promotion_gates,
            complete_kit_gate: record.complete_kit_gate.into(),
            canonical_regeneration: super::CanonicalRegenerationSelectionV2 {
                policy_version: record.canonical_regeneration.policy_version,
                policy:
                    CanonicalRegenerationPolicy::LexicographicallySmallestExactKeyPassingAllGates,
                selected_key: record.canonical_regeneration.selected_key.clone(),
            },
        }
    }
}

fn verify_room_gates_v3(
    rooms: &[RoomRecordV3],
    routes: &[TargetRowRecordV3],
    pickups: &[TargetRowRecordV3],
    runtime_rooms: &BTreeMap<RoomId, super::EvaluatedCorpusRoomV2>,
) -> Result<(), CorpusArtifactV3Error> {
    for room in rooms {
        let states = EvaluationLoadout::ALL
            .into_iter()
            .map(|loadout| {
                (
                    loadout,
                    matrix_gate_state_v3(&room.room_id, loadout, routes, pickups),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut expected_gates = room
            .variants
            .iter()
            .map(|variant| VariantConstructionGateRecordV3 {
                gate_version: CORPUS_FEASIBILITY_GATE_VERSION,
                key: variant.key.clone(),
                construction_loadout: variant.construction_loadout,
                state: states[&variant.construction_loadout],
            })
            .collect::<Vec<_>>();
        expected_gates.sort_unstable_by_key(|gate| gate.key.stable_slug());
        if room.variant_construction_gates != expected_gates {
            return Err(invalid(format!(
                "room {:?} variant construction gates differ from its exact matrices",
                room.room_id.0
            )));
        }
        let complete_kit_gate = states[&EvaluationLoadout::Both];
        if room.complete_kit_gate != complete_kit_gate {
            return Err(invalid(format!(
                "room {:?} complete-kit gate differs from its exact matrix",
                room.room_id.0
            )));
        }
        let runtime_gates = expected_gates
            .iter()
            .map(|gate| VariantConstructionGateV2 {
                gate_version: gate.gate_version,
                key: gate.key.clone(),
                construction_loadout: gate.construction_loadout,
                state: gate.state.into(),
            })
            .collect::<Vec<_>>();
        let promotion_gates = &runtime_rooms
            .get(&room.room_id)
            .expect("ability-promotion gates were replay-verified for every room")
            .variant_ability_promotion_gates;
        let expected_selection = select_canonical_regeneration_v2(
            &runtime_gates,
            promotion_gates,
            complete_kit_gate.into(),
        );
        let expected_record = CanonicalRegenerationRecordV3 {
            policy_version: expected_selection.policy_version,
            policy: expected_selection.policy.into(),
            selected_key: expected_selection.selected_key,
        };
        if room.canonical_regeneration != expected_record {
            return Err(invalid(format!(
                "room {:?} canonical regeneration differs from the frozen post-feasibility policy",
                room.room_id.0
            )));
        }
    }
    Ok(())
}

fn matrix_gate_state_v3(
    room_id: &RoomId,
    loadout: EvaluationLoadout,
    routes: &[TargetRowRecordV3],
    pickups: &[TargetRowRecordV3],
) -> FeasibilityGateRecordV3 {
    let door_rows = routes
        .iter()
        .filter(|row| row.room_id == *room_id && row.loadout == loadout)
        .count();
    let positive_door_rows = routes
        .iter()
        .filter(|row| {
            row.room_id == *room_id
                && row.loadout == loadout
                && matches!(row.evidence, TargetEvidenceRecordV3::PositiveReplay { .. })
        })
        .count();
    let pickup_rows = pickups
        .iter()
        .filter(|row| row.room_id == *room_id && row.loadout == loadout)
        .count();
    let positive_pickup_rows = pickups
        .iter()
        .filter(|row| {
            row.room_id == *room_id
                && row.loadout == loadout
                && matches!(row.evidence, TargetEvidenceRecordV3::PositiveReplay { .. })
        })
        .count();
    if door_rows == positive_door_rows && pickup_rows == positive_pickup_rows {
        FeasibilityGateRecordV3::ReplayCertifiedAllTargets
    } else {
        FeasibilityGateRecordV3::BoundedInconclusive {
            door_rows,
            positive_door_rows,
            pickup_rows,
            positive_pickup_rows,
        }
    }
}

fn verify_stream_hashes_v3(
    bundle: &CorpusArtifactBundleV3,
    recorded: &BTreeMap<String, String>,
) -> Result<(), CorpusArtifactV3Error> {
    let expected_names = STREAM_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual_names = recorded.keys().cloned().collect::<BTreeSet<_>>();
    if actual_names != expected_names {
        return Err(invalid(format!(
            "stream hash keys differ: expected {expected_names:?}, found {actual_names:?}"
        )));
    }
    for (name, bytes) in bundle.streams() {
        let expected = byte_hash_v3(bytes);
        if recorded[name] != expected {
            return Err(invalid(format!(
                "hash mismatch for {name}: {:?} != {:?}",
                recorded[name], expected
            )));
        }
    }
    Ok(())
}

fn parse_canonical_jsonl_v3<T>(file: &str, bytes: &[u8]) -> Result<Vec<T>, CorpusArtifactV3Error>
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
    let mut rows = Vec::new();
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
        let row = serde_json::from_slice::<T>(line).map_err(|source| {
            CorpusArtifactV3Error::JsonLine {
                file: file.to_owned(),
                line: index + 1,
                source,
            }
        })?;
        let canonical = serde_json::to_vec(&row)?;
        if canonical != line {
            return Err(CorpusArtifactV3Error::NonCanonicalJson {
                file: file.to_owned(),
                line: index + 1,
            });
        }
        rows.push(row);
    }
    Ok(rows)
}

fn require_strict_order_v3<T: Ord>(
    label: &str,
    values: impl IntoIterator<Item = T>,
) -> Result<(), CorpusArtifactV3Error> {
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

fn parse_digest_v3(text: &str) -> Result<StateDigest, CorpusArtifactV3Error> {
    if text.len() != 16
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(format!(
            "state digest must be 16 lowercase hexadecimal digits, found {text:?}"
        )));
    }
    u64::from_str_radix(text, 16)
        .map(StateDigest)
        .map_err(|error| invalid(format!("invalid state digest {text:?}: {error}")))
}

/// Read the six required v3 files, rejecting non-regular entries.
pub fn read_artifact_bundle_v3(
    directory: &Path,
) -> Result<CorpusArtifactBundleV3, CorpusArtifactV3Error> {
    let read = |name: &str| -> Result<Vec<u8>, CorpusArtifactV3Error> {
        let path = directory.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|source| CorpusArtifactV3Error::Io {
            path: path.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() {
            return Err(invalid(format!(
                "artifact entry is not a regular file: {}",
                path.display()
            )));
        }
        fs::read(&path).map_err(|source| CorpusArtifactV3Error::Io { path, source })
    };
    Ok(CorpusArtifactBundleV3 {
        run_json: read(FILE_NAMES[0])?,
        attempts_jsonl: read(FILE_NAMES[1])?,
        rooms_jsonl: read(FILE_NAMES[2])?,
        routes_jsonl: read(FILE_NAMES[3])?,
        pickups_jsonl: read(FILE_NAMES[4])?,
        witnesses_jsonl: read(FILE_NAMES[5])?,
    })
}

pub fn verify_artifact_directory_v3(
    directory: &Path,
) -> Result<VerifiedCorpusArtifactV3, CorpusArtifactV3Error> {
    require_real_directory_v3(directory)?;
    verify_artifact_bundle_v3(&read_artifact_bundle_v3(directory)?)
}

/// Create every artifact file without overwriting an existing path.
pub fn write_new_artifact_bundle_v3(
    directory: &Path,
    bundle: &CorpusArtifactBundleV3,
) -> Result<(), CorpusArtifactV3Error> {
    fs::create_dir_all(directory).map_err(|source| CorpusArtifactV3Error::Io {
        path: directory.to_owned(),
        source,
    })?;
    for (name, bytes) in bundle.files() {
        write_new_file_atomic_v3(&directory.join(name), bytes)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusSeedCheckpointV3 {
    pub checkpoint_version: u32,
    pub status: String,
    pub seed: u64,
    pub config_id: String,
    pub artifact_hashes: BTreeMap<String, String>,
}

fn seed_checkpoint_record_v3(
    bundle: &CorpusArtifactBundleV3,
    verified: &VerifiedCorpusArtifactV3,
) -> CorpusSeedCheckpointV3 {
    CorpusSeedCheckpointV3 {
        checkpoint_version: CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION,
        status: "complete".to_owned(),
        seed: verified.config.start_seed,
        config_id: verified.config_id.clone(),
        artifact_hashes: bundle
            .files()
            .into_iter()
            .map(|(name, bytes)| (name.to_owned(), byte_hash_v3(bytes)))
            .collect(),
    }
}

fn render_seed_checkpoint_from_verified_v3(
    bundle: &CorpusArtifactBundleV3,
    verified: &VerifiedCorpusArtifactV3,
) -> Result<Vec<u8>, CorpusArtifactV3Error> {
    let mut bytes = serde_json::to_vec(&seed_checkpoint_record_v3(bundle, verified))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn render_seed_checkpoint_v3(
    bundle: &CorpusArtifactBundleV3,
) -> Result<Vec<u8>, CorpusArtifactV3Error> {
    let verified = verify_artifact_bundle_v3(bundle)?;
    render_seed_checkpoint_from_verified_v3(bundle, &verified)
}

fn parse_seed_checkpoint_v3(bytes: &[u8]) -> Result<CorpusSeedCheckpointV3, CorpusArtifactV3Error> {
    let mut rows: Vec<CorpusSeedCheckpointV3> =
        parse_canonical_jsonl_v3(CORPUS_ARTIFACT_V3_CHECKPOINT_FILE, bytes)?;
    if rows.len() != 1 {
        return Err(invalid(format!(
            "{CORPUS_ARTIFACT_V3_CHECKPOINT_FILE} must contain one record, found {}",
            rows.len()
        )));
    }
    let checkpoint = rows.pop().expect("length was checked");
    if checkpoint.checkpoint_version != CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION
        || checkpoint.status != "complete"
    {
        return Err(invalid("unsupported or incomplete corpus-v3 checkpoint"));
    }
    Ok(checkpoint)
}

fn verify_seed_checkpoint_against_verified_v3(
    checkpoint: CorpusSeedCheckpointV3,
    expected_config: &CorpusBuildConfigV2,
    bundle: &CorpusArtifactBundleV3,
    verified: &VerifiedCorpusArtifactV3,
) -> Result<CorpusSeedCheckpointV3, CorpusArtifactV3Error> {
    if expected_config.seed_count != 1 {
        return Err(invalid("expected checkpoint config must contain one seed"));
    }
    let expected_config_id = corpus_config_id_v3(expected_config)?;
    if checkpoint.config_id != expected_config_id || checkpoint.seed != expected_config.start_seed {
        return Err(invalid("checkpoint seed/config identity mismatch"));
    }
    if &verified.config != expected_config || verified.config_id != expected_config_id {
        return Err(invalid(
            "checkpoint expected config differs from the artifact run config",
        ));
    }
    let actual_hashes = bundle
        .files()
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), byte_hash_v3(bytes)))
        .collect::<BTreeMap<_, _>>();
    if checkpoint.artifact_hashes != actual_hashes {
        return Err(invalid("checkpoint artifact hashes differ from the bundle"));
    }
    Ok(checkpoint)
}

pub fn verify_seed_checkpoint_v3(
    bytes: &[u8],
    expected_config: &CorpusBuildConfigV2,
    bundle: &CorpusArtifactBundleV3,
) -> Result<CorpusSeedCheckpointV3, CorpusArtifactV3Error> {
    let checkpoint = parse_seed_checkpoint_v3(bytes)?;
    let verified = verify_artifact_bundle_v3(bundle)?;
    verify_seed_checkpoint_against_verified_v3(checkpoint, expected_config, bundle, &verified)
}

/// Atomically publish the immutable completion marker. Existing checkpoints
/// are never overwritten, including under concurrent writers.
pub fn write_new_seed_checkpoint_v3(
    path: &Path,
    bundle: &CorpusArtifactBundleV3,
) -> Result<(), CorpusArtifactV3Error> {
    let bytes = render_seed_checkpoint_v3(bundle)?;
    write_new_file_atomic_v3(path, &bytes)
}

fn write_new_seed_checkpoint_from_verified_v3(
    path: &Path,
    bundle: &CorpusArtifactBundleV3,
    verified: &VerifiedCorpusArtifactV3,
) -> Result<Vec<u8>, CorpusArtifactV3Error> {
    let bytes = render_seed_checkpoint_from_verified_v3(bundle, verified)?;
    write_new_file_atomic_v3(path, &bytes)?;
    Ok(bytes)
}

fn read_seed_checkpoint_bytes_v3(directory: &Path) -> Result<Vec<u8>, CorpusArtifactV3Error> {
    let checkpoint_path = directory.join(CORPUS_ARTIFACT_V3_CHECKPOINT_FILE);
    let metadata = fs::symlink_metadata(&checkpoint_path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            invalid(format!(
                "existing shard {} has no completion checkpoint",
                directory.display()
            ))
        } else {
            CorpusArtifactV3Error::Io {
                path: checkpoint_path.clone(),
                source,
            }
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(invalid(format!(
            "checkpoint is not a regular file: {}",
            checkpoint_path.display()
        )));
    }
    fs::read(&checkpoint_path).map_err(|source| CorpusArtifactV3Error::Io {
        path: checkpoint_path,
        source,
    })
}

fn require_stable_seed_shard_bytes_v3(
    directory: &Path,
    bundle: &CorpusArtifactBundleV3,
    checkpoint_bytes: &[u8],
) -> Result<(), CorpusArtifactV3Error> {
    if read_artifact_bundle_v3(directory)? != *bundle
        || read_seed_checkpoint_bytes_v3(directory)? != checkpoint_bytes
    {
        return Err(invalid(format!(
            "v3 shard {} changed during semantic verification",
            directory.display()
        )));
    }
    Ok(())
}

/// Process-local source snapshot produced by one semantic verifier pass and
/// lightweight checkpoint/hash/stability checks. It is intentionally private
/// to the corpus modules and is never accepted as durable evidence itself.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VerifiedRehydratedSeedShardV3 {
    pub checkpoint: CorpusSeedCheckpointV3,
    pub checkpoint_bytes: Vec<u8>,
    pub artifact: VerifiedRehydratedArtifactBundleV3,
}

pub(super) fn load_verified_rehydrated_seed_shard_v3(
    directory: &Path,
    expected_config: Option<&CorpusBuildConfigV2>,
) -> Result<VerifiedRehydratedSeedShardV3, CorpusArtifactV3Error> {
    require_real_directory_v3(directory)?;
    let bundle = read_artifact_bundle_v3(directory)?;
    let checkpoint_bytes = read_seed_checkpoint_bytes_v3(directory)?;
    let checkpoint = parse_seed_checkpoint_v3(&checkpoint_bytes)?;
    let artifact = verify_rehydrated_artifact_bundle_v3(&bundle)?;
    let expected_config = expected_config.unwrap_or(&artifact.verified.config);
    let checkpoint = verify_seed_checkpoint_against_verified_v3(
        checkpoint,
        expected_config,
        &bundle,
        &artifact.verified,
    )?;
    run_before_shard_stability_check_v3();
    require_stable_seed_shard_bytes_v3(directory, &bundle, &checkpoint_bytes)?;
    Ok(VerifiedRehydratedSeedShardV3 {
        checkpoint,
        checkpoint_bytes,
        artifact,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusSeedShardOutcomeV3 {
    Written(VerifiedCorpusArtifactV3),
    AlreadyVerified(VerifiedCorpusArtifactV3),
}

impl CorpusSeedShardOutcomeV3 {
    #[must_use]
    pub const fn verified(&self) -> &VerifiedCorpusArtifactV3 {
        match self {
            Self::Written(verified) | Self::AlreadyVerified(verified) => verified,
        }
    }
}

/// Verify an existing shard against the exact expected single-seed config.
/// A missing checkpoint is an incomplete shard, never permission to reuse or
/// overwrite its files.
pub fn verify_seed_shard_directory_v3(
    directory: &Path,
    expected_config: &CorpusBuildConfigV2,
) -> Result<VerifiedCorpusArtifactV3, CorpusArtifactV3Error> {
    Ok(
        load_verified_rehydrated_seed_shard_v3(directory, Some(expected_config))?
            .artifact
            .verified,
    )
}

/// Publish a newly rendered shard, read every byte back from disk, run the
/// independent verifier, and only then atomically create its checkpoint.
/// Existing directories are skipped only after the same full verification.
pub fn write_or_verify_seed_shard_v3(
    directory: &Path,
    expected_config: &CorpusBuildConfigV2,
    bundle: &CorpusArtifactBundleV3,
) -> Result<CorpusSeedShardOutcomeV3, CorpusArtifactV3Error> {
    if expected_config.seed_count != 1 {
        return Err(invalid(
            "v3 seed shard config must contain exactly one seed",
        ));
    }
    match fs::symlink_metadata(directory) {
        Ok(_) => {
            return verify_seed_shard_directory_v3(directory, expected_config)
                .map(CorpusSeedShardOutcomeV3::AlreadyVerified);
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CorpusArtifactV3Error::Io {
                path: directory.to_owned(),
                source,
            });
        }
    }
    let in_memory = verify_artifact_bundle_v3(bundle)?;
    if &in_memory.config != expected_config {
        return Err(invalid(
            "supplied v3 artifact bundle does not match the expected seed config",
        ));
    }
    let parent = directory.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CorpusArtifactV3Error::Io {
        path: parent.to_owned(),
        source,
    })?;
    match fs::create_dir(directory) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return verify_seed_shard_directory_v3(directory, expected_config)
                .map(CorpusSeedShardOutcomeV3::AlreadyVerified);
        }
        Err(source) => {
            return Err(CorpusArtifactV3Error::Io {
                path: directory.to_owned(),
                source,
            });
        }
    }
    write_new_artifact_bundle_v3(directory, bundle)?;
    let read_back = read_artifact_bundle_v3(directory)?;
    if &read_back != bundle {
        return Err(invalid(format!(
            "v3 shard {} changed during disk read-back",
            directory.display()
        )));
    }
    let verified = verify_artifact_bundle_v3(&read_back)?;
    if verified != in_memory {
        return Err(invalid(format!(
            "v3 shard {} read-back verification differs from in-memory verification",
            directory.display()
        )));
    }
    let checkpoint_path = directory.join(CORPUS_ARTIFACT_V3_CHECKPOINT_FILE);
    let checkpoint_bytes =
        write_new_seed_checkpoint_from_verified_v3(&checkpoint_path, &read_back, &verified)?;
    let checkpoint = parse_seed_checkpoint_v3(&checkpoint_bytes)?;
    verify_seed_checkpoint_against_verified_v3(checkpoint, expected_config, &read_back, &verified)?;
    run_before_shard_stability_check_v3();
    require_stable_seed_shard_bytes_v3(directory, &read_back, &checkpoint_bytes)?;
    Ok(CorpusSeedShardOutcomeV3::Written(verified))
}

/// Deterministic shard directory name used by the narrow multi-seed runner.
#[must_use]
pub fn seed_shard_directory_v3(root: &Path, seed: u64) -> PathBuf {
    root.join(format!("seed-{seed:016x}"))
}

/// Generate/evaluate one seed only when no fully verified matching shard is
/// already present. This is the narrow building block for resumable loops.
pub fn build_or_verify_seed_shard_v3(
    root: &Path,
    seed: u64,
) -> Result<CorpusSeedShardOutcomeV3, CorpusArtifactV3Error> {
    let config = CorpusBuildConfigV2::attempt_zero(seed, 1);
    let directory = seed_shard_directory_v3(root, seed);
    if directory.exists() {
        return verify_seed_shard_directory_v3(&directory, &config)
            .map(CorpusSeedShardOutcomeV3::AlreadyVerified);
    }
    fs::create_dir_all(root).map_err(|source| CorpusArtifactV3Error::Io {
        path: root.to_owned(),
        source,
    })?;
    let generated = generate_seed_block_v2(config.clone())
        .map_err(|error| invalid(format!("could not generate seed {seed}: {error}")))?;
    let evaluated = super::evaluate_route_matrices_v2(generated)
        .map_err(|error| invalid(format!("could not evaluate seed {seed}: {error}")))?;
    let bundle = render_evaluated_artifact_v3(&evaluated)?;
    write_or_verify_seed_shard_v3(&directory, &config, &bundle)
}

fn require_real_directory_v3(directory: &Path) -> Result<(), CorpusArtifactV3Error> {
    let metadata = fs::symlink_metadata(directory).map_err(|source| CorpusArtifactV3Error::Io {
        path: directory.to_owned(),
        source,
    })?;
    if !metadata.file_type().is_dir() {
        return Err(invalid(format!(
            "shard path is not a real directory: {}",
            directory.display()
        )));
    }
    Ok(())
}

fn write_new_file_atomic_v3(path: &Path, bytes: &[u8]) -> Result<(), CorpusArtifactV3Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CorpusArtifactV3Error::Io {
        path: parent.to_owned(),
        source,
    })?;
    if path.exists() {
        return Err(CorpusArtifactV3Error::AlreadyExists(path.to_owned()));
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            invalid(format!(
                "artifact path has no UTF-8 file name: {}",
                path.display()
            ))
        })?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                CorpusArtifactV3Error::AlreadyExists(temporary.clone())
            } else {
                CorpusArtifactV3Error::Io {
                    path: temporary.clone(),
                    source,
                }
            }
        })?;
    let write_result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| CorpusArtifactV3Error::Io {
            path: temporary.clone(),
            source,
        });
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(source) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(if source.kind() == std::io::ErrorKind::AlreadyExists {
            CorpusArtifactV3Error::AlreadyExists(path.to_owned())
        } else {
            CorpusArtifactV3Error::Io {
                path: path.to_owned(),
                source,
            }
        });
    }
    fs::remove_file(&temporary).map_err(|source| CorpusArtifactV3Error::Io {
        path: temporary,
        source,
    })?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> CorpusArtifactV3Error {
    CorpusArtifactV3Error::Invalid(message.into())
}

#[derive(Debug)]
pub enum CorpusArtifactV3Error {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json(serde_json::Error),
    JsonLine {
        file: String,
        line: usize,
        source: serde_json::Error,
    },
    NonCanonicalJson {
        file: String,
        line: usize,
    },
    AlreadyExists(PathBuf),
    WitnessIdentityCollision(String),
    Invalid(String),
}

impl fmt::Display for CorpusArtifactV3Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "I/O error at {}: {source}", path.display())
            }
            Self::Json(source) => write!(formatter, "invalid corpus artifact v3 JSON: {source}"),
            Self::JsonLine { file, line, source } => {
                write!(formatter, "invalid JSON in {file} line {line}: {source}")
            }
            Self::NonCanonicalJson { file, line } => {
                write!(formatter, "non-canonical JSON in {file} line {line}")
            }
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "corpus artifact path already exists: {}",
                    path.display()
                )
            }
            Self::WitnessIdentityCollision(id) => {
                write!(
                    formatter,
                    "distinct positive witnesses share identity {id:?}"
                )
            }
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for CorpusArtifactV3Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(source) => Some(source),
            Self::JsonLine { source, .. } => Some(source),
            Self::NonCanonicalJson { .. }
            | Self::AlreadyExists(_)
            | Self::WitnessIdentityCollision(_)
            | Self::Invalid(_) => None,
        }
    }
}

impl From<serde_json::Error> for CorpusArtifactV3Error {
    fn from(source: serde_json::Error) -> Self {
        Self::Json(source)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use downwards_validation::ValidationConfig;

    use super::*;
    use crate::corpus::{evaluate_route_matrices_v2_with, generate_seed_block_v2};

    fn verified_bundle_v3() -> CorpusArtifactBundleV3 {
        static BUNDLE: OnceLock<CorpusArtifactBundleV3> = OnceLock::new();
        BUNDLE
            .get_or_init(|| {
                let generated =
                    generate_seed_block_v2(CorpusBuildConfigV2::attempt_zero(0, 1)).unwrap();
                let evaluated = evaluate_route_matrices_v2_with(generated, |loadout| {
                    let mut config = ValidationConfig::for_loadout(loadout.abilities());
                    config.solver.max_expanded_nodes = 1;
                    config.solver.max_simulated_ticks = 2_000;
                    config
                })
                .unwrap();
                render_evaluated_artifact_v3(&evaluated).unwrap()
            })
            .clone()
    }

    fn rehash_stream(bundle: &mut CorpusArtifactBundleV3, name: &str) {
        let bytes = bundle
            .streams()
            .into_iter()
            .find_map(|(candidate, bytes)| (candidate == name).then_some(bytes))
            .unwrap();
        let mut run: RunRecordV3 = parse_canonical_jsonl_v3(FILE_NAMES[0], &bundle.run_json)
            .unwrap()
            .pop()
            .unwrap();
        run.stream_hashes
            .insert(name.to_owned(), byte_hash_v3(bytes));
        bundle.run_json = serde_json::to_vec(&run).unwrap();
        bundle.run_json.push(b'\n');
    }

    #[test]
    fn final_path_artifact_is_byte_repeatable_and_round_trips_deep_verification() {
        let first = verified_bundle_v3();
        let second = verified_bundle_v3();
        assert_eq!(first, second);
        let verified = verify_artifact_bundle_v3(&first).unwrap();
        assert_eq!(verified.config.seed_count, 1);
        assert_eq!(verified.summary.attempts, 15);
        assert_eq!(verified.evaluation_configs.len(), 4);
        assert!(verified.summary.ability_aliases > 0);
        assert_eq!(
            verified.summary.ability_aliases,
            verified.summary.promoted_ability_aliases + verified.summary.unpromoted_ability_aliases
        );
        assert_eq!(
            verified.summary.positive_witnesses,
            verified.summary.positive_route_rows + verified.summary.positive_pickup_rows
        );
        assert!(
            !String::from_utf8(first.run_json.clone())
                .unwrap()
                .contains("unreachable")
        );
        assert!(
            String::from_utf8(first.run_json)
                .unwrap()
                .contains("omitted-until-producer-api-is-stable")
        );
    }

    #[test]
    fn config_identity_domain_tracks_the_validated_build_config_schema() {
        let config = CorpusBuildConfigV2::attempt_zero(7, 1);
        let id = corpus_config_id_v3(&config).unwrap();
        assert!(id.starts_with("downwards-corpus-build-config-v4-"), "{id}");

        let config_json = serde_json::to_vec(&config).unwrap();
        let mut expected_input = b"downwards-corpus-build-config-v4\0".to_vec();
        expected_input.extend(&config_json);
        assert_eq!(
            id,
            format!(
                "downwards-corpus-build-config-v4-{:016x}",
                fingerprint_bytes_v3(&expected_input)
            )
        );

        let mut obsolete_input = b"downwards-corpus-build-config-v3\0".to_vec();
        obsolete_input.extend(config_json);
        assert_ne!(
            id,
            format!(
                "downwards-corpus-build-config-v3-{:016x}",
                fingerprint_bytes_v3(&obsolete_input)
            )
        );
    }

    #[test]
    fn strict_parser_rejects_unknown_fields_and_unreachable_claims_after_rehash() {
        let mut unknown = verified_bundle_v3();
        let text = String::from_utf8(unknown.attempts_jsonl).unwrap();
        unknown.attempts_jsonl = text
            .replacen("}\n", ",\"unexpected\":true}\n", 1)
            .into_bytes();
        rehash_stream(&mut unknown, STREAM_NAMES[0]);
        assert!(
            verify_artifact_bundle_v3(&unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );

        let mut unreachable = verified_bundle_v3();
        let text = String::from_utf8(unreachable.routes_jsonl).unwrap();
        assert!(text.contains("bounded-inconclusive"));
        unreachable.routes_jsonl = text
            .replacen("bounded-inconclusive", "unreachable", 1)
            .into_bytes();
        rehash_stream(&mut unreachable, STREAM_NAMES[2]);
        let error = verify_artifact_bundle_v3(&unreachable)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown variant `unreachable`"), "{error}");
    }

    #[test]
    fn descriptor_and_positive_replay_corruption_are_detected_after_rehash() {
        let mut descriptor = verified_bundle_v3();
        let marker = "\"tile_size\":";
        let text = String::from_utf8(descriptor.rooms_jsonl).unwrap();
        let start = text.find(marker).unwrap() + marker.len();
        let mut bytes = text.into_bytes();
        bytes[start] = if bytes[start] == b'9' { b'8' } else { b'9' };
        descriptor.rooms_jsonl = bytes;
        rehash_stream(&mut descriptor, STREAM_NAMES[1]);
        let error = verify_artifact_bundle_v3(&descriptor)
            .unwrap_err()
            .to_string();
        assert!(error.contains("physical descriptor differs"), "{error}");

        let mut replay = verified_bundle_v3();
        assert!(!replay.witnesses_jsonl.is_empty());
        let marker = "\"initial_digest\":\"";
        let text = String::from_utf8(replay.witnesses_jsonl).unwrap();
        let start = text.find(marker).unwrap() + marker.len();
        let mut bytes = text.into_bytes();
        bytes[start] = if bytes[start] == b'0' { b'1' } else { b'0' };
        replay.witnesses_jsonl = bytes;
        rehash_stream(&mut replay, STREAM_NAMES[4]);
        let error = verify_artifact_bundle_v3(&replay).unwrap_err().to_string();
        assert!(error.contains("initial digest differs"), "{error}");
    }

    #[test]
    fn config_incompatible_recorded_search_effort_is_rejected_after_rehash() {
        let mut bundle = verified_bundle_v3();
        let mut rows: Vec<TargetRowRecordV3> =
            parse_canonical_jsonl_v3(STREAM_NAMES[2], &bundle.routes_jsonl).unwrap();
        let search_effort = rows
            .iter_mut()
            .find_map(|row| match &mut row.evidence {
                TargetEvidenceRecordV3::BoundedInconclusive { search_effort, .. } => {
                    Some(search_effort)
                }
                TargetEvidenceRecordV3::PositiveReplay { .. } => None,
            })
            .expect("the one-node test search retains bounded route rows");
        search_effort.expanded_nodes = search_effort.expanded_nodes.saturating_add(1);
        bundle.routes_jsonl = render_json_lines_v3(rows.iter()).unwrap();
        rehash_stream(&mut bundle, STREAM_NAMES[2]);
        let error = verify_artifact_bundle_v3(&bundle).unwrap_err().to_string();
        assert!(
            error.contains("inconsistent recorded search observations"),
            "{error}"
        );
    }

    #[test]
    fn promotion_gate_config_and_direct_witness_corruption_are_rejected_after_rehash() {
        let mut decision_bundle = verified_bundle_v3();
        let mut decision_rooms: Vec<RoomRecordV3> =
            parse_canonical_jsonl_v3(STREAM_NAMES[1], &decision_bundle.rooms_jsonl).unwrap();
        let decision = decision_rooms
            .iter_mut()
            .flat_map(|room| &mut room.variant_ability_promotion_gates)
            .find_map(|gate| {
                matches!(
                    gate.evidence,
                    VariantAbilityPromotionEvidenceRecordV3::Ability { .. }
                )
                .then_some(&mut gate.decision)
            })
            .expect("seed zero retains promoted-source gate evidence");
        *decision = AbilityPromotionDecisionRecordV3::NotApplicable;
        decision_bundle.rooms_jsonl = render_json_lines_v3(decision_rooms.iter()).unwrap();
        rehash_stream(&mut decision_bundle, STREAM_NAMES[1]);
        let error = verify_artifact_bundle_v3(&decision_bundle)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("advertised-pair audit rerun")
                || error.contains("full replay-rehydrated room validation"),
            "{error}"
        );

        let mut config_bundle = verified_bundle_v3();
        let mut config_rooms: Vec<RoomRecordV3> =
            parse_canonical_jsonl_v3(STREAM_NAMES[1], &config_bundle.rooms_jsonl).unwrap();
        let config = &mut config_rooms[0].ability_promotion_audit_config;
        config.direct_controller_solver.max_expanded_nodes = config
            .direct_controller_solver
            .max_expanded_nodes
            .saturating_add(1);
        config.config_id = config.recomputed_config_id();
        config.validate().unwrap();
        config_bundle.rooms_jsonl = render_json_lines_v3(config_rooms.iter()).unwrap();
        rehash_stream(&mut config_bundle, STREAM_NAMES[1]);
        let error = verify_artifact_bundle_v3(&config_bundle)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("differs from the exact shard config"),
            "{error}"
        );

        let mut witness_bundle = verified_bundle_v3();
        let mut witness_rooms: Vec<RoomRecordV3> =
            parse_canonical_jsonl_v3(STREAM_NAMES[1], &witness_bundle.rooms_jsonl).unwrap();
        let corrupted_direct_witness = if let Some(digest) = witness_rooms
            .iter_mut()
            .flat_map(|room| &mut room.variant_ability_promotion_gates)
            .find_map(|gate| match &mut gate.evidence {
                VariantAbilityPromotionEvidenceRecordV3::Ability {
                    direct_route_assessment: Some(assessment),
                    ..
                } => assessment
                    .easiest_first_witnesses
                    .first_mut()
                    .map(|witness| &mut witness.initial_digest),
                VariantAbilityPromotionEvidenceRecordV3::NotApplicable
                | VariantAbilityPromotionEvidenceRecordV3::Ability {
                    direct_route_assessment: None,
                    ..
                } => None,
            }) {
            let replacement = if digest.starts_with('0') { '1' } else { '0' };
            digest.replace_range(..1, &replacement.to_string());
            true
        } else {
            let policy = witness_rooms
                .iter_mut()
                .flat_map(|room| &mut room.variant_ability_promotion_gates)
                .find_map(|gate| match &mut gate.evidence {
                    VariantAbilityPromotionEvidenceRecordV3::Ability {
                        direct_route_assessment: Some(assessment),
                        ..
                    } => Some(&mut assessment.policy),
                    VariantAbilityPromotionEvidenceRecordV3::NotApplicable
                    | VariantAbilityPromotionEvidenceRecordV3::Ability {
                        direct_route_assessment: None,
                        ..
                    } => None,
                })
                .expect("seed zero retains a required direct-controller assessment");
            policy.assessment_version = policy.assessment_version.saturating_add(1);
            false
        };
        witness_bundle.rooms_jsonl = render_json_lines_v3(witness_rooms.iter()).unwrap();
        rehash_stream(&mut witness_bundle, STREAM_NAMES[1]);
        let error = verify_artifact_bundle_v3(&witness_bundle)
            .unwrap_err()
            .to_string();
        if corrupted_direct_witness {
            assert!(error.contains("initial digest differs"), "{error}");
        } else {
            assert!(error.contains("advertised-pair audit rerun"), "{error}");
        }
    }

    #[test]
    fn create_new_bundle_and_atomic_checkpoint_never_overwrite() {
        let bundle = verified_bundle_v3();
        let verified = verify_artifact_bundle_v3(&bundle).unwrap();
        let unique = format!(
            "downwards-corpus-v3-{}-{:016x}",
            std::process::id(),
            fingerprint_bytes_v3(&bundle.run_json)
        );
        let directory = std::env::temp_dir().join(unique);
        reset_semantic_verifier_passes_v3();
        assert!(matches!(
            write_or_verify_seed_shard_v3(&directory, &verified.config, &bundle).unwrap(),
            CorpusSeedShardOutcomeV3::Written(_)
        ));
        assert_eq!(semantic_verifier_passes_v3(), 2);
        reset_semantic_verifier_passes_v3();
        assert_eq!(verify_artifact_directory_v3(&directory).unwrap(), verified);
        assert_eq!(semantic_verifier_passes_v3(), 1);
        reset_semantic_verifier_passes_v3();
        assert!(matches!(
            write_or_verify_seed_shard_v3(&directory, &verified.config, &bundle).unwrap(),
            CorpusSeedShardOutcomeV3::AlreadyVerified(_)
        ));
        assert_eq!(semantic_verifier_passes_v3(), 1);
        assert!(matches!(
            write_new_artifact_bundle_v3(&directory, &bundle),
            Err(CorpusArtifactV3Error::AlreadyExists(_))
        ));

        let checkpoint_path = directory.join(CORPUS_ARTIFACT_V3_CHECKPOINT_FILE);
        assert!(matches!(
            write_new_seed_checkpoint_v3(&checkpoint_path, &bundle),
            Err(CorpusArtifactV3Error::AlreadyExists(path)) if path == checkpoint_path
        ));
        let checkpoint = fs::read(&checkpoint_path).unwrap();
        reset_semantic_verifier_passes_v3();
        verify_seed_checkpoint_v3(&checkpoint, &verified.config, &bundle).unwrap();
        assert_eq!(semantic_verifier_passes_v3(), 1);
        let mut corrupt_checkpoint = parse_seed_checkpoint_v3(&checkpoint).unwrap();
        corrupt_checkpoint
            .artifact_hashes
            .insert(FILE_NAMES[1].to_owned(), "fnv1a64-corrupt".to_owned());
        let mut corrupt_checkpoint_bytes = serde_json::to_vec(&corrupt_checkpoint).unwrap();
        corrupt_checkpoint_bytes.push(b'\n');
        reset_semantic_verifier_passes_v3();
        assert!(
            verify_seed_checkpoint_v3(&corrupt_checkpoint_bytes, &verified.config, &bundle)
                .is_err()
        );
        assert_eq!(semantic_verifier_passes_v3(), 1);
        let mut corrupt_bundle = bundle.clone();
        corrupt_bundle.rooms_jsonl.push(b' ');
        reset_semantic_verifier_passes_v3();
        assert!(verify_seed_checkpoint_v3(&checkpoint, &verified.config, &corrupt_bundle).is_err());
        assert_eq!(semantic_verifier_passes_v3(), 1);
        let mut wrong_config = verified.config.clone();
        wrong_config.start_seed += 1;
        assert!(verify_seed_shard_directory_v3(&directory, &wrong_config).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn directory_verifier_rejects_mutation_between_semantic_and_stability_checks() {
        let bundle = verified_bundle_v3();
        let verified = verify_artifact_bundle_v3(&bundle).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "downwards-corpus-v3-toctou-{}-{:016x}",
            std::process::id(),
            fingerprint_bytes_v3(&bundle.attempts_jsonl)
        ));
        write_or_verify_seed_shard_v3(&directory, &verified.config, &bundle).unwrap();
        let mutated_path = directory.join(FILE_NAMES[1]);
        set_before_shard_stability_check_v3(move || {
            let mut bytes = fs::read(&mutated_path).unwrap();
            bytes.push(b' ');
            fs::write(&mutated_path, bytes).unwrap();
        });
        reset_semantic_verifier_passes_v3();
        let error = verify_seed_shard_directory_v3(&directory, &verified.config)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("changed during semantic verification"),
            "{error}"
        );
        assert_eq!(semantic_verifier_passes_v3(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn existing_incomplete_shard_fails_closed_without_overwrite() {
        let bundle = verified_bundle_v3();
        let verified = verify_artifact_bundle_v3(&bundle).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "downwards-corpus-v3-incomplete-{}-{:016x}",
            std::process::id(),
            fingerprint_bytes_v3(&bundle.rooms_jsonl)
        ));
        fs::create_dir(&directory).unwrap();
        let sentinel = directory.join("sentinel");
        fs::write(&sentinel, b"preserve").unwrap();
        let error = write_or_verify_seed_shard_v3(&directory, &verified.config, &bundle)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(FILE_NAMES[0]) || error.contains("no completion checkpoint"),
            "{error}"
        );
        assert_eq!(fs::read(&sentinel).unwrap(), b"preserve");
        assert!(!directory.join(FILE_NAMES[0]).exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
