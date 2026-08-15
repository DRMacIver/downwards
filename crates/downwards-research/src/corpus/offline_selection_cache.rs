//! Resumable operational cache between verified corpus-v3 shards and selection.
//!
//! Cache rows are checksum-bound deterministic observations, not independent
//! evidence.  They may be used to resume expensive overgeneration and build a
//! provisional archive.  Final publication must rehydrate the source shards,
//! recompute every selected room's complete deep analysis and reports, and
//! require exact descriptor equality.

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    thread,
};

use downwards_ai::{
    DifficultyConfig, SHAKY_HAND_CONFIG_VERSION, SHAKY_HAND_EVIDENCE_DISCLAIMER,
    SHAKY_HAND_POLICY_VERSION,
};
use downwards_core::{BoundarySide, DoorSocket};
use downwards_lab::TraversalGrid;
use serde::{Deserialize, Serialize};

use super::artifact_v3_rehydrate::load_verified_rehydrated_artifact_v3_shards;

use super::{
    ABILITY_BYPASS_PROJECTION_VERSION, CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION,
    CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION, CORPUS_ARTIFACT_V3_SCHEMA_VERSION,
    CORPUS_ARTIFACT_V3_SEARCH_OBSERVATION_POLICY_VERSION, CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION,
    CORPUS_CANDIDATE_KEY_RECORD_VERSION, CORPUS_PHYSICAL_ROOM_ID_VERSION,
    CORPUS_ROOM_ANALYSIS_VERSION, CORPUS_SELECTION_INPUT_V2_VERSION,
    CORPUS_SELECTION_METRICS_DISCLAIMER, CORPUS_SELECTION_METRICS_VERSION,
    CORPUS_SHAKY_HAND_ANALYSIS_VERSION, CORPUS_SHAKY_HAND_DISCLAIMER,
    CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION, CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY,
    CorpusBuildConfigV2, CorpusCandidateKeyRecord, CorpusRoomAnalysisConfig,
    CorpusRoomAnalysisConfigRecord, CorpusRouteRepresentativeStatus, CorpusSeedCheckpointV3,
    CorpusSelectionAudit, CorpusSelectionConfig, CorpusSelectionDescriptor,
    CorpusSelectionMetricsError, CorpusSelectionOutcome, CorpusSelectionProjection,
    CorpusSelectionProjectionId, CorpusSelectionQualityAxis, CorpusSelectionQualityCoordinate,
    CorpusSelectionStep, CorpusShakyHandAnalysis, CorpusShakyHandConfig, CorpusShakyHandEvidence,
    DIRECTED_LOADOUT_CONTROLLER_PROJECTION_VERSION, DIRECTIONAL_ASYMMETRY_PROJECTION_VERSION,
    EASIEST_KNOWN_ROUTE_FUSION_VERSION, EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION,
    EASIEST_KNOWN_ROUTE_SELECTION_VERSION, IntegerCoordinateDistribution,
    LANDING_GEOMETRY_PROJECTION_VERSION, MORPHOLOGY_TOPOLOGY_PROJECTION_VERSION,
    NamedSelectionCoordinate, NormalizedDistanceSamples, OBSERVED_ROUTE_CHOICES_PROJECTION_VERSION,
    ObjectiveDirection, PICKUP_CHALLENGE_DETOUR_PROJECTION_VERSION, PICKUP_DETOUR_ANALYSIS_VERSION,
    PICKUP_DETOUR_EVIDENCE_DISCLAIMER, PICKUP_DETOUR_SELECTION_METRICS_VERSION,
    PickupChallengeCoordinateDistributions, PickupDetourCoordinateDistributions,
    PickupDetourSelectionAggregate, PickupDetourSelectionMetricSummary, QuantizedSelectionEvidence,
    ROOM_METRIC_SUMMARY_DISCLAIMER, ROOM_METRIC_SUMMARY_VERSION, ROUTE_CHOICE_DIVERSITY_DISCLAIMER,
    ROUTE_CHOICE_DIVERSITY_VERSION, ROUTE_CHOICE_SELECTION_METRICS_VERSION,
    ROUTE_DIVERSITY_PROJECTION_VERSION, RehydratedCorpusV3, ReusableSocketSelectionPackage, RoomId,
    RoomPickupDetourAnalysis, RoomRouteChoiceDiversity, RouteAlternativeDistanceReport,
    RouteChoiceSelectionAggregate, RouteChoiceSelectionMetricSummary,
    SHAKY_HAND_ROUTE_SEED_VERSION, SelectionEvidenceState, SocketCoveragePruningStep,
    TERRAIN_ABLATION_PROJECTION_VERSION, WithinCellDistanceDistribution, analyze_corpus_room_v2,
    analyze_pickup_detours_v2, analyze_route_choice_diversity_v2, assess_corpus_shaky_hand_v2,
    describe_corpus_selection_room_v2, describe_corpus_selection_room_v2_exploratory,
    resolve_corpus_metric_candidate_v2, seed_shard_directory_v3,
    select_validated_cached_corpus_descriptors_v2, summarize_pickup_detour_selection_metrics,
    summarize_room_metrics, summarize_route_choice_selection_metrics,
};

/// Stable schema of the operational deep-cache manifest and room rows.
pub const CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION: u32 = 1;
/// Stable hash/checkpoint policy for cache and provisional-selection files.
pub const CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION: u32 = 1;

/// Fixed production-width room parallelism. Room jobs are independent and
/// their outputs remain ordered by exact room identity; this is deliberately
/// not a general-purpose or nested worker pool.
pub const OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1: usize = 2;

const CACHE_STATUS: &str = "operational-deep-cache";
const CACHE_ROW_STATUS: &str = "operational-deep-cache-row";
const COMPLETE_STATUS: &str = "complete";
const CACHE_MANIFEST_FILE: &str = "cache-manifest.json";
const CACHE_COMPLETION_FILE: &str = "cache-checkpoint.json";
const CACHE_ROOMS_DIRECTORY: &str = "rooms";
const CACHE_ROOM_RECORD_FILE: &str = "record.json";
const CACHE_ROOM_CHECKPOINT_FILE: &str = "checkpoint.json";
const SELECTION_RECORD_FILE: &str = "selection.json";
const SELECTION_CHECKPOINT_FILE: &str = "selection-checkpoint.json";

/// Exact current policy identities which can change descriptor meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCachePolicyRecordV1 {
    pub artifact_schema_version: u32,
    pub artifact_checkpoint_version: u32,
    pub artifact_action_encoding_version: u32,
    pub artifact_search_observation_policy_version: u32,
    pub build_config_schema_version: u32,
    pub source_capability_enumeration_policy_version: u32,
    pub candidate_key_record_version: u32,
    pub physical_room_id_version: u32,
    pub analysis_version: u32,
    pub room_metric_summary_version: u32,
    pub route_fusion_version: u32,
    pub route_fusion_selection_version: u32,
    pub route_fusion_replay_identity_version: u32,
    pub route_fusion_controller_demand_version: u32,
    pub route_fusion_measurement_version: u32,
    pub route_fusion_difficulty_vector_version: u32,
    pub route_fusion_perfect_control_comparison_version: u32,
    pub shaky_hand_analysis_version: u32,
    pub shaky_hand_seed_derivation_version: u32,
    pub shaky_hand_ai_policy_version: u32,
    pub shaky_hand_ai_config_version: u32,
    pub route_choice_version: u32,
    pub route_choice_selection_metrics_version: u32,
    pub pickup_detour_version: u32,
    pub pickup_detour_selection_metrics_version: u32,
    pub selection_metrics_version: u32,
    pub selection_input_version: u32,
    pub projection_versions: ProjectionPolicyRecordV1,
    pub disclaimers: OfflineCacheDisclaimerRecordV1,
}

impl OfflineCachePolicyRecordV1 {
    #[must_use]
    pub fn current() -> Self {
        Self {
            artifact_schema_version: CORPUS_ARTIFACT_V3_SCHEMA_VERSION,
            artifact_checkpoint_version: CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION,
            artifact_action_encoding_version: CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION,
            artifact_search_observation_policy_version:
                CORPUS_ARTIFACT_V3_SEARCH_OBSERVATION_POLICY_VERSION,
            build_config_schema_version: CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION,
            source_capability_enumeration_policy_version:
                CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION,
            candidate_key_record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            physical_room_id_version: CORPUS_PHYSICAL_ROOM_ID_VERSION,
            analysis_version: CORPUS_ROOM_ANALYSIS_VERSION,
            room_metric_summary_version: ROOM_METRIC_SUMMARY_VERSION,
            route_fusion_version: EASIEST_KNOWN_ROUTE_FUSION_VERSION,
            route_fusion_selection_version: EASIEST_KNOWN_ROUTE_SELECTION_VERSION,
            route_fusion_replay_identity_version: EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION,
            route_fusion_controller_demand_version: CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY
                .controller_demand_version,
            route_fusion_measurement_version: CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY
                .route_measurement_version,
            route_fusion_difficulty_vector_version: CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY
                .route_difficulty_vector_version,
            route_fusion_perfect_control_comparison_version:
                CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY.perfect_control_comparison_version,
            shaky_hand_analysis_version: CORPUS_SHAKY_HAND_ANALYSIS_VERSION,
            shaky_hand_seed_derivation_version: SHAKY_HAND_ROUTE_SEED_VERSION,
            shaky_hand_ai_policy_version: SHAKY_HAND_POLICY_VERSION,
            shaky_hand_ai_config_version: SHAKY_HAND_CONFIG_VERSION,
            route_choice_version: ROUTE_CHOICE_DIVERSITY_VERSION,
            route_choice_selection_metrics_version: ROUTE_CHOICE_SELECTION_METRICS_VERSION,
            pickup_detour_version: PICKUP_DETOUR_ANALYSIS_VERSION,
            pickup_detour_selection_metrics_version: PICKUP_DETOUR_SELECTION_METRICS_VERSION,
            selection_metrics_version: CORPUS_SELECTION_METRICS_VERSION,
            selection_input_version: CORPUS_SELECTION_INPUT_V2_VERSION,
            projection_versions: ProjectionPolicyRecordV1::current(),
            disclaimers: OfflineCacheDisclaimerRecordV1::current(),
        }
    }

