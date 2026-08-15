//! Domain metrics and hard gates for corpus quality-diversity selection.
//!
//! This module adapts exact generated geometry and [`RoomMetricSummary`]
//! evidence to the generic [`QualityDiversityArchive`].  The nine descriptor
//! projections remain independent; they are not concatenated into one sparse
//! mega-cell and their coordinates are never summed into a scalar score.
//!
//! Optional evidence is encoded with a one-hot state in Pareto quality and
//! farthest-point coordinates.  Consequently an observed value, a missing
//! value, a not-applicable value, and a bounded-inconclusive value cannot
//! silently compare as numeric zero.  Operational solver work appears only
//! in the Pareto quality vector and is not present in controller-demand or
//! route-diversity projections.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    error::Error,
    fmt,
    ops::RangeInclusive,
};

use downwards_core::DoorSocket;
use downwards_validation::BoundedTargetEvidence;

use super::extended_selection_metrics::{
    ExtendedSelectionMetricError, PICKUP_DETOUR_SELECTION_METRICS_VERSION,
    ROUTE_CHOICE_SELECTION_METRICS_VERSION, pickup_detour_selection_projection,
    route_choice_selection_projection, summarize_pickup_detour_selection_metrics,
    summarize_route_choice_selection_metrics,
};
use super::{
    ArchiveBuildSummary, ArchiveCandidate, ArchiveCellKey, ArchiveConfig, ArchiveError,
    CORPUS_ROOM_ANALYSIS_VERSION, CorpusCandidate, CorpusCandidateKeyRecord,
    CorpusMetricInputV2Error, CorpusRoomAnalysis, CorpusRoomAnalysisConfigError,
    CorpusRoomAnalysisConfigRecord, EvaluatedCorpusRoomV2, EvaluationLoadout, ExactFraction,
    GeneratedCorpusRoom, IntegerCoordinateDistribution, LandingCoordinateEvidence,
    LandingPrecisionAggregateMetric, LandingPrecisionMetricSummary, MetricEvidence,
    MissingMetricReason, NotApplicableMetricReason, ObjectiveDirection, ProjectedCell,
    QualityDiversityArchive, ROOM_METRIC_SUMMARY_VERSION, RoomEmbeddingPrefix, RoomId,
    RoomMetricSummary, RoomMetricSummaryError, SelectionPackage,
    SignedIntegerCoordinateDistribution, rerun_validate_evaluated_ability_promotions_v2,
    resolve_corpus_metric_candidate_v2, room_embedding_prefix, room_embedding_prefix_from_parts,
    summarize_room_metrics,
};

/// Version of the complete adapter, hard-gate, and deterministic selection
/// contract.
pub const CORPUS_SELECTION_METRICS_VERSION: u32 = 5;

/// Version of the final-path, generator-neutral identity and hard-gate
/// adapter. The nine projection schemas and the legacy adapter remain
/// independently versioned and unchanged.
pub const CORPUS_SELECTION_INPUT_V2_VERSION: u32 = 2;

/// Independent schema versions.  Bumping one projection does not silently
/// change the interpretation of any other projection.
pub const MORPHOLOGY_TOPOLOGY_PROJECTION_VERSION: u32 = 1;
pub const DIRECTED_LOADOUT_CONTROLLER_PROJECTION_VERSION: u32 = 2;
pub const LANDING_GEOMETRY_PROJECTION_VERSION: u32 = 1;
pub const DIRECTIONAL_ASYMMETRY_PROJECTION_VERSION: u32 = 2;
pub const ROUTE_DIVERSITY_PROJECTION_VERSION: u32 = 1;
pub const ABILITY_BYPASS_PROJECTION_VERSION: u32 = 1;
pub const TERRAIN_ABLATION_PROJECTION_VERSION: u32 = 1;
pub const OBSERVED_ROUTE_CHOICES_PROJECTION_VERSION: u32 = ROUTE_CHOICE_SELECTION_METRICS_VERSION;
pub const PICKUP_CHALLENGE_DETOUR_PROJECTION_VERSION: u32 = PICKUP_DETOUR_SELECTION_METRICS_VERSION;

/// The default production request.  A smaller range can be supplied to
/// focused experiments and tests without changing this policy constant.
pub const DEFAULT_CORPUS_SELECTION_MINIMUM: usize = 500;
pub const DEFAULT_CORPUS_SELECTION_MAXIMUM: usize = 1_000;

const CELL_BUCKET_COUNT: u32 = 4;
const DIVERSITY_BUDGET_PER_PROJECTION: i64 = 20_000;
const CONTROLLER_DURATION_CAP: usize = 720;
const CONTROLLER_COUNT_CAP: usize = 32;
const OPERATIONAL_TICK_CAP: usize = 100_000_000;
const LANDING_EVENT_COUNT_CAP: usize = 32;
const LANDING_FOOTPRINT_OVERLAP_CAP_PIXELS: usize = 8;
const LANDING_SUPPORT_WIDTH_CAP_PIXELS: usize = 320;
const LANDING_NEGATIVE_EDGE_MARGIN_CAP_PIXELS: u32 = 8;
const LANDING_POSITIVE_EDGE_MARGIN_CAP_PIXELS: u32 = 320;

/// Interpretation boundary for persisted selection output.
pub const CORPUS_SELECTION_METRICS_DISCLAIMER: &str = "callers must prefilter construction-loadout and complete-kit all-target positives before selection; selection uses nine independent projections and a Pareto vector, never a weighted fun score or difficulty band; missing, not-applicable, and bounded-inconclusive deep-metric evidence remain distinct; exact landing geometry is diversity evidence rather than input-window proof, and a route with no measured landing is never assigned a precision value; route-choice alternatives are compared only within exact directed-loadout cells; pickup target-directed-only and cross-loadout facts are finite positive/bounded evidence rather than mandatory, unreachable, or ability-requirement proofs, and mere pickup count is not rewarded; solver work is operational cost rather than player difficulty; exact static-visual uniqueness and selected-set cross-room socket-mate coverage are hard gates; uncorroborated terrain is a positive-evidence gap rather than proof of uselessness or unreachability";

/// Stable identity of an independently versioned archive projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CorpusSelectionProjectionId {
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

impl CorpusSelectionProjectionId {
    #[must_use]
    pub const fn schema_version(self) -> u32 {
        match self {
            Self::MorphologyTopologyV1 => MORPHOLOGY_TOPOLOGY_PROJECTION_VERSION,
            Self::DirectedLoadoutControllerDemandV1 => {
                DIRECTED_LOADOUT_CONTROLLER_PROJECTION_VERSION
            }
            Self::LandingGeometryV1 => LANDING_GEOMETRY_PROJECTION_VERSION,
            Self::DirectionalAsymmetryV1 => DIRECTIONAL_ASYMMETRY_PROJECTION_VERSION,
            Self::RouteDiversityV1 => ROUTE_DIVERSITY_PROJECTION_VERSION,
            Self::AbilityBypassStructureV1 => ABILITY_BYPASS_PROJECTION_VERSION,
            Self::TerrainAblationUtilityV1 => TERRAIN_ABLATION_PROJECTION_VERSION,
            Self::ObservedWithinCellRouteChoicesV1 => OBSERVED_ROUTE_CHOICES_PROJECTION_VERSION,
            Self::PickupChallengeDetourV1 => PICKUP_CHALLENGE_DETOUR_PROJECTION_VERSION,
        }
    }
}

/// Selection-layer availability state.  Bounded audit exhaustion is kept
/// separate from other missing evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SelectionEvidenceState {
    Observed,
    Missing,
    NotApplicable,
    BoundedInconclusive,
}

impl SelectionEvidenceState {
    const ALL: [Self; 4] = [
        Self::Observed,
        Self::Missing,
        Self::NotApplicable,
        Self::BoundedInconclusive,
    ];

    const fn cell_code(self) -> i64 {
        match self {
            Self::Observed => 0,
            Self::Missing => 1,
            Self::NotApplicable => 2,
            Self::BoundedInconclusive => 3,
        }
    }
}

/// One named, quantized value.  `value` is present exactly for observed
/// evidence and lies in `0..=65535`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuantizedSelectionEvidence {
    pub state: SelectionEvidenceState,
    pub value: Option<u16>,
}

impl QuantizedSelectionEvidence {
    #[must_use]
    pub const fn observed(value: u16) -> Self {
        Self {
            state: SelectionEvidenceState::Observed,
            value: Some(value),
        }
    }

    #[must_use]
    pub const fn missing() -> Self {
        Self {
            state: SelectionEvidenceState::Missing,
            value: None,
        }
    }

    #[must_use]
    pub const fn not_applicable() -> Self {
        Self {
            state: SelectionEvidenceState::NotApplicable,
            value: None,
        }
    }

    #[must_use]
    pub const fn bounded_inconclusive() -> Self {
        Self {
            state: SelectionEvidenceState::BoundedInconclusive,
            value: None,
        }
    }
}

/// A coordinate retained with a stable human-readable definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedSelectionCoordinate {
    pub name: String,
    pub evidence: QuantizedSelectionEvidence,
}

impl NamedSelectionCoordinate {
    fn observed(name: impl Into<String>, value: u16) -> Self {
        Self {
            name: name.into(),
            evidence: QuantizedSelectionEvidence::observed(value),
        }
    }

    fn new(name: impl Into<String>, evidence: QuantizedSelectionEvidence) -> Self {
        Self {
            name: name.into(),
            evidence,
        }
    }
}

/// Complete auditable coordinates for one independent projection.  `cell`
/// is deliberately smaller than `detail`; the latter supplies equal-budget
/// farthest-point coordinates without exploding archive cell dimensionality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionProjection {
    pub id: CorpusSelectionProjectionId,
    pub cell: Vec<NamedSelectionCoordinate>,
    pub detail: Vec<NamedSelectionCoordinate>,
}

/// Named semantic axis in the Pareto vector.  Each axis expands to four
/// one-hot evidence coordinates plus one directed numeric coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CorpusSelectionQualityAxis {
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

/// One semantic Pareto coordinate before its explicit evidence-state
/// expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionQualityCoordinate {
    pub axis: CorpusSelectionQualityAxis,
    pub direction: ObjectiveDirection,
    pub evidence: QuantizedSelectionEvidence,
}

/// Fully adapted evidence for one room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionDescriptor {
    pub version: u32,
    pub room_id: RoomId,
    pub projections: Vec<CorpusSelectionProjection>,
    pub quality: Vec<CorpusSelectionQualityCoordinate>,
    pub diversity_coordinates: Vec<i64>,
    pub sockets: Vec<DoorSocket>,
}

impl CorpusSelectionDescriptor {
    /// Convert the transparent domain record to the generic archive input.
    #[must_use]
    pub fn to_archive_candidate(&self) -> ArchiveCandidate<RoomId, CorpusSelectionProjectionId> {
        ArchiveCandidate::new(
            self.room_id.clone(),
            self.projections
                .iter()
                .map(|projection| ProjectedCell::new(projection.id, flatten_cell(&projection.cell)))
                .collect(),
            flatten_quality(&self.quality),
            self.diversity_coordinates.clone(),
        )
    }
}

/// Borrowed generated room and already-aggregated evidence offered to corpus
/// selection.  Callers control feasibility beyond this module's two hard
/// gates by deciding which rooms to offer. Production callers must prefilter
/// the construction-loadout and complete-kit all-target positive gates.
#[derive(Clone, Copy, Debug)]
pub struct CorpusSelectionRoom<'a> {
    pub generated: &'a GeneratedCorpusRoom,
    pub metrics: &'a RoomMetricSummary,
}

impl<'a> CorpusSelectionRoom<'a> {
    #[must_use]
    pub const fn new(generated: &'a GeneratedCorpusRoom, metrics: &'a RoomMetricSummary) -> Self {
        Self { generated, metrics }
    }
}

/// Borrowed generated room and unsummarized deep analysis offered to the
/// convenience batch adapter.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzedCorpusSelectionRoom<'a> {
    pub generated: &'a GeneratedCorpusRoom,
    pub analysis: &'a CorpusRoomAnalysis,
}

impl<'a> AnalyzedCorpusSelectionRoom<'a> {
    #[must_use]
    pub const fn new(generated: &'a GeneratedCorpusRoom, analysis: &'a CorpusRoomAnalysis) -> Self {
        Self {
            generated,
            analysis,
        }
    }
}

/// Borrowed room plus both optional deep passes required by the nine-
/// projection adapter.  Unlike the legacy adapter, this type cannot silently
/// omit route-choice or pickup-detour evidence.
#[derive(Clone, Copy, Debug)]
pub struct ExtendedCorpusSelectionRoom<'a> {
    pub generated: &'a GeneratedCorpusRoom,
    pub metrics: &'a RoomMetricSummary,
    pub route_choices: &'a super::RoomRouteChoiceDiversity,
    pub pickup_detours: &'a super::RoomPickupDetourAnalysis,
}

impl<'a> ExtendedCorpusSelectionRoom<'a> {
    #[must_use]
    pub const fn new(
        generated: &'a GeneratedCorpusRoom,
        metrics: &'a RoomMetricSummary,
        route_choices: &'a super::RoomRouteChoiceDiversity,
        pickup_detours: &'a super::RoomPickupDetourAnalysis,
    ) -> Self {
        Self {
            generated,
            metrics,
            route_choices,
            pickup_detours,
        }
    }
}

/// One final-path room offered to selection with already-summarized metrics.
///
/// The selected native candidate is resolved from `evaluated` only after its
/// exact room-v3 identity, canonical construction gate, and complete-kit gate
/// pass. Optional deep reports remain explicit Missing evidence when absent;
/// they are never replaced by numeric zero or a positive claim.
#[derive(Clone, Copy, Debug)]
pub struct CorpusSelectionRoomV2<'a> {
    pub evaluated: &'a EvaluatedCorpusRoomV2,
    pub metrics: &'a RoomMetricSummary,
    pub route_choices: Option<&'a super::RoomRouteChoiceDiversity>,
    pub pickup_detours: Option<&'a super::RoomPickupDetourAnalysis>,
}

impl<'a> CorpusSelectionRoomV2<'a> {
    #[must_use]
    pub const fn new(evaluated: &'a EvaluatedCorpusRoomV2, metrics: &'a RoomMetricSummary) -> Self {
        Self {
            evaluated,
            metrics,
            route_choices: None,
            pickup_detours: None,
        }
    }

    #[must_use]
    pub const fn with_optional_deep_reports(
        mut self,
        route_choices: Option<&'a super::RoomRouteChoiceDiversity>,
        pickup_detours: Option<&'a super::RoomPickupDetourAnalysis>,
    ) -> Self {
        self.route_choices = route_choices;
        self.pickup_detours = pickup_detours;
        self
    }

    #[must_use]
    pub const fn with_deep_reports(
        self,
        route_choices: &'a super::RoomRouteChoiceDiversity,
        pickup_detours: &'a super::RoomPickupDetourAnalysis,
    ) -> Self {
        self.with_optional_deep_reports(Some(route_choices), Some(pickup_detours))
    }
}

/// Final-path selection input when the caller retains unsummarized room
/// analysis. Summarization is deterministic and occurs before archive build.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzedCorpusSelectionRoomV2<'a> {
    pub evaluated: &'a EvaluatedCorpusRoomV2,
    pub analysis: &'a CorpusRoomAnalysis,
    pub route_choices: Option<&'a super::RoomRouteChoiceDiversity>,
    pub pickup_detours: Option<&'a super::RoomPickupDetourAnalysis>,
}

impl<'a> AnalyzedCorpusSelectionRoomV2<'a> {
    #[must_use]
    pub const fn new(
        evaluated: &'a EvaluatedCorpusRoomV2,
        analysis: &'a CorpusRoomAnalysis,
    ) -> Self {
        Self {
            evaluated,
            analysis,
            route_choices: None,
            pickup_detours: None,
        }
    }

    #[must_use]
    pub const fn with_optional_deep_reports(
        mut self,
        route_choices: Option<&'a super::RoomRouteChoiceDiversity>,
        pickup_detours: Option<&'a super::RoomPickupDetourAnalysis>,
    ) -> Self {
        self.route_choices = route_choices;
        self.pickup_detours = pickup_detours;
        self
    }

    #[must_use]
    pub const fn with_deep_reports(
        self,
        route_choices: &'a super::RoomRouteChoiceDiversity,
        pickup_detours: &'a super::RoomPickupDetourAnalysis,
    ) -> Self {
        self.with_optional_deep_reports(Some(route_choices), Some(pickup_detours))
    }
}

/// Archive and output-size policy.  The default is the requested production
/// corpus range; custom ranges exist for pilots and focused tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpusSelectionConfig {
    pub requested_minimum: usize,
    pub requested_maximum: usize,
    pub elites_per_cell: usize,
}

impl CorpusSelectionConfig {
    #[must_use]
    pub const fn new(
        requested_minimum: usize,
        requested_maximum: usize,
        elites_per_cell: usize,
    ) -> Self {
        Self {
            requested_minimum,
            requested_maximum,
            elites_per_cell,
        }
    }

    #[must_use]
    pub fn requested_range(self) -> RangeInclusive<usize> {
        self.requested_minimum..=self.requested_maximum
    }
}

impl Default for CorpusSelectionConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_CORPUS_SELECTION_MINIMUM,
            DEFAULT_CORPUS_SELECTION_MAXIMUM,
            32,
        )
    }
}

/// One independently reusable-socket-covered component of the selected set.
/// Components are stable and disjoint; every socket on every member has a
/// mate on another member of the same package. This is definition-reuse
/// coverage, not the optional stronger one-copy multiplicity balance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReusableSocketSelectionPackage {
    pub package_id: RoomId,
    pub room_ids: Vec<RoomId>,
}

/// Why one room was removed while preserving the socket-mate hard gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketCoveragePruningStep {
    pub requested_removal: RoomId,
    /// Includes the requested room and any deterministic cascade of rooms
    /// whose socket coverage became impossible after that removal.
    pub removed_room_ids: Vec<RoomId>,
    pub remaining_rooms: usize,
}

/// Marginal-coverage and farthest-point rationale recomputed in final
/// selection order, after socket-safe pruning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionStep {
    pub room_id: RoomId,
    pub marginal_cell_coverage: usize,
    pub minimum_l1_distance: Option<u128>,
}

/// Transparent selection audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionAudit {
    pub version: u32,
    pub submitted_rooms: usize,
    pub exact_visual_unique_rooms: usize,
    pub archive: ArchiveBuildSummary,
    pub archive_ranked_rooms: usize,
    pub rooms_excluded_by_initial_socket_core: Vec<RoomId>,
    pub socket_pruning_steps: Vec<SocketCoveragePruningStep>,
    pub selected_rooms: usize,
    pub selected_package_count: usize,
    pub covered_cells: Vec<ArchiveCellKey<CorpusSelectionProjectionId>>,
    pub steps: Vec<CorpusSelectionStep>,
}

/// Complete archive plus deterministic, hard-gated result.
///
/// This outcome certifies exact visual uniqueness and reusable socket-mate
/// coverage. It assumes, but does not itself certify, that its inputs were
/// prefiltered by the construction-loadout and complete-kit route gates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusSelectionOutcome {
    pub archive: QualityDiversityArchive<RoomId, CorpusSelectionProjectionId>,
    pub descriptors: BTreeMap<RoomId, CorpusSelectionDescriptor>,
    pub selected_room_ids: Vec<RoomId>,
    pub socket_packages: Vec<ReusableSocketSelectionPackage>,
    pub audit: CorpusSelectionAudit,
}

/// Deep report whose absence makes a production v2 archive incomplete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionDeepReport {
    RouteChoices,
    PickupDetours,
}

impl ProductionDeepReport {
    const fn slug(self) -> &'static str {
        match self {
            Self::RouteChoices => "route choices",
            Self::PickupDetours => "pickup detours",
        }
    }
}

/// Invalid evidence or an unsatisfied hard selection gate.
#[derive(Debug)]
pub enum CorpusSelectionMetricsError {
    InvalidRequestedRange {
        minimum: usize,
        maximum: usize,
    },
    ZeroElitesPerCell,
    DuplicateRoomId {
        room_id: RoomId,
    },
    RoomMetricIdentityMismatch {
        generated_room_id: RoomId,
        metric_room_id: RoomId,
    },
    RoomMetricVersion {
        room_id: RoomId,
        expected: u32,
        actual: u32,
    },
    RoomMetricAnalysisVersion {
        room_id: RoomId,
        expected: u32,
        actual: u32,
    },
    RoomMetricAnalysisConfig {
        room_id: RoomId,
        source: CorpusRoomAnalysisConfigError,
    },
    RouteChoiceIdentityMismatch {
        generated_room_id: RoomId,
        route_choice_room_id: RoomId,
    },
    RouteChoiceAnalysisVersionMismatch {
        room_id: RoomId,
        metric_version: u32,
        route_choice_version: u32,
    },
    RouteChoiceAnalysisConfigMismatch {
        room_id: RoomId,
        metric_config_id: String,
        route_choice_config_id: String,
    },
    PickupDetourIdentityMismatch {
        generated_room_id: RoomId,
        pickup_detour_room_id: RoomId,
    },
    FinalPathIdentity {
        room_id: RoomId,
        source: CorpusMetricInputV2Error,
    },
    FinalPathCandidateMismatch {
        room_id: RoomId,
        expected_key: Box<CorpusCandidateKeyRecord>,
        supplied_key: Box<CorpusCandidateKeyRecord>,
    },
    FinalPathEvidenceContract {
        room_id: RoomId,
        evidence: &'static str,
        detail: String,
    },
    MissingProductionDeepReport {
        room_id: RoomId,
        report: ProductionDeepReport,
    },
    ProductionEvidenceUnavailable {
        room_id: RoomId,
        coordinate: String,
        state: SelectionEvidenceState,
    },
    MixedAnalysisConfigs {
        first_room_id: RoomId,
        first_config_id: String,
        room_id: RoomId,
        config_id: String,
    },
    DuplicateExactStaticVisual {
        first_room_id: RoomId,
        duplicate_room_id: RoomId,
    },
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    StructuralDescriptor {
        room_id: RoomId,
        detail: String,
    },
    MetricSummary {
        room_id: RoomId,
        source: RoomMetricSummaryError,
    },
    ExtendedMetricSummary {
        room_id: RoomId,
        source: ExtendedSelectionMetricError,
    },
    Archive(ArchiveError),
    RequestedMinimumUnavailable {
        minimum: usize,
        maximum: usize,
        available: usize,
    },
    SocketCoverageInvariant {
        detail: String,
    },
}

impl fmt::Display for CorpusSelectionMetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestedRange { minimum, maximum } => write!(
                formatter,
                "corpus selection range must satisfy 0 < minimum <= maximum, got {minimum}..={maximum}"
            ),
            Self::ZeroElitesPerCell => {
                formatter.write_str("corpus selection requires at least one elite per cell")
            }
            Self::DuplicateRoomId { room_id } => {
                write!(
                    formatter,
                    "duplicate corpus selection room ID {}",
                    room_id.0
                )
            }
            Self::RoomMetricIdentityMismatch {
                generated_room_id,
                metric_room_id,
            } => write!(
                formatter,
                "generated room {} was paired with metrics for {}",
                generated_room_id.0, metric_room_id.0
            ),
            Self::RoomMetricVersion {
                room_id,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} has metric schema version {actual}; expected {expected}",
                room_id.0
            ),
            Self::RoomMetricAnalysisVersion {
                room_id,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} metrics cite analysis version {actual}; expected {expected}",
                room_id.0
            ),
            Self::RoomMetricAnalysisConfig { room_id, source } => write!(
                formatter,
                "room {} metrics have an invalid analysis-config identity: {source}",
                room_id.0
            ),
            Self::RouteChoiceIdentityMismatch {
                generated_room_id,
                route_choice_room_id,
            } => write!(
                formatter,
                "generated room {} was paired with route-choice evidence for {}",
                generated_room_id.0, route_choice_room_id.0
            ),
            Self::RouteChoiceAnalysisVersionMismatch {
                room_id,
                metric_version,
                route_choice_version,
            } => write!(
                formatter,
                "room {} route-choice evidence cites analysis version {route_choice_version}, but metrics cite {metric_version}",
                room_id.0
            ),
            Self::RouteChoiceAnalysisConfigMismatch {
                room_id,
                metric_config_id,
                route_choice_config_id,
            } => write!(
                formatter,
                "room {} route-choice config {route_choice_config_id:?} differs from metric config {metric_config_id:?}",
                room_id.0
            ),
            Self::PickupDetourIdentityMismatch {
                generated_room_id,
                pickup_detour_room_id,
            } => write!(
                formatter,
                "generated room {} was paired with pickup-detour evidence for {}",
                generated_room_id.0, pickup_detour_room_id.0
            ),
            Self::FinalPathIdentity { room_id, source } => write!(
                formatter,
                "corpus-v2 room {} failed exact identity or feasibility validation: {source}",
                room_id.0
            ),
            Self::FinalPathCandidateMismatch {
                room_id,
                expected_key,
                supplied_key,
            } => write!(
                formatter,
                "corpus-v2 room {} requires canonical candidate {}, but candidate {} was supplied",
                room_id.0,
                expected_key.stable_slug(),
                supplied_key.stable_slug()
            ),
            Self::FinalPathEvidenceContract {
                room_id,
                evidence,
                detail,
            } => write!(
                formatter,
                "room {} {evidence} evidence violates the final-path denominator contract: {detail}",
                room_id.0
            ),
            Self::MissingProductionDeepReport { room_id, report } => write!(
                formatter,
                "production corpus-v2 selection for room {} requires the {} report",
                room_id.0,
                report.slug()
            ),
            Self::ProductionEvidenceUnavailable {
                room_id,
                coordinate,
                state,
            } => write!(
                formatter,
                "production corpus-v2 selection for room {} cannot use {state:?} evidence at {coordinate}",
                room_id.0
            ),
            Self::MixedAnalysisConfigs {
                first_room_id,
                first_config_id,
                room_id,
                config_id,
            } => write!(
                formatter,
                "corpus-v2 batch mixes analysis config {first_config_id:?} from room {} with {config_id:?} from room {}",
                first_room_id.0, room_id.0
            ),
            Self::DuplicateExactStaticVisual {
                first_room_id,
                duplicate_room_id,
            } => write!(
                formatter,
                "exact static-visual uniqueness gate rejected rooms {} and {}",
                first_room_id.0, duplicate_room_id.0
            ),
            Self::MissingCanonicalVariant { room_id } => write!(
                formatter,
                "corpus selection room {} has no canonical generated variant",
                room_id.0
            ),
            Self::StructuralDescriptor { room_id, detail } => write!(
                formatter,
                "could not derive selection descriptor for room {}: {detail}",
                room_id.0
            ),
            Self::MetricSummary { room_id, source } => write!(
                formatter,
                "could not summarize analysis for room {}: {source}",
                room_id.0
            ),
            Self::ExtendedMetricSummary { room_id, source } => write!(
                formatter,
                "could not summarize extended selection evidence for room {}: {source}",
                room_id.0
            ),
            Self::Archive(source) => {
                write!(formatter, "quality-diversity archive failed: {source}")
            }
            Self::RequestedMinimumUnavailable {
                minimum,
                maximum,
                available,
            } => write!(
                formatter,
                "only {available} exact-visual-unique, archived, socket-covered rooms remain for requested range {minimum}..={maximum}"
            ),
            Self::SocketCoverageInvariant { detail } => {
                write!(formatter, "socket-mate coverage invariant failed: {detail}")
            }
        }
    }
}

impl Error for CorpusSelectionMetricsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MetricSummary { source, .. } => Some(source),
            Self::RoomMetricAnalysisConfig { source, .. } => Some(source),
            Self::ExtendedMetricSummary { source, .. } => Some(source),
            Self::FinalPathIdentity { source, .. } => Some(source),
            Self::Archive(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ArchiveError> for CorpusSelectionMetricsError {
    fn from(value: ArchiveError) -> Self {
        Self::Archive(value)
    }
}

/// Adapt one generated room plus its transparent aggregate metrics.
pub fn describe_corpus_selection_room(
    generated: &GeneratedCorpusRoom,
    metrics: &RoomMetricSummary,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    describe_corpus_selection_room_from_summaries(generated, metrics, None, None)
}

/// Adapt one room with required identity-matched route-choice and pickup-
/// detour evidence.  Legacy callers use [`describe_corpus_selection_room`]
/// and receive explicit Missing coordinates for these two deep projections.
pub fn describe_extended_corpus_selection_room(
    generated: &GeneratedCorpusRoom,
    metrics: &RoomMetricSummary,
    route_choices: &super::RoomRouteChoiceDiversity,
    pickup_detours: &super::RoomPickupDetourAnalysis,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    if generated.id != route_choices.room_id {
        return Err(CorpusSelectionMetricsError::RouteChoiceIdentityMismatch {
            generated_room_id: generated.id.clone(),
            route_choice_room_id: route_choices.room_id.clone(),
        });
    }
    if generated.id != pickup_detours.room_id {
        return Err(CorpusSelectionMetricsError::PickupDetourIdentityMismatch {
            generated_room_id: generated.id.clone(),
            pickup_detour_room_id: pickup_detours.room_id.clone(),
        });
    }
    let route_choices =
        summarize_route_choice_selection_metrics(route_choices).map_err(|source| {
            CorpusSelectionMetricsError::ExtendedMetricSummary {
                room_id: generated.id.clone(),
                source,
            }
        })?;
    let pickup_detours =
        summarize_pickup_detour_selection_metrics(pickup_detours).map_err(|source| {
            CorpusSelectionMetricsError::ExtendedMetricSummary {
                room_id: generated.id.clone(),
                source,
            }
        })?;
    describe_corpus_selection_room_from_summaries(
        generated,
        metrics,
        Some(&route_choices),
        Some(&pickup_detours),
    )
}

fn describe_corpus_selection_room_from_summaries(
    generated: &GeneratedCorpusRoom,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RouteChoiceSelectionMetricSummary>,
    pickup_detours: Option<&super::PickupDetourSelectionMetricSummary>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    if generated.id != metrics.room_id {
        return Err(CorpusSelectionMetricsError::RoomMetricIdentityMismatch {
            generated_room_id: generated.id.clone(),
            metric_room_id: metrics.room_id.clone(),
        });
    }
    let Some(canonical) = generated.variants.first() else {
        return Err(CorpusSelectionMetricsError::MissingCanonicalVariant {
            room_id: generated.id.clone(),
        });
    };
    let prefix = room_embedding_prefix(canonical).map_err(|source| {
        CorpusSelectionMetricsError::StructuralDescriptor {
            room_id: generated.id.clone(),
            detail: source.to_string(),
        }
    })?;
    let sockets = canonical
        .generated
        .room
        .doors()
        .iter()
        .map(|door| door.socket())
        .collect();

    describe_corpus_selection_evidence(
        &generated.id,
        metrics,
        prefix,
        sockets,
        route_choices,
        pickup_detours,
    )
}

fn describe_corpus_selection_evidence(
    room_id: &RoomId,
    metrics: &RoomMetricSummary,
    prefix: RoomEmbeddingPrefix,
    sockets: Vec<DoorSocket>,
    route_choices: Option<&super::RouteChoiceSelectionMetricSummary>,
    pickup_detours: Option<&super::PickupDetourSelectionMetricSummary>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    if *room_id != metrics.room_id {
        return Err(CorpusSelectionMetricsError::RoomMetricIdentityMismatch {
            generated_room_id: room_id.clone(),
            metric_room_id: metrics.room_id.clone(),
        });
    }

    let route_choices = route_choice_selection_projection(route_choices);
    let pickup_detours = pickup_detour_selection_projection(pickup_detours);
    let projections = vec![
        morphology_topology_projection(&prefix),
        directed_controller_projection(metrics),
        landing_geometry_projection(&metrics.landing_precision),
        directional_asymmetry_projection(metrics),
        route_diversity_projection(metrics),
        ability_bypass_projection(metrics),
        terrain_ablation_projection(metrics),
        CorpusSelectionProjection {
            id: CorpusSelectionProjectionId::ObservedWithinCellRouteChoicesV1,
            cell: route_choices.cell,
            detail: route_choices.detail,
        },
        CorpusSelectionProjection {
            id: CorpusSelectionProjectionId::PickupChallengeDetourV1,
            cell: pickup_detours.cell,
            detail: pickup_detours.detail,
        },
    ];
    let quality = quality_coordinates(metrics);
    let diversity_coordinates = flatten_diversity(&projections);

    Ok(CorpusSelectionDescriptor {
        version: CORPUS_SELECTION_METRICS_VERSION,
        room_id: room_id.clone(),
        projections,
        quality,
        diversity_coordinates,
        sockets,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CorpusV2DescriptorPolicy {
    ProductionAuthoritative,
    Exploratory,
}

/// Adapt a final-path evaluated room for production-authoritative selection.
///
/// Both deep reports are mandatory and every coordinate which drives archive
/// coverage, distance, or Pareto quality must be observed or honestly
/// not-applicable. Missing and bounded evidence fail before archive creation.
pub fn describe_corpus_selection_room_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    describe_corpus_selection_room_v2_with_policy(
        evaluated,
        metrics,
        route_choices,
        pickup_detours,
        CorpusV2DescriptorPolicy::ProductionAuthoritative,
    )
}

/// Adapt a final-path room for an explicitly exploratory archive.
///
/// Unlike [`describe_corpus_selection_room_v2`], this retains Missing and
/// BoundedInconclusive as typed coordinates. It must not be used to construct
/// a production corpus.
pub fn describe_corpus_selection_room_v2_exploratory(
    evaluated: &EvaluatedCorpusRoomV2,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    describe_corpus_selection_room_v2_with_policy(
        evaluated,
        metrics,
        route_choices,
        pickup_detours,
        CorpusV2DescriptorPolicy::Exploratory,
    )
}

fn describe_corpus_selection_room_v2_with_policy(
    evaluated: &EvaluatedCorpusRoomV2,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
    policy: CorpusV2DescriptorPolicy,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative {
        rerun_validate_evaluated_ability_promotions_v2(evaluated).map_err(|source| {
            CorpusSelectionMetricsError::FinalPathIdentity {
                room_id: evaluated.generated.id.clone(),
                source,
            }
        })?;
    }
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        CorpusSelectionMetricsError::FinalPathIdentity {
            room_id: evaluated.generated.id.clone(),
            source,
        }
    })?;
    describe_validated_corpus_selection_candidate_v2(
        evaluated,
        canonical,
        metrics,
        route_choices,
        pickup_detours,
        policy,
    )
}

/// Adapt a caller-supplied canonical native candidate.
///
/// The candidate is accepted only when it is exactly equal to the retained,
/// regenerated candidate selected by the recorded final-path policy. This is
/// useful for checkpoint readers that already materialized the candidate and
/// does not weaken any room-v3 or feasibility validation.
pub fn describe_corpus_selection_candidate_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    describe_corpus_selection_candidate_v2_with_policy(
        evaluated,
        candidate,
        metrics,
        route_choices,
        pickup_detours,
        CorpusV2DescriptorPolicy::ProductionAuthoritative,
    )
}

/// Explicit-candidate form of
/// [`describe_corpus_selection_room_v2_exploratory`].
pub fn describe_corpus_selection_candidate_v2_exploratory(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    describe_corpus_selection_candidate_v2_with_policy(
        evaluated,
        candidate,
        metrics,
        route_choices,
        pickup_detours,
        CorpusV2DescriptorPolicy::Exploratory,
    )
}

fn describe_corpus_selection_candidate_v2_with_policy(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
    policy: CorpusV2DescriptorPolicy,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative {
        rerun_validate_evaluated_ability_promotions_v2(evaluated).map_err(|source| {
            CorpusSelectionMetricsError::FinalPathIdentity {
                room_id: evaluated.generated.id.clone(),
                source,
            }
        })?;
    }
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        CorpusSelectionMetricsError::FinalPathIdentity {
            room_id: evaluated.generated.id.clone(),
            source,
        }
    })?;
    if canonical != candidate {
        return Err(CorpusSelectionMetricsError::FinalPathCandidateMismatch {
            room_id: evaluated.generated.id.clone(),
            expected_key: Box::new(canonical.exact_key()),
            supplied_key: Box::new(candidate.exact_key()),
        });
    }
    describe_validated_corpus_selection_candidate_v2(
        evaluated,
        candidate,
        metrics,
        route_choices,
        pickup_detours,
        policy,
    )
}

fn describe_validated_corpus_selection_candidate_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    metrics: &RoomMetricSummary,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
    policy: CorpusV2DescriptorPolicy,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    let room_id = &evaluated.generated.id;
    if *room_id != metrics.room_id {
        return Err(CorpusSelectionMetricsError::RoomMetricIdentityMismatch {
            generated_room_id: room_id.clone(),
            metric_room_id: metrics.room_id.clone(),
        });
    }
    validate_room_metric_provenance(room_id, metrics)?;
    if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative {
        if route_choices.is_none() {
            return Err(CorpusSelectionMetricsError::MissingProductionDeepReport {
                room_id: room_id.clone(),
                report: ProductionDeepReport::RouteChoices,
            });
        }
        if pickup_detours.is_none() {
            return Err(CorpusSelectionMetricsError::MissingProductionDeepReport {
                room_id: room_id.clone(),
                report: ProductionDeepReport::PickupDetours,
            });
        }
    }
    validate_final_path_room_metric_contract(evaluated, candidate, metrics, policy)?;
    let route_choice_summary = route_choices
        .map(|report| {
            if report.room_id != *room_id {
                return Err(CorpusSelectionMetricsError::RouteChoiceIdentityMismatch {
                    generated_room_id: room_id.clone(),
                    route_choice_room_id: report.room_id.clone(),
                });
            }
            if report.source_analysis_version != metrics.source_analysis_version {
                return Err(
                    CorpusSelectionMetricsError::RouteChoiceAnalysisVersionMismatch {
                        room_id: room_id.clone(),
                        metric_version: metrics.source_analysis_version,
                        route_choice_version: report.source_analysis_version,
                    },
                );
            }
            report.source_analysis_config.validate().map_err(|source| {
                CorpusSelectionMetricsError::RoomMetricAnalysisConfig {
                    room_id: room_id.clone(),
                    source,
                }
            })?;
            if report.source_analysis_config != metrics.source_analysis_config {
                return Err(
                    CorpusSelectionMetricsError::RouteChoiceAnalysisConfigMismatch {
                        room_id: room_id.clone(),
                        metric_config_id: metrics.source_analysis_config.config_id.clone(),
                        route_choice_config_id: report.source_analysis_config.config_id.clone(),
                    },
                );
            }
            validate_final_path_route_choice_contract(evaluated, metrics, report, policy)?;
            summarize_route_choice_selection_metrics(report).map_err(|source| {
                CorpusSelectionMetricsError::ExtendedMetricSummary {
                    room_id: room_id.clone(),
                    source,
                }
            })
        })
        .transpose()?;
    let pickup_detour_summary = pickup_detours
        .map(|report| {
            if report.room_id != *room_id {
                return Err(CorpusSelectionMetricsError::PickupDetourIdentityMismatch {
                    generated_room_id: room_id.clone(),
                    pickup_detour_room_id: report.room_id.clone(),
                });
            }
            validate_final_path_pickup_detour_contract(evaluated, candidate, report)?;
            summarize_pickup_detour_selection_metrics(report).map_err(|source| {
                CorpusSelectionMetricsError::ExtendedMetricSummary {
                    room_id: room_id.clone(),
                    source,
                }
            })
        })
        .transpose()?;
    let prefix = room_embedding_prefix_from_parts(
        candidate.generated(),
        candidate.route_plan(),
        candidate.route_plan_summary(),
    )
    .map_err(|source| CorpusSelectionMetricsError::StructuralDescriptor {
        room_id: room_id.clone(),
        detail: source.to_string(),
    })?;
    // Native boundary ports, not inferred exit metadata, define the tiling
    // contract used by socket closure.
    let sockets = candidate
        .boundary_ports()
        .iter()
        .map(|port| port.door.socket())
        .collect();

    let descriptor = describe_corpus_selection_evidence(
        room_id,
        metrics,
        prefix,
        sockets,
        route_choice_summary.as_ref(),
        pickup_detour_summary.as_ref(),
    )?;
    if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative {
        validate_production_descriptor_evidence(&descriptor)?;
    }
    Ok(descriptor)
}

fn validate_room_metric_provenance(
    room_id: &RoomId,
    metrics: &RoomMetricSummary,
) -> Result<(), CorpusSelectionMetricsError> {
    if metrics.version != ROOM_METRIC_SUMMARY_VERSION {
        return Err(CorpusSelectionMetricsError::RoomMetricVersion {
            room_id: room_id.clone(),
            expected: ROOM_METRIC_SUMMARY_VERSION,
            actual: metrics.version,
        });
    }
    if metrics.source_analysis_version != CORPUS_ROOM_ANALYSIS_VERSION {
        return Err(CorpusSelectionMetricsError::RoomMetricAnalysisVersion {
            room_id: room_id.clone(),
            expected: CORPUS_ROOM_ANALYSIS_VERSION,
            actual: metrics.source_analysis_version,
        });
    }
    metrics.source_analysis_config.validate().map_err(|source| {
        CorpusSelectionMetricsError::RoomMetricAnalysisConfig {
            room_id: room_id.clone(),
            source,
        }
    })
}

fn final_path_evidence_error(
    room_id: &RoomId,
    evidence: &'static str,
    detail: impl Into<String>,
) -> CorpusSelectionMetricsError {
    CorpusSelectionMetricsError::FinalPathEvidenceContract {
        room_id: room_id.clone(),
        evidence,
        detail: detail.into(),
    }
}

fn canonical_room_coordinates(
    candidate: &CorpusCandidate,
) -> (Vec<String>, Vec<String>, Vec<(String, String)>) {
    let mut door_ids = candidate
        .generated()
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    let mut pickup_ids = candidate
        .generated()
        .room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect::<Vec<_>>();
    pickup_ids.sort_unstable();
    let directed_routes = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect();
    (door_ids, pickup_ids, directed_routes)
}