    fn validate(&self) -> Result<(), OfflineSelectionCacheError> {
        if self != &Self::current() {
            return Err(invalid(
                "offline cache policy does not equal the current exact policy",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheDisclaimerRecordV1 {
    pub cache_trust_boundary: String,
    pub room_metrics: String,
    pub shaky_hand: String,
    pub route_choices: String,
    pub pickup_detours: String,
    pub selection: String,
}

impl OfflineCacheDisclaimerRecordV1 {
    fn current() -> Self {
        Self {
            cache_trust_boundary: "operational cache rows are checksum-bound deterministic observations, not independent proof; final publication requires source rehydration and exact full recomputation of every selected descriptor".to_owned(),
            room_metrics: ROOM_METRIC_SUMMARY_DISCLAIMER.to_owned(),
            shaky_hand: format!(
                "{CORPUS_SHAKY_HAND_DISCLAIMER}; {SHAKY_HAND_EVIDENCE_DISCLAIMER}"
            ),
            route_choices: ROUTE_CHOICE_DIVERSITY_DISCLAIMER.to_owned(),
            pickup_detours: PICKUP_DETOUR_EVIDENCE_DISCLAIMER.to_owned(),
            selection: CORPUS_SELECTION_METRICS_DISCLAIMER.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPolicyRecordV1 {
    pub morphology_topology: u32,
    pub directed_loadout_controller: u32,
    pub landing_geometry: u32,
    pub directional_asymmetry: u32,
    pub route_diversity: u32,
    pub ability_bypass: u32,
    pub terrain_ablation: u32,
    pub observed_route_choices: u32,
    pub pickup_challenge_detour: u32,
}

impl ProjectionPolicyRecordV1 {
    const fn current() -> Self {
        Self {
            morphology_topology: MORPHOLOGY_TOPOLOGY_PROJECTION_VERSION,
            directed_loadout_controller: DIRECTED_LOADOUT_CONTROLLER_PROJECTION_VERSION,
            landing_geometry: LANDING_GEOMETRY_PROJECTION_VERSION,
            directional_asymmetry: DIRECTIONAL_ASYMMETRY_PROJECTION_VERSION,
            route_diversity: ROUTE_DIVERSITY_PROJECTION_VERSION,
            ability_bypass: ABILITY_BYPASS_PROJECTION_VERSION,
            terrain_ablation: TERRAIN_ABLATION_PROJECTION_VERSION,
            observed_route_choices: OBSERVED_ROUTE_CHOICES_PROJECTION_VERSION,
            pickup_challenge_detour: PICKUP_CHALLENGE_DETOUR_PROJECTION_VERSION,
        }
    }
}

/// Immutable identity of one independently verified artifact-v3 source shard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheSourceShardV1 {
    pub seed: u64,
    pub config_id: String,
    pub checkpoint_hash: String,
    pub checkpoint: CorpusSeedCheckpointV3,
}

/// One room expected from the exact source-shard pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheExpectedRoomV1 {
    pub source_seed: u64,
    pub room_id: RoomId,
    pub canonical_key: CorpusCandidateKeyRecord,
}

/// Immutable run identity written before any expensive room work begins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheManifestV1 {
    pub schema_version: u32,
    pub status: String,
    pub run_id: String,
    pub start_seed: u64,
    pub seed_count: usize,
    pub sources: Vec<OfflineCacheSourceShardV1>,
    pub analysis_config: CorpusRoomAnalysisConfigRecord,
    pub shaky_hand_config: CorpusShakyHandConfig,
    pub policies: OfflineCachePolicyRecordV1,
    pub expected_rooms: Vec<OfflineCacheExpectedRoomV1>,
}

/// Stable wire mirror of [`CorpusSelectionDescriptor`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionDescriptorRecordV1 {
    pub version: u32,
    pub room_id: RoomId,
    pub projections: Vec<SelectionProjectionRecordV1>,
    pub quality: Vec<SelectionQualityRecordV1>,
    pub diversity_coordinates: Vec<i64>,
    pub sockets: Vec<DoorSocketRecordV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionProjectionRecordV1 {
    pub id: SelectionProjectionIdRecordV1,
    pub cell: Vec<NamedSelectionCoordinateRecordV1>,
    pub detail: Vec<NamedSelectionCoordinateRecordV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionProjectionIdRecordV1 {
    MorphologyTopologyV1,
    DirectedLoadoutControllerDemandV1,
    LandingGeometryV1,
    DirectionalAsymmetryV1,
    RouteDiversityV1,
    AbilityBypassStructureV1,
    TerrainAblationUtilityV1,
    ObservedWithinCellRouteChoicesV1,
    PickupChallengeDetourV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedSelectionCoordinateRecordV1 {
    pub name: String,
    pub evidence: SelectionEvidenceRecordV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionEvidenceRecordV1 {
    pub state: SelectionEvidenceStateRecordV1,
    pub value: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionEvidenceStateRecordV1 {
    Observed,
    Missing,
    NotApplicable,
    BoundedInconclusive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionQualityRecordV1 {
    pub axis: SelectionQualityAxisRecordV1,
    pub direction: ObjectiveDirectionRecordV1,
    pub evidence: SelectionEvidenceRecordV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionQualityAxisRecordV1 {
    CompleteKitOtherControllerDemand,
    CompleteKitMedianDuration,
    DirectionalDurationAsymmetry,
    JointRouteStyleDiversity,
    UncorroboratedTerrainComponents,
    UncorroboratedTerrainTiles,
    AblatedTilesAffectingStoredControllers,
    ControllerAblationEffect,
    OperationalSimulatedTicks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObjectiveDirectionRecordV1 {
    Maximize,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorSocketRecordV1 {
    pub side: BoundarySideRecordV1,
    pub offset: i32,
    pub span: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundarySideRecordV1 {
    Left,
    Right,
    Ceiling,
    Floor,
}

impl SelectionDescriptorRecordV1 {
    #[must_use]
    pub fn from_descriptor(descriptor: &CorpusSelectionDescriptor) -> Self {
        Self {
            version: descriptor.version,
            room_id: descriptor.room_id.clone(),
            projections: descriptor
                .projections
                .iter()
                .map(SelectionProjectionRecordV1::from_projection)
                .collect(),
            quality: descriptor
                .quality
                .iter()
                .map(SelectionQualityRecordV1::from_quality)
                .collect(),
            diversity_coordinates: descriptor.diversity_coordinates.clone(),
            sockets: descriptor
                .sockets
                .iter()
                .copied()
                .map(DoorSocketRecordV1::from_socket)
                .collect(),
        }
    }

    pub fn to_descriptor(&self) -> Result<CorpusSelectionDescriptor, OfflineSelectionCacheError> {
        let descriptor = CorpusSelectionDescriptor {
            version: self.version,
            room_id: self.room_id.clone(),
            projections: self
                .projections
                .iter()
                .map(SelectionProjectionRecordV1::to_projection)
                .collect::<Result<Vec<_>, _>>()?,
            quality: self
                .quality
                .iter()
                .map(SelectionQualityRecordV1::to_quality)
                .collect::<Result<Vec<_>, _>>()?,
            diversity_coordinates: self.diversity_coordinates.clone(),
            sockets: self
                .sockets
                .iter()
                .copied()
                .map(DoorSocketRecordV1::to_socket)
                .collect::<Result<Vec<_>, _>>()?,
        };
        if SelectionDescriptorRecordV1::from_descriptor(&descriptor) != *self {
            return Err(invalid(
                "selection descriptor wire round trip changed the record",
            ));
        }
        Ok(descriptor)
    }
}

impl SelectionProjectionRecordV1 {
    fn from_projection(projection: &CorpusSelectionProjection) -> Self {
        Self {
            id: projection.id.into(),
            cell: projection
                .cell
                .iter()
                .map(NamedSelectionCoordinateRecordV1::from_coordinate)
                .collect(),
            detail: projection
                .detail
                .iter()
                .map(NamedSelectionCoordinateRecordV1::from_coordinate)
                .collect(),
        }
    }

    fn to_projection(&self) -> Result<CorpusSelectionProjection, OfflineSelectionCacheError> {
        Ok(CorpusSelectionProjection {
            id: self.id.into(),
            cell: self
                .cell
                .iter()
                .map(NamedSelectionCoordinateRecordV1::to_coordinate)
                .collect::<Result<Vec<_>, _>>()?,
            detail: self
                .detail
                .iter()
                .map(NamedSelectionCoordinateRecordV1::to_coordinate)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

impl NamedSelectionCoordinateRecordV1 {
    fn from_coordinate(coordinate: &NamedSelectionCoordinate) -> Self {
        Self {
            name: coordinate.name.clone(),
            evidence: SelectionEvidenceRecordV1::from_evidence(&coordinate.evidence),
        }
    }

    fn to_coordinate(&self) -> Result<NamedSelectionCoordinate, OfflineSelectionCacheError> {
        Ok(NamedSelectionCoordinate {
            name: self.name.clone(),
            evidence: self.evidence.to_evidence()?,
        })
    }
}

impl SelectionEvidenceRecordV1 {
    fn from_evidence(evidence: &QuantizedSelectionEvidence) -> Self {
        Self {
            state: evidence.state.into(),
            value: evidence.value,
        }
    }

    fn to_evidence(&self) -> Result<QuantizedSelectionEvidence, OfflineSelectionCacheError> {
        let evidence = QuantizedSelectionEvidence {
            state: self.state.into(),
            value: self.value,
        };
        let valid_shape = matches!(
            (evidence.state, evidence.value),
            (SelectionEvidenceState::Observed, Some(_))
                | (
                    SelectionEvidenceState::Missing
                        | SelectionEvidenceState::NotApplicable
                        | SelectionEvidenceState::BoundedInconclusive,
                    None
                )
        );
        if !valid_shape {
            return Err(invalid("selection evidence state/value shape is invalid"));
        }
        Ok(evidence)
    }
}

impl SelectionQualityRecordV1 {
    fn from_quality(quality: &CorpusSelectionQualityCoordinate) -> Self {
        Self {
            axis: quality.axis.into(),
            direction: quality.direction.into(),
            evidence: SelectionEvidenceRecordV1::from_evidence(&quality.evidence),
        }
    }

    fn to_quality(&self) -> Result<CorpusSelectionQualityCoordinate, OfflineSelectionCacheError> {
        Ok(CorpusSelectionQualityCoordinate {
            axis: self.axis.into(),
            direction: self.direction.into(),
            evidence: self.evidence.to_evidence()?,
        })
    }
}

impl DoorSocketRecordV1 {
    const fn from_socket(socket: DoorSocket) -> Self {
        Self {
            side: match socket.side {
                BoundarySide::Left => BoundarySideRecordV1::Left,
                BoundarySide::Right => BoundarySideRecordV1::Right,
                BoundarySide::Ceiling => BoundarySideRecordV1::Ceiling,
                BoundarySide::Floor => BoundarySideRecordV1::Floor,
            },
            offset: socket.offset,
            span: socket.span,
        }
    }

    fn to_socket(self) -> Result<DoorSocket, OfflineSelectionCacheError> {
        if self.offset < 0 || self.span <= 0 {
            return Err(invalid(
                "cached door socket has negative offset or non-positive span",
            ));
        }
        Ok(DoorSocket {
            side: match self.side {
                BoundarySideRecordV1::Left => BoundarySide::Left,
                BoundarySideRecordV1::Right => BoundarySide::Right,
                BoundarySideRecordV1::Ceiling => BoundarySide::Ceiling,
                BoundarySideRecordV1::Floor => BoundarySide::Floor,
            },
            offset: self.offset,
            span: self.span,
        })
    }
}

macro_rules! bidirectional_enum_conversion {
    ($wire:ty, $native:ty, {$($variant:ident),+ $(,)?}) => {
        impl From<$native> for $wire {
            fn from(value: $native) -> Self {
                match value { $(<$native>::$variant => Self::$variant),+ }
            }
        }
        impl From<$wire> for $native {
            fn from(value: $wire) -> Self {
                match value { $(<$wire>::$variant => Self::$variant),+ }
            }
        }
    };
}

bidirectional_enum_conversion!(SelectionProjectionIdRecordV1, CorpusSelectionProjectionId, {
    MorphologyTopologyV1,
    DirectedLoadoutControllerDemandV1,
    LandingGeometryV1,
    DirectionalAsymmetryV1,
    RouteDiversityV1,
    AbilityBypassStructureV1,
    TerrainAblationUtilityV1,
    ObservedWithinCellRouteChoicesV1,
    PickupChallengeDetourV1,
});
bidirectional_enum_conversion!(SelectionEvidenceStateRecordV1, SelectionEvidenceState, {
    Observed,
    Missing,
    NotApplicable,
    BoundedInconclusive,
});
bidirectional_enum_conversion!(SelectionQualityAxisRecordV1, CorpusSelectionQualityAxis, {
    CompleteKitOtherControllerDemand,
    CompleteKitMedianDuration,
    DirectionalDurationAsymmetry,
    JointRouteStyleDiversity,
    UncorroboratedTerrainComponents,
    UncorroboratedTerrainTiles,
    AblatedTilesAffectingStoredControllers,
    ControllerAblationEffect,
    OperationalSimulatedTicks,
});
bidirectional_enum_conversion!(ObjectiveDirectionRecordV1, ObjectiveDirection, {
    Maximize,
    Minimize,
});

/// Why a cache row may or may not enter authoritative production selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OfflineCacheProductionEligibilityV1 {
    Eligible,
    Ineligible {
        unavailable_coordinates: Vec<UnavailableSelectionCoordinateV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnavailableSelectionCoordinateV1 {
    pub coordinate: String,
    pub state: SelectionEvidenceStateRecordV1,
}

/// One immutable completed operational observation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheRoomRecordV1 {
    pub schema_version: u32,
    pub status: String,
    pub run_id: String,
    pub source: OfflineCacheSourceShardV1,
    pub room_id: RoomId,
    pub canonical_key: CorpusCandidateKeyRecord,
    pub analysis_config: CorpusRoomAnalysisConfigRecord,
    pub policies: OfflineCachePolicyRecordV1,
    pub descriptor: SelectionDescriptorRecordV1,
    pub production_eligibility: OfflineCacheProductionEligibilityV1,
    /// Full deterministic diagnostic over the fused exact representative.
    /// It is not a perfect-play selection scalar and remains operational
    /// cache data until exact selected-room recomputation.
    pub shaky_hand: CorpusShakyHandAnalysis,
    pub reports: OfflineCacheReportSummariesV1,
}

/// Report summaries are descriptive and never used to reconstruct native
/// evidence.  The authoritative selection input is `descriptor` above.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheReportSummariesV1 {
    pub room_metrics: TransparentReportSummaryV1,
    pub route_choices: TransparentReportSummaryV1,
    pub pickup_detours: TransparentReportSummaryV1,
}

/// Stable, independently named report coordinates. These are deliberately a
/// vector rather than an untyped JSON object: rows are strictly ordered,
/// names are unique, and every value has an explicit semantic shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransparentReportSummaryV1 {
    pub source_version: u32,
    pub room_id: RoomId,
    pub coordinates: Vec<NamedReportCoordinateV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedReportCoordinateV1 {
    pub name: String,
    pub value: ReportValueV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReportValueV1 {
    Count {
        value: usize,
    },
    OperationalCount {
        value: usize,
    },
    Fraction {
        numerator: usize,
        denominator: usize,
    },
    IntegerDistribution {
        sample_count: usize,
        minimum: usize,
        median_lower: usize,
        median_upper: usize,
        maximum: usize,
        spread: usize,
    },
    SignedIntegerDistribution {
        sample_count: usize,
        minimum: i32,
        median_lower: i32,
        median_upper: i32,
        maximum: i32,
        spread: u32,
    },
    Histogram {
        bins: Vec<HistogramBinV1>,
    },
    NormalizedSamples {
        samples: usize,
        minimum: Option<UnitIntervalFloatV1>,
        p10: Option<UnitIntervalFloatV1>,
        median: Option<UnitIntervalFloatV1>,
        p90: Option<UnitIntervalFloatV1>,
        maximum: Option<UnitIntervalFloatV1>,
        mean: Option<UnitIntervalFloatV1>,
    },
    EvidenceState {
        state: ReportEvidenceStateV1,
        reason: String,
    },
}

/// One explicit histogram row. JSON object keys are always strings, so using
/// a numeric-keyed map here would not round-trip through `serde_json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistogramBinV1 {
    pub value: usize,
    pub count: usize,
}

/// Canonical, lossless representation of a finite normalized `f64`.
/// Persisting a JSON number directly is unsafe for byte-canonical read-back:
/// parser/formatter implementations can choose adjacent shortest decimals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitIntervalFloatV1 {
    pub decimal: String,
    pub ieee754_bits: u64,
}

impl UnitIntervalFloatV1 {
    fn from_value(value: f64) -> Self {
        Self {
            decimal: value.to_string(),
            ieee754_bits: value.to_bits(),
        }
    }

    fn validate(&self) -> bool {
        let value = f64::from_bits(self.ieee754_bits);
        value.is_finite() && (0.0..=1.0).contains(&value) && self.decimal == value.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportEvidenceStateV1 {
    Missing,
    NotApplicable,
    BoundedInconclusive,
}

/// Completion marker written only after its room record is read back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheRoomCheckpointV1 {
    pub checkpoint_version: u32,
    pub status: String,
    pub run_id: String,
    pub room_id: RoomId,
    pub record_hash: String,
}

/// Run marker written only after every expected room row verifies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineCacheCompletionV1 {
    pub checkpoint_version: u32,
    pub status: String,
    pub run_id: String,
    pub room_count: usize,
    pub eligible_room_count: usize,
    pub room_checkpoint_hashes: BTreeMap<RoomId, String>,
}

/// Provisional output may drive inspection and resume decisions, but only the
/// final-recomputed state may be published as corpus evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OfflineSelectionPublicationStateV1 {
    ProvisionalOperationalCache,
    FinalRecomputed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionConfigRecordV1 {
    pub requested_minimum: usize,
    pub requested_maximum: usize,
    pub elites_per_cell: usize,
}

impl From<CorpusSelectionConfig> for SelectionConfigRecordV1 {
    fn from(config: CorpusSelectionConfig) -> Self {
        Self {
            requested_minimum: config.requested_minimum,
            requested_maximum: config.requested_maximum,
            elites_per_cell: config.elites_per_cell,
        }
    }
}

impl From<SelectionConfigRecordV1> for CorpusSelectionConfig {
    fn from(config: SelectionConfigRecordV1) -> Self {
        Self::new(
            config.requested_minimum,
            config.requested_maximum,
            config.elites_per_cell,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveSummaryRecordV1 {
    pub submitted_candidates: usize,
    pub retained_candidates: usize,
    pub cell_count: usize,
    pub elite_placements: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveCellRecordV1 {
    pub projection: SelectionProjectionIdRecordV1,
    pub coordinates: Vec<i64>,
    pub elites: Vec<RoomId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveCellKeyRecordV1 {
    pub projection: SelectionProjectionIdRecordV1,
    pub coordinates: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketPackageRecordV1 {
    pub package_id: RoomId,
    pub room_ids: Vec<RoomId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketPruningStepRecordV1 {
    pub requested_removal: RoomId,
    pub removed_room_ids: Vec<RoomId>,
    pub remaining_rooms: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionStepRecordV1 {
    pub room_id: RoomId,
    pub marginal_cell_coverage: usize,
    pub minimum_l1_distance: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionAuditRecordV1 {
    pub version: u32,
    pub submitted_rooms: usize,
    pub exact_visual_unique_rooms: usize,
    pub archive: ArchiveSummaryRecordV1,
    pub archive_ranked_rooms: usize,
    pub rooms_excluded_by_initial_socket_core: Vec<RoomId>,
    pub socket_pruning_steps: Vec<SocketPruningStepRecordV1>,
    pub selected_rooms: usize,
    pub selected_package_count: usize,
    pub covered_cells: Vec<ArchiveCellKeyRecordV1>,
    pub steps: Vec<SelectionStepRecordV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionOutcomeRecordV1 {
    pub archive_summary: ArchiveSummaryRecordV1,
    pub archive_cells: Vec<ArchiveCellRecordV1>,
    pub descriptors: Vec<SelectionDescriptorRecordV1>,
    pub descriptor_hashes: BTreeMap<RoomId, String>,
    pub selected_room_ids: Vec<RoomId>,
    pub socket_packages: Vec<SocketPackageRecordV1>,
    pub audit: SelectionAuditRecordV1,
}

/// Deterministic selection artifact. `recomputed_selected_descriptor_hashes`
/// is empty for provisional output and exact/equal to every selected cached
/// descriptor only after full final-path recomputation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineSelectionArtifactV1 {
    pub schema_version: u32,
    pub publication_state: OfflineSelectionPublicationStateV1,
    pub source_cache_run_id: String,
    pub source_cache_completion_hash: String,
    pub analysis_config: CorpusRoomAnalysisConfigRecord,
    pub policies: OfflineCachePolicyRecordV1,
    pub selection_config: SelectionConfigRecordV1,
    pub outcome: SelectionOutcomeRecordV1,
    pub recomputed_selected_descriptor_hashes: BTreeMap<RoomId, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineSelectionArtifactCheckpointV1 {
    pub checkpoint_version: u32,
    pub status: String,
    pub publication_state: OfflineSelectionPublicationStateV1,
    pub source_cache_run_id: String,
    pub selection_hash: String,
}

impl SelectionOutcomeRecordV1 {
    fn from_outcome(outcome: &CorpusSelectionOutcome) -> Result<Self, OfflineSelectionCacheError> {
        let summary = outcome.archive.summary();
        let archive_summary = ArchiveSummaryRecordV1 {
            submitted_candidates: summary.submitted_candidates,
            retained_candidates: summary.retained_candidates,
            cell_count: summary.cell_count,
            elite_placements: summary.elite_placements,
        };
        let archive_cells = outcome
            .archive
            .cells()
            .map(|(key, cell)| ArchiveCellRecordV1 {
                projection: key.projection.into(),
                coordinates: key.coordinates.clone(),
                elites: cell.elites().to_vec(),
            })
            .collect::<Vec<_>>();
        let descriptors = outcome
            .descriptors
            .values()
            .map(SelectionDescriptorRecordV1::from_descriptor)
            .collect::<Vec<_>>();
        let descriptor_hashes = descriptors
            .iter()
            .map(|descriptor| {
                Ok((
                    descriptor.room_id.clone(),
                    stable_byte_hash(&canonical_json_line(descriptor)?),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, OfflineSelectionCacheError>>()?;
        Ok(Self {
            archive_summary,
            archive_cells,
            descriptors,
            descriptor_hashes,
            selected_room_ids: outcome.selected_room_ids.clone(),
            socket_packages: outcome
                .socket_packages
                .iter()
                .map(socket_package_record)
                .collect(),
            audit: selection_audit_record(&outcome.audit),
        })
    }

    fn validate(&self) -> Result<(), OfflineSelectionCacheError> {
        for pair in self.descriptors.windows(2) {
            if pair[0].room_id >= pair[1].room_id {
                return Err(invalid(
                    "selection artifact descriptors must be in strict room-ID order",
                ));
            }
        }
        let mut recomputed_hashes = BTreeMap::new();
        for descriptor in &self.descriptors {
            let native = descriptor.to_descriptor()?;
            if !matches!(
                production_eligibility(&native),
                OfflineCacheProductionEligibilityV1::Eligible
            ) {
                return Err(invalid(format!(
                    "selection artifact contains production-ineligible descriptor {:?}",
                    descriptor.room_id.0
                )));
            }
            recomputed_hashes.insert(
                descriptor.room_id.clone(),
                stable_byte_hash(&canonical_json_line(descriptor)?),
            );
        }
        if recomputed_hashes != self.descriptor_hashes {
            return Err(invalid(
                "selection artifact descriptor hashes do not match descriptors",
            ));
        }
        if self
            .selected_room_ids
            .iter()
            .any(|room_id| !self.descriptor_hashes.contains_key(room_id))
            || self
                .selected_room_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.selected_room_ids.len()
        {
            return Err(invalid(
                "selection artifact selected room IDs are duplicate or absent from descriptors",
            ));
        }
        if self.archive_summary.submitted_candidates != self.descriptors.len()
            || self.archive_summary.retained_candidates
                != self
                    .archive_cells
                    .iter()
                    .flat_map(|cell| &cell.elites)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
            || self.archive_summary.cell_count != self.archive_cells.len()
            || self.archive_summary.elite_placements
                != self
                    .archive_cells
                    .iter()
                    .map(|cell| cell.elites.len())
                    .sum::<usize>()
        {
            return Err(invalid(
                "selection artifact archive summary is inconsistent",
            ));
        }
        if self.audit.version != CORPUS_SELECTION_METRICS_VERSION
            || self.audit.submitted_rooms != self.descriptors.len()
            || self.audit.archive != self.archive_summary
            || self.audit.selected_rooms != self.selected_room_ids.len()
            || self.audit.selected_package_count != self.socket_packages.len()
            || self.audit.steps.len() != self.selected_room_ids.len()
            || self
                .audit
                .steps
                .iter()
                .map(|step| &step.room_id)
                .ne(self.selected_room_ids.iter())
        {
            return Err(invalid(
                "selection artifact audit summary/order is inconsistent",
            ));
        }
        Ok(())
    }
}

fn socket_package_record(package: &ReusableSocketSelectionPackage) -> SocketPackageRecordV1 {
    SocketPackageRecordV1 {
        package_id: package.package_id.clone(),
        room_ids: package.room_ids.clone(),
    }
}

fn selection_audit_record(audit: &CorpusSelectionAudit) -> SelectionAuditRecordV1 {
    let summary = audit.archive;
    SelectionAuditRecordV1 {
        version: audit.version,
        submitted_rooms: audit.submitted_rooms,
        exact_visual_unique_rooms: audit.exact_visual_unique_rooms,
        archive: ArchiveSummaryRecordV1 {
            submitted_candidates: summary.submitted_candidates,
            retained_candidates: summary.retained_candidates,
            cell_count: summary.cell_count,
            elite_placements: summary.elite_placements,
        },
        archive_ranked_rooms: audit.archive_ranked_rooms,
        rooms_excluded_by_initial_socket_core: audit.rooms_excluded_by_initial_socket_core.clone(),
        socket_pruning_steps: audit
            .socket_pruning_steps
            .iter()
            .map(socket_pruning_step_record)
            .collect(),
        selected_rooms: audit.selected_rooms,
        selected_package_count: audit.selected_package_count,
        covered_cells: audit
            .covered_cells
            .iter()
            .map(|cell| ArchiveCellKeyRecordV1 {
                projection: cell.projection.into(),
                coordinates: cell.coordinates.clone(),
            })
            .collect(),
        steps: audit.steps.iter().map(selection_step_record).collect(),
    }
}

fn socket_pruning_step_record(step: &SocketCoveragePruningStep) -> SocketPruningStepRecordV1 {
    SocketPruningStepRecordV1 {
        requested_removal: step.requested_removal.clone(),
        removed_room_ids: step.removed_room_ids.clone(),
        remaining_rooms: step.remaining_rooms,
    }
}

fn selection_step_record(step: &CorpusSelectionStep) -> SelectionStepRecordV1 {
    SelectionStepRecordV1 {
        room_id: step.room_id.clone(),
        marginal_cell_coverage: step.marginal_cell_coverage,
        minimum_l1_distance: step
            .minimum_l1_distance
            .map(|distance| distance.to_string()),
    }
}

impl OfflineSelectionArtifactV1 {
    fn provisional(
        cache: &VerifiedOfflineSelectionCacheV1,
        cache_root: &Path,
        selection_config: CorpusSelectionConfig,
        outcome: &CorpusSelectionOutcome,
    ) -> Result<Self, OfflineSelectionCacheError> {
        let artifact = Self {
            schema_version: CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION,
            publication_state: OfflineSelectionPublicationStateV1::ProvisionalOperationalCache,
            source_cache_run_id: cache.manifest.run_id.clone(),
            source_cache_completion_hash: stable_byte_hash(&read(
                &cache_root.join(CACHE_COMPLETION_FILE),
            )?),
            analysis_config: cache.manifest.analysis_config.clone(),
            policies: cache.manifest.policies.clone(),
            selection_config: selection_config.into(),
            outcome: SelectionOutcomeRecordV1::from_outcome(outcome)?,
            recomputed_selected_descriptor_hashes: BTreeMap::new(),
        };
        artifact.validate()?;
        Ok(artifact)
    }

    fn final_recomputed(
        provisional: &Self,
        recomputed_selected_descriptor_hashes: BTreeMap<RoomId, String>,
    ) -> Result<Self, OfflineSelectionCacheError> {
        if provisional.publication_state
            != OfflineSelectionPublicationStateV1::ProvisionalOperationalCache
        {
            return Err(invalid(
                "finalization input is not a provisional cache selection",
            ));
        }
        let mut artifact = provisional.clone();
        artifact.publication_state = OfflineSelectionPublicationStateV1::FinalRecomputed;
        artifact.recomputed_selected_descriptor_hashes = recomputed_selected_descriptor_hashes;
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), OfflineSelectionCacheError> {
        if self.schema_version != CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION
            || self.source_cache_run_id.is_empty()
            || self.source_cache_completion_hash.is_empty()
        {
            return Err(invalid("unsupported or unbound offline selection artifact"));
        }
        self.analysis_config
            .validate()
            .map_err(|error| invalid(format!("invalid selection analysis config: {error}")))?;
        self.policies.validate()?;
        let config: CorpusSelectionConfig = self.selection_config.into();
        if config.requested_minimum == 0
            || config.requested_minimum > config.requested_maximum
            || config.elites_per_cell == 0
            || !config
                .requested_range()
                .contains(&self.outcome.selected_room_ids.len())
        {
            return Err(invalid(
                "offline selection artifact has an invalid or unsatisfied selection range",
            ));
        }
        self.outcome.validate()?;
        let expected_recomputed = self
            .outcome
            .selected_room_ids
            .iter()
            .map(|room_id| {
                (
                    room_id.clone(),
                    self.outcome.descriptor_hashes[room_id].clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        match self.publication_state {
            OfflineSelectionPublicationStateV1::ProvisionalOperationalCache
                if self.recomputed_selected_descriptor_hashes.is_empty() => {}
            OfflineSelectionPublicationStateV1::FinalRecomputed
                if self.recomputed_selected_descriptor_hashes == expected_recomputed => {}
            _ => {
                return Err(invalid(
                    "offline selection publication state does not match recomputation hashes",
                ));
            }
        }
        Ok(())
    }
}

fn write_new_offline_selection_artifact_v1(
    directory: &Path,
    artifact: &OfflineSelectionArtifactV1,
) -> Result<(), OfflineSelectionCacheError> {
    artifact.validate()?;
    if directory.exists() {
        return Err(OfflineSelectionCacheError::AlreadyExists(
            directory.to_owned(),
        ));
    }
    fs::create_dir(directory).map_err(|source| OfflineSelectionCacheError::Io {
        path: directory.to_owned(),
        source,
    })?;
    let record_bytes = canonical_json_line(artifact)?;
    write_new(&directory.join(SELECTION_RECORD_FILE), &record_bytes)?;
    let read_back = read(&directory.join(SELECTION_RECORD_FILE))?;
    let parsed: OfflineSelectionArtifactV1 =
        parse_one_json_line(&read_back, SELECTION_RECORD_FILE)?;
    if parsed != *artifact {
        return Err(invalid("selection artifact changed across disk read-back"));
    }
    parsed.validate()?;
    let checkpoint = OfflineSelectionArtifactCheckpointV1 {
        checkpoint_version: CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION,
        status: COMPLETE_STATUS.to_owned(),
        publication_state: artifact.publication_state,
        source_cache_run_id: artifact.source_cache_run_id.clone(),
        selection_hash: stable_byte_hash(&read_back),
    };
    write_new(
        &directory.join(SELECTION_CHECKPOINT_FILE),
        &canonical_json_line(&checkpoint)?,
    )?;
    let verified = load_offline_selection_artifact_v1(directory)?;
    if verified != *artifact {
        return Err(invalid("selection artifact final verification mismatch"));
    }
    Ok(())
}

pub fn load_offline_selection_artifact_v1(
    directory: &Path,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    let record_bytes = read(&directory.join(SELECTION_RECORD_FILE))?;
    let checkpoint_bytes = read(&directory.join(SELECTION_CHECKPOINT_FILE))?;
    let artifact: OfflineSelectionArtifactV1 =
        parse_one_json_line(&record_bytes, SELECTION_RECORD_FILE)?;
    let checkpoint: OfflineSelectionArtifactCheckpointV1 =
        parse_one_json_line(&checkpoint_bytes, SELECTION_CHECKPOINT_FILE)?;
    artifact.validate()?;
    if checkpoint.checkpoint_version != CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION
        || checkpoint.status != COMPLETE_STATUS
        || checkpoint.publication_state != artifact.publication_state
        || checkpoint.source_cache_run_id != artifact.source_cache_run_id
        || checkpoint.selection_hash != stable_byte_hash(&record_bytes)
    {
        return Err(invalid("offline selection artifact checkpoint mismatch"));
    }
    Ok(artifact)
}

/// Loaded, checksum-verified operational cache.  This establishes storage and
/// source identity only; it does not independently prove deep observations.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedOfflineSelectionCacheV1 {
    pub manifest: OfflineCacheManifestV1,
    pub rooms: Vec<OfflineCacheRoomRecordV1>,
    pub completion: OfflineCacheCompletionV1,
}

/// Source evidence and run manifest prepared before any deep room work.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedOfflineCacheRunV1 {
    pub manifest: OfflineCacheManifestV1,
    pub corpus: RehydratedCorpusV3,
    pub shard_directories: Vec<PathBuf>,
}

/// Cache storage re-bound to the current independently verified source pool.
/// Descriptor measurements remain operational observations until selected
/// rooms are fully recomputed during finalization.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceBoundOfflineSelectionCacheV1 {
    pub cache: VerifiedOfflineSelectionCacheV1,
    pub corpus: RehydratedCorpusV3,
    pub validated_descriptors: Vec<ValidatedCachedCorpusSelectionDescriptorV2>,
}

/// Opaque selection input produced only after the cache has been rebound to
/// exact current source shards, keys, sockets, configs, versions, and hashes.
/// The selector performs an additional structural validation before use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedCachedCorpusSelectionDescriptorV2 {
    descriptor: CorpusSelectionDescriptor,
}

impl ValidatedCachedCorpusSelectionDescriptorV2 {
    pub(crate) fn descriptor(&self) -> &CorpusSelectionDescriptor {
        &self.descriptor
    }
}

/// Verify and replay-rehydrate one exact seed range, then freeze its cache
/// manifest. No deep solver work is performed here.
pub fn prepare_offline_cache_run_v1(
    shard_root: &Path,
    start_seed: u64,
    seed_count: usize,
    analysis_config: &CorpusRoomAnalysisConfig,
) -> Result<PreparedOfflineCacheRunV1, OfflineSelectionCacheError> {
    if seed_count == 0 {
        return Err(invalid("offline cache seed count must be positive"));
    }
    let mut directories = Vec::with_capacity(seed_count);
    let mut expected_sources = Vec::with_capacity(seed_count);
    for offset in 0..seed_count {
        let seed = start_seed
            .checked_add(offset as u64)
            .ok_or_else(|| invalid("offline cache seed range overflows u64"))?;
        let directory = seed_shard_directory_v3(shard_root, seed);
        let expected_config = CorpusBuildConfigV2::attempt_zero(seed, 1);
        expected_sources.push((seed, expected_config));
        directories.push(directory);
    }
    let loaded = load_verified_rehydrated_artifact_v3_shards(&directories).map_err(|error| {
        invalid(format!(
            "could not replay-rehydrate exact artifact-v3 seed range: {error}"
        ))
    })?;
    if loaded.sources.len() != expected_sources.len() {
        return Err(invalid(
            "verified artifact-v3 source count differs from the requested seed range",
        ));
    }
    let sources = expected_sources
        .iter()
        .zip(&loaded.sources)
        .map(|((seed, expected_config), source)| {
            if source.seed != *seed || &source.config != expected_config {
                return Err(invalid(format!(
                    "artifact-v3 shard {seed} differs from its exact requested config"
                )));
            }
            Ok(OfflineCacheSourceShardV1 {
                seed: *seed,
                config_id: source.checkpoint.config_id.clone(),
                checkpoint_hash: stable_byte_hash(&source.checkpoint_bytes),
                checkpoint: source.checkpoint.clone(),
            })
        })
        .collect::<Result<Vec<_>, OfflineSelectionCacheError>>()?;
    let corpus = loaded.corpus;
    let mut expected_rooms = corpus
        .rooms
        .iter()
        .filter_map(|room| {
            room.canonical_regeneration
                .selected_key
                .as_ref()
                .map(|key| OfflineCacheExpectedRoomV1 {
                    source_seed: key.source_seed(),
                    room_id: room.generated.id.clone(),
                    canonical_key: key.clone(),
                })
        })
        .collect::<Vec<_>>();
    expected_rooms.sort_unstable_by(|left, right| left.room_id.cmp(&right.room_id));
    let analysis_config = analysis_config.identity_record().map_err(|error| {
        invalid(format!(
            "could not freeze offline deep-analysis config: {error}"
        ))
    })?;
    let manifest = OfflineCacheManifestV1::new(
        start_seed,
        seed_count,
        sources,
        analysis_config,
        expected_rooms,
    )?;
    Ok(PreparedOfflineCacheRunV1 {
        manifest,
        corpus,
        shard_directories: directories,
    })
}

fn native_analysis_config(
    record: &CorpusRoomAnalysisConfigRecord,
) -> Result<CorpusRoomAnalysisConfig, OfflineSelectionCacheError> {
    record
        .validate()
        .map_err(|error| invalid(format!("invalid cached analysis config: {error}")))?;
    let config = CorpusRoomAnalysisConfig {
        direct_controller_solver: record.direct_controller_solver.to_solver_config(),
        canonical_witness_difficulty: DifficultyConfig {
            perturbation_grace_ticks: record.canonical_witness_difficulty.perturbation_grace_ticks,
        },
    };
    if config.identity_record().map_err(|error| {
        invalid(format!(
            "could not round-trip cached analysis config: {error}"
        ))
    })? != *record
    {
        return Err(invalid(
            "cached analysis config differs after exact native reconstruction",
        ));
    }
    Ok(config)
}

/// Compute one deterministic operational row. Positive evidence remains in
/// the verified source shard/native analysis; this cache retains only the
/// strict descriptor and transparent summaries.
pub fn compute_offline_cache_room_v1(
    manifest: &OfflineCacheManifestV1,
    evaluated: &super::EvaluatedCorpusRoomV2,
    analysis_config: &CorpusRoomAnalysisConfig,
) -> Result<OfflineCacheRoomRecordV1, OfflineSelectionCacheError> {
    manifest.validate()?;
    let room_id = &evaluated.generated.id;
    let expected = manifest
        .expected_rooms
        .binary_search_by(|room| room.room_id.cmp(room_id))
        .ok()
        .map(|index| &manifest.expected_rooms[index])
        .ok_or_else(|| {
            invalid(format!(
                "room {:?} is not an expected cache candidate",
                room_id.0
            ))
        })?;
    let supplied_config = analysis_config.identity_record().map_err(|error| {
        invalid(format!(
            "could not freeze supplied deep-analysis config: {error}"
        ))
    })?;
    if supplied_config != manifest.analysis_config {
        return Err(invalid(format!(
            "room {:?} supplied analysis config differs from cache manifest",
            room_id.0
        )));
    }
    let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|error| {
        invalid(format!(
            "room {:?} failed exact final-path identity before deep analysis: {error}",
            room_id.0
        ))
    })?;
    if candidate.exact_key() != expected.canonical_key {
        return Err(invalid(format!(
            "room {:?} canonical key differs from cache manifest",
            room_id.0
        )));
    }
    let analysis = analyze_corpus_room_v2(evaluated, analysis_config).map_err(|error| {
        invalid(format!(
            "room {:?} deep analysis failed: {error}",
            room_id.0
        ))
    })?;
    let shaky_hand = assess_corpus_shaky_hand_v2(&analysis, evaluated, manifest.shaky_hand_config)
        .map_err(|error| {
            invalid(format!(
                "room {:?} shaky-hand analysis failed: {error}",
                room_id.0
            ))
        })?;
    let metrics = summarize_room_metrics(&analysis).map_err(|error| {
        invalid(format!(
            "room {:?} metric summarization failed: {error}",
            room_id.0
        ))
    })?;
    let route_choices =
        analyze_route_choice_diversity_v2(evaluated, &analysis).map_err(|error| {
            invalid(format!(
                "room {:?} route-choice analysis failed: {error}",
                room_id.0
            ))
        })?;
    let pickup_detours =
        analyze_pickup_detours_v2(evaluated, TraversalGrid::default()).map_err(|error| {
            invalid(format!(
                "room {:?} pickup-detour analysis failed: {error}",
                room_id.0
            ))
        })?;
    let route_choice_summary =
        summarize_route_choice_selection_metrics(&route_choices).map_err(|error| {
            invalid(format!(
                "room {:?} route-choice report summarization failed: {error}",
                room_id.0
            ))
        })?;
    let pickup_detour_summary = summarize_pickup_detour_selection_metrics(&pickup_detours)
        .map_err(|error| {
            invalid(format!(
                "room {:?} pickup-detour report summarization failed: {error}",
                room_id.0
            ))
        })?;
    let exploratory = describe_corpus_selection_room_v2_exploratory(
        evaluated,
        &metrics,
        Some(&route_choices),
        Some(&pickup_detours),
    )
    .map_err(|error| {
        invalid(format!(
            "room {:?} exploratory descriptor adaptation failed: {error}",
            room_id.0
        ))
    })?;
    let eligibility = production_eligibility(&exploratory);
    match describe_corpus_selection_room_v2(
        evaluated,
        &metrics,
        Some(&route_choices),
        Some(&pickup_detours),
    ) {
        Ok(production) => {
            if production != exploratory
                || !matches!(eligibility, OfflineCacheProductionEligibilityV1::Eligible)
            {
                return Err(invalid(format!(
                    "room {:?} production/exploratory descriptor contract disagrees",
                    room_id.0
                )));
            }
        }
        Err(CorpusSelectionMetricsError::ProductionEvidenceUnavailable {
            coordinate,
            state,
            ..
        }) => {
            let OfflineCacheProductionEligibilityV1::Ineligible {
                unavailable_coordinates,
            } = &eligibility
            else {
                return Err(invalid(format!(
                    "room {:?} production rejected evidence but cache found none unavailable",
                    room_id.0
                )));
            };
            let first = unavailable_coordinates.first().ok_or_else(|| {
                invalid("production-ineligible cache descriptor has no unavailable coordinate")
            })?;
            if first.coordinate != coordinate || SelectionEvidenceState::from(first.state) != state
            {
                return Err(invalid(format!(
                    "room {:?} production rejection differs from cached evidence partition",
                    room_id.0
                )));
            }
        }
        Err(error) => {
            return Err(invalid(format!(
                "room {:?} production descriptor validation failed: {error}",
                room_id.0
            )));
        }
    }
    let source = manifest
        .source_for_seed(expected.source_seed)
        .ok_or_else(|| invalid("expected cache source disappeared"))?
        .clone();
    let record = OfflineCacheRoomRecordV1 {
        schema_version: CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION,
        status: CACHE_ROW_STATUS.to_owned(),
        run_id: manifest.run_id.clone(),
        source,
        room_id: room_id.clone(),
        canonical_key: expected.canonical_key.clone(),
        analysis_config: analysis.config.clone(),
        policies: manifest.policies.clone(),
        descriptor: SelectionDescriptorRecordV1::from_descriptor(&exploratory),
        production_eligibility: eligibility,
        shaky_hand,
        reports: OfflineCacheReportSummariesV1 {
            room_metrics: room_metric_report_summary_v1(&metrics),
            route_choices: route_choice_report_summary_v1(&route_choices, &route_choice_summary),
            pickup_detours: pickup_detour_report_summary_v1(
                &pickup_detours,
                &pickup_detour_summary,
            ),
        },
    };
    record.validate_against(manifest)?;
    Ok(record)
}

struct OrderedRoomWorkerOutputV1<T> {
    values: Vec<(usize, T)>,
    failure: Option<(usize, RoomId, OfflineSelectionCacheError)>,
}

fn validate_room_worker_count_v1(worker_count: usize) -> Result<(), OfflineSelectionCacheError> {
    if matches!(worker_count, 1 | OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1) {
        Ok(())
    } else {
        Err(invalid(format!(
            "offline cache room worker count must be 1 or {}, found {worker_count}",
            OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1
        )))
    }
}

fn panic_payload_detail_v1(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

/// Run exact room-ordered work using static-stride assignment. Every worker is
/// joined before any failure is returned, and failures are selected by the
/// canonical input index rather than scheduling order.
fn run_ordered_room_jobs_v1<T, F>(
    room_ids: &[RoomId],
    worker_count: usize,
    job: F,
) -> Result<Vec<T>, OfflineSelectionCacheError>
where
    T: Send,
    F: Fn(&RoomId) -> Result<T, OfflineSelectionCacheError> + Sync,
{
    validate_room_worker_count_v1(worker_count)?;
    if room_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(
            "offline cache room jobs must be in strict room-ID order",
        ));
    }
    let mut outputs = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for worker_index in 0..worker_count {
            let job = &job;
            handles.push((
                worker_index,
                scope.spawn(move || {
                    let mut values = Vec::new();
                    for room_index in (worker_index..room_ids.len()).step_by(worker_count) {
                        let room_id = &room_ids[room_index];
                        let result = catch_unwind(AssertUnwindSafe(|| job(room_id)));
                        match result {
                            Ok(Ok(value)) => values.push((room_index, value)),
                            Ok(Err(error)) => {
                                return OrderedRoomWorkerOutputV1 {
                                    values,
                                    failure: Some((room_index, room_id.clone(), error)),
                                };
                            }
                            Err(payload) => {
                                return OrderedRoomWorkerOutputV1 {
                                    values,
                                    failure: Some((
                                        room_index,
                                        room_id.clone(),
                                        invalid(format!(
                                            "room {:?} worker panicked: {}",
                                            room_id.0,
                                            panic_payload_detail_v1(payload.as_ref())
                                        )),
                                    )),
                                };
                            }
                        }
                    }
                    OrderedRoomWorkerOutputV1 {
                        values,
                        failure: None,
                    }
                }),
            ));
        }

        let mut joined = Vec::with_capacity(worker_count);
        for (worker_index, handle) in handles {
            match handle.join() {
                Ok(output) => joined.push(output),
                Err(payload) => joined.push(OrderedRoomWorkerOutputV1 {
                    values: Vec::new(),
                    failure: Some((
                        usize::MAX,
                        RoomId(format!("worker-{worker_index}")),
                        invalid(format!(
                            "offline cache room worker {worker_index} panicked outside a room job: {}",
                            panic_payload_detail_v1(payload.as_ref())
                        )),
                    )),
                }),
            }
        }
        joined
    });

    let mut failures = outputs
        .iter_mut()
        .filter_map(|output| output.failure.take())
        .collect::<Vec<_>>();
    failures
        .sort_unstable_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if let Some((_, _, error)) = failures.into_iter().next() {
        return Err(error);
    }

    let mut values = outputs
        .into_iter()
        .flat_map(|output| output.values)
        .collect::<Vec<_>>();
    values.sort_unstable_by_key(|(room_index, _)| *room_index);
    if values.len() != room_ids.len() {
        return Err(invalid(
            "offline cache room workers returned an incomplete result set",
        ));
    }
    Ok(values.into_iter().map(|(_, value)| value).collect())
}

/// Resume an operational cache: exact-verify completed rows, compute only
/// absent rows, and publish the run checkpoint last.
pub fn build_or_resume_offline_cache_v1(
    shard_root: &Path,
    start_seed: u64,
    seed_count: usize,
    cache_root: &Path,
    analysis_config: &CorpusRoomAnalysisConfig,
) -> Result<VerifiedOfflineSelectionCacheV1, OfflineSelectionCacheError> {
    build_or_resume_offline_cache_v1_with_workers(
        shard_root,
        start_seed,
        seed_count,
        cache_root,
        analysis_config,
        OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1,
    )
}

/// Explicit one-versus-two worker seam for deterministic regression tests and
/// operational benchmarking. Production callers use the fixed two-worker
/// wrapper above.
pub fn build_or_resume_offline_cache_v1_with_workers(
    shard_root: &Path,
    start_seed: u64,
    seed_count: usize,
    cache_root: &Path,
    analysis_config: &CorpusRoomAnalysisConfig,
    worker_count: usize,
) -> Result<VerifiedOfflineSelectionCacheV1, OfflineSelectionCacheError> {
    validate_room_worker_count_v1(worker_count)?;
    let prepared =
        prepare_offline_cache_run_v1(shard_root, start_seed, seed_count, analysis_config)?;
    create_or_verify_offline_cache_manifest_v1(cache_root, &prepared.manifest)?;
    let rooms = prepared
        .corpus
        .rooms
        .iter()
        .map(|room| (&room.generated.id, room))
        .collect::<BTreeMap<_, _>>();
    let room_ids = prepared
        .manifest
        .expected_rooms
        .iter()
        .map(|expected| expected.room_id.clone())
        .collect::<Vec<_>>();
    run_ordered_room_jobs_v1(&room_ids, worker_count, |room_id| {
        let directory = cache_room_directory(cache_root, room_id);
        if directory.exists() {
            verify_offline_cache_room_v1(cache_root, &prepared.manifest, room_id)?;
            return Ok(());
        }
        let evaluated = rooms.get(room_id).ok_or_else(|| {
            invalid(format!(
                "manifest room {:?} disappeared before deep analysis",
                room_id.0
            ))
        })?;
        let record = compute_offline_cache_room_v1(&prepared.manifest, evaluated, analysis_config)?;
        write_or_verify_offline_cache_room_v1(cache_root, &prepared.manifest, &record)?;
        Ok(())
    })?;
    complete_or_verify_offline_cache_v1(cache_root, &prepared.manifest)?;
    load_verified_offline_cache_v1(cache_root)
}

/// Verify cache hashes, independently verify every named source checkpoint,
/// rehydrate the exact v3 rooms, and bind every descriptor to its current
/// canonical key and native boundary sockets.
pub fn load_source_bound_offline_cache_v1(
    cache_root: &Path,
    shard_root: &Path,
) -> Result<SourceBoundOfflineSelectionCacheV1, OfflineSelectionCacheError> {
    let cache = load_verified_offline_cache_v1(cache_root)?;
    let mut directories = Vec::with_capacity(cache.manifest.sources.len());
    for source in &cache.manifest.sources {
        directories.push(seed_shard_directory_v3(shard_root, source.seed));
    }
    let loaded = load_verified_rehydrated_artifact_v3_shards(&directories).map_err(|error| {
        invalid(format!(
            "could not rehydrate cache source shards under the current policy: {error}"
        ))
    })?;
    if loaded.sources.len() != cache.manifest.sources.len() {
        return Err(invalid(
            "verified cache source count differs from the cache manifest",
        ));
    }
    for (cached, source) in cache.manifest.sources.iter().zip(&loaded.sources) {
        let expected_config = CorpusBuildConfigV2::attempt_zero(cached.seed, 1);
        if source.seed != cached.seed
            || source.config != expected_config
            || source.checkpoint != cached.checkpoint
            || stable_byte_hash(&source.checkpoint_bytes) != cached.checkpoint_hash
        {
            return Err(invalid(format!(
                "cached source shard {} differs from its exact checkpoint identity",
                cached.seed
            )));
        }
    }
    let corpus = loaded.corpus;
    let expected = corpus
        .rooms
        .iter()
        .filter_map(|room| {
            room.canonical_regeneration
                .selected_key
                .as_ref()
                .map(|key| OfflineCacheExpectedRoomV1 {
                    source_seed: key.source_seed(),
                    room_id: room.generated.id.clone(),
                    canonical_key: key.clone(),
                })
        })
        .collect::<Vec<_>>();
    if expected != cache.manifest.expected_rooms {
        return Err(invalid(
            "cached expected rooms differ from the currently rehydrated source pool",
        ));
    }
    let by_id = corpus
        .rooms
        .iter()
        .map(|room| (&room.generated.id, room))
        .collect::<BTreeMap<_, _>>();
    let mut validated_descriptors = Vec::new();
    let mut static_visuals = HashMap::new();
    for row in &cache.rooms {
        let descriptor = row.validate_against(&cache.manifest)?;
        let evaluated = by_id.get(&row.room_id).ok_or_else(|| {
            invalid(format!(
                "cached room {:?} is absent from its rehydrated source pool",
                row.room_id.0
            ))
        })?;
        let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|error| {
            invalid(format!(
                "cached room {:?} failed current final-path identity validation: {error}",
                row.room_id.0
            ))
        })?;
        if candidate.exact_key() != row.canonical_key {
            return Err(invalid(format!(
                "cached room {:?} canonical key changed",
                row.room_id.0
            )));
        }
        let sockets = candidate
            .boundary_ports()
            .iter()
            .map(|port| port.door.socket())
            .collect::<Vec<_>>();
        if descriptor.sockets != sockets {
            return Err(invalid(format!(
                "cached room {:?} sockets differ from exact regeneration",
                row.room_id.0
            )));
        }
        if matches!(
            row.production_eligibility,
            OfflineCacheProductionEligibilityV1::Eligible
        ) {
            if let Some(first_room_id) = static_visuals.insert(
                evaluated
                    .generated
                    .physical_descriptor
                    .static_visual
                    .clone(),
                row.room_id.clone(),
            ) {
                return Err(invalid(format!(
                    "production-eligible cache rooms {:?} and {:?} have identical static visuals",
                    first_room_id.0, row.room_id.0
                )));
            }
            validated_descriptors.push(ValidatedCachedCorpusSelectionDescriptorV2 { descriptor });
        }
    }
    Ok(SourceBoundOfflineSelectionCacheV1 {
        cache,
        corpus,
        validated_descriptors,
    })
}

/// Run the strict current QD/socket selector over source-bound cache rows and
/// write a create-new provisional artifact. The result is operational only:
/// it is not publishable evidence until [`finalize_offline_selection_v1`]
/// recomputes every selected room from independently verified source shards.
pub fn select_and_write_provisional_offline_cache_v1(
    cache_root: &Path,
    shard_root: &Path,
    selection_config: CorpusSelectionConfig,
    output_directory: &Path,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    let source_bound = load_source_bound_offline_cache_v1(cache_root, shard_root)?;
    let outcome = select_validated_cached_corpus_descriptors_v2(
        &source_bound.validated_descriptors,
        selection_config,
    )
    .map_err(|error| invalid(format!("cached descriptor selection failed: {error}")))?;
    let artifact = OfflineSelectionArtifactV1::provisional(
        &source_bound.cache,
        cache_root,
        selection_config,
        &outcome,
    )?;
    write_new_offline_selection_artifact_v1(output_directory, &artifact)?;
    load_offline_selection_artifact_v1(output_directory)
}

/// Finalize one provisional selection only after independently verifying its
/// source cache and shards, exactly rerunning selection, and fully recomputing
/// every selected room's analysis, reports, production descriptor, identity,
/// and ability gates. Every recomputed row must equal the cached observation;
/// the output directory is create-new and its checkpoint is written last.
pub fn finalize_offline_selection_v1(
    cache_root: &Path,
    shard_root: &Path,
    provisional_directory: &Path,
    output_directory: &Path,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    finalize_offline_selection_v1_with_workers(
        cache_root,
        shard_root,
        provisional_directory,
        output_directory,
        OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1,
    )
}

/// Explicit one-versus-two worker seam for deterministic final-recomputation
/// regression tests and operational benchmarking.
pub fn finalize_offline_selection_v1_with_workers(
    cache_root: &Path,
    shard_root: &Path,
    provisional_directory: &Path,
    output_directory: &Path,
    worker_count: usize,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    validate_room_worker_count_v1(worker_count)?;
    let provisional = load_offline_selection_artifact_v1(provisional_directory)?;
    if provisional.publication_state
        != OfflineSelectionPublicationStateV1::ProvisionalOperationalCache
    {
        return Err(invalid(
            "offline selection finalization requires a provisional artifact",
        ));
    }
    let recomputed_selected_descriptor_hashes =
        recompute_selected_cache_rows_v1(cache_root, shard_root, &provisional, worker_count)?;
    let artifact = OfflineSelectionArtifactV1::final_recomputed(
        &provisional,
        recomputed_selected_descriptor_hashes,
    )?;
    write_new_offline_selection_artifact_v1(output_directory, &artifact)?;
    load_offline_selection_artifact_v1(output_directory)
}

/// Independently verify an existing final artifact by repeating the exact
/// source binding, selector run, and full selected-room recomputation. This is
/// intentionally much stronger than [`load_offline_selection_artifact_v1`],
/// which validates storage/checkpoint structure only.
pub fn verify_final_offline_selection_v1(
    cache_root: &Path,
    shard_root: &Path,
    selection_directory: &Path,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    verify_final_offline_selection_v1_with_workers(
        cache_root,
        shard_root,
        selection_directory,
        OFFLINE_CACHE_DEFAULT_ROOM_WORKERS_V1,
    )
}

/// Explicit one-versus-two worker seam for deterministic final verification.
pub fn verify_final_offline_selection_v1_with_workers(
    cache_root: &Path,
    shard_root: &Path,
    selection_directory: &Path,
    worker_count: usize,
) -> Result<OfflineSelectionArtifactV1, OfflineSelectionCacheError> {
    validate_room_worker_count_v1(worker_count)?;
    let artifact = load_offline_selection_artifact_v1(selection_directory)?;
    if artifact.publication_state != OfflineSelectionPublicationStateV1::FinalRecomputed {
        return Err(invalid(
            "full selection verification requires a final-recomputed artifact",
        ));
    }
    let recomputed =
        recompute_selected_cache_rows_v1(cache_root, shard_root, &artifact, worker_count)?;
    if recomputed != artifact.recomputed_selected_descriptor_hashes {
        return Err(invalid(
            "final selection recomputation hashes differ from the persisted artifact",
        ));
    }
    Ok(artifact)
}

fn recompute_selected_cache_rows_v1(
    cache_root: &Path,
    shard_root: &Path,
    artifact: &OfflineSelectionArtifactV1,
    worker_count: usize,
) -> Result<BTreeMap<RoomId, String>, OfflineSelectionCacheError> {
    validate_room_worker_count_v1(worker_count)?;
    artifact.validate()?;
    let source_bound = load_source_bound_offline_cache_v1(cache_root, shard_root)?;
    let completion_hash = stable_byte_hash(&read(&cache_root.join(CACHE_COMPLETION_FILE))?);
    if artifact.source_cache_run_id != source_bound.cache.manifest.run_id
        || artifact.source_cache_completion_hash != completion_hash
        || artifact.analysis_config != source_bound.cache.manifest.analysis_config
        || artifact.policies != source_bound.cache.manifest.policies
    {
        return Err(invalid(
            "selection artifact differs from the exact current source cache identity",
        ));
    }

    let selection_config: CorpusSelectionConfig = artifact.selection_config.into();
    let rerun = select_validated_cached_corpus_descriptors_v2(
        &source_bound.validated_descriptors,
        selection_config,
    )
    .map_err(|error| invalid(format!("final selection rerun failed: {error}")))?;
    if SelectionOutcomeRecordV1::from_outcome(&rerun)? != artifact.outcome {
        return Err(invalid(
            "selection artifact differs from the exact current selector rerun",
        ));
    }

    let analysis_config = native_analysis_config(&source_bound.cache.manifest.analysis_config)?;
    let evaluated_by_id = source_bound
        .corpus
        .rooms
        .iter()
        .map(|room| (&room.generated.id, room))
        .collect::<BTreeMap<_, _>>();
    let cached_by_id = source_bound
        .cache
        .rooms
        .iter()
        .map(|row| (&row.room_id, row))
        .collect::<BTreeMap<_, _>>();
    let selected_descriptor_by_id = artifact
        .outcome
        .descriptors
        .iter()
        .map(|descriptor| (&descriptor.room_id, descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut room_ids = artifact.outcome.selected_room_ids.clone();
    room_ids.sort_unstable();
    let recomputed = run_ordered_room_jobs_v1(&room_ids, worker_count, |room_id| {
        let evaluated = evaluated_by_id.get(room_id).ok_or_else(|| {
            invalid(format!(
                "selected room {:?} disappeared from the verified source pool",
                room_id.0
            ))
        })?;
        let cached = cached_by_id.get(room_id).ok_or_else(|| {
            invalid(format!(
                "selected room {:?} disappeared from the verified cache",
                room_id.0
            ))
        })?;
        let recomputed = compute_offline_cache_room_v1(
            &source_bound.cache.manifest,
            evaluated,
            &analysis_config,
        )?;
        require_exact_recomputed_cache_row_v1(cached, &recomputed)?;
        if !matches!(
            recomputed.production_eligibility,
            OfflineCacheProductionEligibilityV1::Eligible
        ) {
            return Err(invalid(format!(
                "selected room {:?} became production-ineligible during final recomputation",
                room_id.0
            )));
        }
        let expected_descriptor = selected_descriptor_by_id.get(room_id).ok_or_else(|| {
            invalid(format!(
                "selected room {:?} has no provisional descriptor",
                room_id.0
            ))
        })?;
        if &recomputed.descriptor != *expected_descriptor {
            return Err(invalid(format!(
                "selected room {:?} recomputed descriptor differs from provisional selection",
                room_id.0
            )));
        }
        let descriptor_hash = stable_byte_hash(&canonical_json_line(&recomputed.descriptor)?);
        if artifact.outcome.descriptor_hashes.get(room_id) != Some(&descriptor_hash) {
            return Err(invalid(format!(
                "selected room {:?} recomputed descriptor hash differs from provisional selection",
                room_id.0
            )));
        }
        Ok((room_id.clone(), descriptor_hash))
    })?;
    Ok(recomputed.into_iter().collect())
}

fn require_exact_recomputed_cache_row_v1(
    cached: &OfflineCacheRoomRecordV1,
    recomputed: &OfflineCacheRoomRecordV1,
) -> Result<(), OfflineSelectionCacheError> {
    if cached != recomputed {
        return Err(invalid(format!(
            "selected room {:?} full deep recomputation (including shaky-hand diagnostics) differs from its cache row",
            recomputed.room_id.0
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub enum OfflineCacheRoomWriteOutcomeV1 {
    Written(OfflineCacheRoomRecordV1),
    AlreadyVerified(OfflineCacheRoomRecordV1),
}

impl OfflineCacheManifestV1 {
    /// Build a manifest and derive its stable content identity. Inputs must
    /// already be in canonical seed/room order.
    pub fn new(
        start_seed: u64,
        seed_count: usize,
        sources: Vec<OfflineCacheSourceShardV1>,
        analysis_config: CorpusRoomAnalysisConfigRecord,
        expected_rooms: Vec<OfflineCacheExpectedRoomV1>,
    ) -> Result<Self, OfflineSelectionCacheError> {
        let mut manifest = Self {
            schema_version: CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION,
            status: CACHE_STATUS.to_owned(),
            run_id: String::new(),
            start_seed,
            seed_count,
            sources,
            analysis_config,
            shaky_hand_config: CorpusShakyHandConfig::default(),
            policies: OfflineCachePolicyRecordV1::current(),
            expected_rooms,
        };
        manifest.run_id = manifest.recomputed_run_id()?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), OfflineSelectionCacheError> {
        if self.schema_version != CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION
            || self.status != CACHE_STATUS
            || self.seed_count == 0
        {
            return Err(invalid("unsupported or empty offline cache manifest"));
        }
        self.analysis_config
            .validate()
            .map_err(|error| invalid(format!("invalid offline cache analysis config: {error}")))?;
        if self.shaky_hand_config != CorpusShakyHandConfig::default() {
            return Err(invalid(
                "offline cache shaky-hand config is not the exact current default",
            ));
        }
        self.policies.validate()?;
        let expected_last = self
            .start_seed
            .checked_add((self.seed_count - 1) as u64)
            .ok_or_else(|| invalid("offline cache seed range overflows u64"))?;
        if self.sources.len() != self.seed_count {
            return Err(invalid(
                "offline cache source count differs from seed count",
            ));
        }
        for (offset, source) in self.sources.iter().enumerate() {
            let expected_seed = self
                .start_seed
                .checked_add(offset as u64)
                .ok_or_else(|| invalid("offline cache source seed overflows u64"))?;
            if source.seed != expected_seed
                || source.checkpoint.seed != source.seed
                || source.checkpoint.config_id != source.config_id
                || source.checkpoint.checkpoint_version != CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION
                || source.checkpoint.status != COMPLETE_STATUS
                || source.checkpoint_hash.is_empty()
                || source.checkpoint.artifact_hashes.is_empty()
            {
                return Err(invalid(format!(
                    "offline cache source identity is invalid for expected seed {expected_seed}"
                )));
            }
        }
        if self.sources.last().map(|source| source.seed) != Some(expected_last) {
            return Err(invalid(
                "offline cache sources do not cover the exact seed range",
            ));
        }
        for pair in self.expected_rooms.windows(2) {
            if pair[0].room_id >= pair[1].room_id {
                return Err(invalid(
                    "offline cache expected room IDs must be strictly increasing",
                ));
            }
        }
        for room in &self.expected_rooms {
            if room.canonical_key.record_version() != CORPUS_CANDIDATE_KEY_RECORD_VERSION
                || room.canonical_key.source_seed() != room.source_seed
                || !self
                    .sources
                    .iter()
                    .any(|source| source.seed == room.source_seed)
            {
                return Err(invalid(format!(
                    "offline cache expected room {:?} has an invalid canonical source key",
                    room.room_id.0
                )));
            }
        }
        let expected_run_id = self.recomputed_run_id()?;
        if self.run_id != expected_run_id {
            return Err(invalid(format!(
                "offline cache run ID mismatch: {:?} != {:?}",
                self.run_id, expected_run_id
            )));
        }
        Ok(())
    }

    #[must_use]
    pub fn source_for_seed(&self, seed: u64) -> Option<&OfflineCacheSourceShardV1> {
        self.sources
            .binary_search_by_key(&seed, |source| source.seed)
            .ok()
            .map(|index| &self.sources[index])
    }

    fn recomputed_run_id(&self) -> Result<String, OfflineSelectionCacheError> {
        let mut body = self.clone();
        body.run_id.clear();
        let bytes = canonical_json_line(&body)?;
        Ok(format!(
            "downwards-offline-cache-run-v{}-{}",
            self.schema_version,
            stable_byte_hash(&bytes)
                .rsplit('-')
                .next()
                .expect("stable hash has a suffix")
        ))
    }
}

impl OfflineCacheRoomRecordV1 {
    pub fn validate_against(
        &self,
        manifest: &OfflineCacheManifestV1,
    ) -> Result<CorpusSelectionDescriptor, OfflineSelectionCacheError> {
        manifest.validate()?;
        if self.schema_version != CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION
            || self.status != CACHE_ROW_STATUS
            || self.run_id != manifest.run_id
            || self.analysis_config != manifest.analysis_config
            || self.policies != manifest.policies
            || self.descriptor.room_id != self.room_id
        {
            return Err(invalid(format!(
                "offline cache row {:?} has stale run/config/policy identity",
                self.room_id.0
            )));
        }
        let expected = manifest
            .expected_rooms
            .binary_search_by(|room| room.room_id.cmp(&self.room_id))
            .ok()
            .map(|index| &manifest.expected_rooms[index])
            .ok_or_else(|| {
                invalid(format!(
                    "offline cache row {:?} is absent from the manifest",
                    self.room_id.0
                ))
            })?;
        if self.canonical_key != expected.canonical_key
            || self.source_seed() != expected.source_seed
            || self.source
                != *manifest
                    .source_for_seed(expected.source_seed)
                    .ok_or_else(|| invalid("cache row source seed is absent from manifest"))?
        {
            return Err(invalid(format!(
                "offline cache row {:?} differs from its exact source/key identity",
                self.room_id.0
            )));
        }
        let descriptor = self.descriptor.to_descriptor()?;
        validate_cached_shaky_hand_v1(&self.shaky_hand, manifest, &self.room_id)?;
        self.reports.validate(&self.room_id)?;
        let expected_eligibility = production_eligibility(&descriptor);
        if self.production_eligibility != expected_eligibility {
            return Err(invalid(format!(
                "offline cache row {:?} production eligibility is not descriptor-derived",
                self.room_id.0
            )));
        }
        Ok(descriptor)
    }

    fn source_seed(&self) -> u64 {
        self.canonical_key.source_seed()
    }
}

fn validate_cached_shaky_hand_v1(
    report: &CorpusShakyHandAnalysis,
    manifest: &OfflineCacheManifestV1,
    room_id: &RoomId,
) -> Result<(), OfflineSelectionCacheError> {
    if report.version != CORPUS_SHAKY_HAND_ANALYSIS_VERSION
        || report.source_analysis_version != CORPUS_ROOM_ANALYSIS_VERSION
        || report.seed_derivation_version != SHAKY_HAND_ROUTE_SEED_VERSION
        || report.ai_policy_version != SHAKY_HAND_POLICY_VERSION
        || report.ai_config_version != SHAKY_HAND_CONFIG_VERSION
        || report.room_id != *room_id
        || report.config != manifest.shaky_hand_config
        || report.evidence_disclaimer != manifest.policies.disclaimers.shaky_hand
    {
        return Err(invalid(format!(
            "offline cache row {:?} has stale shaky-hand identity/config/disclaimer",
            room_id.0
        )));
    }
    for cell in &report.cells {
        if cell.source_door_id.is_empty()
            || cell.target_door_id.is_empty()
            || cell.source_door_id == cell.target_door_id
        {
            return Err(invalid(format!(
                "offline cache row {:?} has an invalid shaky-hand cell coordinate",
                room_id.0
            )));
        }
        if let CorpusShakyHandEvidence::Observed(observation) = &cell.evidence {
            let representative_matches = match observation.representative_status {
                CorpusRouteRepresentativeStatus::UniqueNondominatedCandidate => {
                    observation.nondominated_front_size == 1
                }
                CorpusRouteRepresentativeStatus::AmbiguousNondominatedFront { front_size } => {
                    front_size > 1 && front_size == observation.nondominated_front_size
                }
            };
            if observation.selected_provenance.is_empty()
                || !observation.exact_control_succeeded
                || !representative_matches
                || observation.identity.corpus_analysis_version
                    != CORPUS_SHAKY_HAND_ANALYSIS_VERSION
                || observation.identity.seed_derivation_version != SHAKY_HAND_ROUTE_SEED_VERSION
                || observation.identity.ai_policy_version != SHAKY_HAND_POLICY_VERSION
                || observation.identity.ai_config_version != SHAKY_HAND_CONFIG_VERSION
                || observation.curves.iter().any(|curve| {
                    curve.requested_trials
                        != curve
                            .applicable_trials
                            .saturating_add(curve.not_applicable_trials)
                        || curve.successes > curve.applicable_trials
                        || curve.trials_with_death > curve.applicable_trials
                        || curve.successes_after_death > curve.successes
                        || curve.wrong_target_outcomes > curve.applicable_trials
                        || curve.other_door_outcomes > curve.applicable_trials
                        || curve.other_exit_outcomes > curve.applicable_trials
                        || curve.timeouts > curve.applicable_trials
                        || curve.divergent_successes > curve.successes
                        || curve.exact_state_convergences > curve.applicable_trials
                })
            {
                return Err(invalid(format!(
                    "offline cache row {:?} has an invalid shaky-hand observation",
                    room_id.0
                )));
            }
        }
    }
    if report.cells.windows(2).any(|pair| {
        (
            &pair[0].source_door_id,
            &pair[0].target_door_id,
            pair[0].loadout,
        ) >= (
            &pair[1].source_door_id,
            &pair[1].target_door_id,
            pair[1].loadout,
        )
    }) {
        return Err(invalid(format!(
            "offline cache row {:?} shaky-hand cells are not in strict canonical order",
            room_id.0
        )));
    }
    Ok(())
}

impl OfflineCacheReportSummariesV1 {
    fn validate(&self, room_id: &RoomId) -> Result<(), OfflineSelectionCacheError> {
        for (label, expected_version, summary) in [
            (
                "room metrics",
                ROOM_METRIC_SUMMARY_VERSION,
                &self.room_metrics,
            ),
            (
                "route-choice selection metrics",
                ROUTE_CHOICE_SELECTION_METRICS_VERSION,
                &self.route_choices,
            ),
            (
                "pickup-detour selection metrics",
                PICKUP_DETOUR_SELECTION_METRICS_VERSION,
                &self.pickup_detours,
            ),
        ] {
            if summary.room_id != *room_id || summary.source_version != expected_version {
                return Err(invalid(format!(
                    "cached {label} summary has stale room/version identity"
                )));
            }
            summary.validate(label)?;
        }
        Ok(())
    }
}

impl TransparentReportSummaryV1 {
    fn validate(&self, label: &str) -> Result<(), OfflineSelectionCacheError> {
        for pair in self.coordinates.windows(2) {
            if pair[0].name >= pair[1].name {
                return Err(invalid(format!(
                    "cached {label} coordinate names must be strictly increasing"
                )));
            }
        }
        for coordinate in &self.coordinates {
            if coordinate.name.is_empty() {
                return Err(invalid(format!(
                    "cached {label} has an empty coordinate name"
                )));
            }
            coordinate.value.validate(label, &coordinate.name)?;
        }
        Ok(())
    }
}

impl ReportValueV1 {
    fn validate(&self, label: &str, name: &str) -> Result<(), OfflineSelectionCacheError> {
        let bad = || invalid(format!("cached {label} coordinate {name:?} is invalid"));
        match self {
            Self::Count { .. } | Self::OperationalCount { .. } => Ok(()),
            Self::Fraction {
                numerator,
                denominator,
            } if *denominator > 0 && numerator <= denominator => Ok(()),
            Self::IntegerDistribution {
                sample_count,
                minimum,
                median_lower,
                median_upper,
                maximum,
                spread,
            } if *sample_count > 0
                && minimum <= median_lower
                && median_lower <= median_upper
                && median_upper <= maximum
                && *spread == maximum.saturating_sub(*minimum) =>
            {
                Ok(())
            }
            Self::SignedIntegerDistribution {
                sample_count,
                minimum,
                median_lower,
                median_upper,
                maximum,
                spread,
            } if *sample_count > 0
                && minimum <= median_lower
                && median_lower <= median_upper
                && median_upper <= maximum
                && *spread == minimum.abs_diff(*maximum) =>
            {
                Ok(())
            }
            Self::Histogram { bins }
                if bins.iter().all(|bin| bin.count > 0)
                    && bins.windows(2).all(|pair| pair[0].value < pair[1].value) =>
            {
                Ok(())
            }
            Self::NormalizedSamples {
                samples,
                minimum,
                p10,
                median,
                p90,
                maximum,
                mean,
            } => {
                let values = [minimum, p10, median, p90, maximum, mean];
                let shape = if *samples == 0 {
                    values.iter().all(|value| value.is_none())
                } else {
                    values
                        .iter()
                        .all(|value| value.as_ref().is_some_and(UnitIntervalFloatV1::validate))
                };
                if shape { Ok(()) } else { Err(bad()) }
            }
            Self::EvidenceState { reason, .. } if !reason.is_empty() => Ok(()),
            _ => Err(bad()),
        }
    }
}

struct ReportBuilderV1 {
    source_version: u32,
    room_id: RoomId,
    coordinates: BTreeMap<String, ReportValueV1>,
}

impl ReportBuilderV1 {
    fn new(source_version: u32, room_id: &RoomId) -> Self {
        Self {
            source_version,
            room_id: room_id.clone(),
            coordinates: BTreeMap::new(),
        }
    }

    fn insert(&mut self, name: impl Into<String>, value: ReportValueV1) {
        let name = name.into();
        assert!(
            self.coordinates.insert(name.clone(), value).is_none(),
            "duplicate transparent report coordinate {name:?}"
        );
    }

    fn count(&mut self, name: impl Into<String>, value: usize) {
        self.insert(name, ReportValueV1::Count { value });
    }

    fn operational(&mut self, name: impl Into<String>, value: usize) {
        self.insert(name, ReportValueV1::OperationalCount { value });
    }

    fn fraction(&mut self, name: impl Into<String>, value: super::ExactFraction) {
        self.insert(
            name,
            ReportValueV1::Fraction {
                numerator: value.numerator,
                denominator: value.denominator,
            },
        );
    }

    fn evidence(
        &mut self,
        name: impl Into<String>,
        state: ReportEvidenceStateV1,
        reason: impl Into<String>,
    ) {
        self.insert(
            name,
            ReportValueV1::EvidenceState {
                state,
                reason: reason.into(),
            },
        );
    }

    fn distribution(&mut self, name: impl Into<String>, value: IntegerCoordinateDistribution) {
        self.insert(
            name,
            ReportValueV1::IntegerDistribution {
                sample_count: value.sample_count,
                minimum: value.minimum,
                median_lower: value.median_lower,
                median_upper: value.median_upper,
                maximum: value.maximum,
                spread: value.spread,
            },
        );
    }

    fn signed_distribution(
        &mut self,
        name: impl Into<String>,
        value: super::SignedIntegerCoordinateDistribution,
    ) {
        self.insert(
            name,
            ReportValueV1::SignedIntegerDistribution {
                sample_count: value.sample_count,
                minimum: value.minimum,
                median_lower: value.median_lower,
                median_upper: value.median_upper,
                maximum: value.maximum,
                spread: value.spread,
            },
        );
    }

    fn optional_distribution(
        &mut self,
        name: impl Into<String>,
        value: Option<IntegerCoordinateDistribution>,
        reason: &'static str,
    ) {
        let name = name.into();
        if let Some(value) = value {
            self.distribution(name, value);
        } else {
            self.insert(
                name,
                ReportValueV1::EvidenceState {
                    state: ReportEvidenceStateV1::NotApplicable,
                    reason: reason.to_owned(),
                },
            );
        }
    }

    fn histogram(&mut self, name: impl Into<String>, bins: &BTreeMap<usize, usize>) {
        self.insert(
            name,
            ReportValueV1::Histogram {
                bins: bins
                    .iter()
                    .map(|(value, count)| HistogramBinV1 {
                        value: *value,
                        count: *count,
                    })
                    .collect(),
            },
        );
    }

    fn normalized(&mut self, name: impl Into<String>, samples: NormalizedDistanceSamples) {
        self.insert(
            name,
            ReportValueV1::NormalizedSamples {
                samples: samples.samples,
                minimum: samples.minimum.map(UnitIntervalFloatV1::from_value),
                p10: samples.p10.map(UnitIntervalFloatV1::from_value),
                median: samples.median.map(UnitIntervalFloatV1::from_value),
                p90: samples.p90.map(UnitIntervalFloatV1::from_value),
                maximum: samples.maximum.map(UnitIntervalFloatV1::from_value),
                mean: samples.mean.map(UnitIntervalFloatV1::from_value),
            },
        );
    }

    fn finish(self) -> TransparentReportSummaryV1 {
        TransparentReportSummaryV1 {
            source_version: self.source_version,
            room_id: self.room_id,
            coordinates: self
                .coordinates
                .into_iter()
                .map(|(name, value)| NamedReportCoordinateV1 { name, value })
                .collect(),
        }
    }
}

fn room_metric_report_summary_v1(metrics: &super::RoomMetricSummary) -> TransparentReportSummaryV1 {
    let mut builder = ReportBuilderV1::new(metrics.version, &metrics.room_id);
    let canonical = &metrics.canonical_routes;
    for (name, value) in [
        ("canonical/directed-routes", canonical.directed_route_count),
        (
            "canonical/loadout-route-cells",
            canonical.loadout_route_cell_count,
        ),
        (
            "canonical/positive-route-cells",
            canonical.positive_route_count,
        ),
        (
            "canonical/bounded-inconclusive-route-cells",
            canonical.bounded_inconclusive_route_count,
        ),
        (
            "canonical/behavior/route-count",
            canonical.behavior_diversity.route_count,
        ),
        (
            "canonical/behavior/reached-target-count",
            canonical.behavior_diversity.reached_target_count,
        ),
        (
            "canonical/behavior/spatial-path-classes",
            canonical.behavior_diversity.spatial_path_classes,
        ),
        (
            "canonical/behavior/semantic-controller-classes",
            canonical.behavior_diversity.semantic_controller_classes,
        ),
        (
            "canonical/behavior/joint-play-style-classes",
            canonical.behavior_diversity.joint_play_style_classes,
        ),
    ] {
        builder.count(name, value);
    }
    for loadout in &canonical.by_loadout {
        let prefix = format!("canonical/loadout/{}", loadout.loadout.slug());
        for (name, value) in [
            ("route-cells", loadout.route_cell_count),
            ("positive-route-cells", loadout.positive_route_count),
            (
                "bounded-inconclusive-route-cells",
                loadout.bounded_inconclusive_route_count,
            ),
            (
                "behavior/route-count",
                loadout.behavior_diversity.route_count,
            ),
            (
                "behavior/reached-target-count",
                loadout.behavior_diversity.reached_target_count,
            ),
            (
                "behavior/spatial-path-classes",
                loadout.behavior_diversity.spatial_path_classes,
            ),
            (
                "behavior/semantic-controller-classes",
                loadout.behavior_diversity.semantic_controller_classes,
            ),
            (
                "behavior/joint-play-style-classes",
                loadout.behavior_diversity.joint_play_style_classes,
            ),
        ] {
            builder.count(format!("{prefix}/{name}"), value);
        }
    }

    builder.count(
        "direct/directed-routes",
        metrics.direct_controllers.directed_route_count,
    );
    for loadout in &metrics.direct_controllers.by_loadout {
        let prefix = format!("direct/loadout/{}", loadout.loadout.slug());
        for (name, value) in [
            (
                "source-audits/expected",
                loadout.source_audit_completeness.expected,
            ),
            (
                "source-audits/complete-finite-vocabulary",
                loadout.source_audit_completeness.complete_finite_vocabulary,
            ),
            (
                "source-audits/bounded-incomplete",
                loadout.source_audit_completeness.bounded_incomplete,
            ),
            (
                "source-audits/missing",
                loadout.source_audit_completeness.missing,
            ),
            (
                "route-audits/expected",
                loadout.route_audit_completeness.expected,
            ),
            (
                "route-audits/complete-finite-vocabulary",
                loadout.route_audit_completeness.complete_finite_vocabulary,
            ),
            (
                "route-audits/bounded-incomplete",
                loadout.route_audit_completeness.bounded_incomplete,
            ),
            (
                "route-audits/missing",
                loadout.route_audit_completeness.missing,
            ),
            (
                "known-positive-directed-routes",
                loadout.known_positive_directed_routes,
            ),
            (
                "ambiguous-nondominated-front-directed-routes",
                loadout.ambiguous_nondominated_front_directed_routes,
            ),
            (
                "no-positive-in-complete-finite-vocabulary",
                loadout.no_positive_in_complete_finite_vocabulary,
            ),
            (
                "inconclusive-without-positive",
                loadout.inconclusive_without_positive,
            ),
            ("missing-route-or-audit", loadout.missing_route_or_audit),
        ] {
            builder.count(format!("{prefix}/{name}"), value);
        }
        append_easiest_controller_fractions(
            &mut builder,
            &format!("{prefix}/easiest-controller-fractions"),
            &loadout.easiest_controller_fractions,
        );
        append_controller_demand_distributions(
            &mut builder,
            &format!("{prefix}/demand"),
            &loadout.demand_coordinates,
        );
    }
    let bypass = &metrics.direct_controllers.ability_bypasses;
    for (name, value) in [
        (
            "ability-bypass/directed-routes-with-any-bypass",
            bypass.directed_routes_with_any_bypass,
        ),
        (
            "ability-bypass/directed-routes-with-wall-jump-bypass",
            bypass.directed_routes_with_wall_jump_bypass,
        ),
        (
            "ability-bypass/directed-routes-with-dash-bypass",
            bypass.directed_routes_with_dash_bypass,
        ),
        (
            "ability-bypass/directed-route-loadout-bypasses",
            bypass.directed_route_loadout_bypasses,
        ),
        (
            "ability-bypass/retained-semantic-bypass-witnesses",
            bypass.retained_semantic_bypass_witnesses,
        ),
    ] {
        builder.count(name, value);
    }
    for loadout in &bypass.by_successful_loadout {
        let prefix = format!(
            "ability-bypass/loadout/{}",
            loadout.successful_loadout.slug()
        );
        builder.count(
            format!("{prefix}/directed-route-bypasses"),
            loadout.directed_route_bypasses,
        );
        builder.count(
            format!("{prefix}/retained-semantic-witnesses"),
            loadout.retained_semantic_witnesses,
        );
    }

    append_landing_aggregate(
        &mut builder,
        "landing/aggregate",
        &metrics.landing_precision.aggregate,
    );
    for loadout in &metrics.landing_precision.by_loadout {
        append_landing_aggregate(
            &mut builder,
            &format!("landing/loadout/{}", loadout.loadout.slug()),
            &loadout.aggregate,
        );
    }
    for asymmetry in &metrics.directional_asymmetry {
        let prefix = format!(
            "directional/{}/{}/{}",
            asymmetry.loadout.slug(),
            asymmetry.door_a,
            asymmetry.door_b
        );
        match &asymmetry.comparison {
            super::MetricEvidence::Observed(comparison) => {
                for (axis, difference) in [
                    ("duration-ticks", comparison.duration_ticks),
                    ("semantic-spans", comparison.semantic_spans),
                    ("semantic-transitions", comparison.semantic_transitions),
                    ("ability-events", comparison.ability_events),
                ] {
                    builder.count(format!("{prefix}/{axis}/a-to-b"), difference.a_to_b);
                    builder.count(format!("{prefix}/{axis}/b-to-a"), difference.b_to_a);
                    builder.count(
                        format!("{prefix}/{axis}/absolute-difference"),
                        difference.absolute_difference,
                    );
                }
                builder.count(
                    format!("{prefix}/ability-use/a-to-b-wall-jumps"),
                    comparison.ability_use.a_to_b.wall_jump_events,
                );
                builder.count(
                    format!("{prefix}/ability-use/a-to-b-dashes"),
                    comparison.ability_use.a_to_b.dash_events,
                );
                builder.count(
                    format!("{prefix}/ability-use/b-to-a-wall-jumps"),
                    comparison.ability_use.b_to_a.wall_jump_events,
                );
                builder.count(
                    format!("{prefix}/ability-use/b-to-a-dashes"),
                    comparison.ability_use.b_to_a.dash_events,
                );
                builder.count(
                    format!("{prefix}/ability-use/wall-jump-differs"),
                    usize::from(comparison.ability_use.wall_jump_use_differs),
                );
                builder.count(
                    format!("{prefix}/ability-use/dash-differs"),
                    usize::from(comparison.ability_use.dash_use_differs),
                );
            }
            super::MetricEvidence::Missing { reason } => builder.evidence(
                format!("{prefix}/status"),
                ReportEvidenceStateV1::Missing,
                missing_metric_reason(*reason),
            ),
            super::MetricEvidence::NotApplicable { reason } => builder.evidence(
                format!("{prefix}/status"),
                ReportEvidenceStateV1::NotApplicable,
                not_applicable_metric_reason(*reason),
            ),
        }
    }

    append_terrain_summary(&mut builder, &metrics.terrain);
    append_operational_cost(&mut builder, &metrics.operational_cost);
    builder.finish()
}

fn append_easiest_controller_fractions(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    evidence: &super::MetricEvidence<super::EasiestControllerFractionSummary>,
) {
    match evidence {
        super::MetricEvidence::Observed(value) => {
            builder.count(
                format!("{prefix}/successful-route-count"),
                value.successful_route_count,
            );
            for (name, fraction) in [
                ("run-only-class", value.run_only_class_fraction),
                (
                    "monotone-simple-class",
                    value.monotone_simple_class_fraction,
                ),
                (
                    "other-controller-class",
                    value.other_controller_class_fraction,
                ),
                ("run-only", value.run_only_fraction),
                ("monotone-simple", value.monotone_simple_fraction),
            ] {
                builder.fraction(format!("{prefix}/{name}"), fraction);
            }
        }
        super::MetricEvidence::Missing { reason } => builder.evidence(
            format!("{prefix}/status"),
            ReportEvidenceStateV1::Missing,
            missing_metric_reason(*reason),
        ),
        super::MetricEvidence::NotApplicable { reason } => builder.evidence(
            format!("{prefix}/status"),
            ReportEvidenceStateV1::NotApplicable,
            not_applicable_metric_reason(*reason),
        ),
    }
}

fn append_controller_demand_distributions(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    evidence: &super::MetricEvidence<super::ControllerDemandCoordinateSummary>,
) {
    match evidence {
        super::MetricEvidence::Observed(value) => {
            for (name, distribution) in [
                ("controller-class", value.controller_class),
                ("ability-events", value.ability_events),
                ("horizontal-reversals", value.horizontal_reversals),
                ("vertical-decisions", value.vertical_decisions),
                ("semantic-spans", value.semantic_spans),
                ("semantic-transitions", value.semantic_transitions),
                ("duration-ticks", value.duration_ticks),
            ] {
                builder.distribution(format!("{prefix}/{name}"), distribution);
            }
        }
        super::MetricEvidence::Missing { reason } => builder.evidence(
            format!("{prefix}/status"),
            ReportEvidenceStateV1::Missing,
            missing_metric_reason(*reason),
        ),
        super::MetricEvidence::NotApplicable { reason } => builder.evidence(
            format!("{prefix}/status"),
            ReportEvidenceStateV1::NotApplicable,
            not_applicable_metric_reason(*reason),
        ),
    }
}

fn append_landing_aggregate(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    aggregate: &super::LandingPrecisionAggregateMetric,
) {
    for (name, value) in [
        (
            "canonical-positive-route-count",
            aggregate.canonical_positive_route_count,
        ),
        ("inspected-ticks", aggregate.inspected_ticks),
        (
            "routes-with-landing-events",
            aggregate.routes_with_landing_events,
        ),
        (
            "routes-without-landing-events",
            aggregate.routes_without_landing_events,
        ),
        (
            "routes-with-measured-landings",
            aggregate.routes_with_measured_landings,
        ),
        (
            "routes-with-unmeasured-landings",
            aggregate.routes_with_unmeasured_landings,
        ),
        (
            "routes-with-only-unmeasured-landings",
            aggregate.routes_with_only_unmeasured_landings,
        ),
        ("landing-event-count", aggregate.landing_event_count),
        ("measured-landing-count", aggregate.measured_landing_count),
        (
            "unmeasured-landing-count",
            aggregate.unmeasured_landing_count,
        ),
        ("edge-overhang-landings", aggregate.edge_overhang_landings),
        (
            "one-way-or-mixed-landings",
            aggregate.one_way_or_mixed_landings,
        ),
    ] {
        builder.count(format!("{prefix}/{name}"), value);
    }
    match aggregate.minimum_edge_margin_pixels {
        super::LandingCoordinateEvidence::Observed(value) => {
            builder.signed_distribution(format!("{prefix}/minimum-edge-margin-pixels"), value)
        }
        super::LandingCoordinateEvidence::NotApplicable { reason } => builder.evidence(
            format!("{prefix}/minimum-edge-margin-pixels"),
            ReportEvidenceStateV1::NotApplicable,
            landing_not_applicable_reason(reason),
        ),
    }
    for (name, evidence) in [
        (
            "footprint-overlap-pixels",
            &aggregate.footprint_overlap_pixels,
        ),
        ("support-width-pixels", &aggregate.support_width_pixels),
    ] {
        match evidence {
            super::LandingCoordinateEvidence::Observed(value) => {
                builder.distribution(format!("{prefix}/{name}"), *value)
            }
            super::LandingCoordinateEvidence::NotApplicable { reason } => builder.evidence(
                format!("{prefix}/{name}"),
                ReportEvidenceStateV1::NotApplicable,
                landing_not_applicable_reason(*reason),
            ),
        }
    }
}

fn append_terrain_summary(builder: &mut ReportBuilderV1, terrain: &super::TerrainMetricSummary) {
    let coverage = terrain.coverage;
    for (name, value) in [
        (
            "positive-controller-count",
            coverage.positive_controller_count,
        ),
        (
            "interior-component-count",
            coverage.interior_component_count,
        ),
        ("interior-tile-count", coverage.interior_tile_count),
        (
            "structurally-attributed-component-count",
            coverage.structurally_attributed_component_count,
        ),
        (
            "structurally-attributed-tile-count",
            coverage.structurally_attributed_tile_count,
        ),
        (
            "traversal-near-component-count",
            coverage.traversal_near_component_count,
        ),
        (
            "traversal-near-tile-count",
            coverage.traversal_near_tile_count,
        ),
        (
            "positively-corroborated-component-count",
            coverage.positively_corroborated_component_count,
        ),
        (
            "positively-corroborated-tile-count",
            coverage.positively_corroborated_tile_count,
        ),
        (
            "uncorroborated-component-count",
            coverage.uncorroborated_component_count,
        ),
        (
            "uncorroborated-tile-count",
            coverage.uncorroborated_tile_count,
        ),
    ] {
        builder.count(format!("terrain/coverage/{name}"), value);
    }
    let utility = terrain.ablation_utility;
    for (name, value) in [
        ("variant-count", utility.variant_count),
        (
            "all-stored-controllers-survived",
            utility.all_stored_controllers_survived,
        ),
        (
            "at-least-one-stored-controller-affected",
            utility.at_least_one_stored_controller_affected,
        ),
        ("no-stored-controllers", utility.no_stored_controllers),
        (
            "removed-tiles-all-survived",
            utility.removed_tiles_all_survived,
        ),
        (
            "removed-tiles-some-affected",
            utility.removed_tiles_some_affected,
        ),
        (
            "removed-tiles-no-controllers",
            utility.removed_tiles_no_controllers,
        ),
    ] {
        builder.count(format!("terrain/ablation/{name}"), value);
    }
    append_fraction_evidence(
        builder,
        "terrain/ablation/aggregate-exact-controller-survival",
        &terrain.aggregate_ablation_survival_fraction,
    );
    for (name, evidence) in [
        (
            "structurally-attributed-components",
            &terrain
                .coverage_fractions
                .structurally_attributed_components,
        ),
        (
            "structurally-attributed-tiles",
            &terrain.coverage_fractions.structurally_attributed_tiles,
        ),
        (
            "traversal-near-components",
            &terrain.coverage_fractions.traversal_near_components,
        ),
        (
            "traversal-near-tiles",
            &terrain.coverage_fractions.traversal_near_tiles,
        ),
        (
            "positively-corroborated-components",
            &terrain
                .coverage_fractions
                .positively_corroborated_components,
        ),
        (
            "positively-corroborated-tiles",
            &terrain.coverage_fractions.positively_corroborated_tiles,
        ),
    ] {
        append_fraction_evidence(builder, &format!("terrain/coverage/{name}"), evidence);
    }
}

fn append_fraction_evidence(
    builder: &mut ReportBuilderV1,
    name: &str,
    evidence: &super::MetricEvidence<super::ExactFraction>,
) {
    match evidence {
        super::MetricEvidence::Observed(value) => builder.fraction(name, *value),
        super::MetricEvidence::Missing { reason } => builder.evidence(
            name,
            ReportEvidenceStateV1::Missing,
            missing_metric_reason(*reason),
        ),
        super::MetricEvidence::NotApplicable { reason } => builder.evidence(
            name,
            ReportEvidenceStateV1::NotApplicable,
            not_applicable_metric_reason(*reason),
        ),
    }
}

fn append_operational_cost(builder: &mut ReportBuilderV1, cost: &super::OperationalCostSummary) {
    for (prefix, value) in [
        ("direct-reported", cost.direct_controller_reported_total),
        ("direct-recomputed", cost.direct_controller_recomputed_total),
        ("canonical-positive", cost.canonical_positive_route_total),
    ] {
        builder.operational(
            format!("operational/{prefix}/expanded-nodes"),
            value.expanded_nodes,
        );
        builder.operational(
            format!("operational/{prefix}/generated-nodes"),
            value.generated_nodes,
        );
        builder.operational(
            format!("operational/{prefix}/simulated-ticks"),
            value.simulated_ticks,
        );
        builder.operational(
            format!("operational/{prefix}/deepest-path-ticks"),
            value.deepest_path_ticks,
        );
    }
}

const fn missing_metric_reason(reason: super::MissingMetricReason) -> &'static str {
    match reason {
        super::MissingMetricReason::MissingDirectedRouteAssessment => {
            "missing-directed-route-assessment"
        }
        super::MissingMetricReason::MissingDirectControllerAudit => {
            "missing-direct-controller-audit"
        }
        super::MissingMetricReason::BoundedDirectControllerAuditWithoutPositive => {
            "bounded-direct-controller-audit-without-positive"
        }
        super::MissingMetricReason::NoPositiveTerrainController => "no-positive-terrain-controller",
        super::MissingMetricReason::ReverseDirectionsRequireTwoKnownPositiveControllers => {
            "reverse-directions-require-two-known-positive-controllers"
        }
    }
}

const fn not_applicable_metric_reason(reason: super::NotApplicableMetricReason) -> &'static str {
    match reason {
        super::NotApplicableMetricReason::NoDirectedRoutes => "no-directed-routes",
        super::NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary => {
            "no-known-positive-controller-in-complete-finite-vocabulary"
        }
        super::NotApplicableMetricReason::NoSuccessfulRoutesForLoadout => {
            "no-successful-routes-for-loadout"
        }
        super::NotApplicableMetricReason::AmbiguousNondominatedFront => {
            "ambiguous-nondominated-front"
        }
        super::NotApplicableMetricReason::NoInteriorTerrainComponents => {
            "no-interior-terrain-components"
        }
        super::NotApplicableMetricReason::NoInteriorTerrainTiles => "no-interior-terrain-tiles",
        super::NotApplicableMetricReason::NoAblationVariants => "no-ablation-variants",
        super::NotApplicableMetricReason::NoExactControllersForAblation => {
            "no-exact-controllers-for-ablation"
        }
    }
}

const fn landing_not_applicable_reason(
    reason: super::LandingCoordinateNotApplicableReason,
) -> &'static str {
    match reason {
        super::LandingCoordinateNotApplicableReason::NoCanonicalPositiveRoutes => {
            "no-canonical-positive-routes"
        }
        super::LandingCoordinateNotApplicableReason::NoLandingEvents => "no-landing-events",
        super::LandingCoordinateNotApplicableReason::NoMeasuredLandings => "no-measured-landings",
    }
}

fn route_choice_report_summary_v1(
    report: &RoomRouteChoiceDiversity,
    summary: &RouteChoiceSelectionMetricSummary,
) -> TransparentReportSummaryV1 {
    let mut builder = ReportBuilderV1::new(summary.version, &summary.room_id);
    append_route_choice_aggregate(&mut builder, "aggregate", &summary.aggregate);
    for loadout in &summary.by_loadout {
        append_route_choice_aggregate(
            &mut builder,
            &format!("loadout/{}/aggregate", loadout.loadout.slug()),
            &loadout.aggregate,
        );
    }
    let operational = report.operational_cost;
    builder.operational(
        "operational/inherited/expanded-nodes",
        operational.inherited_direct_controller_audit.expanded_nodes,
    );
    builder.operational(
        "operational/inherited/generated-nodes",
        operational
            .inherited_direct_controller_audit
            .generated_nodes,
    );
    builder.operational(
        "operational/inherited/simulated-ticks",
        operational
            .inherited_direct_controller_audit
            .simulated_ticks,
    );
    builder.operational(
        "operational/inherited/deepest-path-ticks",
        operational
            .inherited_direct_controller_audit
            .deepest_path_ticks,
    );
    builder.operational(
        "operational/replayed-direct-witnesses",
        operational.replayed_direct_witnesses,
    );
    builder.operational(
        "operational/replayed-direct-ticks",
        operational.replayed_direct_ticks,
    );
    builder.operational(
        "operational/reused-canonical-observations",
        operational.reused_canonical_observations,
    );
    builder.finish()
}

fn append_route_choice_aggregate(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    aggregate: &RouteChoiceSelectionAggregate,
) {
    let source = &aggregate.source;
    for (name, value) in [
        ("expected-cells", source.expected_cells),
        ("positive-cells", source.positive_cells),
        (
            "positive-complete-audit-cells",
            source.positive_complete_audit_cells,
        ),
        (
            "positive-bounded-audit-cells",
            source.positive_bounded_audit_cells,
        ),
        (
            "positive-missing-route-or-audit-cells",
            source.positive_missing_route_or_audit_cells,
        ),
        (
            "complete-without-positive-cells",
            source.complete_without_positive_cells,
        ),
        (
            "bounded-inconclusive-without-positive-cells",
            source.bounded_inconclusive_without_positive_cells,
        ),
        ("missing-route-cells", source.missing_route_cells),
        ("missing-audit-cells", source.missing_audit_cells),
        (
            "cells-with-multiple-alternatives",
            source.cells_with_multiple_alternatives,
        ),
        ("observed-positive-count", source.observed_positive_count),
        (
            "positive-alternative-count",
            source.positive_alternative_count,
        ),
        (
            "timing-or-exact-aliases-collapsed",
            source.timing_or_exact_aliases_collapsed,
        ),
        (
            "cells-with-spatially-distinct-alternatives",
            aggregate.cells_with_spatially_distinct_alternatives,
        ),
        (
            "cells-with-action-distinct-alternatives",
            aggregate.cells_with_action_distinct_alternatives,
        ),
        (
            "cells-with-event-distinct-alternatives",
            aggregate.cells_with_event_distinct_alternatives,
        ),
        (
            "cells-with-gate-style-distinct-alternatives",
            aggregate.cells_with_gate_style_distinct_alternatives,
        ),
    ] {
        builder.count(format!("{prefix}/{name}"), value);
    }
    for (name, bins) in [
        (
            "alternative-count-histogram",
            &source.alternative_count_histogram,
        ),
        (
            "spatial-path-class-histogram",
            &source.spatial_path_class_histogram,
        ),
        (
            "semantic-action-class-histogram",
            &source.semantic_action_class_histogram,
        ),
        (
            "accepted-event-class-histogram",
            &source.accepted_event_class_histogram,
        ),
        (
            "gate-path-style-class-histogram",
            &source.gate_path_style_class_histogram,
        ),
    ] {
        builder.histogram(format!("{prefix}/{name}"), bins);
    }
    for (name, value) in [
        ("alternative-count", aggregate.alternative_count),
        ("spatial-path-classes", aggregate.spatial_path_classes),
        ("semantic-action-classes", aggregate.semantic_action_classes),
        ("accepted-event-classes", aggregate.accepted_event_classes),
        ("gate-path-style-classes", aggregate.gate_path_style_classes),
    ] {
        builder.optional_distribution(
            format!("{prefix}/{name}"),
            value,
            "no-positive-route-choice-cells",
        );
    }
    append_route_choice_distances(builder, prefix, &source.distances);
}

fn append_route_choice_distances(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    distances: &RouteAlternativeDistanceReport,
) {
    for (axis, distribution) in [
        ("spatial-trajectory", &distances.spatial_trajectory),
        ("semantic-actions", &distances.semantic_actions),
        (
            "accepted-event-sequence",
            &distances.accepted_event_sequence,
        ),
        ("gate-path-style", &distances.gate_path_style),
    ] {
        append_within_cell_distance(builder, &format!("{prefix}/distance/{axis}"), distribution);
    }
}

fn append_within_cell_distance(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    distribution: &WithinCellDistanceDistribution,
) {
    builder.normalized(format!("{prefix}/pairwise"), distribution.pairwise);
    builder.normalized(
        format!("{prefix}/nearest-neighbor"),
        distribution.nearest_neighbor,
    );
}

fn pickup_detour_report_summary_v1(
    report: &RoomPickupDetourAnalysis,
    summary: &PickupDetourSelectionMetricSummary,
) -> TransparentReportSummaryV1 {
    let mut builder = ReportBuilderV1::new(summary.version, &summary.room_id);
    let structural = &summary.structural;
    for (name, value) in [
        ("structural/expected-pickups", structural.expected_pickups),
        ("structural/unique-mappings", structural.unique_mappings),
        (
            "structural/unique-on-shortest-port-spine",
            structural.unique_on_shortest_port_spine,
        ),
        (
            "structural/unique-off-shortest-port-spine",
            structural.unique_off_shortest_port_spine,
        ),
        (
            "structural/authored-leaf-mappings",
            structural.authored_leaf_mappings,
        ),
        (
            "structural/no-matching-authored-node",
            structural.no_matching_authored_node,
        ),
        (
            "structural/ambiguous-authored-node",
            structural.ambiguous_authored_node,
        ),
    ] {
        builder.count(name, value);
    }
    builder.optional_distribution(
        "structural/off-spine-distance",
        structural.off_spine_distance,
        "no-unique-off-spine-pickup-mapping",
    );
    append_pickup_aggregate(&mut builder, "aggregate", &summary.aggregate);
    for loadout in &summary.by_loadout {
        append_pickup_aggregate(
            &mut builder,
            &format!("loadout/{}/aggregate", loadout.loadout.slug()),
            &loadout.aggregate,
        );
    }
    let cross = &summary.cross_loadout;
    for (name, value) in [
        (
            "cross-loadout/expected-source-pickup-cells",
            cross.expected_source_pickup_cells,
        ),
        (
            "cross-loadout/cells-with-any-positive-loadout",
            cross.cells_with_any_positive_loadout,
        ),
        (
            "cross-loadout/cells-with-any-bounded-loadout",
            cross.cells_with_any_bounded_loadout,
        ),
        (
            "cross-loadout/cells-positive-at-baseline",
            cross.cells_positive_at_baseline,
        ),
        (
            "cross-loadout/cells-bounded-at-baseline",
            cross.cells_bounded_at_baseline,
        ),
        (
            "cross-loadout/cells-positive-without-wall-jump",
            cross.cells_positive_without_wall_jump,
        ),
        (
            "cross-loadout/cells-with-bounded-non-wall-jump-loadout",
            cross.cells_with_bounded_non_wall_jump_loadout,
        ),
        (
            "cross-loadout/cells-positive-without-dash",
            cross.cells_positive_without_dash,
        ),
        (
            "cross-loadout/cells-with-bounded-non-dash-loadout",
            cross.cells_with_bounded_non_dash_loadout,
        ),
        (
            "cross-loadout/cells-with-wall-jump-use-in-positive-witness",
            cross.cells_with_wall_jump_use_in_positive_witness,
        ),
        (
            "cross-loadout/cells-with-dash-use-in-positive-witness",
            cross.cells_with_dash_use_in_positive_witness,
        ),
    ] {
        builder.count(name, value);
    }
    for source in &report.operational_cost.source_searches {
        let prefix = format!(
            "operational/source/{}/{:08}/{}",
            source.loadout.slug(),
            source.source_index,
            source.source_door_id
        );
        builder.operational(
            format!("{prefix}/expanded-nodes"),
            source.search.expanded_nodes,
        );
        builder.operational(
            format!("{prefix}/generated-nodes"),
            source.search.generated_nodes,
        );
        builder.operational(
            format!("{prefix}/simulated-ticks"),
            source.search.simulated_ticks,
        );
        builder.operational(
            format!("{prefix}/deepest-path-ticks"),
            source.search.deepest_path_ticks,
        );
    }
    builder.finish()
}

fn append_pickup_aggregate(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    aggregate: &PickupDetourSelectionAggregate,
) {
    for (name, value) in [
        ("expected-cells", aggregate.expected_cells),
        ("positive-cells", aggregate.positive_cells),
        (
            "bounded-inconclusive-cells",
            aggregate.bounded_inconclusive_cells,
        ),
        (
            "opportunistic-context-cells",
            aggregate.opportunistic_context_cells,
        ),
        (
            "target-directed-only-context-cells",
            aggregate.target_directed_only_context_cells,
        ),
        (
            "no-positive-door-witness-context-cells",
            aggregate.no_positive_door_witness_context_cells,
        ),
        (
            "contexts-with-bounded-door-cells",
            aggregate.contexts_with_bounded_door_cells,
        ),
        (
            "positive-cells-with-door-comparison",
            aggregate.positive_cells_with_door_comparison,
        ),
        (
            "target-directed-positive-cells-with-door-comparison",
            aggregate.target_directed_positive_cells_with_door_comparison,
        ),
    ] {
        builder.count(format!("{prefix}/{name}"), value);
    }
    append_pickup_challenge_distributions(
        builder,
        &format!("{prefix}/retained-positive-challenge"),
        &aggregate.retained_positive_challenge,
    );
    append_pickup_detour_distributions(
        builder,
        &format!("{prefix}/minimum-known-detour"),
        &aggregate.minimum_known_detour,
    );
    append_pickup_detour_distributions(
        builder,
        &format!("{prefix}/target-directed-minimum-known-detour"),
        &aggregate.target_directed_minimum_known_detour,
    );
}

fn append_pickup_challenge_distributions(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    distributions: &PickupChallengeCoordinateDistributions,
) {
    for (name, value) in [
        ("completion-ticks", distributions.completion_ticks),
        ("horizontal-reversals", distributions.horizontal_reversals),
        ("vertical-decisions", distributions.vertical_decisions),
        ("semantic-transitions", distributions.semantic_transitions),
        ("coarse-path-steps", distributions.coarse_path_steps),
    ] {
        builder.optional_distribution(
            format!("{prefix}/{name}"),
            value,
            "no-positive-pickup-cells",
        );
    }
}

fn append_pickup_detour_distributions(
    builder: &mut ReportBuilderV1,
    prefix: &str,
    distributions: &PickupDetourCoordinateDistributions,
) {
    for (name, value) in [
        (
            "pickup-only-visited-cells",
            distributions.pickup_only_visited_cells,
        ),
        (
            "traversal-span-edit-distance",
            distributions.traversal_span_edit_distance,
        ),
        (
            "semantic-span-edit-distance",
            distributions.semantic_span_edit_distance,
        ),
        (
            "absolute-duration-difference",
            distributions.absolute_duration_difference,
        ),
    ] {
        builder.optional_distribution(
            format!("{prefix}/{name}"),
            value,
            "no-comparable-positive-door-witness",
        );
    }
}

/// Derive all production-unavailable coordinates without collapsing Missing
/// and BoundedInconclusive into one state.
#[must_use]
pub fn production_eligibility(
    descriptor: &CorpusSelectionDescriptor,
) -> OfflineCacheProductionEligibilityV1 {
    let mut unavailable = Vec::new();
    let mut visit = |coordinate: String, evidence: &QuantizedSelectionEvidence| {
        if matches!(
            evidence.state,
            SelectionEvidenceState::Missing | SelectionEvidenceState::BoundedInconclusive
        ) {
            unavailable.push(UnavailableSelectionCoordinateV1 {
                coordinate,
                state: evidence.state.into(),
            });
        }
    };
    for quality in &descriptor.quality {
        visit(format!("quality::{:?}", quality.axis), &quality.evidence);
    }
    for projection in &descriptor.projections {
        for coordinate in &projection.cell {
            visit(
                format!("projection::{:?}::cell::{}", projection.id, coordinate.name),
                &coordinate.evidence,
            );
        }
        for coordinate in &projection.detail {
            visit(
                format!(
                    "projection::{:?}::detail::{}",
                    projection.id, coordinate.name
                ),
                &coordinate.evidence,
            );
        }
    }
    if unavailable.is_empty() {
        OfflineCacheProductionEligibilityV1::Eligible
    } else {
        OfflineCacheProductionEligibilityV1::Ineligible {
            unavailable_coordinates: unavailable,
        }
    }
}

/// Create or exact-verify the immutable run manifest and room directory.
pub fn create_or_verify_offline_cache_manifest_v1(
    root: &Path,
    manifest: &OfflineCacheManifestV1,
) -> Result<(), OfflineSelectionCacheError> {
    manifest.validate()?;
    if !root.exists() {
        fs::create_dir(root).map_err(|source| OfflineSelectionCacheError::Io {
            path: root.to_owned(),
            source,
        })?;
    }
    let metadata = fs::symlink_metadata(root).map_err(|source| OfflineSelectionCacheError::Io {
        path: root.to_owned(),
        source,
    })?;
    if !metadata.file_type().is_dir() {
        return Err(invalid(format!(
            "offline cache root is not a real directory: {}",
            root.display()
        )));
    }
    let path = root.join(CACHE_MANIFEST_FILE);
    let bytes = canonical_json_line(manifest)?;
    if path.exists() {
        if read(&path)? != bytes {
            return Err(invalid(
                "existing offline cache manifest differs from requested run",
            ));
        }
    } else {
        write_new(&path, &bytes)?;
    }
    let rooms = root.join(CACHE_ROOMS_DIRECTORY);
    if !rooms.exists() {
        fs::create_dir(&rooms).map_err(|source| OfflineSelectionCacheError::Io {
            path: rooms.clone(),
            source,
        })?;
    }
    if !fs::symlink_metadata(&rooms)
        .map_err(|source| OfflineSelectionCacheError::Io {
            path: rooms.clone(),
            source,
        })?
        .file_type()
        .is_dir()
    {
        return Err(invalid("offline cache rooms path is not a real directory"));
    }
    Ok(())
}

/// Write one create-new row and its completion marker, or exact-verify an
/// already completed row. A partial/corrupt directory fails closed.
pub fn write_or_verify_offline_cache_room_v1(
    root: &Path,
    manifest: &OfflineCacheManifestV1,
    record: &OfflineCacheRoomRecordV1,
) -> Result<OfflineCacheRoomWriteOutcomeV1, OfflineSelectionCacheError> {
    record.validate_against(manifest)?;
    let directory = cache_room_directory(root, &record.room_id);
    if directory.exists() {
        return verify_offline_cache_room_v1(root, manifest, &record.room_id).and_then(
            |existing| {
                if existing != *record {
                    Err(invalid(format!(
                        "existing offline cache row {:?} differs from recomputation",
                        record.room_id.0
                    )))
                } else {
                    Ok(OfflineCacheRoomWriteOutcomeV1::AlreadyVerified(existing))
                }
            },
        );
    }
    fs::create_dir(&directory).map_err(|source| OfflineSelectionCacheError::Io {
        path: directory.clone(),
        source,
    })?;
    let record_bytes = canonical_json_line(record)?;
    write_new(&directory.join(CACHE_ROOM_RECORD_FILE), &record_bytes)?;
    let read_back = read(&directory.join(CACHE_ROOM_RECORD_FILE))?;
    let parsed: OfflineCacheRoomRecordV1 = parse_one_json_line(&read_back, CACHE_ROOM_RECORD_FILE)?;
    if parsed != *record {
        return Err(invalid("offline cache row changed across disk read-back"));
    }
    parsed.validate_against(manifest)?;
    let checkpoint = OfflineCacheRoomCheckpointV1 {
        checkpoint_version: CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION,
        status: COMPLETE_STATUS.to_owned(),
        run_id: manifest.run_id.clone(),
        room_id: record.room_id.clone(),
        record_hash: stable_byte_hash(&read_back),
    };
    write_new(
        &directory.join(CACHE_ROOM_CHECKPOINT_FILE),
        &canonical_json_line(&checkpoint)?,
    )?;
    let verified = verify_offline_cache_room_v1(root, manifest, &record.room_id)?;
    Ok(OfflineCacheRoomWriteOutcomeV1::Written(verified))
}

pub fn verify_offline_cache_room_v1(
    root: &Path,
    manifest: &OfflineCacheManifestV1,
    room_id: &RoomId,
) -> Result<OfflineCacheRoomRecordV1, OfflineSelectionCacheError> {
    let directory = cache_room_directory(root, room_id);
    let metadata =
        fs::symlink_metadata(&directory).map_err(|source| OfflineSelectionCacheError::Io {
            path: directory.clone(),
            source,
        })?;
    if !metadata.file_type().is_dir() {
        return Err(invalid(format!(
            "cache row path is not a real directory: {}",
            directory.display()
        )));
    }
    let record_bytes = read(&directory.join(CACHE_ROOM_RECORD_FILE))?;
    let checkpoint_bytes = read(&directory.join(CACHE_ROOM_CHECKPOINT_FILE))?;
    let record: OfflineCacheRoomRecordV1 =
        parse_one_json_line(&record_bytes, CACHE_ROOM_RECORD_FILE)?;
    let checkpoint: OfflineCacheRoomCheckpointV1 =
        parse_one_json_line(&checkpoint_bytes, CACHE_ROOM_CHECKPOINT_FILE)?;
    if checkpoint.checkpoint_version != CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION
        || checkpoint.status != COMPLETE_STATUS
        || checkpoint.run_id != manifest.run_id
        || checkpoint.room_id != *room_id
        || checkpoint.record_hash != stable_byte_hash(&record_bytes)
        || record.room_id != *room_id
    {
        return Err(invalid(format!(
            "cache row {:?} checkpoint mismatch",
            room_id.0
        )));
    }
    record.validate_against(manifest)?;
    Ok(record)
}

/// Publish the run checkpoint only after every expected row independently
/// verifies. Existing completion is accepted only when byte-identical.
pub fn complete_or_verify_offline_cache_v1(
    root: &Path,
    manifest: &OfflineCacheManifestV1,
) -> Result<OfflineCacheCompletionV1, OfflineSelectionCacheError> {
    let mut rooms = Vec::with_capacity(manifest.expected_rooms.len());
    let mut hashes = BTreeMap::new();
    for expected in &manifest.expected_rooms {
        let room = verify_offline_cache_room_v1(root, manifest, &expected.room_id)?;
        let checkpoint_path =
            cache_room_directory(root, &expected.room_id).join(CACHE_ROOM_CHECKPOINT_FILE);
        hashes.insert(
            expected.room_id.clone(),
            stable_byte_hash(&read(&checkpoint_path)?),
        );
        rooms.push(room);
    }
    let completion = OfflineCacheCompletionV1 {
        checkpoint_version: CORPUS_V3_OFFLINE_CACHE_CHECKPOINT_VERSION,
        status: COMPLETE_STATUS.to_owned(),
        run_id: manifest.run_id.clone(),
        room_count: rooms.len(),
        eligible_room_count: rooms
            .iter()
            .filter(|room| {
                matches!(
                    room.production_eligibility,
                    OfflineCacheProductionEligibilityV1::Eligible
                )
            })
            .count(),
        room_checkpoint_hashes: hashes,
    };
    let bytes = canonical_json_line(&completion)?;
    let path = root.join(CACHE_COMPLETION_FILE);
    if path.exists() {
        if read(&path)? != bytes {
            return Err(invalid(
                "existing offline cache completion differs from verified rows",
            ));
        }
    } else {
        write_new(&path, &bytes)?;
    }
    Ok(completion)
}

pub fn load_verified_offline_cache_v1(
    root: &Path,
) -> Result<VerifiedOfflineSelectionCacheV1, OfflineSelectionCacheError> {
    let manifest_bytes = read(&root.join(CACHE_MANIFEST_FILE))?;
    let manifest: OfflineCacheManifestV1 =
        parse_one_json_line(&manifest_bytes, CACHE_MANIFEST_FILE)?;
    manifest.validate()?;
    let completion_bytes = read(&root.join(CACHE_COMPLETION_FILE))?;
    let completion: OfflineCacheCompletionV1 =
        parse_one_json_line(&completion_bytes, CACHE_COMPLETION_FILE)?;
    let recomputed = complete_or_verify_offline_cache_v1(root, &manifest)?;
    if completion != recomputed {
        return Err(invalid(
            "offline cache completion differs from verified room rows",
        ));
    }
    let rooms = manifest
        .expected_rooms
        .iter()
        .map(|expected| verify_offline_cache_room_v1(root, &manifest, &expected.room_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VerifiedOfflineSelectionCacheV1 {
        manifest,
        rooms,
        completion,
    })
}

fn cache_room_directory(root: &Path, room_id: &RoomId) -> PathBuf {
    root.join(CACHE_ROOMS_DIRECTORY).join(format!(
        "room-{}",
        stable_byte_hash(room_id.0.as_bytes())
            .rsplit('-')
            .next()
            .expect("stable hash has a suffix")
    ))
}

#[derive(Debug)]
pub enum OfflineSelectionCacheError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json(serde_json::Error),
    AlreadyExists(PathBuf),
    Invalid(String),
}

impl fmt::Display for OfflineSelectionCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "invalid offline-cache JSON: {source}"),
            Self::AlreadyExists(path) => {
                write!(formatter, "refusing to overwrite {}", path.display())
            }
            Self::Invalid(detail) => formatter.write_str(detail),
        }
    }
}

impl Error for OfflineSelectionCacheError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(source) => Some(source),
            Self::AlreadyExists(_) | Self::Invalid(_) => None,
        }
    }
}

impl From<serde_json::Error> for OfflineSelectionCacheError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn invalid(detail: impl Into<String>) -> OfflineSelectionCacheError {
    OfflineSelectionCacheError::Invalid(detail.into())
}

pub(crate) fn stable_byte_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("downwards-offline-cache-fnv1a64-{hash:016x}")
}

fn canonical_json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, OfflineSelectionCacheError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn parse_one_json_line<T>(bytes: &[u8], label: &str) -> Result<T, OfflineSelectionCacheError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    if !bytes.ends_with(b"\n") || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n') {
        return Err(invalid(format!(
            "{label} must be exactly one newline-terminated JSON record"
        )));
    }
    let value = serde_json::from_slice::<T>(&bytes[..bytes.len() - 1])?;
    let canonical = canonical_json_line(&value)?;
    if canonical != bytes {
        let first_difference = bytes
            .iter()
            .zip(&canonical)
            .position(|(stored, rerendered)| stored != rerendered)
            .unwrap_or_else(|| bytes.len().min(canonical.len()));
        let start = first_difference.saturating_sub(32);
        let stored_end = bytes.len().min(first_difference.saturating_add(64));
        let canonical_end = canonical.len().min(first_difference.saturating_add(64));
        return Err(invalid(format!(
            "{label} is not canonical JSON at byte {first_difference}: stored {:?}, rerendered {:?}",
            String::from_utf8_lossy(&bytes[start..stored_end]),
            String::from_utf8_lossy(&canonical[start..canonical_end]),
        )));
    }
    Ok(value)
}

fn read(path: &Path) -> Result<Vec<u8>, OfflineSelectionCacheError> {
    fs::read(path).map_err(|source| OfflineSelectionCacheError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), OfflineSelectionCacheError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                OfflineSelectionCacheError::AlreadyExists(path.to_owned())
            } else {
                OfflineSelectionCacheError::Io {
                    path: path.to_owned(),
                    source,
                }
            }
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| OfflineSelectionCacheError::Io {
            path: path.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    };

    use super::super::{DirectedLoadoutShakyHandCell, EvaluationLoadout};
    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn room_parallelism_crosses_only_send_sync_boundaries() {
        assert_send_sync::<OfflineCacheManifestV1>();
        assert_send_sync::<CorpusRoomAnalysisConfig>();
        assert_send_sync::<super::super::EvaluatedCorpusRoomV2>();
        assert_send_sync::<OfflineCacheRoomRecordV1>();
        assert_send_sync::<OfflineSelectionArtifactV1>();
    }

    #[test]
    fn two_room_workers_preserve_canonical_result_order() {
        let room_ids = ["a", "b", "c", "d"]
            .into_iter()
            .map(|suffix| RoomId(format!("room-{suffix}")))
            .collect::<Vec<_>>();
        let worker_threads = Mutex::new(Vec::new());
        let results = run_ordered_room_jobs_v1(&room_ids, 2, |room_id| {
            worker_threads.lock().unwrap().push(thread::current().id());
            Ok(room_id.clone())
        })
        .unwrap();
        assert_eq!(results, room_ids);

        let mut unique_threads = Vec::new();
        for thread_id in worker_threads.into_inner().unwrap() {
            if !unique_threads.contains(&thread_id) {
                unique_threads.push(thread_id);
            }
        }
        assert_eq!(unique_threads.len(), 2);
    }

    #[test]
    fn room_workers_join_all_and_report_canonical_earliest_error() {
        let room_ids = ["a", "b", "c", "d"]
            .into_iter()
            .map(|suffix| RoomId(format!("room-{suffix}")))
            .collect::<Vec<_>>();
        let visited = AtomicUsize::new(0);
        let error = run_ordered_room_jobs_v1(&room_ids, 2, |room_id| {
            visited.fetch_add(1, Ordering::SeqCst);
            if matches!(room_id.0.as_str(), "room-c" | "room-d") {
                Err(invalid(format!("failure at {}", room_id.0)))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(visited.load(Ordering::SeqCst), room_ids.len());
        assert_eq!(error.to_string(), "failure at room-c");
    }

    #[test]
    fn room_worker_seam_rejects_unsupported_width_and_unsorted_jobs() {
        let sorted = vec![RoomId("room-a".to_owned())];
        assert!(run_ordered_room_jobs_v1(&sorted, 0, |_| Ok(())).is_err());
        assert!(run_ordered_room_jobs_v1(&sorted, 3, |_| Ok(())).is_err());

        let unsorted = vec![RoomId("room-b".to_owned()), RoomId("room-a".to_owned())];
        assert!(run_ordered_room_jobs_v1(&unsorted, 2, |_| Ok(())).is_err());
    }

    fn sample_descriptor(room_id: &RoomId) -> CorpusSelectionDescriptor {
        CorpusSelectionDescriptor {
            version: CORPUS_SELECTION_METRICS_VERSION,
            room_id: room_id.clone(),
            projections: vec![CorpusSelectionProjection {
                id: CorpusSelectionProjectionId::MorphologyTopologyV1,
                cell: vec![NamedSelectionCoordinate {
                    name: "sample".to_owned(),
                    evidence: QuantizedSelectionEvidence::observed(7),
                }],
                detail: vec![NamedSelectionCoordinate {
                    name: "sample".to_owned(),
                    evidence: QuantizedSelectionEvidence::observed(7),
                }],
            }],
            quality: vec![CorpusSelectionQualityCoordinate {
                axis: CorpusSelectionQualityAxis::CompleteKitMedianDuration,
                direction: ObjectiveDirection::Maximize,
                evidence: QuantizedSelectionEvidence::observed(11),
            }],
            diversity_coordinates: vec![1, 0, 0, 0, 7],
            sockets: vec![DoorSocket {
                side: BoundarySide::Floor,
                offset: 40,
                span: 20,
            }],
        }
    }

    fn sample_manifest_and_row() -> (OfflineCacheManifestV1, OfflineCacheRoomRecordV1) {
        let room_id = RoomId("room-v3-cache-test".to_owned());
        let key = CorpusBuildConfigV2::attempt_zero(0, 1)
            .exact_candidate_keys()
            .unwrap()
            .remove(0);
        let checkpoint = CorpusSeedCheckpointV3 {
            checkpoint_version: CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION,
            status: COMPLETE_STATUS.to_owned(),
            seed: 0,
            config_id: "config-test".to_owned(),
            artifact_hashes: BTreeMap::from([("manifest.json".to_owned(), "hash".to_owned())]),
        };
        let source = OfflineCacheSourceShardV1 {
            seed: 0,
            config_id: checkpoint.config_id.clone(),
            checkpoint_hash: "checkpoint-hash".to_owned(),
            checkpoint,
        };
        let analysis_config = CorpusRoomAnalysisConfig::default()
            .identity_record()
            .unwrap();
        let manifest = OfflineCacheManifestV1::new(
            0,
            1,
            vec![source.clone()],
            analysis_config.clone(),
            vec![OfflineCacheExpectedRoomV1 {
                source_seed: 0,
                room_id: room_id.clone(),
                canonical_key: key.clone(),
            }],
        )
        .unwrap();
        let descriptor = sample_descriptor(&room_id);
        let report = |source_version| TransparentReportSummaryV1 {
            source_version,
            room_id: room_id.clone(),
            coordinates: vec![NamedReportCoordinateV1 {
                name: "positive-count".to_owned(),
                value: ReportValueV1::Count { value: 1 },
            }],
        };
        let row = OfflineCacheRoomRecordV1 {
            schema_version: CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION,
            status: CACHE_ROW_STATUS.to_owned(),
            run_id: manifest.run_id.clone(),
            source,
            room_id: room_id.clone(),
            canonical_key: key,
            analysis_config,
            policies: OfflineCachePolicyRecordV1::current(),
            descriptor: SelectionDescriptorRecordV1::from_descriptor(&descriptor),
            production_eligibility: production_eligibility(&descriptor),
            shaky_hand: CorpusShakyHandAnalysis {
                version: CORPUS_SHAKY_HAND_ANALYSIS_VERSION,
                source_analysis_version: CORPUS_ROOM_ANALYSIS_VERSION,
                seed_derivation_version: SHAKY_HAND_ROUTE_SEED_VERSION,
                ai_policy_version: SHAKY_HAND_POLICY_VERSION,
                ai_config_version: SHAKY_HAND_CONFIG_VERSION,
                room_id: room_id.clone(),
                config: manifest.shaky_hand_config,
                evidence_disclaimer: manifest.policies.disclaimers.shaky_hand.clone(),
                cells: Vec::new(),
            },
            reports: OfflineCacheReportSummariesV1 {
                room_metrics: report(ROOM_METRIC_SUMMARY_VERSION),
                route_choices: report(ROUTE_CHOICE_SELECTION_METRICS_VERSION),
                pickup_detours: report(PICKUP_DETOUR_SELECTION_METRICS_VERSION),
            },
        };
        (manifest, row)
    }

    fn sample_selection_artifact(
        manifest: &OfflineCacheManifestV1,
        row: &OfflineCacheRoomRecordV1,
    ) -> OfflineSelectionArtifactV1 {
        let descriptor = row.descriptor.clone();
        let descriptor_hash = stable_byte_hash(&canonical_json_line(&descriptor).unwrap());
        let archive_summary = ArchiveSummaryRecordV1 {
            submitted_candidates: 1,
            retained_candidates: 1,
            cell_count: 1,
            elite_placements: 1,
        };
        let cell = ArchiveCellRecordV1 {
            projection: SelectionProjectionIdRecordV1::MorphologyTopologyV1,
            coordinates: vec![7],
            elites: vec![row.room_id.clone()],
        };
        OfflineSelectionArtifactV1 {
            schema_version: CORPUS_V3_OFFLINE_CACHE_SCHEMA_VERSION,
            publication_state: OfflineSelectionPublicationStateV1::ProvisionalOperationalCache,
            source_cache_run_id: manifest.run_id.clone(),
            source_cache_completion_hash: "cache-completion-hash".to_owned(),
            analysis_config: manifest.analysis_config.clone(),
            policies: manifest.policies.clone(),
            selection_config: SelectionConfigRecordV1 {
                requested_minimum: 1,
                requested_maximum: 1,
                elites_per_cell: 1,
            },
            outcome: SelectionOutcomeRecordV1 {
                archive_summary: archive_summary.clone(),
                archive_cells: vec![cell],
                descriptors: vec![descriptor],
                descriptor_hashes: BTreeMap::from([(row.room_id.clone(), descriptor_hash)]),
                selected_room_ids: vec![row.room_id.clone()],
                socket_packages: vec![SocketPackageRecordV1 {
                    package_id: row.room_id.clone(),
                    room_ids: vec![row.room_id.clone()],
                }],
                audit: SelectionAuditRecordV1 {
                    version: CORPUS_SELECTION_METRICS_VERSION,
                    submitted_rooms: 1,
                    exact_visual_unique_rooms: 1,
                    archive: archive_summary,
                    archive_ranked_rooms: 1,
                    rooms_excluded_by_initial_socket_core: Vec::new(),
                    socket_pruning_steps: Vec::new(),
                    selected_rooms: 1,
                    selected_package_count: 1,
                    covered_cells: vec![ArchiveCellKeyRecordV1 {
                        projection: SelectionProjectionIdRecordV1::MorphologyTopologyV1,
                        coordinates: vec![7],
                    }],
                    steps: vec![SelectionStepRecordV1 {
                        room_id: row.room_id.clone(),
                        marginal_cell_coverage: 1,
                        minimum_l1_distance: None,
                    }],
                },
            },
            recomputed_selected_descriptor_hashes: BTreeMap::new(),
        }
    }

    fn temp_directory(label: &str) -> PathBuf {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "downwards-offline-cache-{label}-{}-{serial}",
            std::process::id()
        ))
    }

    #[test]
    fn descriptor_wire_round_trip_is_exact_and_preserves_bounded_state() {
        let room_id = RoomId("room-v3-wire-test".to_owned());
        let mut descriptor = sample_descriptor(&room_id);
        descriptor.projections[0]
            .detail
            .push(NamedSelectionCoordinate {
                name: "bounded".to_owned(),
                evidence: QuantizedSelectionEvidence::bounded_inconclusive(),
            });
        let record = SelectionDescriptorRecordV1::from_descriptor(&descriptor);
        assert_eq!(record.to_descriptor().unwrap(), descriptor);
        assert!(matches!(
            production_eligibility(&descriptor),
            OfflineCacheProductionEligibilityV1::Ineligible { .. }
        ));
    }

    #[test]
    fn production_eligibility_preserves_missing_and_bounded_coordinates() {
        let room_id = RoomId("room-v3-wire-unavailable".to_owned());
        let mut descriptor = sample_descriptor(&room_id);
        descriptor.quality[0].evidence = QuantizedSelectionEvidence::missing();
        descriptor.projections[0]
            .detail
            .push(NamedSelectionCoordinate {
                name: "bounded".to_owned(),
                evidence: QuantizedSelectionEvidence::bounded_inconclusive(),
            });
        let OfflineCacheProductionEligibilityV1::Ineligible {
            unavailable_coordinates,
        } = production_eligibility(&descriptor)
        else {
            panic!("missing/bounded descriptor was incorrectly production eligible");
        };
        assert_eq!(unavailable_coordinates.len(), 2);
        assert_eq!(
            unavailable_coordinates[0].state,
            SelectionEvidenceStateRecordV1::Missing
        );
        assert_eq!(
            unavailable_coordinates[1].state,
            SelectionEvidenceStateRecordV1::BoundedInconclusive
        );
    }

    #[test]
    fn cache_policy_binds_final_fifteen_key_source_enumeration() {
        let config = CorpusBuildConfigV2::attempt_zero(19, 1);
        let keys = config.exact_candidate_keys().unwrap();
        let policy = OfflineCachePolicyRecordV1::current();
        assert_eq!(keys.len(), 15);
        assert_eq!(policy.build_config_schema_version, config.schema_version);
        assert_eq!(
            policy.source_capability_enumeration_policy_version,
            config.source_capability_policy_version
        );
    }

    #[test]
    fn room_checkpoint_is_create_new_hash_bound_and_corruption_fails() {
        let root = temp_directory("row");
        let (manifest, row) = sample_manifest_and_row();
        create_or_verify_offline_cache_manifest_v1(&root, &manifest).unwrap();
        assert!(matches!(
            write_or_verify_offline_cache_room_v1(&root, &manifest, &row).unwrap(),
            OfflineCacheRoomWriteOutcomeV1::Written(_)
        ));
        assert!(matches!(
            write_or_verify_offline_cache_room_v1(&root, &manifest, &row).unwrap(),
            OfflineCacheRoomWriteOutcomeV1::AlreadyVerified(_)
        ));
        complete_or_verify_offline_cache_v1(&root, &manifest).unwrap();
        load_verified_offline_cache_v1(&root).unwrap();

        let record_path = cache_room_directory(&root, &row.room_id).join(CACHE_ROOM_RECORD_FILE);
        let mut bytes = fs::read(&record_path).unwrap();
        let byte = bytes.iter_mut().find(|byte| **byte == b'1').unwrap();
        *byte = b'2';
        fs::write(record_path, bytes).unwrap();
        assert!(load_verified_offline_cache_v1(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_descriptor_fields_are_denied() {
        let room_id = RoomId("room-v3-wire-unknown".to_owned());
        let record = SelectionDescriptorRecordV1::from_descriptor(&sample_descriptor(&room_id));
        let mut value = serde_json::to_value(record).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<SelectionDescriptorRecordV1>(value).is_err());
    }

    #[test]
    fn histogram_wire_is_sorted_numeric_row_json_and_fails_closed() {
        let summary = TransparentReportSummaryV1 {
            source_version: 1,
            room_id: RoomId("room-v3-histogram-wire".to_owned()),
            coordinates: vec![NamedReportCoordinateV1 {
                name: "route-class-histogram".to_owned(),
                value: ReportValueV1::Histogram {
                    bins: vec![
                        HistogramBinV1 {
                            value: 1,
                            count: 14,
                        },
                        HistogramBinV1 { value: 2, count: 2 },
                        HistogramBinV1 { value: 7, count: 1 },
                    ],
                },
            }],
        };
        summary.validate("histogram regression").unwrap();
        let bytes = canonical_json_line(&summary).unwrap();
        let rendered = std::str::from_utf8(&bytes).unwrap();
        assert!(rendered.contains(
            r#""bins":[{"value":1,"count":14},{"value":2,"count":2},{"value":7,"count":1}]"#
        ));
        let restored: TransparentReportSummaryV1 =
            parse_one_json_line(&bytes, "histogram regression").unwrap();
        assert_eq!(restored, summary);
        restored.validate("histogram regression").unwrap();

        let mut unknown = serde_json::to_value(&summary).unwrap();
        unknown["coordinates"][0]["value"]["bins"][0]
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<TransparentReportSummaryV1>(unknown).is_err());

        let mut unsorted = summary.clone();
        let ReportValueV1::Histogram { bins } = &mut unsorted.coordinates[0].value else {
            unreachable!();
        };
        bins.swap(0, 1);
        assert!(unsorted.validate("histogram regression").is_err());

        let mut zero_count = summary;
        let ReportValueV1::Histogram { bins } = &mut zero_count.coordinates[0].value else {
            unreachable!();
        };
        bins[0].count = 0;
        assert!(zero_count.validate("histogram regression").is_err());
    }

    #[test]
    fn normalized_float_wire_is_lossless_and_byte_canonical() {
        let value = UnitIntervalFloatV1::from_value(0.117_491_532_558_185_05);
        assert!(value.validate());
        let summary = TransparentReportSummaryV1 {
            source_version: 1,
            room_id: RoomId("room-v3-normalized-wire".to_owned()),
            coordinates: vec![NamedReportCoordinateV1 {
                name: "normalized".to_owned(),
                value: ReportValueV1::NormalizedSamples {
                    samples: 1,
                    minimum: Some(value.clone()),
                    p10: Some(value.clone()),
                    median: Some(value.clone()),
                    p90: Some(value.clone()),
                    maximum: Some(value.clone()),
                    mean: Some(value),
                },
            }],
        };
        summary.validate("normalized regression").unwrap();
        let bytes = canonical_json_line(&summary).unwrap();
        let restored: TransparentReportSummaryV1 =
            parse_one_json_line(&bytes, "normalized regression").unwrap();
        assert_eq!(restored, summary);

        let mut corrupt = restored;
        let ReportValueV1::NormalizedSamples {
            minimum: Some(minimum),
            ..
        } = &mut corrupt.coordinates[0].value
        else {
            unreachable!();
        };
        minimum.decimal.push('0');
        assert!(corrupt.validate("normalized regression").is_err());
    }

    #[test]
    fn stale_policy_config_and_duplicate_room_identity_fail_closed() {
        let (manifest, row) = sample_manifest_and_row();

        let mut stale_policy = row.clone();
        stale_policy.policies.selection_metrics_version += 1;
        assert!(stale_policy.validate_against(&manifest).is_err());

        let mut stale_config = row.clone();
        stale_config.analysis_config.config_id.push_str("-stale");
        assert!(stale_config.validate_against(&manifest).is_err());

        let mut duplicate = manifest.clone();
        duplicate
            .expected_rooms
            .push(duplicate.expected_rooms[0].clone());
        assert!(duplicate.validate().is_err());

        let mut stale_shaky_config = manifest.clone();
        stale_shaky_config.shaky_hand_config.trials_per_curve_point += 1;
        stale_shaky_config.run_id = stale_shaky_config.recomputed_run_id().unwrap();
        assert!(stale_shaky_config.validate().is_err());

        let mut stale_shaky_report = row.clone();
        stale_shaky_report.shaky_hand.ai_policy_version += 1;
        assert!(stale_shaky_report.validate_against(&manifest).is_err());
    }

    #[test]
    fn selection_artifact_is_create_new_hash_bound_and_range_checked() {
        let root = temp_directory("selection");
        let (manifest, row) = sample_manifest_and_row();
        let artifact = sample_selection_artifact(&manifest, &row);
        artifact.validate().unwrap();
        write_new_offline_selection_artifact_v1(&root, &artifact).unwrap();
        assert_eq!(load_offline_selection_artifact_v1(&root).unwrap(), artifact);
        assert!(matches!(
            write_new_offline_selection_artifact_v1(&root, &artifact),
            Err(OfflineSelectionCacheError::AlreadyExists(_))
        ));

        let mut unsatisfied = artifact.clone();
        unsatisfied.selection_config.requested_minimum = 2;
        unsatisfied.selection_config.requested_maximum = 2;
        assert!(unsatisfied.validate().is_err());

        let record_path = root.join(SELECTION_RECORD_FILE);
        let mut bytes = fs::read(&record_path).unwrap();
        let byte = bytes.iter_mut().find(|byte| **byte == b'1').unwrap();
        *byte = b'2';
        fs::write(record_path, bytes).unwrap();
        assert!(load_offline_selection_artifact_v1(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn final_state_requires_exact_selected_descriptor_hashes() {
        let (manifest, row) = sample_manifest_and_row();
        let provisional = sample_selection_artifact(&manifest, &row);
        let expected = provisional.outcome.descriptor_hashes.clone();
        let final_artifact =
            OfflineSelectionArtifactV1::final_recomputed(&provisional, expected).unwrap();
        assert_eq!(
            final_artifact.publication_state,
            OfflineSelectionPublicationStateV1::FinalRecomputed
        );

        let mut forged = final_artifact;
        forged
            .recomputed_selected_descriptor_hashes
            .insert(row.room_id, "different".to_owned());
        assert!(forged.validate().is_err());
    }

    #[test]
    fn selected_room_recomputation_rejects_shaky_hand_report_mismatch() {
        let (manifest, row) = sample_manifest_and_row();
        let mut different = row.clone();
        different
            .shaky_hand
            .cells
            .push(DirectedLoadoutShakyHandCell {
                source_door_id: "door-a".to_owned(),
                target_door_id: "door-b".to_owned(),
                loadout: EvaluationLoadout::Baseline,
                evidence: CorpusShakyHandEvidence::NoPositiveCompleteFiniteVocabulary,
            });
        different.validate_against(&manifest).unwrap();
        assert!(require_exact_recomputed_cache_row_v1(&row, &different).is_err());
    }
}