fn validate_final_path_room_metric_contract(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    metrics: &RoomMetricSummary,
    policy: CorpusV2DescriptorPolicy,
) -> Result<(), CorpusSelectionMetricsError> {
    let room_id = &evaluated.generated.id;
    let (door_ids, _pickup_ids, directed_routes) = canonical_room_coordinates(candidate);
    let route_count = directed_routes.len();
    let loadout_route_count = route_count.saturating_mul(EvaluationLoadout::ALL.len());
    let canonical = &metrics.canonical_routes;

    if canonical.directed_route_count != route_count
        || canonical.loadout_route_cell_count != loadout_route_count
        || canonical.directed_loadout_routes.len() != loadout_route_count
        || canonical.by_loadout.len() != EvaluationLoadout::ALL.len()
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            format!(
                "canonical route denominators are not exactly {route_count} directed routes by four loadouts"
            ),
        ));
    }

    let mut positive_total = 0usize;
    let mut metric_rows = canonical.directed_loadout_routes.iter();
    for (loadout_index, loadout) in EvaluationLoadout::ALL.into_iter().enumerate() {
        let matrix = &evaluated.matrices[loadout_index];
        let positive_count = matrix.summary.positive_door_rows;
        positive_total = positive_total.saturating_add(positive_count);
        let loadout_summary = &canonical.by_loadout[loadout_index];
        if loadout_summary.loadout != loadout
            || loadout_summary.route_cell_count != route_count
            || loadout_summary.positive_route_count != positive_count
            || loadout_summary.bounded_inconclusive_route_count
                != route_count.saturating_sub(positive_count)
            || loadout_summary.behavior_diversity.route_count != positive_count
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "{} canonical loadout aggregate is not bound to its evaluated matrix",
                    loadout.slug()
                ),
            ));
        }
        for ((expected_source, expected_target), matrix_row) in
            directed_routes.iter().zip(matrix.evidence.door_routes())
        {
            let metric_row = metric_rows
                .next()
                .expect("canonical metric cardinality was checked");
            if metric_row.loadout != loadout
                || metric_row.source_door_id != *expected_source
                || metric_row.target_door_id != *expected_target
                || matrix_row.source_door_id != *expected_source
                || matrix_row.target_door_id != *expected_target
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "room-metric",
                    format!(
                        "canonical rows do not use exact {} {:?}->{:?} coordinates",
                        loadout.slug(),
                        expected_source,
                        expected_target
                    ),
                ));
            }
            let state_matches = match (&metric_row.evidence, &matrix_row.evidence) {
                (
                    super::CanonicalRouteMetricEvidence::Positive(metric_positive),
                    BoundedTargetEvidence::Positive(matrix_positive),
                ) => metric_positive.witness_fingerprint == matrix_positive.witness_fingerprint(),
                (
                    super::CanonicalRouteMetricEvidence::BoundedInconclusiveReasonNotRetained,
                    BoundedTargetEvidence::Inconclusive(_),
                ) => true,
                _ => false,
            };
            if !state_matches {
                return Err(final_path_evidence_error(
                    room_id,
                    "room-metric",
                    format!(
                        "canonical {} {:?}->{:?} state/fingerprint differs from the evaluated matrix",
                        loadout.slug(),
                        expected_source,
                        expected_target
                    ),
                ));
            }
        }
    }
    if canonical.positive_route_count != positive_total
        || canonical.bounded_inconclusive_route_count
            != loadout_route_count.saturating_sub(positive_total)
        || canonical.behavior_diversity.route_count != positive_total
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "room-wide canonical counts do not partition the evaluated matrix",
        ));
    }

    if metrics
        .landing_precision
        .by_loadout
        .iter()
        .map(|summary| summary.loadout)
        .ne(EvaluationLoadout::ALL)
        || metrics
            .landing_precision
            .aggregate
            .canonical_positive_route_count
            != positive_total
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "landing denominators are not the exact four canonical matrix loadouts",
        ));
    }
    for (summary, matrix) in metrics
        .landing_precision
        .by_loadout
        .iter()
        .zip(&evaluated.matrices)
    {
        if summary.aggregate.canonical_positive_route_count != matrix.summary.positive_door_rows {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "{} landing denominator differs from canonical positives",
                    summary.loadout.slug()
                ),
            ));
        }
    }

    let direct = &metrics.direct_controllers;
    if direct.directed_route_count != route_count
        || direct.directed_routes.len() != route_count
        || direct.by_loadout.len() != EvaluationLoadout::ALL.len()
        || direct
            .directed_routes
            .iter()
            .map(|route| (&route.source_door_id, &route.target_door_id))
            .ne(directed_routes
                .iter()
                .map(|(source, target)| (source, target)))
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            format!(
                "direct-controller rows are not exactly the {route_count} evaluated directed routes"
            ),
        ));
    }

    let mut route_counts = vec![[0usize; 4]; EvaluationLoadout::ALL.len()];
    for route in &direct.directed_routes {
        if route
            .exact_loadouts
            .iter()
            .map(|cell| cell.loadout)
            .ne(EvaluationLoadout::ALL)
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "direct route {:?}->{:?} does not contain all four exact loadouts",
                    route.source_door_id, route.target_door_id
                ),
            ));
        }
        if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative
            && !matches!(
                route.audit_completeness,
                MetricEvidence::Observed(super::RouteAuditCompleteness::CompleteFiniteVocabulary)
            )
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "production direct route {:?}->{:?} is not complete for the finite vocabulary",
                    route.source_door_id, route.target_door_id
                ),
            ));
        }
        for (loadout_index, cell) in route.exact_loadouts.iter().enumerate() {
            let (kind, raw, retained) = match cell.audit {
                super::DirectControllerAuditMetric::CompleteFiniteVocabulary {
                    raw_positive_witnesses,
                    retained_semantic_witnesses,
                } => (0, raw_positive_witnesses, retained_semantic_witnesses),
                super::DirectControllerAuditMetric::BoundedIncomplete {
                    raw_positive_witnesses,
                    retained_semantic_witnesses,
                    ..
                } => (1, raw_positive_witnesses, retained_semantic_witnesses),
                super::DirectControllerAuditMetric::MissingDirectedRouteAssessment
                | super::DirectControllerAuditMetric::MissingLoadoutAudit => (2, 0, 0),
            };
            route_counts[loadout_index][kind] += 1;
            let easiest_is_observed = matches!(cell.easiest_known, MetricEvidence::Observed(_));
            let canonical_is_positive = metrics
                .canonical_routes
                .directed_loadout_routes
                .iter()
                .any(|canonical| {
                    canonical.source_door_id == route.source_door_id
                        && canonical.target_door_id == route.target_door_id
                        && canonical.loadout == cell.loadout
                        && matches!(
                            canonical.evidence,
                            super::CanonicalRouteMetricEvidence::Positive(_)
                        )
                });
            if raw < retained
                || (retained > 0 || canonical_is_positive) != easiest_is_observed
                || (retained == 0 && raw > 0)
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "room-metric",
                    format!(
                        "direct/canonical {} {:?}->{:?} positives disagree with fused representative evidence",
                        cell.loadout.slug(),
                        route.source_door_id,
                        route.target_door_id
                    ),
                ));
            }
            if let MetricEvidence::Observed(easiest) = cell.easiest_known
                && (easiest.successful_loadout != cell.loadout
                    || match easiest.selection_status {
                        super::EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate => {
                            easiest.nondominated_front_size != 1
                        }
                        super::EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                            front_size,
                        } => front_size <= 1 || easiest.nondominated_front_size != front_size,
                    })
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "room-metric",
                    "an exact-loadout fused representative has invalid loadout/front provenance",
                ));
            }
            if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative && kind != 0 {
                return Err(final_path_evidence_error(
                    room_id,
                    "room-metric",
                    format!(
                        "production direct audit {} {:?}->{:?} is bounded or missing",
                        cell.loadout.slug(),
                        route.source_door_id,
                        route.target_door_id
                    ),
                ));
            }
        }
    }

    if direct
        .by_loadout
        .iter()
        .map(|summary| summary.loadout)
        .ne(EvaluationLoadout::ALL)
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "direct-controller aggregates are not the four canonical loadouts",
        ));
    }
    for (loadout_index, summary) in direct.by_loadout.iter().enumerate() {
        let counts = route_counts[loadout_index];
        let route_audits = summary.route_audit_completeness;
        let exact_cells = direct
            .directed_routes
            .iter()
            .map(|route| &route.exact_loadouts[loadout_index])
            .collect::<Vec<_>>();
        let known_positive = exact_cells
            .iter()
            .filter(|cell| matches!(cell.easiest_known, MetricEvidence::Observed(_)))
            .count();
        let ambiguous_nondominated_fronts =
            exact_cells
                .iter()
                .filter(|cell| {
                    matches!(
                    cell.easiest_known,
                    MetricEvidence::Observed(super::EasiestKnownControllerMetric {
                        selection_status:
                            super::EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                                ..
                            },
                        ..
                    })
                )
                })
                .count();
        let complete_without_positive = exact_cells
            .iter()
            .filter(|cell| {
                matches!(
                    cell.audit,
                    super::DirectControllerAuditMetric::CompleteFiniteVocabulary { .. }
                ) && !matches!(cell.easiest_known, MetricEvidence::Observed(_))
            })
            .count();
        let bounded_without_positive = exact_cells
            .iter()
            .filter(|cell| {
                matches!(
                    cell.audit,
                    super::DirectControllerAuditMetric::BoundedIncomplete { .. }
                ) && !matches!(cell.easiest_known, MetricEvidence::Observed(_))
            })
            .count();
        let missing_route_or_audit = exact_cells
            .iter()
            .filter(|cell| {
                matches!(
                    cell.audit,
                    super::DirectControllerAuditMetric::MissingDirectedRouteAssessment
                        | super::DirectControllerAuditMetric::MissingLoadoutAudit
                )
            })
            .count();
        let controller_aggregates_match = if ambiguous_nondominated_fronts > 0 {
            matches!(
                (
                    &summary.easiest_controller_fractions,
                    &summary.demand_coordinates,
                ),
                (
                    MetricEvidence::NotApplicable {
                        reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
                    },
                    MetricEvidence::NotApplicable {
                        reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
                    },
                )
            )
        } else {
            (known_positive > 0)
                == matches!(
                    summary.easiest_controller_fractions,
                    MetricEvidence::Observed(_)
                )
                && (known_positive > 0)
                    == matches!(summary.demand_coordinates, MetricEvidence::Observed(_))
        };
        if route_audits.expected != route_count
            || route_audits.complete_finite_vocabulary != counts[0]
            || route_audits.bounded_incomplete != counts[1]
            || route_audits.missing != counts[2]
            || route_audits.complete_finite_vocabulary
                + route_audits.bounded_incomplete
                + route_audits.missing
                != route_count
            || summary.source_audit_completeness.expected != door_ids.len()
            || summary.source_audit_completeness.complete_finite_vocabulary
                + summary.source_audit_completeness.bounded_incomplete
                + summary.source_audit_completeness.missing
                != door_ids.len()
            || summary.known_positive_directed_routes != known_positive
            || summary.ambiguous_nondominated_front_directed_routes != ambiguous_nondominated_fronts
            || summary.no_positive_in_complete_finite_vocabulary != complete_without_positive
            || summary.inconclusive_without_positive != bounded_without_positive
            || summary.missing_route_or_audit != missing_route_or_audit
            || !controller_aggregates_match
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "{} direct-controller aggregate shrinks or misstates its source/route denominator",
                    summary.loadout.slug()
                ),
            ));
        }
        if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative
            && (route_audits.complete_finite_vocabulary != route_count
                || summary.source_audit_completeness.complete_finite_vocabulary != door_ids.len())
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "production {} direct-controller aggregate is not fully complete",
                    summary.loadout.slug()
                ),
            ));
        }
    }

    let expected_directional_count = door_ids
        .len()
        .saturating_mul(door_ids.len().saturating_sub(1))
        .saturating_div(2)
        .saturating_mul(EvaluationLoadout::ALL.len());
    let expected_directional = door_ids.iter().enumerate().flat_map(|(left, door_a)| {
        door_ids.iter().skip(left + 1).flat_map(move |door_b| {
            EvaluationLoadout::ALL
                .into_iter()
                .map(move |loadout| (door_a, door_b, loadout))
        })
    });
    if metrics.directional_asymmetry.len() != expected_directional_count
        || metrics
            .directional_asymmetry
            .iter()
            .map(|row| (&row.door_a, &row.door_b, row.loadout))
            .ne(expected_directional)
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "directional-asymmetry rows are not the exact unordered-door/loadout universe",
        ));
    }

    if direct
        .ability_bypasses
        .by_successful_loadout
        .iter()
        .map(|summary| summary.successful_loadout)
        .ne([
            EvaluationLoadout::Baseline,
            EvaluationLoadout::WallJump,
            EvaluationLoadout::Dash,
        ])
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "ability-bypass aggregates are not the three exact lower loadouts",
        ));
    }
    if direct.directed_routes.iter().any(|route| {
        route
            .positive_bypasses
            .iter()
            .map(|bypass| bypass.successful_loadout)
            .any(|loadout| loadout == EvaluationLoadout::Both)
            || route
                .positive_bypasses
                .windows(2)
                .any(|pair| pair[0].successful_loadout >= pair[1].successful_loadout)
    }) {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "per-route ability bypasses are not distinct lower loadouts in canonical order",
        ));
    }
    let bypasses = &direct.ability_bypasses;
    let expected_bypass_rows = direct
        .directed_routes
        .iter()
        .flat_map(|route| &route.positive_bypasses)
        .collect::<Vec<_>>();
    let expected_routes_with_any = direct
        .directed_routes
        .iter()
        .filter(|route| !route.positive_bypasses.is_empty())
        .count();
    let expected_routes_without_wall = direct
        .directed_routes
        .iter()
        .filter(|route| {
            route
                .positive_bypasses
                .iter()
                .any(|bypass| !bypass.successful_loadout.abilities().wall_jump)
        })
        .count();
    let expected_routes_without_dash = direct
        .directed_routes
        .iter()
        .filter(|route| {
            route
                .positive_bypasses
                .iter()
                .any(|bypass| !bypass.successful_loadout.abilities().dash)
        })
        .count();
    if bypasses.directed_routes_with_any_bypass != expected_routes_with_any
        || bypasses.directed_routes_with_wall_jump_bypass != expected_routes_without_wall
        || bypasses.directed_routes_with_dash_bypass != expected_routes_without_dash
        || bypasses.directed_route_loadout_bypasses != expected_bypass_rows.len()
        || bypasses.retained_semantic_bypass_witnesses
            != expected_bypass_rows
                .iter()
                .map(|bypass| bypass.retained_semantic_witnesses)
                .sum::<usize>()
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "ability-bypass aggregate counts differ from the exact per-route bypass rows",
        ));
    }
    for summary in &bypasses.by_successful_loadout {
        let matching = expected_bypass_rows
            .iter()
            .filter(|bypass| bypass.successful_loadout == summary.successful_loadout)
            .collect::<Vec<_>>();
        if summary.directed_route_bypasses != matching.len()
            || summary.retained_semantic_witnesses
                != matching
                    .iter()
                    .map(|bypass| bypass.retained_semantic_witnesses)
                    .sum::<usize>()
        {
            return Err(final_path_evidence_error(
                room_id,
                "room-metric",
                format!(
                    "{} ability-bypass aggregate differs from its exact route rows",
                    summary.successful_loadout.slug()
                ),
            ));
        }
    }

    let audit_units = &metrics.operational_cost.direct_controller_audit_units;
    let expected_audit_units = door_ids.iter().flat_map(|source| {
        EvaluationLoadout::ALL
            .into_iter()
            .map(move |loadout| (source, loadout))
    });
    if audit_units.len() != door_ids.len().saturating_mul(EvaluationLoadout::ALL.len())
        || audit_units
            .iter()
            .map(|unit| (&unit.source_door_id, unit.loadout))
            .ne(expected_audit_units)
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "direct operational audit units are not every source/loadout exactly once",
        ));
    }
    if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative
        && audit_units.iter().any(|unit| {
            !matches!(
                unit.status,
                super::LoadoutControllerAuditStatus::CompleteFiniteVocabulary
            )
        })
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "production direct operational audit units include a bounded audit",
        ));
    }

    let expected_canonical_cost_rows = evaluated.matrices.iter().flat_map(|matrix| {
        matrix
            .evidence
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .map(move |row| {
                (
                    row.source_door_id.as_str(),
                    row.target_door_id.as_str(),
                    matrix.loadout,
                )
            })
    });
    if metrics
        .operational_cost
        .canonical_positive_routes
        .iter()
        .map(|row| {
            (
                row.source_door_id.as_str(),
                row.target_door_id.as_str(),
                row.loadout,
            )
        })
        .ne(expected_canonical_cost_rows)
    {
        return Err(final_path_evidence_error(
            room_id,
            "room-metric",
            "canonical operational rows are not exactly the evaluated positive route cells",
        ));
    }
    Ok(())
}

fn positive_route_audit_matches_metric(
    status: super::PositiveAlternativeAuditStatus,
    audit: &super::DirectControllerAuditMetric,
) -> bool {
    match (status, audit) {
        (
            super::PositiveAlternativeAuditStatus::CompleteFiniteVocabulary,
            super::DirectControllerAuditMetric::CompleteFiniteVocabulary { .. },
        ) => true,
        (
            super::PositiveAlternativeAuditStatus::BoundedIncomplete { limit: left },
            super::DirectControllerAuditMetric::BoundedIncomplete { limit: right, .. },
        ) => left == *right,
        (
            super::PositiveAlternativeAuditStatus::MissingDirectedRouteAssessment,
            super::DirectControllerAuditMetric::MissingDirectedRouteAssessment,
        )
        | (
            super::PositiveAlternativeAuditStatus::MissingLoadoutAudit,
            super::DirectControllerAuditMetric::MissingLoadoutAudit,
        ) => true,
        _ => false,
    }
}

fn validate_route_choice_aggregate_counts<'a>(
    room_id: &RoomId,
    label: &str,
    aggregate: &super::RouteChoiceAggregate,
    cells: impl IntoIterator<Item = &'a super::DirectedRouteChoiceCell>,
) -> Result<(), CorpusSelectionMetricsError> {
    let cells = cells.into_iter().collect::<Vec<_>>();
    let positives = cells
        .iter()
        .filter_map(|cell| match &cell.evidence {
            super::RouteChoiceCellEvidence::Positive(set) => Some(set.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let positive_complete = positives
        .iter()
        .filter(|set| {
            matches!(
                set.direct_audit_status,
                super::PositiveAlternativeAuditStatus::CompleteFiniteVocabulary
            )
        })
        .count();
    let positive_bounded = positives
        .iter()
        .filter(|set| {
            matches!(
                set.direct_audit_status,
                super::PositiveAlternativeAuditStatus::BoundedIncomplete { .. }
            )
        })
        .count();
    let positive_missing = positives
        .len()
        .saturating_sub(positive_complete.saturating_add(positive_bounded));
    let complete_without_positive = cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.evidence,
                super::RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary
            )
        })
        .count();
    let bounded_without_positive = cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.evidence,
                super::RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive { .. }
            )
        })
        .count();
    let missing_route = cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.evidence,
                super::RouteChoiceCellEvidence::MissingDirectedRouteAssessment
            )
        })
        .count();
    let missing_audit = cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.evidence,
                super::RouteChoiceCellEvidence::MissingLoadoutAudit
            )
        })
        .count();
    let histogram = |values: Vec<usize>| {
        let mut result = BTreeMap::new();
        for value in values {
            *result.entry(value).or_insert(0) += 1;
        }
        result
    };
    let distance_sample_totals = |axis: fn(
        &super::RouteAlternativeDistanceReport,
    ) -> &super::WithinCellDistanceDistribution| {
        positives.iter().fold((0usize, 0usize), |totals, set| {
            let distribution = axis(&set.distances);
            (
                totals.0.saturating_add(distribution.pairwise.samples),
                totals
                    .1
                    .saturating_add(distribution.nearest_neighbor.samples),
            )
        })
    };
    let distance_counts_match = [
        (
            &aggregate.distances.spatial_trajectory,
            distance_sample_totals(|report| &report.spatial_trajectory),
        ),
        (
            &aggregate.distances.semantic_actions,
            distance_sample_totals(|report| &report.semantic_actions),
        ),
        (
            &aggregate.distances.accepted_event_sequence,
            distance_sample_totals(|report| &report.accepted_event_sequence),
        ),
        (
            &aggregate.distances.gate_path_style,
            distance_sample_totals(|report| &report.gate_path_style),
        ),
    ]
    .into_iter()
    .all(|(actual, expected)| {
        (actual.pairwise.samples, actual.nearest_neighbor.samples) == expected
    });
    if aggregate.expected_cells != cells.len()
        || aggregate.positive_cells != positives.len()
        || aggregate.positive_complete_audit_cells != positive_complete
        || aggregate.positive_bounded_audit_cells != positive_bounded
        || aggregate.positive_missing_route_or_audit_cells != positive_missing
        || aggregate.complete_without_positive_cells != complete_without_positive
        || aggregate.bounded_inconclusive_without_positive_cells != bounded_without_positive
        || aggregate.missing_route_cells != missing_route
        || aggregate.missing_audit_cells != missing_audit
        || aggregate.cells_with_multiple_alternatives
            != positives
                .iter()
                .filter(|set| set.positive_alternative_count > 1)
                .count()
        || aggregate.observed_positive_count
            != positives
                .iter()
                .map(|set| set.observed_positive_count)
                .sum::<usize>()
        || aggregate.positive_alternative_count
            != positives
                .iter()
                .map(|set| set.positive_alternative_count)
                .sum::<usize>()
        || aggregate.timing_or_exact_aliases_collapsed
            != positives
                .iter()
                .map(|set| set.timing_or_exact_aliases_collapsed)
                .sum::<usize>()
        || aggregate.alternative_count_histogram
            != histogram(
                positives
                    .iter()
                    .map(|set| set.positive_alternative_count)
                    .collect(),
            )
        || aggregate.spatial_path_class_histogram
            != histogram(
                positives
                    .iter()
                    .map(|set| set.spatial_path_classes)
                    .collect(),
            )
        || aggregate.semantic_action_class_histogram
            != histogram(
                positives
                    .iter()
                    .map(|set| set.semantic_action_classes)
                    .collect(),
            )
        || aggregate.accepted_event_class_histogram
            != histogram(
                positives
                    .iter()
                    .map(|set| set.accepted_event_sequence_classes)
                    .collect(),
            )
        || aggregate.gate_path_style_class_histogram
            != histogram(
                positives
                    .iter()
                    .map(|set| set.gate_path_style_classes)
                    .collect(),
            )
        || !distance_counts_match
    {
        return Err(final_path_evidence_error(
            room_id,
            "route-choice",
            format!("{label} aggregate does not reaggregate from its exact cells"),
        ));
    }
    Ok(())
}

fn validate_final_path_route_choice_contract(
    evaluated: &EvaluatedCorpusRoomV2,
    metrics: &RoomMetricSummary,
    report: &super::RoomRouteChoiceDiversity,
    policy: CorpusV2DescriptorPolicy,
) -> Result<(), CorpusSelectionMetricsError> {
    let room_id = &evaluated.generated.id;
    let route_count = metrics.direct_controllers.directed_routes.len();
    let expected_cell_count = route_count.saturating_mul(EvaluationLoadout::ALL.len());
    if report.cells.len() != expected_cell_count
        || report
            .by_loadout
            .iter()
            .map(|summary| summary.loadout)
            .ne(EvaluationLoadout::ALL)
    {
        return Err(final_path_evidence_error(
            room_id,
            "route-choice",
            "cells/by-loadout rows do not cover all exact directed route/loadout coordinates",
        ));
    }

    for (loadout_index, matrix) in evaluated.matrices.iter().enumerate() {
        for (route_index, matrix_row) in matrix.evidence.door_routes().iter().enumerate() {
            let cell = &report.cells[loadout_index * route_count + route_index];
            let direct_cell = &metrics.direct_controllers.directed_routes[route_index]
                .exact_loadouts[loadout_index];
            if cell.loadout != matrix.loadout
                || cell.source_door_id != matrix_row.source_door_id
                || cell.target_door_id != matrix_row.target_door_id
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "route-choice",
                    format!(
                        "cell {loadout_index}/{route_index} is not bound to its evaluated matrix coordinate"
                    ),
                ));
            }
            let canonical_positive =
                matches!(matrix_row.evidence, BoundedTargetEvidence::Positive(_));
            let (raw_direct, retained_direct) = match direct_cell.audit {
                super::DirectControllerAuditMetric::CompleteFiniteVocabulary {
                    raw_positive_witnesses,
                    retained_semantic_witnesses,
                }
                | super::DirectControllerAuditMetric::BoundedIncomplete {
                    raw_positive_witnesses,
                    retained_semantic_witnesses,
                    ..
                } => (raw_positive_witnesses, retained_semantic_witnesses),
                super::DirectControllerAuditMetric::MissingDirectedRouteAssessment
                | super::DirectControllerAuditMetric::MissingLoadoutAudit => (0, 0),
            };
            if retained_direct > 0 || canonical_positive {
                let super::RouteChoiceCellEvidence::Positive(set) = &cell.evidence else {
                    return Err(final_path_evidence_error(
                        room_id,
                        "route-choice",
                        format!(
                            "positive evaluated/direct evidence at {} {:?}->{:?} was omitted",
                            matrix.loadout.slug(),
                            matrix_row.source_door_id,
                            matrix_row.target_door_id
                        ),
                    ));
                };
                if !positive_route_audit_matches_metric(set.direct_audit_status, &direct_cell.audit)
                    || set.raw_direct_positive_witnesses != raw_direct
                    || set.retained_direct_positive_witnesses != retained_direct
                    || set.canonical_positive_observations != usize::from(canonical_positive)
                    || set.observed_positive_count
                        != retained_direct.saturating_add(usize::from(canonical_positive))
                {
                    return Err(final_path_evidence_error(
                        room_id,
                        "route-choice",
                        format!(
                            "positive {} {:?}->{:?} provenance/counts differ from matrix and direct metrics",
                            matrix.loadout.slug(),
                            matrix_row.source_door_id,
                            matrix_row.target_door_id
                        ),
                    ));
                }
            } else {
                let expected = match direct_cell.audit {
                    super::DirectControllerAuditMetric::CompleteFiniteVocabulary { .. } => {
                        matches!(
                            cell.evidence,
                            super::RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary
                        )
                    }
                    super::DirectControllerAuditMetric::BoundedIncomplete { limit, .. } => {
                        matches!(
                            cell.evidence,
                            super::RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive {
                                limit: actual
                            } if actual == limit
                        )
                    }
                    super::DirectControllerAuditMetric::MissingDirectedRouteAssessment => {
                        matches!(
                            cell.evidence,
                            super::RouteChoiceCellEvidence::MissingDirectedRouteAssessment
                        )
                    }
                    super::DirectControllerAuditMetric::MissingLoadoutAudit => matches!(
                        cell.evidence,
                        super::RouteChoiceCellEvidence::MissingLoadoutAudit
                    ),
                };
                if !expected {
                    return Err(final_path_evidence_error(
                        room_id,
                        "route-choice",
                        format!(
                            "nonpositive {} {:?}->{:?} state differs from its direct audit",
                            matrix.loadout.slug(),
                            matrix_row.source_door_id,
                            matrix_row.target_door_id
                        ),
                    ));
                }
            }
            if policy == CorpusV2DescriptorPolicy::ProductionAuthoritative
                && !matches!(
                    direct_cell.audit,
                    super::DirectControllerAuditMetric::CompleteFiniteVocabulary { .. }
                )
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "route-choice",
                    format!(
                        "production {} {:?}->{:?} retains bounded or missing direct-audit evidence",
                        matrix.loadout.slug(),
                        matrix_row.source_door_id,
                        matrix_row.target_door_id
                    ),
                ));
            }
        }
        if report.by_loadout[loadout_index].aggregate.expected_cells != route_count {
            return Err(final_path_evidence_error(
                room_id,
                "route-choice",
                format!(
                    "{} aggregate expected-cell denominator is not {route_count}",
                    matrix.loadout.slug()
                ),
            ));
        }
        let start = loadout_index.saturating_mul(route_count);
        validate_route_choice_aggregate_counts(
            room_id,
            matrix.loadout.slug(),
            &report.by_loadout[loadout_index].aggregate,
            report.cells[start..start + route_count].iter(),
        )?;
    }
    if report.room.expected_cells != expected_cell_count {
        return Err(final_path_evidence_error(
            room_id,
            "route-choice",
            format!("room aggregate expected-cell denominator is not {expected_cell_count}"),
        ));
    }
    validate_route_choice_aggregate_counts(
        room_id,
        "room-wide",
        &report.room,
        report.cells.iter(),
    )?;
    Ok(())
}

fn validate_final_path_pickup_detour_contract(
    evaluated: &EvaluatedCorpusRoomV2,
    candidate: &CorpusCandidate,
    report: &super::RoomPickupDetourAnalysis,
) -> Result<(), CorpusSelectionMetricsError> {
    let room_id = &evaluated.generated.id;
    let (door_ids, pickup_ids, _directed_routes) = canonical_room_coordinates(candidate);
    let expected_cells = door_ids
        .len()
        .saturating_mul(pickup_ids.len())
        .saturating_mul(EvaluationLoadout::ALL.len());
    if report
        .structural_placements
        .iter()
        .map(|placement| &placement.pickup_id)
        .ne(pickup_ids.iter())
        || report.cells.len() != expected_cells
    {
        return Err(final_path_evidence_error(
            room_id,
            "pickup-detour",
            "structural placements or cells do not cover the exact pickup coordinate universe",
        ));
    }

    let mut cell_index = 0usize;
    for matrix in &evaluated.matrices {
        for matrix_row in matrix.evidence.pickup_routes() {
            let cell = &report.cells[cell_index];
            cell_index += 1;
            if cell.loadout != matrix.loadout
                || cell.source_door_id != matrix_row.source_door_id
                || cell.pickup_id != matrix_row.required_pickup_id
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "pickup-detour",
                    format!(
                        "pickup cell {} is not bound to its evaluated matrix coordinate",
                        cell_index - 1
                    ),
                ));
            }
            let state_matches = match (&cell.evidence, &matrix_row.evidence) {
                (
                    super::PickupCellEvidence::Positive(pickup),
                    BoundedTargetEvidence::Positive(matrix_positive),
                ) => {
                    pickup.witness_fingerprint == matrix_positive.witness_fingerprint()
                        && pickup.ability.loadout == matrix.loadout
                }
                (
                    super::PickupCellEvidence::BoundedInconclusive { reason: left },
                    BoundedTargetEvidence::Inconclusive(right),
                ) => *left == right.reason,
                _ => false,
            };
            if !state_matches {
                return Err(final_path_evidence_error(
                    room_id,
                    "pickup-detour",
                    format!(
                        "pickup {} {:?}->{:?} state/fingerprint differs from the evaluated matrix",
                        matrix.loadout.slug(),
                        matrix_row.source_door_id,
                        matrix_row.required_pickup_id
                    ),
                ));
            }
            let expected_targets = matrix
                .evidence
                .door_routes()
                .iter()
                .filter(|row| row.source_door_id == matrix_row.source_door_id);
            if cell.canonical_door_context.routes.len() != door_ids.len().saturating_sub(1)
                || cell
                    .canonical_door_context
                    .routes
                    .iter()
                    .zip(expected_targets)
                    .any(
                        |(route, matrix_door)| match (route, &matrix_door.evidence) {
                            (
                                super::CanonicalDoorPickupRoute::Positive {
                                    target_door_id,
                                    difference_from_pickup_witness,
                                    ..
                                },
                                BoundedTargetEvidence::Positive(_),
                            ) => {
                                target_door_id != &matrix_door.target_door_id
                                    || difference_from_pickup_witness.is_some()
                                        != matches!(
                                            cell.evidence,
                                            super::PickupCellEvidence::Positive(_)
                                        )
                            }
                            (
                                super::CanonicalDoorPickupRoute::BoundedInconclusive {
                                    target_door_id,
                                    reason,
                                },
                                BoundedTargetEvidence::Inconclusive(matrix_inconclusive),
                            ) => {
                                target_door_id != &matrix_door.target_door_id
                                    || *reason != matrix_inconclusive.reason
                            }
                            _ => true,
                        },
                    )
            {
                return Err(final_path_evidence_error(
                    room_id,
                    "pickup-detour",
                    format!(
                        "canonical door context for {} {:?}->{:?} is not the exact same-source door matrix",
                        matrix.loadout.slug(),
                        matrix_row.source_door_id,
                        matrix_row.required_pickup_id
                    ),
                ));
            }
        }
    }

    let expected_source_searches = evaluated.matrices.iter().flat_map(|matrix| {
        matrix
            .evidence
            .source_search_effort()
            .iter()
            .enumerate()
            .map(move |(source_index, source)| {
                (
                    matrix.loadout,
                    source_index,
                    source.source_door_id.as_str(),
                    source.stats,
                )
            })
    });
    if report
        .operational_cost
        .source_searches
        .iter()
        .map(|source| {
            (
                source.loadout,
                source.source_index,
                source.source_door_id.as_str(),
                source.search,
            )
        })
        .ne(expected_source_searches)
    {
        return Err(final_path_evidence_error(
            room_id,
            "pickup-detour",
            "operational source searches are not every evaluated source/loadout exactly once",
        ));
    }

    let expected_cross_coordinates = door_ids.iter().flat_map(|source| {
        pickup_ids
            .iter()
            .map(move |pickup| (source.as_str(), pickup.as_str()))
    });
    if report
        .cross_loadout
        .iter()
        .map(|row| (row.source_door_id.as_str(), row.pickup_id.as_str()))
        .ne(expected_cross_coordinates)
    {
        return Err(final_path_evidence_error(
            room_id,
            "pickup-detour",
            "cross-loadout rows are not every source/pickup coordinate exactly once",
        ));
    }
    let source_pickup_count = door_ids.len().saturating_mul(pickup_ids.len());
    for (coordinate_index, row) in report.cross_loadout.iter().enumerate() {
        let mut positive_loadouts = Vec::new();
        let mut bounded_loadouts = Vec::new();
        let mut witnesses_using_wall_jump = Vec::new();
        let mut witnesses_using_dash = Vec::new();
        for (loadout_index, matrix) in evaluated.matrices.iter().enumerate() {
            match &matrix.evidence.pickup_routes()[coordinate_index].evidence {
                BoundedTargetEvidence::Positive(_) => positive_loadouts.push(matrix.loadout),
                BoundedTargetEvidence::Inconclusive(inconclusive) => {
                    bounded_loadouts.push((matrix.loadout, inconclusive.reason))
                }
            }
            if let super::PickupCellEvidence::Positive(positive) =
                &report.cells[loadout_index * source_pickup_count + coordinate_index].evidence
            {
                if positive.ability.used_wall_jump {
                    witnesses_using_wall_jump.push(matrix.loadout);
                }
                if positive.ability.used_dash {
                    witnesses_using_dash.push(matrix.loadout);
                }
            }
        }
        if row.positive_loadouts != positive_loadouts
            || row
                .bounded_loadouts
                .iter()
                .map(|bounded| (bounded.loadout, bounded.reason))
                .ne(bounded_loadouts)
            || row.positive_without_wall_jump
                != positive_loadouts
                    .iter()
                    .any(|loadout| !loadout.abilities().wall_jump)
            || row.positive_without_dash
                != positive_loadouts
                    .iter()
                    .any(|loadout| !loadout.abilities().dash)
            || row.witnesses_using_wall_jump != witnesses_using_wall_jump
            || row.witnesses_using_dash != witnesses_using_dash
        {
            return Err(final_path_evidence_error(
                room_id,
                "pickup-detour",
                format!(
                    "cross-loadout {:?}->{:?} partition differs from its four exact pickup cells",
                    row.source_door_id, row.pickup_id
                ),
            ));
        }
    }
    Ok(())
}

fn validate_production_descriptor_evidence(
    descriptor: &CorpusSelectionDescriptor,
) -> Result<(), CorpusSelectionMetricsError> {
    let validate = |coordinate: String,
                    evidence: &QuantizedSelectionEvidence|
     -> Result<(), CorpusSelectionMetricsError> {
        let shape_is_valid = matches!(
            (evidence.state, evidence.value),
            (SelectionEvidenceState::Observed, Some(_))
                | (SelectionEvidenceState::NotApplicable, None)
        );
        if shape_is_valid {
            Ok(())
        } else {
            Err(CorpusSelectionMetricsError::ProductionEvidenceUnavailable {
                room_id: descriptor.room_id.clone(),
                coordinate,
                state: evidence.state,
            })
        }
    };
    for coordinate in &descriptor.quality {
        validate(
            format!("quality::{:?}", coordinate.axis),
            &coordinate.evidence,
        )?;
    }
    for projection in &descriptor.projections {
        for coordinate in &projection.cell {
            validate(
                format!("projection::{:?}::cell::{}", projection.id, coordinate.name),
                &coordinate.evidence,
            )?;
        }
        for coordinate in &projection.detail {
            validate(
                format!(
                    "projection::{:?}::detail::{}",
                    projection.id, coordinate.name
                ),
                &coordinate.evidence,
            )?;
        }
    }
    Ok(())
}

/// Convenience adapter when the caller has not yet constructed a
/// [`RoomMetricSummary`].
pub fn describe_analyzed_corpus_selection_room(
    generated: &GeneratedCorpusRoom,
    analysis: &CorpusRoomAnalysis,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    let metrics = summarize_room_metrics(analysis).map_err(|source| {
        CorpusSelectionMetricsError::MetricSummary {
            room_id: generated.id.clone(),
            source,
        }
    })?;
    describe_corpus_selection_room(generated, &metrics)
}

/// Convenience final-path adapter for unsummarized deep analysis.
pub fn describe_analyzed_corpus_selection_room_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    let metrics = summarize_room_metrics(analysis).map_err(|source| {
        CorpusSelectionMetricsError::MetricSummary {
            room_id: evaluated.generated.id.clone(),
            source,
        }
    })?;
    describe_corpus_selection_room_v2(evaluated, &metrics, route_choices, pickup_detours)
}

/// Unsummarized-analysis convenience form of the explicitly exploratory v2
/// descriptor adapter.
pub fn describe_analyzed_corpus_selection_room_v2_exploratory(
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
    route_choices: Option<&super::RoomRouteChoiceDiversity>,
    pickup_detours: Option<&super::RoomPickupDetourAnalysis>,
) -> Result<CorpusSelectionDescriptor, CorpusSelectionMetricsError> {
    let metrics = summarize_room_metrics(analysis).map_err(|source| {
        CorpusSelectionMetricsError::MetricSummary {
            room_id: evaluated.generated.id.clone(),
            source,
        }
    })?;
    describe_corpus_selection_room_v2_exploratory(
        evaluated,
        &metrics,
        route_choices,
        pickup_detours,
    )
}

/// Build the generic archive and select a deterministic hard-gated corpus.
///
/// Exact static-visual duplicates reject the entire input rather than being
/// silently tie-broken.  After the generic archive establishes its
/// marginal-coverage/max-min ranking, a deterministic greatest-fixed-point
/// pass removes rooms without cross-room socket mates.  If downselection is
/// needed, lowest-ranked removable rooms are pruned while recomputing that
/// fixed point.  The reported packages are connected components of the final
/// compatibility graph and are each independently mate-covered.
///
/// This layer intentionally does not infer the route feasibility policy for
/// its caller. Production callers must offer only rooms that already pass the
/// construction-loadout and complete-kit all-target positive gates. Missing
/// and inconclusive *deep metric* evidence remains admissible and explicit.
pub fn select_corpus_rooms(
    rooms: &[CorpusSelectionRoom<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    validate_selection_config(config)?;
    validate_input_identity_and_visual_uniqueness(rooms)?;

    let mut descriptors = BTreeMap::new();
    for room in rooms {
        let descriptor = describe_corpus_selection_room(room.generated, room.metrics)?;
        descriptors.insert(descriptor.room_id.clone(), descriptor);
    }
    select_descriptors(descriptors, rooms.len(), config)
}

/// Select rooms with both deep projections populated.  The input type makes
/// omission impossible, and each source report is identity checked before the
/// generic archive sees a descriptor.
pub fn select_extended_corpus_rooms(
    rooms: &[ExtendedCorpusSelectionRoom<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    validate_selection_config(config)?;
    let base_rooms = rooms
        .iter()
        .map(|room| CorpusSelectionRoom::new(room.generated, room.metrics))
        .collect::<Vec<_>>();
    validate_input_identity_and_visual_uniqueness(&base_rooms)?;

    let mut descriptors = BTreeMap::new();
    for room in rooms {
        let descriptor = describe_extended_corpus_selection_room(
            room.generated,
            room.metrics,
            room.route_choices,
            room.pickup_detours,
        )?;
        descriptors.insert(descriptor.room_id.clone(), descriptor);
    }
    select_descriptors(descriptors, rooms.len(), config)
}

/// Build and select the production-authoritative final-path corpus.
///
/// Each room is resolved through exact room-v3 and gate validation before its
/// static visual, native boundary sockets, or metric evidence enters the
/// nine-projection archive. Both optional-at-the-type-level deep reports are
/// mandatory here, Missing/Bounded selection evidence is rejected, and every
/// room must share one exact content-addressed deep-analysis configuration.
pub fn select_corpus_rooms_v2(
    rooms: &[CorpusSelectionRoomV2<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    select_corpus_rooms_v2_with_policy(
        rooms,
        config,
        CorpusV2DescriptorPolicy::ProductionAuthoritative,
    )
}

/// Build an explicitly exploratory final-path archive.
///
/// Missing and bounded deep evidence remains visible as typed coordinates,
/// but mixed or invalid analysis configuration identities are still rejected.
/// This API must not be used by final shard/corpus production.
pub fn select_corpus_rooms_v2_exploratory(
    rooms: &[CorpusSelectionRoomV2<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    select_corpus_rooms_v2_with_policy(rooms, config, CorpusV2DescriptorPolicy::Exploratory)
}

/// Select only source-bound operational-cache descriptors.
///
/// The opaque input can be constructed only by the offline cache module after
/// current artifact-v3 checkpoint, room/key, native socket, config, policy,
/// and cache-hash validation. This function independently revalidates the
/// complete descriptor shape before entering the existing QD/socket selector.
/// Its result remains provisional until selected rooms are fully recomputed.
pub fn select_validated_cached_corpus_descriptors_v2(
    rooms: &[super::ValidatedCachedCorpusSelectionDescriptorV2],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    validate_selection_config(config)?;
    let mut descriptors = BTreeMap::new();
    for room in rooms {
        let descriptor = room.descriptor();
        validate_cached_descriptor_structure_v2(descriptor)?;
        if descriptors
            .insert(descriptor.room_id.clone(), descriptor.clone())
            .is_some()
        {
            return Err(CorpusSelectionMetricsError::DuplicateRoomId {
                room_id: descriptor.room_id.clone(),
            });
        }
    }
    select_descriptors(descriptors, rooms.len(), config)
}

fn validate_cached_descriptor_structure_v2(
    descriptor: &CorpusSelectionDescriptor,
) -> Result<(), CorpusSelectionMetricsError> {
    let invalid = |detail: String| CorpusSelectionMetricsError::StructuralDescriptor {
        room_id: descriptor.room_id.clone(),
        detail,
    };
    if descriptor.version != CORPUS_SELECTION_METRICS_VERSION || descriptor.room_id.0.is_empty() {
        return Err(invalid(format!(
            "cached descriptor has version {} or an empty room ID; expected version {}",
            descriptor.version, CORPUS_SELECTION_METRICS_VERSION
        )));
    }
    let expected_projections = [
        CorpusSelectionProjectionId::MorphologyTopologyV1,
        CorpusSelectionProjectionId::DirectedLoadoutControllerDemandV1,
        CorpusSelectionProjectionId::LandingGeometryV1,
        CorpusSelectionProjectionId::DirectionalAsymmetryV1,
        CorpusSelectionProjectionId::RouteDiversityV1,
        CorpusSelectionProjectionId::AbilityBypassStructureV1,
        CorpusSelectionProjectionId::TerrainAblationUtilityV1,
        CorpusSelectionProjectionId::ObservedWithinCellRouteChoicesV1,
        CorpusSelectionProjectionId::PickupChallengeDetourV1,
    ];
    if descriptor
        .projections
        .iter()
        .map(|projection| projection.id)
        .ne(expected_projections)
    {
        return Err(invalid(
            "cached descriptor does not contain the exact current nine projections in policy order"
                .to_owned(),
        ));
    }
    for projection in &descriptor.projections {
        let mut detail_names = BTreeSet::new();
        for coordinate in &projection.detail {
            if coordinate.name.is_empty() || !detail_names.insert(coordinate.name.as_str()) {
                return Err(invalid(format!(
                    "cached {:?} detail coordinates contain an empty or duplicate name",
                    projection.id
                )));
            }
        }
        let mut cell_names = BTreeSet::new();
        for coordinate in &projection.cell {
            if coordinate.name.is_empty() || !cell_names.insert(coordinate.name.as_str()) {
                return Err(invalid(format!(
                    "cached {:?} cell coordinates contain an empty or duplicate name",
                    projection.id
                )));
            }
            if projection
                .detail
                .iter()
                .filter(|detail| detail.name == coordinate.name && *detail == coordinate)
                .count()
                != 1
            {
                return Err(invalid(format!(
                    "cached {:?} cell coordinate {:?} is not an exact detail coordinate",
                    projection.id, coordinate.name
                )));
            }
        }
    }
    let expected_quality = [
        (
            CorpusSelectionQualityAxis::CompleteKitOtherControllerDemand,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::CompleteKitMedianDuration,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::DirectionalDurationAsymmetry,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::JointRouteStyleDiversity,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::UncorroboratedTerrainComponents,
            ObjectiveDirection::Minimize,
        ),
        (
            CorpusSelectionQualityAxis::UncorroboratedTerrainTiles,
            ObjectiveDirection::Minimize,
        ),
        (
            CorpusSelectionQualityAxis::AblatedTilesAffectingStoredControllers,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::ControllerAblationEffect,
            ObjectiveDirection::Maximize,
        ),
        (
            CorpusSelectionQualityAxis::OperationalSimulatedTicks,
            ObjectiveDirection::Minimize,
        ),
    ];
    if descriptor
        .quality
        .iter()
        .map(|quality| (quality.axis, quality.direction))
        .ne(expected_quality)
    {
        return Err(invalid(
            "cached descriptor does not contain the exact current quality axes/directions in policy order"
                .to_owned(),
        ));
    }
    validate_production_descriptor_evidence(descriptor)?;
    if descriptor.diversity_coordinates != flatten_diversity(&descriptor.projections) {
        return Err(invalid(
            "cached descriptor diversity vector does not recompute from projection details"
                .to_owned(),
        ));
    }
    if descriptor.sockets.is_empty()
        || descriptor
            .sockets
            .iter()
            .any(|socket| socket.offset < 0 || socket.span <= 0)
    {
        return Err(invalid(
            "cached descriptor has no sockets or an invalid socket aperture".to_owned(),
        ));
    }
    Ok(())
}

fn select_corpus_rooms_v2_with_policy(
    rooms: &[CorpusSelectionRoomV2<'_>],
    config: CorpusSelectionConfig,
    policy: CorpusV2DescriptorPolicy,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    validate_selection_config(config)?;
    let mut stable_rooms = rooms.iter().collect::<Vec<_>>();
    stable_rooms.sort_unstable_by(|left, right| {
        left.evaluated
            .generated
            .id
            .cmp(&right.evaluated.generated.id)
    });
    let mut batch_analysis_config: Option<(RoomId, CorpusRoomAnalysisConfigRecord)> = None;
    for room in &stable_rooms {
        let room_id = &room.evaluated.generated.id;
        validate_room_metric_provenance(room_id, room.metrics)?;
        if let Some((first_room_id, first_config)) = &batch_analysis_config {
            if first_config != &room.metrics.source_analysis_config {
                return Err(CorpusSelectionMetricsError::MixedAnalysisConfigs {
                    first_room_id: first_room_id.clone(),
                    first_config_id: first_config.config_id.clone(),
                    room_id: room_id.clone(),
                    config_id: room.metrics.source_analysis_config.config_id.clone(),
                });
            }
        } else {
            batch_analysis_config =
                Some((room_id.clone(), room.metrics.source_analysis_config.clone()));
        }
    }
    let mut ids = BTreeSet::new();
    let mut visuals = HashMap::new();
    let mut descriptors = BTreeMap::new();
    for room in stable_rooms {
        let room_id = &room.evaluated.generated.id;
        let descriptor = describe_corpus_selection_room_v2_with_policy(
            room.evaluated,
            room.metrics,
            room.route_choices,
            room.pickup_detours,
            policy,
        )?;
        if let Some(first_room_id) = visuals.insert(
            room.evaluated
                .generated
                .physical_descriptor
                .static_visual
                .clone(),
            room_id.clone(),
        ) {
            return Err(CorpusSelectionMetricsError::DuplicateExactStaticVisual {
                first_room_id,
                duplicate_room_id: room_id.clone(),
            });
        }
        if !ids.insert(room_id.clone()) {
            return Err(CorpusSelectionMetricsError::DuplicateRoomId {
                room_id: room_id.clone(),
            });
        }
        descriptors.insert(room_id.clone(), descriptor);
    }
    select_descriptors(descriptors, rooms.len(), config)
}

fn select_descriptors(
    descriptors: BTreeMap<RoomId, CorpusSelectionDescriptor>,
    submitted_rooms: usize,
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    validate_selection_config(config)?;
    let archive = QualityDiversityArchive::build(
        ArchiveConfig::new(
            corpus_selection_quality_directions(),
            config.elites_per_cell,
        ),
        descriptors
            .values()
            .map(CorpusSelectionDescriptor::to_archive_candidate),
    )?;
    let archive_summary = archive.summary();

    // Asking for every retained singleton yields the generic archive's full
    // deterministic marginal-coverage/max-min ranking.
    let ranking = archive
        .select_farthest(0..=archive.len())
        .map_err(
            |source| CorpusSelectionMetricsError::SocketCoverageInvariant {
                detail: format!("could not rank archive candidates: {source}"),
            },
        )?;
    let ranked_ids = ranking.selected_candidates;
    let ranked_set = ranked_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut selected = mate_covered_core(&ranked_set, &descriptors);
    let rooms_excluded_by_initial_socket_core = ranked_set
        .difference(&selected)
        .cloned()
        .collect::<Vec<_>>();

    if selected.len() < config.requested_minimum {
        return Err(CorpusSelectionMetricsError::RequestedMinimumUnavailable {
            minimum: config.requested_minimum,
            maximum: config.requested_maximum,
            available: selected.len(),
        });
    }

    let mut pruning_steps = Vec::new();
    while selected.len() > config.requested_maximum {
        let mut chosen = None;
        // Prefer the least favored room whose removal does not cascade.
        for room_id in ranked_ids.iter().rev().filter(|id| selected.contains(*id)) {
            let mut trial = selected.clone();
            trial.remove(room_id);
            if mate_coverage_is_complete(&trial, &descriptors) {
                chosen = Some((room_id.clone(), trial));
                break;
            }
        }
        // If every single removal strands another room, compute deterministic
        // removal cascades and retain the largest still-admissible core.
        if chosen.is_none() {
            for room_id in ranked_ids.iter().rev().filter(|id| selected.contains(*id)) {
                let mut trial = selected.clone();
                trial.remove(room_id);
                trial = mate_covered_core(&trial, &descriptors);
                if trial.len() < config.requested_minimum {
                    continue;
                }
                let replace =
                    chosen
                        .as_ref()
                        .is_none_or(|(_, current): &(RoomId, BTreeSet<RoomId>)| {
                            trial.len() > current.len()
                        });
                if replace {
                    chosen = Some((room_id.clone(), trial));
                }
            }
        }
        let Some((requested_removal, next)) = chosen else {
            return Err(CorpusSelectionMetricsError::RequestedMinimumUnavailable {
                minimum: config.requested_minimum,
                maximum: config.requested_maximum,
                available: selected.len(),
            });
        };
        let removed_room_ids = selected.difference(&next).cloned().collect::<Vec<_>>();
        selected = next;
        pruning_steps.push(SocketCoveragePruningStep {
            requested_removal,
            removed_room_ids,
            remaining_rooms: selected.len(),
        });
    }

    if selected.len() < config.requested_minimum {
        return Err(CorpusSelectionMetricsError::RequestedMinimumUnavailable {
            minimum: config.requested_minimum,
            maximum: config.requested_maximum,
            available: selected.len(),
        });
    }
    if !mate_coverage_is_complete(&selected, &descriptors) {
        return Err(CorpusSelectionMetricsError::SocketCoverageInvariant {
            detail: "final selected set is not cross-room socket-mate covered".to_owned(),
        });
    }

    let final_ranking = archive
        .select_farthest_packages(
            archive
                .candidate_ids()
                .cloned()
                .map(|id| SelectionPackage::new(id.clone(), vec![id])),
            selected.len()..=selected.len(),
            |package| selected.contains(&package.id),
        )
        .map_err(
            |source| CorpusSelectionMetricsError::SocketCoverageInvariant {
                detail: format!("could not rerank the final socket-covered set: {source}"),
            },
        )?;
    let selected_room_ids = final_ranking.selected_candidates;
    let socket_packages = reusable_socket_packages(&selected, &descriptors)?;
    let covered_cells = final_ranking.covered_cells;
    let steps = final_ranking
        .steps
        .into_iter()
        .map(|step| CorpusSelectionStep {
            room_id: step.package_id,
            marginal_cell_coverage: step.marginal_cell_coverage,
            minimum_l1_distance: step.minimum_l1_distance,
        })
        .collect::<Vec<_>>();
    let audit = CorpusSelectionAudit {
        version: CORPUS_SELECTION_METRICS_VERSION,
        submitted_rooms,
        exact_visual_unique_rooms: descriptors.len(),
        archive: archive_summary,
        archive_ranked_rooms: ranked_ids.len(),
        rooms_excluded_by_initial_socket_core,
        socket_pruning_steps: pruning_steps,
        selected_rooms: selected_room_ids.len(),
        selected_package_count: socket_packages.len(),
        covered_cells,
        steps,
    };

    Ok(CorpusSelectionOutcome {
        archive,
        descriptors,
        selected_room_ids,
        socket_packages,
        audit,
    })
}

/// Summarize deep analyses and run [`select_corpus_rooms`] without requiring
/// callers to persist a duplicate in-memory metric layer first.
pub fn select_analyzed_corpus_rooms(
    rooms: &[AnalyzedCorpusSelectionRoom<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    let summaries = rooms
        .iter()
        .map(|room| {
            summarize_room_metrics(room.analysis).map_err(|source| {
                CorpusSelectionMetricsError::MetricSummary {
                    room_id: room.generated.id.clone(),
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let summarized = rooms
        .iter()
        .zip(&summaries)
        .map(|(room, metrics)| CorpusSelectionRoom::new(room.generated, metrics))
        .collect::<Vec<_>>();
    select_corpus_rooms(&summarized, config)
}

/// Summarize final-path analyses and run [`select_corpus_rooms_v2`].
pub fn select_analyzed_corpus_rooms_v2(
    rooms: &[AnalyzedCorpusSelectionRoomV2<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    let summaries = rooms
        .iter()
        .map(|room| {
            summarize_room_metrics(room.analysis).map_err(|source| {
                CorpusSelectionMetricsError::MetricSummary {
                    room_id: room.evaluated.generated.id.clone(),
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let summarized = rooms
        .iter()
        .zip(&summaries)
        .map(|(room, metrics)| {
            CorpusSelectionRoomV2::new(room.evaluated, metrics)
                .with_optional_deep_reports(room.route_choices, room.pickup_detours)
        })
        .collect::<Vec<_>>();
    select_corpus_rooms_v2(&summarized, config)
}

/// Summarize final-path analyses and run the explicitly exploratory v2
/// selector.
pub fn select_analyzed_corpus_rooms_v2_exploratory(
    rooms: &[AnalyzedCorpusSelectionRoomV2<'_>],
    config: CorpusSelectionConfig,
) -> Result<CorpusSelectionOutcome, CorpusSelectionMetricsError> {
    let summaries = rooms
        .iter()
        .map(|room| {
            summarize_room_metrics(room.analysis).map_err(|source| {
                CorpusSelectionMetricsError::MetricSummary {
                    room_id: room.evaluated.generated.id.clone(),
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let summarized = rooms
        .iter()
        .zip(&summaries)
        .map(|(room, metrics)| {
            CorpusSelectionRoomV2::new(room.evaluated, metrics)
                .with_optional_deep_reports(room.route_choices, room.pickup_detours)
        })
        .collect::<Vec<_>>();
    select_corpus_rooms_v2_exploratory(&summarized, config)
}

fn validate_selection_config(
    config: CorpusSelectionConfig,
) -> Result<(), CorpusSelectionMetricsError> {
    if config.requested_minimum == 0 || config.requested_minimum > config.requested_maximum {
        return Err(CorpusSelectionMetricsError::InvalidRequestedRange {
            minimum: config.requested_minimum,
            maximum: config.requested_maximum,
        });
    }
    if config.elites_per_cell == 0 {
        return Err(CorpusSelectionMetricsError::ZeroElitesPerCell);
    }
    Ok(())
}

fn validate_input_identity_and_visual_uniqueness(
    rooms: &[CorpusSelectionRoom<'_>],
) -> Result<(), CorpusSelectionMetricsError> {
    let mut ids = BTreeSet::new();
    let mut visuals = HashMap::new();
    let mut stable_rooms = rooms.iter().collect::<Vec<_>>();
    stable_rooms.sort_unstable_by(|left, right| left.generated.id.cmp(&right.generated.id));
    for room in stable_rooms {
        if !ids.insert(room.generated.id.clone()) {
            return Err(CorpusSelectionMetricsError::DuplicateRoomId {
                room_id: room.generated.id.clone(),
            });
        }
        if let Some(first_room_id) = visuals.insert(
            room.generated.static_visual.clone(),
            room.generated.id.clone(),
        ) {
            return Err(CorpusSelectionMetricsError::DuplicateExactStaticVisual {
                first_room_id,
                duplicate_room_id: room.generated.id.clone(),
            });
        }
    }
    Ok(())
}

fn morphology_topology_projection(prefix: &RoomEmbeddingPrefix) -> CorpusSelectionProjection {
    let morphology_names = [
        "solid-tile-density",
        "one-way-tile-density",
        "static-hazard-density",
        "timed-hazard-area",
        "door-count",
        "pickup-count",
        "collision-region-count",
        "exposed-face-length",
        "left-door-presence",
        "right-door-presence",
        "ceiling-door-presence",
        "floor-door-presence",
    ];
    let topology_names = [
        "route-node-count",
        "route-edge-count",
        "cycle-rank",
        "branch-node-count",
        "vertical-span",
        "wall-edge-fraction",
        "dash-edge-fraction",
        "interior-component-count",
    ];
    let mut detail = morphology_names
        .into_iter()
        .zip(prefix.morphology)
        .map(|(name, value)| NamedSelectionCoordinate::observed(name, value))
        .collect::<Vec<_>>();
    detail.extend(
        topology_names
            .into_iter()
            .zip(prefix.topology)
            .map(|(name, value)| NamedSelectionCoordinate::observed(name, value)),
    );
    let cell_names = [
        "solid-tile-density",
        "one-way-tile-density",
        "door-count",
        "collision-region-count",
        "route-node-count",
        "cycle-rank",
        "branch-node-count",
        "vertical-span",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::MorphologyTopologyV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn directed_controller_projection(metrics: &RoomMetricSummary) -> CorpusSelectionProjection {
    let mut detail = Vec::new();
    let mut cell_names = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        let slug = loadout.slug();
        cell_names.extend([
            format!("{slug}-known-positive-routes"),
            format!("{slug}-other-controller"),
            format!("{slug}-median-ability-events"),
            format!("{slug}-median-duration"),
        ]);
        let summary = metrics
            .direct_controllers
            .by_loadout
            .iter()
            .find(|summary| summary.loadout == loadout);
        let Some(summary) = summary else {
            for suffix in [
                "known-positive-routes",
                "complete-audits",
                "bounded-audits",
                "missing-audits",
                "run-only",
                "monotone-simple",
                "other-controller",
                "median-controller-class",
                "median-ability-events",
                "median-horizontal-reversals",
                "median-vertical-decisions",
                "median-semantic-spans",
                "median-semantic-transitions",
                "median-duration",
            ] {
                detail.push(NamedSelectionCoordinate::new(
                    format!("{slug}-{suffix}"),
                    QuantizedSelectionEvidence::missing(),
                ));
            }
            continue;
        };
        let route_count = metrics.direct_controllers.directed_route_count;
        detail.push(named_fraction(
            format!("{slug}-known-positive-routes"),
            summary.known_positive_directed_routes,
            route_count,
        ));
        detail.push(named_fraction(
            format!("{slug}-complete-audits"),
            summary.route_audit_completeness.complete_finite_vocabulary,
            summary.route_audit_completeness.expected,
        ));
        detail.push(named_fraction(
            format!("{slug}-bounded-audits"),
            summary.route_audit_completeness.bounded_incomplete,
            summary.route_audit_completeness.expected,
        ));
        detail.push(named_fraction(
            format!("{slug}-missing-audits"),
            summary.route_audit_completeness.missing,
            summary.route_audit_completeness.expected,
        ));
        match &summary.easiest_controller_fractions {
            MetricEvidence::Observed(fractions) => {
                detail.push(named_exact_fraction(
                    format!("{slug}-run-only"),
                    fractions.run_only_fraction,
                ));
                detail.push(named_exact_fraction(
                    format!("{slug}-monotone-simple"),
                    fractions.monotone_simple_fraction,
                ));
                detail.push(named_exact_fraction(
                    format!("{slug}-other-controller"),
                    fractions.other_controller_class_fraction,
                ));
            }
            evidence => {
                let converted = quantized_from_metric(evidence, |_| 0);
                for suffix in ["run-only", "monotone-simple", "other-controller"] {
                    detail.push(NamedSelectionCoordinate::new(
                        format!("{slug}-{suffix}"),
                        converted.clone(),
                    ));
                }
            }
        }
        let coordinate_names = [
            "median-controller-class",
            "median-ability-events",
            "median-horizontal-reversals",
            "median-vertical-decisions",
            "median-semantic-spans",
            "median-semantic-transitions",
            "median-duration",
        ];
        match &summary.demand_coordinates {
            MetricEvidence::Observed(distributions) => {
                let values = [
                    quantized_distribution(distributions.controller_class, 2),
                    quantized_distribution(distributions.ability_events, CONTROLLER_COUNT_CAP),
                    quantized_distribution(
                        distributions.horizontal_reversals,
                        CONTROLLER_COUNT_CAP,
                    ),
                    quantized_distribution(distributions.vertical_decisions, CONTROLLER_COUNT_CAP),
                    quantized_distribution(distributions.semantic_spans, CONTROLLER_COUNT_CAP),
                    quantized_distribution(
                        distributions.semantic_transitions,
                        CONTROLLER_COUNT_CAP,
                    ),
                    quantized_distribution(distributions.duration_ticks, CONTROLLER_DURATION_CAP),
                ];
                for (suffix, value) in coordinate_names.into_iter().zip(values) {
                    detail.push(NamedSelectionCoordinate::observed(
                        format!("{slug}-{suffix}"),
                        value,
                    ));
                }
            }
            evidence => {
                let converted = quantized_from_metric(evidence, |_| 0);
                for suffix in coordinate_names {
                    detail.push(NamedSelectionCoordinate::new(
                        format!("{slug}-{suffix}"),
                        converted.clone(),
                    ));
                }
            }
        }
    }
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::DirectedLoadoutControllerDemandV1,
        cell: select_named_owned(&detail, &cell_names),
        detail,
    }
}

fn landing_geometry_projection(
    metrics: &LandingPrecisionMetricSummary,
) -> CorpusSelectionProjection {
    let mut detail = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        let slug = loadout.slug();
        match metrics
            .by_loadout
            .iter()
            .find(|summary| summary.loadout == loadout)
        {
            Some(summary) => append_landing_geometry(&mut detail, slug, &summary.aggregate),
            None => append_missing_landing_geometry(&mut detail, slug),
        }
    }
    append_landing_geometry(&mut detail, "all-loadouts", &metrics.aggregate);

    let cell_names = [
        "both-routes-with-landing-events",
        "both-measured-landing-fraction",
        "both-minimum-edge-margin-pixels",
        "both-minimum-footprint-overlap-pixels",
        "both-minimum-support-width-pixels",
        "all-loadouts-measured-landing-fraction",
        "all-loadouts-minimum-edge-margin-pixels",
        "all-loadouts-edge-overhang-fraction",
        "all-loadouts-one-way-or-mixed-fraction",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::LandingGeometryV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn append_landing_geometry(
    target: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    aggregate: &LandingPrecisionAggregateMetric,
) {
    target.extend([
        NamedSelectionCoordinate::observed(
            format!("{prefix}-canonical-positive-route-count"),
            quantized_capped(
                aggregate.canonical_positive_route_count,
                CONTROLLER_COUNT_CAP,
            ),
        ),
        named_fraction(
            format!("{prefix}-routes-with-landing-events"),
            aggregate.routes_with_landing_events,
            aggregate.canonical_positive_route_count,
        ),
        named_fraction(
            format!("{prefix}-routes-without-landing-events"),
            aggregate.routes_without_landing_events,
            aggregate.canonical_positive_route_count,
        ),
        named_fraction(
            format!("{prefix}-routes-with-measured-landings"),
            aggregate.routes_with_measured_landings,
            aggregate.canonical_positive_route_count,
        ),
        named_fraction(
            format!("{prefix}-routes-with-unmeasured-landings"),
            aggregate.routes_with_unmeasured_landings,
            aggregate.canonical_positive_route_count,
        ),
        named_fraction(
            format!("{prefix}-routes-with-only-unmeasured-landings"),
            aggregate.routes_with_only_unmeasured_landings,
            aggregate.canonical_positive_route_count,
        ),
        NamedSelectionCoordinate::observed(
            format!("{prefix}-landing-event-count"),
            quantized_capped(aggregate.landing_event_count, LANDING_EVENT_COUNT_CAP),
        ),
        NamedSelectionCoordinate::observed(
            format!("{prefix}-measured-landing-count"),
            quantized_capped(aggregate.measured_landing_count, LANDING_EVENT_COUNT_CAP),
        ),
        NamedSelectionCoordinate::observed(
            format!("{prefix}-unmeasured-landing-count"),
            quantized_capped(aggregate.unmeasured_landing_count, LANDING_EVENT_COUNT_CAP),
        ),
        named_fraction(
            format!("{prefix}-measured-landing-fraction"),
            aggregate.measured_landing_count,
            aggregate.landing_event_count,
        ),
        named_fraction(
            format!("{prefix}-unmeasured-landing-fraction"),
            aggregate.unmeasured_landing_count,
            aggregate.landing_event_count,
        ),
    ]);
    append_landing_signed_distribution(target, prefix, &aggregate.minimum_edge_margin_pixels);
    append_landing_unsigned_distribution(
        target,
        prefix,
        "footprint-overlap-pixels",
        &aggregate.footprint_overlap_pixels,
        LANDING_FOOTPRINT_OVERLAP_CAP_PIXELS,
    );
    append_landing_unsigned_distribution(
        target,
        prefix,
        "support-width-pixels",
        &aggregate.support_width_pixels,
        LANDING_SUPPORT_WIDTH_CAP_PIXELS,
    );
    target.extend([
        named_fraction(
            format!("{prefix}-edge-overhang-fraction"),
            aggregate.edge_overhang_landings,
            aggregate.measured_landing_count,
        ),
        named_fraction(
            format!("{prefix}-one-way-or-mixed-fraction"),
            aggregate.one_way_or_mixed_landings,
            aggregate.measured_landing_count,
        ),
    ]);
}

fn append_missing_landing_geometry(target: &mut Vec<NamedSelectionCoordinate>, prefix: &str) {
    for suffix in [
        "canonical-positive-route-count",
        "routes-with-landing-events",
        "routes-without-landing-events",
        "routes-with-measured-landings",
        "routes-with-unmeasured-landings",
        "routes-with-only-unmeasured-landings",
        "landing-event-count",
        "measured-landing-count",
        "unmeasured-landing-count",
        "measured-landing-fraction",
        "unmeasured-landing-fraction",
        "minimum-edge-margin-pixels",
        "median-lower-edge-margin-pixels",
        "median-upper-edge-margin-pixels",
        "maximum-edge-margin-pixels",
        "minimum-footprint-overlap-pixels",
        "median-lower-footprint-overlap-pixels",
        "median-upper-footprint-overlap-pixels",
        "maximum-footprint-overlap-pixels",
        "minimum-support-width-pixels",
        "median-lower-support-width-pixels",
        "median-upper-support-width-pixels",
        "maximum-support-width-pixels",
        "edge-overhang-fraction",
        "one-way-or-mixed-fraction",
    ] {
        target.push(NamedSelectionCoordinate::new(
            format!("{prefix}-{suffix}"),
            QuantizedSelectionEvidence::missing(),
        ));
    }
}

fn append_landing_signed_distribution(
    target: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    evidence: &LandingCoordinateEvidence<SignedIntegerCoordinateDistribution>,
) {
    let names = [
        "minimum-edge-margin-pixels",
        "median-lower-edge-margin-pixels",
        "median-upper-edge-margin-pixels",
        "maximum-edge-margin-pixels",
    ];
    match evidence {
        LandingCoordinateEvidence::Observed(distribution) => {
            for (name, value) in names.into_iter().zip([
                distribution.minimum,
                distribution.median_lower,
                distribution.median_upper,
                distribution.maximum,
            ]) {
                target.push(NamedSelectionCoordinate::observed(
                    format!("{prefix}-{name}"),
                    quantized_signed_edge_margin(value),
                ));
            }
        }
        LandingCoordinateEvidence::NotApplicable { .. } => {
            for name in names {
                target.push(NamedSelectionCoordinate::new(
                    format!("{prefix}-{name}"),
                    QuantizedSelectionEvidence::not_applicable(),
                ));
            }
        }
    }
}

fn append_landing_unsigned_distribution(
    target: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    coordinate: &str,
    evidence: &LandingCoordinateEvidence<IntegerCoordinateDistribution>,
    cap: usize,
) {
    let names = ["minimum", "median-lower", "median-upper", "maximum"];
    match evidence {
        LandingCoordinateEvidence::Observed(distribution) => {
            for (name, value) in names.into_iter().zip([
                distribution.minimum,
                distribution.median_lower,
                distribution.median_upper,
                distribution.maximum,
            ]) {
                target.push(NamedSelectionCoordinate::observed(
                    format!("{prefix}-{name}-{coordinate}"),
                    quantized_capped(value, cap),
                ));
            }
        }
        LandingCoordinateEvidence::NotApplicable { .. } => {
            for name in names {
                target.push(NamedSelectionCoordinate::new(
                    format!("{prefix}-{name}-{coordinate}"),
                    QuantizedSelectionEvidence::not_applicable(),
                ));
            }
        }
    }
}

fn directional_asymmetry_projection(metrics: &RoomMetricSummary) -> CorpusSelectionProjection {
    let total = metrics.directional_asymmetry.len();
    let mut observed = Vec::new();
    let mut missing = 0;
    let mut not_applicable = 0;
    let mut inconclusive = 0;
    let mut ability_differs = 0;
    for row in &metrics.directional_asymmetry {
        match &row.comparison {
            MetricEvidence::Observed(values) => {
                observed.push(*values);
                ability_differs += usize::from(
                    values.ability_use.wall_jump_use_differs || values.ability_use.dash_use_differs,
                );
            }
            MetricEvidence::NotApplicable { .. } => not_applicable += 1,
            MetricEvidence::Missing { reason } => {
                let bounded = [&row.a_to_b, &row.b_to_a].into_iter().any(|evidence| {
                    matches!(
                        evidence,
                        MetricEvidence::Missing {
                            reason:
                                MissingMetricReason::BoundedDirectControllerAuditWithoutPositive
                        }
                    )
                });
                if bounded {
                    inconclusive += 1;
                } else {
                    let _ = reason;
                    missing += 1;
                }
            }
        }
    }
    let aggregate_state = if !observed.is_empty() {
        SelectionEvidenceState::Observed
    } else if inconclusive > 0 {
        SelectionEvidenceState::BoundedInconclusive
    } else if missing > 0 {
        SelectionEvidenceState::Missing
    } else {
        SelectionEvidenceState::NotApplicable
    };
    let aggregate = |value: Option<u16>| QuantizedSelectionEvidence {
        state: aggregate_state,
        value: (aggregate_state == SelectionEvidenceState::Observed)
            .then(|| value.unwrap_or_default()),
    };
    let duration_values = observed
        .iter()
        .map(|values| values.duration_ticks.absolute_difference)
        .collect::<Vec<_>>();
    let span_values = observed
        .iter()
        .map(|values| values.semantic_spans.absolute_difference)
        .collect::<Vec<_>>();
    let transition_values = observed
        .iter()
        .map(|values| values.semantic_transitions.absolute_difference)
        .collect::<Vec<_>>();
    let detail = vec![
        named_fraction("observed-comparisons", observed.len(), total),
        named_fraction("missing-comparisons", missing, total),
        named_fraction("not-applicable-comparisons", not_applicable, total),
        named_fraction("bounded-inconclusive-comparisons", inconclusive, total),
        NamedSelectionCoordinate::new(
            "median-duration-difference",
            aggregate(
                median(&duration_values)
                    .map(|value| quantized_capped(value, CONTROLLER_DURATION_CAP)),
            ),
        ),
        NamedSelectionCoordinate::new(
            "maximum-duration-difference",
            aggregate(
                duration_values
                    .iter()
                    .max()
                    .copied()
                    .map(|value| quantized_capped(value, CONTROLLER_DURATION_CAP)),
            ),
        ),
        NamedSelectionCoordinate::new(
            "median-semantic-span-difference",
            aggregate(
                median(&span_values).map(|value| quantized_capped(value, CONTROLLER_COUNT_CAP)),
            ),
        ),
        NamedSelectionCoordinate::new(
            "median-semantic-transition-difference",
            aggregate(
                median(&transition_values)
                    .map(|value| quantized_capped(value, CONTROLLER_COUNT_CAP)),
            ),
        ),
        if observed.is_empty() {
            NamedSelectionCoordinate::new("ability-use-differs", aggregate(None))
        } else {
            named_fraction("ability-use-differs", ability_differs, observed.len())
        },
    ];
    let cell_names = [
        "observed-comparisons",
        "bounded-inconclusive-comparisons",
        "median-duration-difference",
        "median-semantic-span-difference",
        "ability-use-differs",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::DirectionalAsymmetryV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn route_diversity_projection(metrics: &RoomMetricSummary) -> CorpusSelectionProjection {
    let mut detail = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        let slug = loadout.slug();
        if let Some(summary) = metrics
            .canonical_routes
            .by_loadout
            .iter()
            .find(|summary| summary.loadout == loadout)
        {
            detail.push(named_fraction(
                format!("{slug}-positive-routes"),
                summary.positive_route_count,
                summary.route_cell_count,
            ));
            append_route_diversity(&mut detail, slug, &summary.behavior_diversity);
        } else {
            for suffix in [
                "positive-routes",
                "spatial-path-classes",
                "semantic-controller-classes",
                "joint-style-classes",
                "mean-nearest-combined-distance",
                "median-combined-distance",
            ] {
                detail.push(NamedSelectionCoordinate::new(
                    format!("{slug}-{suffix}"),
                    QuantizedSelectionEvidence::missing(),
                ));
            }
        }
    }
    append_route_diversity(
        &mut detail,
        "all-loadouts",
        &metrics.canonical_routes.behavior_diversity,
    );
    let cell_names = [
        "baseline-joint-style-classes",
        "wall-jump-joint-style-classes",
        "dash-joint-style-classes",
        "both-joint-style-classes",
        "all-loadouts-mean-nearest-combined-distance",
        "all-loadouts-median-combined-distance",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::RouteDiversityV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn append_route_diversity(
    target: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    report: &downwards_lab::RouteDiversityReport,
) {
    target.push(named_fraction(
        format!("{prefix}-spatial-path-classes"),
        report.spatial_path_classes,
        report.route_count,
    ));
    target.push(named_fraction(
        format!("{prefix}-semantic-controller-classes"),
        report.semantic_controller_classes,
        report.route_count,
    ));
    target.push(named_fraction(
        format!("{prefix}-joint-style-classes"),
        report.joint_play_style_classes,
        report.route_count,
    ));
    target.push(NamedSelectionCoordinate::new(
        format!("{prefix}-mean-nearest-combined-distance"),
        optional_unit_interval(report.combined_behavior_distance.mean_nearest_neighbor),
    ));
    target.push(NamedSelectionCoordinate::new(
        format!("{prefix}-median-combined-distance"),
        optional_unit_interval(report.combined_behavior_distance.median),
    ));
}

fn ability_bypass_projection(metrics: &RoomMetricSummary) -> CorpusSelectionProjection {
    let bypass = &metrics.direct_controllers.ability_bypasses;
    let routes = metrics.direct_controllers.directed_route_count;
    let mut detail = vec![
        named_fraction(
            "routes-with-any-positive-lower-loadout-bypass",
            bypass.directed_routes_with_any_bypass,
            routes,
        ),
        named_fraction(
            "routes-with-wall-jump-bypass",
            bypass.directed_routes_with_wall_jump_bypass,
            routes,
        ),
        named_fraction(
            "routes-with-dash-bypass",
            bypass.directed_routes_with_dash_bypass,
            routes,
        ),
        NamedSelectionCoordinate::observed(
            "retained-semantic-bypass-witnesses",
            quantized_capped(bypass.retained_semantic_bypass_witnesses, 64),
        ),
    ];
    for loadout in EvaluationLoadout::ALL {
        let row = bypass
            .by_successful_loadout
            .iter()
            .find(|row| row.successful_loadout == loadout);
        detail.push(row.map_or_else(
            || {
                NamedSelectionCoordinate::new(
                    format!("{}-positive-bypass-routes", loadout.slug()),
                    if routes == 0 {
                        QuantizedSelectionEvidence::not_applicable()
                    } else {
                        QuantizedSelectionEvidence::observed(0)
                    },
                )
            },
            |row| {
                named_fraction(
                    format!("{}-positive-bypass-routes", loadout.slug()),
                    row.directed_route_bypasses,
                    routes,
                )
            },
        ));
    }
    let cell_names = [
        "routes-with-any-positive-lower-loadout-bypass",
        "routes-with-wall-jump-bypass",
        "routes-with-dash-bypass",
        "baseline-positive-bypass-routes",
        "wall-jump-positive-bypass-routes",
        "dash-positive-bypass-routes",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::AbilityBypassStructureV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn terrain_ablation_projection(metrics: &RoomMetricSummary) -> CorpusSelectionProjection {
    let coverage = &metrics.terrain.coverage_fractions;
    let utility = metrics.terrain.ablation_utility;
    let tested_variants = utility
        .all_stored_controllers_survived
        .saturating_add(utility.at_least_one_stored_controller_affected);
    let detail = vec![
        named_metric_fraction(
            "structurally-attributed-components",
            &coverage.structurally_attributed_components,
        ),
        named_metric_fraction(
            "structurally-attributed-tiles",
            &coverage.structurally_attributed_tiles,
        ),
        named_metric_fraction(
            "traversal-near-components",
            &coverage.traversal_near_components,
        ),
        named_metric_fraction("traversal-near-tiles", &coverage.traversal_near_tiles),
        named_metric_fraction(
            "positively-corroborated-components",
            &coverage.positively_corroborated_components,
        ),
        named_metric_fraction(
            "positively-corroborated-tiles",
            &coverage.positively_corroborated_tiles,
        ),
        named_fraction(
            "ablations-affecting-stored-controllers",
            utility.at_least_one_stored_controller_affected,
            tested_variants,
        ),
        named_fraction(
            "ablations-all-stored-controllers-survive",
            utility.all_stored_controllers_survived,
            tested_variants,
        ),
        named_fraction(
            "ablations-with-no-stored-controllers",
            utility.no_stored_controllers,
            utility.variant_count,
        ),
        named_metric_fraction(
            "aggregate-exact-controller-survival",
            &metrics.terrain.aggregate_ablation_survival_fraction,
        ),
    ];
    let cell_names = [
        "structurally-attributed-tiles",
        "traversal-near-tiles",
        "positively-corroborated-tiles",
        "ablations-affecting-stored-controllers",
        "ablations-all-stored-controllers-survive",
        "ablations-with-no-stored-controllers",
        "aggregate-exact-controller-survival",
    ];
    CorpusSelectionProjection {
        id: CorpusSelectionProjectionId::TerrainAblationUtilityV1,
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn quality_coordinates(metrics: &RoomMetricSummary) -> Vec<CorpusSelectionQualityCoordinate> {
    let all_loadouts = &metrics.direct_controllers.by_loadout;
    let complete_kit = all_loadouts
        .iter()
        .find(|summary| summary.loadout == EvaluationLoadout::Both);
    let other_controller = complete_kit.map_or_else(
        QuantizedSelectionEvidence::missing,
        |summary| match &summary.easiest_controller_fractions {
            MetricEvidence::Observed(fractions) => {
                quantized_exact_fraction(fractions.other_controller_class_fraction)
            }
            evidence => quantized_from_metric(evidence, |_| 0),
        },
    );
    let median_duration = complete_kit.map_or_else(
        QuantizedSelectionEvidence::missing,
        |summary| match &summary.demand_coordinates {
            MetricEvidence::Observed(distributions) => QuantizedSelectionEvidence::observed(
                quantized_distribution(distributions.duration_ticks, CONTROLLER_DURATION_CAP),
            ),
            evidence => quantized_from_metric(evidence, |_| 0),
        },
    );
    let asymmetry = directional_asymmetry_projection(metrics)
        .detail
        .into_iter()
        .find(|coordinate| coordinate.name == "maximum-duration-difference")
        .map_or_else(QuantizedSelectionEvidence::missing, |coordinate| {
            coordinate.evidence
        });
    let diversity = if metrics.canonical_routes.behavior_diversity.route_count == 0 {
        QuantizedSelectionEvidence::not_applicable()
    } else {
        QuantizedSelectionEvidence::observed(quantized_ratio(
            metrics
                .canonical_routes
                .behavior_diversity
                .joint_play_style_classes,
            metrics.canonical_routes.behavior_diversity.route_count,
        ))
    };
    // Fractions remain useful diversity descriptors, but using only a
    // corroborated fraction as quality would let a nearly empty room dominate
    // richer terrain.  Pareto quality therefore keeps absolute positive-
    // evidence gaps.  These are audit priorities, not claims that an
    // uncorroborated tile is useless or unreachable.
    let terrain_coverage = metrics.terrain.coverage;
    let uncorroborated_components = if terrain_coverage.interior_component_count == 0 {
        QuantizedSelectionEvidence::not_applicable()
    } else if terrain_coverage.positive_controller_count == 0 {
        QuantizedSelectionEvidence::missing()
    } else {
        QuantizedSelectionEvidence::observed(quantized_capped(
            terrain_coverage.uncorroborated_component_count,
            16,
        ))
    };
    let uncorroborated_tiles = if terrain_coverage.interior_tile_count == 0 {
        QuantizedSelectionEvidence::not_applicable()
    } else if terrain_coverage.positive_controller_count == 0 {
        QuantizedSelectionEvidence::missing()
    } else {
        QuantizedSelectionEvidence::observed(quantized_capped(
            terrain_coverage.uncorroborated_tile_count,
            512,
        ))
    };
    let utility = metrics.terrain.ablation_utility;
    let tested = utility
        .all_stored_controllers_survived
        .saturating_add(utility.at_least_one_stored_controller_affected);
    let ablation_effect = if tested == 0 {
        QuantizedSelectionEvidence::not_applicable()
    } else {
        QuantizedSelectionEvidence::observed(quantized_ratio(
            utility.at_least_one_stored_controller_affected,
            tested,
        ))
    };
    let operational_ticks = metrics
        .operational_cost
        .direct_controller_reported_total
        .simulated_ticks
        .saturating_add(
            metrics
                .operational_cost
                .canonical_positive_route_total
                .simulated_ticks,
        );

    vec![
        quality_axis(
            CorpusSelectionQualityAxis::CompleteKitOtherControllerDemand,
            ObjectiveDirection::Maximize,
            other_controller,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::CompleteKitMedianDuration,
            ObjectiveDirection::Maximize,
            median_duration,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::DirectionalDurationAsymmetry,
            ObjectiveDirection::Maximize,
            asymmetry,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::JointRouteStyleDiversity,
            ObjectiveDirection::Maximize,
            diversity,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::UncorroboratedTerrainComponents,
            ObjectiveDirection::Minimize,
            uncorroborated_components,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::UncorroboratedTerrainTiles,
            ObjectiveDirection::Minimize,
            uncorroborated_tiles,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::AblatedTilesAffectingStoredControllers,
            ObjectiveDirection::Maximize,
            if tested == 0 {
                QuantizedSelectionEvidence::not_applicable()
            } else {
                QuantizedSelectionEvidence::observed(quantized_capped(
                    utility.removed_tiles_some_affected,
                    512,
                ))
            },
        ),
        quality_axis(
            CorpusSelectionQualityAxis::ControllerAblationEffect,
            ObjectiveDirection::Maximize,
            ablation_effect,
        ),
        quality_axis(
            CorpusSelectionQualityAxis::OperationalSimulatedTicks,
            ObjectiveDirection::Minimize,
            QuantizedSelectionEvidence::observed(quantized_capped(
                operational_ticks,
                OPERATIONAL_TICK_CAP,
            )),
        ),
    ]
}

fn quality_axis(
    axis: CorpusSelectionQualityAxis,
    direction: ObjectiveDirection,
    evidence: QuantizedSelectionEvidence,
) -> CorpusSelectionQualityCoordinate {
    CorpusSelectionQualityCoordinate {
        axis,
        direction,
        evidence,
    }
}

/// Flat generic-archive directions corresponding to the one-hot expansion of
/// every semantic Pareto axis.
#[must_use]
pub fn corpus_selection_quality_directions() -> Vec<ObjectiveDirection> {
    let semantic_directions = [
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Minimize,
        ObjectiveDirection::Minimize,
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Maximize,
        ObjectiveDirection::Minimize,
    ];
    semantic_directions
        .into_iter()
        .flat_map(|numeric| {
            [
                ObjectiveDirection::Maximize,
                ObjectiveDirection::Maximize,
                ObjectiveDirection::Maximize,
                ObjectiveDirection::Maximize,
                numeric,
            ]
        })
        .collect()
}

fn flatten_quality(quality: &[CorpusSelectionQualityCoordinate]) -> Vec<i64> {
    quality
        .iter()
        .flat_map(|coordinate| {
            let one_hot = SelectionEvidenceState::ALL
                .map(|state| i64::from(coordinate.evidence.state == state));
            one_hot
                .into_iter()
                .chain([i64::from(coordinate.evidence.value.unwrap_or_default())])
        })
        .collect()
}

fn flatten_cell(coordinates: &[NamedSelectionCoordinate]) -> Vec<i64> {
    coordinates
        .iter()
        .flat_map(|coordinate| {
            [
                coordinate.evidence.state.cell_code(),
                i64::from(cell_bucket(coordinate.evidence.value)),
            ]
        })
        .collect()
}

fn flatten_diversity(projections: &[CorpusSelectionProjection]) -> Vec<i64> {
    projections
        .iter()
        .flat_map(|projection| {
            let dimensions = projection.detail.len().saturating_mul(5).max(1);
            let coordinate_scale =
                DIVERSITY_BUDGET_PER_PROJECTION / i64::try_from(dimensions).unwrap_or(i64::MAX);
            projection.detail.iter().flat_map(move |coordinate| {
                let one_hot = SelectionEvidenceState::ALL.map(|state| {
                    if coordinate.evidence.state == state {
                        coordinate_scale
                    } else {
                        0
                    }
                });
                let numeric = i64::from(coordinate.evidence.value.unwrap_or_default())
                    .saturating_mul(coordinate_scale)
                    / i64::from(u16::MAX);
                one_hot.into_iter().chain([numeric])
            })
        })
        .collect()
}

fn select_named(
    detail: &[NamedSelectionCoordinate],
    names: &[&str],
) -> Vec<NamedSelectionCoordinate> {
    names
        .iter()
        .filter_map(|name| detail.iter().find(|coordinate| coordinate.name == *name))
        .cloned()
        .collect()
}

fn select_named_owned(
    detail: &[NamedSelectionCoordinate],
    names: &[String],
) -> Vec<NamedSelectionCoordinate> {
    names
        .iter()
        .filter_map(|name| detail.iter().find(|coordinate| coordinate.name == *name))
        .cloned()
        .collect()
}

fn named_fraction(
    name: impl Into<String>,
    numerator: usize,
    denominator: usize,
) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate::new(name, fraction_or_na(numerator, denominator))
}

fn named_exact_fraction(
    name: impl Into<String>,
    fraction: ExactFraction,
) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate::new(name, quantized_exact_fraction(fraction))
}

fn named_metric_fraction(
    name: impl Into<String>,
    evidence: &MetricEvidence<ExactFraction>,
) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate::new(
        name,
        quantized_from_metric(evidence, |fraction| {
            quantized_ratio(fraction.numerator, fraction.denominator)
        }),
    )
}

fn fraction_or_na(numerator: usize, denominator: usize) -> QuantizedSelectionEvidence {
    if denominator == 0 {
        QuantizedSelectionEvidence::not_applicable()
    } else {
        QuantizedSelectionEvidence::observed(quantized_ratio(numerator, denominator))
    }
}

fn quantized_exact_fraction(fraction: ExactFraction) -> QuantizedSelectionEvidence {
    QuantizedSelectionEvidence::observed(quantized_ratio(fraction.numerator, fraction.denominator))
}

fn quantized_from_metric<T>(
    evidence: &MetricEvidence<T>,
    observed: impl FnOnce(&T) -> u16,
) -> QuantizedSelectionEvidence {
    match evidence {
        MetricEvidence::Observed(value) => QuantizedSelectionEvidence::observed(observed(value)),
        MetricEvidence::Missing { reason } => match reason {
            MissingMetricReason::BoundedDirectControllerAuditWithoutPositive => {
                QuantizedSelectionEvidence::bounded_inconclusive()
            }
            MissingMetricReason::MissingDirectedRouteAssessment
            | MissingMetricReason::MissingDirectControllerAudit
            | MissingMetricReason::NoPositiveTerrainController
            | MissingMetricReason::ReverseDirectionsRequireTwoKnownPositiveControllers => {
                QuantizedSelectionEvidence::missing()
            }
        },
        MetricEvidence::NotApplicable { reason } => {
            match reason {
                NotApplicableMetricReason::NoDirectedRoutes
                | NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary
                | NotApplicableMetricReason::NoSuccessfulRoutesForLoadout
                | NotApplicableMetricReason::AmbiguousNondominatedFront
                | NotApplicableMetricReason::NoInteriorTerrainComponents
                | NotApplicableMetricReason::NoInteriorTerrainTiles
                | NotApplicableMetricReason::NoAblationVariants
                | NotApplicableMetricReason::NoExactControllersForAblation => (),
            }
            QuantizedSelectionEvidence::not_applicable()
        }
    }
}

fn optional_unit_interval(value: Option<f64>) -> QuantizedSelectionEvidence {
    value.map_or_else(QuantizedSelectionEvidence::not_applicable, |value| {
        let clamped = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        QuantizedSelectionEvidence::observed((clamped * f64::from(u16::MAX)).round() as u16)
    })
}

fn quantized_distribution(distribution: IntegerCoordinateDistribution, cap: usize) -> u16 {
    let median_sum = distribution
        .median_lower
        .saturating_add(distribution.median_upper);
    quantized_capped(median_sum, cap.saturating_mul(2))
}

fn quantized_ratio(numerator: usize, denominator: usize) -> u16 {
    if denominator == 0 {
        return 0;
    }
    let numerator = numerator.min(denominator) as u128;
    let denominator = denominator as u128;
    let scaled = numerator.saturating_mul(u128::from(u16::MAX));
    ((scaled + denominator / 2) / denominator) as u16
}

fn quantized_capped(value: usize, cap: usize) -> u16 {
    quantized_ratio(value.min(cap), cap)
}

/// Map signed edge margins around an exact zero boundary. Every negative
/// value (an overhang) maps below 32768 and every nonnegative value maps to
/// 32768 or above, so cell quantization cannot merge a -1 px overhang with a
/// flush 0 px landing.
fn quantized_signed_edge_margin(value: i32) -> u16 {
    const ZERO: u32 = u16::MAX as u32 / 2 + 1;
    if value < 0 {
        let magnitude = value
            .unsigned_abs()
            .min(LANDING_NEGATIVE_EDGE_MARGIN_CAP_PIXELS);
        let offset = magnitude
            .saturating_mul(ZERO)
            .saturating_add(LANDING_NEGATIVE_EDGE_MARGIN_CAP_PIXELS - 1)
            / LANDING_NEGATIVE_EDGE_MARGIN_CAP_PIXELS;
        u16::try_from(ZERO.saturating_sub(offset)).unwrap_or_default()
    } else {
        let margin = value
            .unsigned_abs()
            .min(LANDING_POSITIVE_EDGE_MARGIN_CAP_PIXELS);
        let positive_range = u32::from(u16::MAX) - ZERO;
        let offset = margin
            .saturating_mul(positive_range)
            .saturating_add(LANDING_POSITIVE_EDGE_MARGIN_CAP_PIXELS / 2)
            / LANDING_POSITIVE_EDGE_MARGIN_CAP_PIXELS;
        u16::try_from(ZERO.saturating_add(offset)).unwrap_or(u16::MAX)
    }
}

fn cell_bucket(value: Option<u16>) -> u16 {
    let Some(value) = value else {
        return 0;
    };
    let bucket = u32::from(value).saturating_mul(CELL_BUCKET_COUNT) / (u32::from(u16::MAX) + 1);
    u16::try_from(bucket.min(CELL_BUCKET_COUNT - 1)).unwrap_or_default()
}

fn median(values: &[usize]) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let upper = sorted.len() / 2;
    let lower = (sorted.len() - 1) / 2;
    Some(sorted[lower].saturating_add(sorted[upper]) / 2)
}

fn mate_covered_core(
    offered: &BTreeSet<RoomId>,
    descriptors: &BTreeMap<RoomId, CorpusSelectionDescriptor>,
) -> BTreeSet<RoomId> {
    let mut active = offered.clone();
    loop {
        let providers = socket_providers(&active, descriptors);
        let removals = active
            .iter()
            .filter(|room_id| {
                descriptors[*room_id].sockets.iter().any(|socket| {
                    !providers
                        .get(&socket.mate())
                        .is_some_and(|ids| ids.iter().any(|provider_id| provider_id != *room_id))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if removals.is_empty() {
            return active;
        }
        for room_id in removals {
            active.remove(&room_id);
        }
    }
}

fn mate_coverage_is_complete(
    selected: &BTreeSet<RoomId>,
    descriptors: &BTreeMap<RoomId, CorpusSelectionDescriptor>,
) -> bool {
    mate_covered_core(selected, descriptors) == *selected
}

fn socket_providers(
    selected: &BTreeSet<RoomId>,
    descriptors: &BTreeMap<RoomId, CorpusSelectionDescriptor>,
) -> BTreeMap<DoorSocket, BTreeSet<RoomId>> {
    let mut providers = BTreeMap::<DoorSocket, BTreeSet<RoomId>>::new();
    for room_id in selected {
        for socket in &descriptors[room_id].sockets {
            providers
                .entry(*socket)
                .or_default()
                .insert(room_id.clone());
        }
    }
    providers
}

fn reusable_socket_packages(
    selected: &BTreeSet<RoomId>,
    descriptors: &BTreeMap<RoomId, CorpusSelectionDescriptor>,
) -> Result<Vec<ReusableSocketSelectionPackage>, CorpusSelectionMetricsError> {
    let providers = socket_providers(selected, descriptors);
    let mut unseen = selected.clone();
    let mut packages = Vec::new();
    while let Some(seed) = unseen.first().cloned() {
        let mut pending = vec![seed.clone()];
        let mut members = BTreeSet::new();
        unseen.remove(&seed);
        while let Some(room_id) = pending.pop() {
            if !members.insert(room_id.clone()) {
                continue;
            }
            for socket in &descriptors[&room_id].sockets {
                if let Some(mates) = providers.get(&socket.mate()) {
                    for mate in mates {
                        if mate != &room_id && unseen.remove(mate) {
                            pending.push(mate.clone());
                        }
                    }
                }
            }
        }
        let room_ids = members.into_iter().collect::<Vec<_>>();
        let component = room_ids.iter().cloned().collect::<BTreeSet<_>>();
        if !mate_coverage_is_complete(&component, descriptors) {
            return Err(CorpusSelectionMetricsError::SocketCoverageInvariant {
                detail: format!(
                    "compatibility component beginning with {} is not independently covered",
                    room_ids[0].0
                ),
            });
        }
        packages.push(ReusableSocketSelectionPackage {
            package_id: room_ids[0].clone(),
            room_ids,
        });
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use downwards_ai::{DifficultyConfig, DirectProbeBudgetLimit, SearchStats, SolverConfig};
    use downwards_core::{AbilitySet, BoundarySide};
    use downwards_gen::experimental::{ChallengeIntent, PartitionRouteKey, PartitionRouteProfile};
    use downwards_lab::{ROOM_ABLATION_VERSION, TraversalGrid};
    use downwards_validation::ValidationConfig;

    use super::*;
    use crate::corpus::{
        CORPUS_ROOM_ANALYSIS_VERSION, CorpusBuildConfigV1, CorpusBuildConfigV2,
        CorpusFeasibilityGateState, CorpusRoomAnalysisConfig, GeneratedCorpusBatchV2,
        GeneratedCorpusRoomV2, GenerationBatchSummaryV2, LandingCoordinateNotApplicableReason,
        LandingPrecisionLoadoutMetricSummary, PositiveTerrainCoverage, TERRAIN_AUDIT_VERSION,
        TerrainAuditReport, analyze_corpus_room, analyze_corpus_room_v2, analyze_pickup_detours,
        analyze_pickup_detours_v2, analyze_route_choice_diversity,
        analyze_route_choice_diversity_v2, evaluate_route_matrices,
        evaluate_route_matrices_v2_with, generate_seed_block, select_canonical_regeneration_v2,
    };
    use crate::structural::{STRUCTURAL_DESCRIPTOR_VERSION, TerrainUtilityDescriptor};

    fn synthetic_descriptor(
        id: &str,
        first_quality: u16,
        second_quality: u16,
        state: SelectionEvidenceState,
        sockets: Vec<DoorSocket>,
    ) -> CorpusSelectionDescriptor {
        let evidence = QuantizedSelectionEvidence {
            state,
            value: (state == SelectionEvidenceState::Observed).then_some(first_quality),
        };
        let projection_coordinate = NamedSelectionCoordinate::new("value", evidence.clone());
        let projections = vec![CorpusSelectionProjection {
            id: CorpusSelectionProjectionId::MorphologyTopologyV1,
            cell: vec![projection_coordinate.clone()],
            detail: vec![projection_coordinate],
        }];
        CorpusSelectionDescriptor {
            version: CORPUS_SELECTION_METRICS_VERSION,
            room_id: RoomId(id.to_owned()),
            diversity_coordinates: flatten_diversity(&projections),
            projections,
            quality: vec![
                quality_axis(
                    CorpusSelectionQualityAxis::CompleteKitOtherControllerDemand,
                    ObjectiveDirection::Maximize,
                    evidence,
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::CompleteKitMedianDuration,
                    ObjectiveDirection::Maximize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::DirectionalDurationAsymmetry,
                    ObjectiveDirection::Maximize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::JointRouteStyleDiversity,
                    ObjectiveDirection::Maximize,
                    QuantizedSelectionEvidence::observed(second_quality),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::UncorroboratedTerrainComponents,
                    ObjectiveDirection::Minimize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::UncorroboratedTerrainTiles,
                    ObjectiveDirection::Minimize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::AblatedTilesAffectingStoredControllers,
                    ObjectiveDirection::Maximize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::ControllerAblationEffect,
                    ObjectiveDirection::Maximize,
                    QuantizedSelectionEvidence::observed(0),
                ),
                quality_axis(
                    CorpusSelectionQualityAxis::OperationalSimulatedTicks,
                    ObjectiveDirection::Minimize,
                    QuantizedSelectionEvidence::observed(0),
                ),
            ],
            sockets,
        }
    }

    fn synthetic_archive(
        descriptors: &BTreeMap<RoomId, CorpusSelectionDescriptor>,
    ) -> QualityDiversityArchive<RoomId, CorpusSelectionProjectionId> {
        QualityDiversityArchive::build(
            ArchiveConfig::new(corpus_selection_quality_directions(), 8),
            descriptors
                .values()
                .map(CorpusSelectionDescriptor::to_archive_candidate),
        )
        .unwrap()
    }

    fn landing_aggregate_fixture(
        landing_event_count: usize,
        measured_landing_count: usize,
        unmeasured_landing_count: usize,
        coordinate_evidence: LandingCoordinateEvidence<SignedIntegerCoordinateDistribution>,
    ) -> LandingPrecisionAggregateMetric {
        let has_events = landing_event_count > 0;
        let has_measured = measured_landing_count > 0;
        let has_unmeasured = unmeasured_landing_count > 0;
        let unsigned_evidence = match coordinate_evidence {
            LandingCoordinateEvidence::Observed(_) => {
                LandingCoordinateEvidence::Observed(IntegerCoordinateDistribution {
                    sample_count: measured_landing_count,
                    minimum: 8,
                    median_lower: 8,
                    median_upper: 8,
                    maximum: 8,
                    spread: 0,
                })
            }
            LandingCoordinateEvidence::NotApplicable { reason } => {
                LandingCoordinateEvidence::NotApplicable { reason }
            }
        };
        LandingPrecisionAggregateMetric {
            canonical_positive_route_count: 1,
            inspected_ticks: 20,
            routes_with_landing_events: usize::from(has_events),
            routes_without_landing_events: usize::from(!has_events),
            routes_with_measured_landings: usize::from(has_measured),
            routes_with_unmeasured_landings: usize::from(has_unmeasured),
            routes_with_only_unmeasured_landings: usize::from(has_unmeasured && !has_measured),
            landing_event_count,
            measured_landing_count,
            unmeasured_landing_count,
            minimum_edge_margin_pixels: coordinate_evidence,
            footprint_overlap_pixels: unsigned_evidence.clone(),
            support_width_pixels: unsigned_evidence,
            edge_overhang_landings: 0,
            one_way_or_mixed_landings: 0,
        }
    }

    fn landing_summary_fixture(
        aggregate: LandingPrecisionAggregateMetric,
    ) -> LandingPrecisionMetricSummary {
        LandingPrecisionMetricSummary {
            source_landing_precision_versions: vec![1],
            by_loadout: EvaluationLoadout::ALL
                .into_iter()
                .map(|loadout| LandingPrecisionLoadoutMetricSummary {
                    loadout,
                    aggregate: aggregate.clone(),
                })
                .collect(),
            aggregate,
        }
    }

    fn empty_metric_summary(room_id: RoomId) -> RoomMetricSummary {
        summarize_room_metrics(&CorpusRoomAnalysis {
            version: CORPUS_ROOM_ANALYSIS_VERSION,
            config: CorpusRoomAnalysisConfig::default()
                .identity_record()
                .expect("default deep-analysis config has a stable identity"),
            room_id,
            source_route_assessments: Vec::new(),
            direct_controller_operational_stats: SearchStats::default(),
            canonical_route_measurements: Vec::new(),
            fused_route_cells: Vec::new(),
            terrain_audit: TerrainAuditReport {
                version: TERRAIN_AUDIT_VERSION,
                structural_descriptor_version: STRUCTURAL_DESCRIPTOR_VERSION,
                room_ablation_version: ROOM_ABLATION_VERSION,
                loadout: AbilitySet::ALL,
                positive_door_controller_count: 0,
                positive_pickup_controller_count: 0,
                positive_witnesses: Vec::new(),
                aggregate_terrain_utility: TerrainUtilityDescriptor::default(),
                coverage: PositiveTerrainCoverage::default(),
                ablations: Vec::new(),
            },
        })
        .expect("empty explicit-evidence analysis is well formed")
    }

    fn evaluated_partition_fixture(
        seed: u64,
        intent: ChallengeIntent,
        profile: PartitionRouteProfile,
    ) -> Option<EvaluatedCorpusRoomV2> {
        let candidate = PartitionRouteKey::new(seed, AbilitySet::NONE, intent, profile)
            .regenerate()
            .ok()
            .map(CorpusCandidate::from)?;
        let physical_descriptor = candidate.physical_room_descriptor_v3();
        let batch = GeneratedCorpusBatchV2 {
            config: CorpusBuildConfigV2::attempt_zero(seed, 1),
            construction_records: Vec::new(),
            rooms: vec![GeneratedCorpusRoomV2 {
                id: physical_descriptor.room_id(),
                physical_descriptor,
                variants: vec![candidate],
            }],
            summary: GenerationBatchSummaryV2::default(),
        };
        let evaluated = evaluate_route_matrices_v2_with(batch, |loadout| {
            ValidationConfig::for_loadout(loadout.abilities())
        })
        .ok()?
        .rooms
        .pop()?;
        evaluated
            .canonical_regeneration
            .selected_key
            .is_some()
            .then_some(evaluated)
    }

    fn real_partition_fixture() -> EvaluatedCorpusRoomV2 {
        static FIXTURE: OnceLock<EvaluatedCorpusRoomV2> = OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                (0_u64..64)
                    .find_map(|seed| {
                        evaluated_partition_fixture(
                            seed,
                            ChallengeIntent::Standard,
                            PartitionRouteProfile::MixedBsp,
                        )
                    })
                    .expect(
                        "the frozen PartitionRoute v3 source has a fully positive seed in the focused fixture range",
                    )
            })
            .clone()
    }

    fn real_partition_analysis_fixture() -> CorpusRoomAnalysis {
        static FIXTURE: OnceLock<CorpusRoomAnalysis> = OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let evaluated = real_partition_fixture();
                analyze_corpus_room_v2(
                    &evaluated,
                    &CorpusRoomAnalysisConfig {
                        direct_controller_solver: SolverConfig {
                            max_expanded_nodes: 64,
                            max_simulated_ticks: 100_000,
                            max_ticks_per_path: 60,
                            ..SolverConfig::for_abilities(AbilitySet::ALL)
                        },
                        canonical_witness_difficulty: DifficultyConfig::default(),
                    },
                )
                .expect("real final-path fixture admits denominator-bound deep analysis")
            })
            .clone()
    }

    fn real_partition_metric_fixture() -> RoomMetricSummary {
        summarize_room_metrics(&real_partition_analysis_fixture())
            .expect("real deep analysis summarizes")
    }

    fn real_partition_deep_report_fixture() -> (
        crate::corpus::RoomRouteChoiceDiversity,
        crate::corpus::RoomPickupDetourAnalysis,
    ) {
        static FIXTURE: OnceLock<(
            crate::corpus::RoomRouteChoiceDiversity,
            crate::corpus::RoomPickupDetourAnalysis,
        )> = OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let evaluated = real_partition_fixture();
                let analysis = real_partition_analysis_fixture();
                (
                    analyze_route_choice_diversity_v2(&evaluated, &analysis)
                        .expect("real route-choice report is bound to final-path evidence"),
                    analyze_pickup_detours_v2(&evaluated, TraversalGrid::default())
                        .expect("real pickup report is bound to final-path evidence"),
                )
            })
            .clone()
    }

    #[test]
    fn pareto_tradeoffs_are_not_collapsed_to_a_weighted_score() {
        let mut left = synthetic_descriptor(
            "hard-controller",
            u16::MAX,
            0,
            SelectionEvidenceState::Observed,
            Vec::new(),
        );
        let mut right = synthetic_descriptor(
            "diverse-route",
            0,
            u16::MAX,
            SelectionEvidenceState::Observed,
            Vec::new(),
        );
        left.projections[0].cell[0].evidence = QuantizedSelectionEvidence::observed(0);
        right.projections[0].cell[0].evidence = QuantizedSelectionEvidence::observed(0);
        let descriptors = [left, right]
            .into_iter()
            .map(|descriptor| (descriptor.room_id.clone(), descriptor))
            .collect::<BTreeMap<_, _>>();
        let archive = synthetic_archive(&descriptors);

        assert_eq!(archive.len(), 2);
        assert!(
            archive
                .candidate(&RoomId("hard-controller".into()))
                .is_some()
        );
        assert!(archive.candidate(&RoomId("diverse-route".into())).is_some());
    }

    #[test]
    fn unknown_and_not_applicable_states_are_not_numeric_zero_or_each_other() {
        let mut missing = synthetic_descriptor(
            "missing",
            0,
            10,
            SelectionEvidenceState::Missing,
            Vec::new(),
        );
        let mut not_applicable = synthetic_descriptor(
            "not-applicable",
            0,
            10,
            SelectionEvidenceState::NotApplicable,
            Vec::new(),
        );
        missing.projections[0].cell[0].evidence = QuantizedSelectionEvidence::observed(0);
        not_applicable.projections[0].cell[0].evidence = QuantizedSelectionEvidence::observed(0);
        let missing_quality = flatten_quality(&missing.quality);
        let na_quality = flatten_quality(&not_applicable.quality);
        assert_ne!(missing_quality, na_quality);
        assert_ne!(
            missing.diversity_coordinates,
            not_applicable.diversity_coordinates
        );

        let descriptors = [missing, not_applicable]
            .into_iter()
            .map(|descriptor| (descriptor.room_id.clone(), descriptor))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(synthetic_archive(&descriptors).len(), 2);
    }

    #[test]
    fn landing_cells_distinguish_no_event_unmeasured_and_observed_zero_margin() {
        let no_event = landing_summary_fixture(landing_aggregate_fixture(
            0,
            0,
            0,
            LandingCoordinateEvidence::NotApplicable {
                reason: LandingCoordinateNotApplicableReason::NoLandingEvents,
            },
        ));
        let unmeasured = landing_summary_fixture(landing_aggregate_fixture(
            1,
            0,
            1,
            LandingCoordinateEvidence::NotApplicable {
                reason: LandingCoordinateNotApplicableReason::NoMeasuredLandings,
            },
        ));
        let zero_margin = landing_summary_fixture(landing_aggregate_fixture(
            1,
            1,
            0,
            LandingCoordinateEvidence::Observed(SignedIntegerCoordinateDistribution {
                sample_count: 1,
                minimum: 0,
                median_lower: 0,
                median_upper: 0,
                maximum: 0,
                spread: 0,
            }),
        ));

        let no_event = landing_geometry_projection(&no_event);
        let unmeasured = landing_geometry_projection(&unmeasured);
        let zero_margin = landing_geometry_projection(&zero_margin);
        assert_eq!(no_event.cell.len(), 9);
        assert_eq!(
            no_event.id.schema_version(),
            LANDING_GEOMETRY_PROJECTION_VERSION
        );
        assert_ne!(flatten_cell(&no_event.cell), flatten_cell(&unmeasured.cell));
        assert_ne!(
            flatten_cell(&no_event.cell),
            flatten_cell(&zero_margin.cell)
        );
        assert_ne!(
            flatten_cell(&unmeasured.cell),
            flatten_cell(&zero_margin.cell)
        );

        let zero_coordinate = zero_margin
            .cell
            .iter()
            .find(|coordinate| coordinate.name == "both-minimum-edge-margin-pixels")
            .unwrap();
        assert_eq!(
            zero_coordinate.evidence,
            QuantizedSelectionEvidence::observed(quantized_signed_edge_margin(0))
        );
        assert!(quantized_signed_edge_margin(-1) < quantized_signed_edge_margin(0));
        assert!(
            cell_bucket(Some(quantized_signed_edge_margin(-1)))
                < cell_bucket(Some(quantized_signed_edge_margin(0)))
        );
    }

    #[test]
    fn socket_core_and_reported_packages_are_cross_room_covered() {
        let left = DoorSocket {
            side: BoundarySide::Left,
            offset: 16,
            span: 16,
        };
        let right = left.mate();
        let orphan = DoorSocket {
            side: BoundarySide::Ceiling,
            offset: 80,
            span: 16,
        };
        let descriptors = [
            synthetic_descriptor(
                "a",
                u16::MAX,
                0,
                SelectionEvidenceState::Observed,
                vec![left],
            ),
            synthetic_descriptor(
                "b",
                0,
                u16::MAX,
                SelectionEvidenceState::Observed,
                vec![right],
            ),
            synthetic_descriptor(
                "orphan",
                u16::MAX / 2,
                u16::MAX / 2,
                SelectionEvidenceState::Observed,
                vec![orphan],
            ),
        ]
        .into_iter()
        .map(|descriptor| (descriptor.room_id.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
        let offered = descriptors.keys().cloned().collect::<BTreeSet<_>>();
        let core = mate_covered_core(&offered, &descriptors);

        assert_eq!(
            core,
            [RoomId("a".into()), RoomId("b".into())]
                .into_iter()
                .collect()
        );
        let packages = reusable_socket_packages(&core, &descriptors).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].room_ids.len(), 2);

        let error =
            select_descriptors(descriptors, 3, CorpusSelectionConfig::new(3, 3, 8)).unwrap_err();
        assert!(
            matches!(
                error,
                CorpusSelectionMetricsError::RequestedMinimumUnavailable {
                    minimum: 3,
                    maximum: 3,
                    available: 2
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn deterministic_selection_honors_requested_range_and_socket_coverage() {
        let left = DoorSocket {
            side: BoundarySide::Left,
            offset: 32,
            span: 16,
        };
        let right = left.mate();
        let descriptors = (0_u16..6)
            .map(|index| {
                let descriptor = synthetic_descriptor(
                    &format!("room-{index}"),
                    index.saturating_mul(10_000),
                    u16::MAX.saturating_sub(index.saturating_mul(10_000)),
                    SelectionEvidenceState::Observed,
                    vec![if index % 2 == 0 { left } else { right }],
                );
                (descriptor.room_id.clone(), descriptor)
            })
            .collect::<BTreeMap<_, _>>();
        let config = CorpusSelectionConfig::new(4, 4, 8);
        let first = select_descriptors(descriptors.clone(), 6, config).unwrap();
        let second = select_descriptors(descriptors, 6, config).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.selected_room_ids.len(), 4);
        assert_eq!(first.audit.steps.len(), 4);
        assert_eq!(
            first
                .audit
                .steps
                .iter()
                .map(|step| step.marginal_cell_coverage)
                .sum::<usize>(),
            first.audit.covered_cells.len()
        );
        assert_eq!(first.audit.socket_pruning_steps.len(), 2);
        assert_eq!(first.socket_packages.len(), 1);
        assert_eq!(first.socket_packages[0].room_ids.len(), 4);
        let selected = first
            .selected_room_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert!(mate_coverage_is_complete(&selected, &first.descriptors));
        assert_eq!(first.audit.steps[0].minimum_l1_distance, None);
        assert!(
            first.audit.steps[1..]
                .iter()
                .all(|step| step.minimum_l1_distance.is_some())
        );
        assert_eq!(
            CorpusSelectionConfig::default().requested_range(),
            DEFAULT_CORPUS_SELECTION_MINIMUM..=DEFAULT_CORPUS_SELECTION_MAXIMUM
        );
    }

    #[test]
    fn exploratory_v2_adapter_retains_missing_deep_evidence() {
        let evaluated = real_partition_fixture();
        let metrics = real_partition_metric_fixture();
        let descriptor =
            describe_corpus_selection_room_v2_exploratory(&evaluated, &metrics, None, None)
                .expect("positive-gated real PartitionRoute v3 fixture adapts");
        let canonical = resolve_corpus_metric_candidate_v2(&evaluated).unwrap();
        let expected_sockets = canonical
            .boundary_ports()
            .iter()
            .map(|port| port.door.socket())
            .collect::<Vec<_>>();

        assert_eq!(CORPUS_SELECTION_INPUT_V2_VERSION, 2);
        assert_eq!(descriptor.room_id, evaluated.generated.id);
        assert_eq!(descriptor.projections.len(), 9);
        assert_eq!(descriptor.sockets, expected_sockets);
        for projection_id in [
            CorpusSelectionProjectionId::ObservedWithinCellRouteChoicesV1,
            CorpusSelectionProjectionId::PickupChallengeDetourV1,
        ] {
            let projection = descriptor
                .projections
                .iter()
                .find(|projection| projection.id == projection_id)
                .unwrap();
            assert!(projection.detail.iter().all(|coordinate| {
                coordinate.evidence.state == SelectionEvidenceState::Missing
                    && coordinate.evidence.value.is_none()
            }));
        }

        let supplied = describe_corpus_selection_candidate_v2_exploratory(
            &evaluated, canonical, &metrics, None, None,
        )
        .expect("the exact resolved native candidate is accepted");
        assert_eq!(descriptor, supplied);
    }

    #[test]
    fn production_v2_rejects_absent_reports_and_missing_or_bounded_novelty() {
        let evaluated = real_partition_fixture();
        let metrics = empty_metric_summary(evaluated.generated.id.clone());
        assert!(matches!(
            describe_corpus_selection_room_v2(&evaluated, &metrics, None, None),
            Err(CorpusSelectionMetricsError::MissingProductionDeepReport {
                report: ProductionDeepReport::RouteChoices,
                ..
            })
        ));

        for state in [
            SelectionEvidenceState::Missing,
            SelectionEvidenceState::BoundedInconclusive,
        ] {
            let descriptor = synthetic_descriptor("uncertain", 0, 0, state, Vec::new());
            assert!(matches!(
                validate_production_descriptor_evidence(&descriptor),
                Err(CorpusSelectionMetricsError::ProductionEvidenceUnavailable {
                    state: rejected,
                    ..
                }) if rejected == state
            ));
        }
    }

    #[test]
    fn v2_denominators_are_bound_to_exact_evaluated_coordinates() {
        let evaluated = real_partition_fixture();
        let canonical = resolve_corpus_metric_candidate_v2(&evaluated).unwrap();
        let metrics = real_partition_metric_fixture();
        let (route_choices, pickup_detours) = real_partition_deep_report_fixture();

        validate_final_path_room_metric_contract(
            &evaluated,
            canonical,
            &metrics,
            CorpusV2DescriptorPolicy::Exploratory,
        )
        .expect("genuine metric rows bind to the evaluated room");
        validate_final_path_route_choice_contract(
            &evaluated,
            &metrics,
            &route_choices,
            CorpusV2DescriptorPolicy::Exploratory,
        )
        .expect("genuine route-choice rows bind to the evaluated matrices");
        validate_final_path_pickup_detour_contract(&evaluated, canonical, &pickup_detours)
            .expect("genuine pickup rows bind to the evaluated matrices");

        let empty = empty_metric_summary(evaluated.generated.id.clone());
        assert!(matches!(
            validate_final_path_room_metric_contract(
                &evaluated,
                canonical,
                &empty,
                CorpusV2DescriptorPolicy::Exploratory,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract {
                evidence: "room-metric",
                ..
            })
        ));

        let mut missing_metric_coordinate = metrics.clone();
        missing_metric_coordinate
            .canonical_routes
            .directed_loadout_routes
            .pop();
        assert!(matches!(
            validate_final_path_room_metric_contract(
                &evaluated,
                canonical,
                &missing_metric_coordinate,
                CorpusV2DescriptorPolicy::Exploratory,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract { .. })
        ));

        let mut missing_route_cell = route_choices.clone();
        missing_route_cell.cells.pop();
        assert!(matches!(
            validate_final_path_route_choice_contract(
                &evaluated,
                &metrics,
                &missing_route_cell,
                CorpusV2DescriptorPolicy::Exploratory,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract {
                evidence: "route-choice",
                ..
            })
        ));

        let mut emptied_route_aggregate = route_choices.clone();
        emptied_route_aggregate.room = crate::corpus::RouteChoiceAggregate::default();
        assert!(matches!(
            validate_final_path_route_choice_contract(
                &evaluated,
                &metrics,
                &emptied_route_aggregate,
                CorpusV2DescriptorPolicy::Exploratory,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract {
                evidence: "route-choice",
                ..
            })
        ));

        let mut missing_source_searches = pickup_detours.clone();
        missing_source_searches
            .operational_cost
            .source_searches
            .clear();
        assert!(matches!(
            validate_final_path_pickup_detour_contract(
                &evaluated,
                canonical,
                &missing_source_searches,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract {
                evidence: "pickup-detour",
                ..
            })
        ));
    }

    #[test]
    fn final_path_projection_preserves_mixed_front_ambiguity_as_not_applicable() {
        let evaluated = real_partition_fixture();
        let canonical = resolve_corpus_metric_candidate_v2(&evaluated).unwrap();
        let mut metrics = real_partition_metric_fixture();
        let loadout = EvaluationLoadout::Both;
        let summary_index = metrics
            .direct_controllers
            .by_loadout
            .iter()
            .position(|summary| {
                summary.loadout == loadout
                    && summary.known_positive_directed_routes >= 2
                    && summary.ambiguous_nondominated_front_directed_routes == 0
                    && matches!(
                        summary.easiest_controller_fractions,
                        MetricEvidence::Observed(_)
                    )
            })
            .expect("the focused final-path fixture has multiple unique complete-kit positives");
        let exact = metrics
            .direct_controllers
            .directed_routes
            .iter_mut()
            .find_map(|route| {
                route.exact_loadouts.iter_mut().find(|cell| {
                    cell.loadout == loadout
                        && matches!(
                            cell.easiest_known,
                            MetricEvidence::Observed(crate::corpus::EasiestKnownControllerMetric {
                                selection_status: crate::corpus::EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate,
                                ..
                            })
                        )
                })
            })
            .expect("a unique exact representative is available");
        let MetricEvidence::Observed(representative) = &mut exact.easiest_known else {
            unreachable!();
        };
        representative.nondominated_front_size = 2;
        representative.selection_status =
            crate::corpus::EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                front_size: 2,
            };

        let mut collapsed = metrics.clone();
        collapsed.direct_controllers.by_loadout[summary_index]
            .ambiguous_nondominated_front_directed_routes = 1;
        assert!(matches!(
            validate_final_path_room_metric_contract(
                &evaluated,
                canonical,
                &collapsed,
                CorpusV2DescriptorPolicy::Exploratory,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract { .. })
        ));

        let summary = &mut metrics.direct_controllers.by_loadout[summary_index];
        summary.ambiguous_nondominated_front_directed_routes = 1;
        summary.easiest_controller_fractions = MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
        };
        summary.demand_coordinates = MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
        };
        validate_final_path_room_metric_contract(
            &evaluated,
            canonical,
            &metrics,
            CorpusV2DescriptorPolicy::Exploratory,
        )
        .expect("an explicit ambiguous-front N/A preserves the full positive denominator");

        let projection = directed_controller_projection(&metrics);
        let slug = loadout.slug();
        for suffix in [
            "run-only",
            "monotone-simple",
            "other-controller",
            "median-controller-class",
            "median-ability-events",
            "median-horizontal-reversals",
            "median-vertical-decisions",
            "median-semantic-spans",
            "median-semantic-transitions",
            "median-duration",
        ] {
            let coordinate = projection
                .detail
                .iter()
                .find(|coordinate| coordinate.name == format!("{slug}-{suffix}"))
                .unwrap();
            assert_eq!(
                coordinate.evidence,
                QuantizedSelectionEvidence::not_applicable()
            );
        }
    }

    #[test]
    fn production_rejects_bounded_and_missing_direct_audits_before_projection() {
        let evaluated = real_partition_fixture();
        let canonical = resolve_corpus_metric_candidate_v2(&evaluated).unwrap();
        let metrics = real_partition_metric_fixture();

        let mut bounded = metrics.clone();
        let first = &mut bounded.direct_controllers.directed_routes[0].exact_loadouts[0];
        let (raw_positive_witnesses, retained_semantic_witnesses) = match first.audit {
            crate::corpus::DirectControllerAuditMetric::CompleteFiniteVocabulary {
                raw_positive_witnesses,
                retained_semantic_witnesses,
            }
            | crate::corpus::DirectControllerAuditMetric::BoundedIncomplete {
                raw_positive_witnesses,
                retained_semantic_witnesses,
                ..
            } => (raw_positive_witnesses, retained_semantic_witnesses),
            crate::corpus::DirectControllerAuditMetric::MissingDirectedRouteAssessment
            | crate::corpus::DirectControllerAuditMetric::MissingLoadoutAudit => (0, 0),
        };
        first.audit = crate::corpus::DirectControllerAuditMetric::BoundedIncomplete {
            limit: DirectProbeBudgetLimit::ExpandedNodes,
            raw_positive_witnesses,
            retained_semantic_witnesses,
        };
        assert!(matches!(
            validate_final_path_room_metric_contract(
                &evaluated,
                canonical,
                &bounded,
                CorpusV2DescriptorPolicy::ProductionAuthoritative,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract { .. })
        ));

        let mut missing = metrics.clone();
        missing.direct_controllers.directed_routes[0].exact_loadouts[0].audit =
            crate::corpus::DirectControllerAuditMetric::MissingLoadoutAudit;
        assert!(matches!(
            validate_final_path_room_metric_contract(
                &evaluated,
                canonical,
                &missing,
                CorpusV2DescriptorPolicy::ProductionAuthoritative,
            ),
            Err(CorpusSelectionMetricsError::FinalPathEvidenceContract { .. })
        ));
    }

    #[test]
    fn direct_audit_coverage_and_known_positives_are_descriptive_not_pareto_axes() {
        let evaluated = real_partition_fixture();
        let metrics = real_partition_metric_fixture();
        let descriptor =
            describe_corpus_selection_room_v2_exploratory(&evaluated, &metrics, None, None)
                .expect("coordinate-bound exploratory fixture adapts");
        let axes = descriptor
            .quality
            .iter()
            .map(|coordinate| coordinate.axis)
            .collect::<Vec<_>>();

        assert_eq!(CORPUS_SELECTION_METRICS_VERSION, 5);
        assert_eq!(axes.len(), 9);
        assert_eq!(
            axes,
            vec![
                CorpusSelectionQualityAxis::CompleteKitOtherControllerDemand,
                CorpusSelectionQualityAxis::CompleteKitMedianDuration,
                CorpusSelectionQualityAxis::DirectionalDurationAsymmetry,
                CorpusSelectionQualityAxis::JointRouteStyleDiversity,
                CorpusSelectionQualityAxis::UncorroboratedTerrainComponents,
                CorpusSelectionQualityAxis::UncorroboratedTerrainTiles,
                CorpusSelectionQualityAxis::AblatedTilesAffectingStoredControllers,
                CorpusSelectionQualityAxis::ControllerAblationEffect,
                CorpusSelectionQualityAxis::OperationalSimulatedTicks,
            ]
        );
        let direct_projection = descriptor
            .projections
            .iter()
            .find(|projection| {
                projection.id == CorpusSelectionProjectionId::DirectedLoadoutControllerDemandV1
            })
            .unwrap();
        assert!(
            direct_projection
                .detail
                .iter()
                .any(|coordinate| { coordinate.name.ends_with("known-positive-routes") })
        );
        assert_eq!(corpus_selection_quality_directions().len(), 9 * 5);
    }

    #[test]
    fn v2_refuses_room_v3_identity_and_each_mandatory_gate_failure() {
        let evaluated = real_partition_fixture();
        let metrics = empty_metric_summary(evaluated.generated.id.clone());

        let mut wrong_identity = evaluated.clone();
        wrong_identity.generated.id = RoomId("room-v3-forged".to_owned());
        assert!(matches!(
            describe_corpus_selection_room_v2(&wrong_identity, &metrics, None, None),
            Err(CorpusSelectionMetricsError::FinalPathIdentity { .. })
        ));

        let bounded = CorpusFeasibilityGateState::BoundedInconclusive {
            door_rows: 2,
            positive_door_rows: 1,
            pickup_rows: 0,
            positive_pickup_rows: 0,
        };
        let mut refused_complete_kit = evaluated.clone();
        refused_complete_kit.complete_kit_gate = bounded;
        refused_complete_kit.canonical_regeneration = select_canonical_regeneration_v2(
            &refused_complete_kit.variant_construction_gates,
            &refused_complete_kit.variant_ability_promotion_gates,
            refused_complete_kit.complete_kit_gate,
        );
        assert!(matches!(
            describe_corpus_selection_room_v2(&refused_complete_kit, &metrics, None, None),
            Err(CorpusSelectionMetricsError::FinalPathIdentity { .. })
        ));

        let mut refused_construction = evaluated.clone();
        refused_construction.variant_construction_gates[0].state = bounded;
        refused_construction.canonical_regeneration = select_canonical_regeneration_v2(
            &refused_construction.variant_construction_gates,
            &refused_construction.variant_ability_promotion_gates,
            refused_construction.complete_kit_gate,
        );
        assert!(matches!(
            describe_corpus_selection_room_v2(&refused_construction, &metrics, None, None),
            Err(CorpusSelectionMetricsError::FinalPathIdentity { .. })
        ));
    }

    #[test]
    fn v2_exact_visual_dedupe_rejects_even_a_repeated_valid_room() {
        let evaluated = real_partition_fixture();
        let metrics = real_partition_metric_fixture();
        let offered = [
            CorpusSelectionRoomV2::new(&evaluated, &metrics),
            CorpusSelectionRoomV2::new(&evaluated, &metrics),
        ];

        assert!(matches!(
            select_corpus_rooms_v2_exploratory(&offered, CorpusSelectionConfig::new(1, 2, 8)),
            Err(CorpusSelectionMetricsError::DuplicateExactStaticVisual { .. })
        ));
    }

    #[test]
    fn v2_batch_rejects_mixed_deep_analysis_configs_before_archive_construction() {
        let evaluated = real_partition_fixture();
        let first = empty_metric_summary(evaluated.generated.id.clone());
        let mut second = first.clone();
        second
            .source_analysis_config
            .direct_controller_solver
            .max_expanded_nodes += 1;
        second.source_analysis_config.config_id =
            second.source_analysis_config.recomputed_config_id();
        second.source_analysis_config.validate().unwrap();
        let offered = [
            CorpusSelectionRoomV2::new(&evaluated, &first),
            CorpusSelectionRoomV2::new(&evaluated, &second),
        ];

        assert!(matches!(
            select_corpus_rooms_v2(&offered, CorpusSelectionConfig::new(1, 2, 8)),
            Err(CorpusSelectionMetricsError::MixedAnalysisConfigs { .. })
        ));
    }

    #[test]
    fn real_generated_analyzed_room_adapts_to_all_nine_projections() {
        let mut generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1))
            .expect("stable real generation fixture");
        let room = generated
            .rooms
            .iter()
            .find(|room| room.variants[0].generated.room.doors().len() == 2)
            .cloned()
            .expect("seed zero contains a two-door room");
        generated.rooms = vec![room];
        let evaluated = evaluate_route_matrices(generated)
            .expect("real room evaluates")
            .rooms
            .pop()
            .unwrap();
        let analysis = analyze_corpus_room(
            &evaluated,
            &CorpusRoomAnalysisConfig {
                direct_controller_solver: SolverConfig {
                    max_expanded_nodes: 10_000,
                    max_simulated_ticks: 2_000_000,
                    max_ticks_per_path: 240,
                    ..SolverConfig::for_abilities(AbilitySet::ALL)
                },
                canonical_witness_difficulty: DifficultyConfig::default(),
            },
        )
        .expect("real room deep analysis succeeds");
        let metrics = summarize_room_metrics(&analysis).unwrap();
        let descriptor = describe_corpus_selection_room(&evaluated.generated, &metrics)
            .expect("real room adapts");

        assert_eq!(descriptor.projections.len(), 9);
        assert_eq!(descriptor.quality.len(), 9);
        assert!(!descriptor.diversity_coordinates.is_empty());
        assert_eq!(descriptor.sockets.len(), 2);
        assert!(
            descriptor
                .projections
                .iter()
                .all(|projection| !projection.cell.is_empty() && !projection.detail.is_empty())
        );

        let missing_route_choice = descriptor
            .projections
            .iter()
            .find(|projection| {
                projection.id == CorpusSelectionProjectionId::ObservedWithinCellRouteChoicesV1
            })
            .expect("legacy adapter retains an explicit route-choice projection");
        let missing_pickup_detour = descriptor
            .projections
            .iter()
            .find(|projection| {
                projection.id == CorpusSelectionProjectionId::PickupChallengeDetourV1
            })
            .expect("legacy adapter retains an explicit pickup-detour projection");
        assert!(
            missing_route_choice
                .detail
                .iter()
                .all(|coordinate| { coordinate.evidence.state == SelectionEvidenceState::Missing })
        );
        assert!(
            missing_pickup_detour
                .detail
                .iter()
                .all(|coordinate| { coordinate.evidence.state == SelectionEvidenceState::Missing })
        );

        let route_choices = analyze_route_choice_diversity(&evaluated, &analysis)
            .expect("real route-choice post-pass succeeds");
        let pickup_detours = analyze_pickup_detours(&evaluated, TraversalGrid::default())
            .expect("real pickup-detour post-pass succeeds");
        let extended = describe_extended_corpus_selection_room(
            &evaluated.generated,
            &metrics,
            &route_choices,
            &pickup_detours,
        )
        .expect("identity-matched extended evidence adapts");
        assert_eq!(extended.projections.len(), 9);
        assert_eq!(extended.quality, descriptor.quality);
        for id in [
            CorpusSelectionProjectionId::ObservedWithinCellRouteChoicesV1,
            CorpusSelectionProjectionId::PickupChallengeDetourV1,
        ] {
            let missing = descriptor
                .projections
                .iter()
                .find(|projection| projection.id == id)
                .unwrap();
            let supplied = extended
                .projections
                .iter()
                .find(|projection| projection.id == id)
                .unwrap();
            assert_ne!(flatten_cell(&missing.cell), flatten_cell(&supplied.cell));
            assert!(
                supplied
                    .detail
                    .iter()
                    .any(|coordinate| coordinate.evidence.state != SelectionEvidenceState::Missing)
            );
        }
        let mut mislabeled_route_choices = route_choices.clone();
        mislabeled_route_choices.room_id = RoomId("wrong-route-choice-room".to_owned());
        assert!(matches!(
            describe_extended_corpus_selection_room(
                &evaluated.generated,
                &metrics,
                &mislabeled_route_choices,
                &pickup_detours,
            ),
            Err(CorpusSelectionMetricsError::RouteChoiceIdentityMismatch { .. })
        ));
        let mut mislabeled_pickup_detours = pickup_detours.clone();
        mislabeled_pickup_detours.room_id = RoomId("wrong-pickup-detour-room".to_owned());
        assert!(matches!(
            describe_extended_corpus_selection_room(
                &evaluated.generated,
                &metrics,
                &route_choices,
                &mislabeled_pickup_detours,
            ),
            Err(CorpusSelectionMetricsError::PickupDetourIdentityMismatch { .. })
        ));

        let controller = descriptor
            .projections
            .iter()
            .find(|projection| {
                projection.id == CorpusSelectionProjectionId::DirectedLoadoutControllerDemandV1
            })
            .unwrap();
        let landing = descriptor
            .projections
            .iter()
            .find(|projection| projection.id == CorpusSelectionProjectionId::LandingGeometryV1)
            .expect("landing geometry is an independent projection");
        assert!(
            landing
                .detail
                .iter()
                .any(|coordinate| coordinate.name == "all-loadouts-minimum-edge-margin-pixels")
        );
        let both = metrics
            .direct_controllers
            .by_loadout
            .iter()
            .find(|summary| summary.loadout == EvaluationLoadout::Both)
            .unwrap();
        let MetricEvidence::Observed(distributions) = &both.demand_coordinates else {
            panic!("real fixture has complete-kit direct-controller coordinates");
        };
        let expected_names_and_values = [
            (
                "both-median-controller-class",
                quantized_distribution(distributions.controller_class, 2),
            ),
            (
                "both-median-ability-events",
                quantized_distribution(distributions.ability_events, CONTROLLER_COUNT_CAP),
            ),
            (
                "both-median-horizontal-reversals",
                quantized_distribution(distributions.horizontal_reversals, CONTROLLER_COUNT_CAP),
            ),
            (
                "both-median-vertical-decisions",
                quantized_distribution(distributions.vertical_decisions, CONTROLLER_COUNT_CAP),
            ),
            (
                "both-median-semantic-spans",
                quantized_distribution(distributions.semantic_spans, CONTROLLER_COUNT_CAP),
            ),
            (
                "both-median-semantic-transitions",
                quantized_distribution(distributions.semantic_transitions, CONTROLLER_COUNT_CAP),
            ),
            (
                "both-median-duration",
                quantized_distribution(distributions.duration_ticks, CONTROLLER_DURATION_CAP),
            ),
        ];
        for (name, expected) in expected_names_and_values {
            let coordinate = controller
                .detail
                .iter()
                .find(|coordinate| coordinate.name == name)
                .unwrap();
            assert_eq!(coordinate.evidence.value, Some(expected), "{name}");
        }
        assert_eq!(
            expected_names_and_values
                .iter()
                .map(|(name, _)| *name)
                .collect::<BTreeSet<_>>()
                .len(),
            7
        );

        assert!(descriptor.projections.iter().all(|projection| {
            projection
                .detail
                .iter()
                .all(|coordinate| !coordinate.name.contains("operational"))
        }));
        assert!(descriptor.quality.iter().any(|coordinate| {
            coordinate.axis == CorpusSelectionQualityAxis::OperationalSimulatedTicks
        }));
        let uncorroborated_tiles = descriptor
            .quality
            .iter()
            .find(|coordinate| {
                coordinate.axis == CorpusSelectionQualityAxis::UncorroboratedTerrainTiles
            })
            .unwrap();
        assert_eq!(uncorroborated_tiles.direction, ObjectiveDirection::Minimize);
        let expected_uncorroborated_tiles = if metrics.terrain.coverage.interior_tile_count == 0 {
            QuantizedSelectionEvidence::not_applicable()
        } else if metrics.terrain.coverage.positive_controller_count == 0 {
            QuantizedSelectionEvidence::missing()
        } else {
            QuantizedSelectionEvidence::observed(quantized_capped(
                metrics.terrain.coverage.uncorroborated_tile_count,
                512,
            ))
        };
        assert_eq!(uncorroborated_tiles.evidence, expected_uncorroborated_tiles);
        let directions = corpus_selection_quality_directions();
        assert_eq!(directions.len(), descriptor.quality.len() * 5);
        for (coordinate, flat) in descriptor.quality.iter().zip(directions.chunks_exact(5)) {
            assert_eq!(flat[4], coordinate.direction);
        }

        let mut duplicate = evaluated.generated.clone();
        duplicate.id = RoomId("duplicate-static-visual".into());
        let duplicate_inputs = [
            CorpusSelectionRoom::new(&evaluated.generated, &metrics),
            CorpusSelectionRoom::new(&duplicate, &metrics),
        ];
        assert!(matches!(
            select_corpus_rooms(&duplicate_inputs, CorpusSelectionConfig::new(1, 2, 8)),
            Err(CorpusSelectionMetricsError::DuplicateExactStaticVisual { .. })
        ));
        assert!(matches!(
            describe_corpus_selection_room(&duplicate, &metrics),
            Err(CorpusSelectionMetricsError::RoomMetricIdentityMismatch { .. })
        ));
    }
}
