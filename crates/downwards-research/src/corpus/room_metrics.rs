//! Transparent room-level aggregation of directed corpus evidence.
//!
//! This module deliberately does not produce a room score or difficulty band.
//! Every aggregate keeps its denominator, and the directed route/loadout cells
//! remain available beside the aggregates.  In particular, a missing positive
//! after a bounded audit is never rewritten as zero demand or unreachable.

use std::collections::{BTreeMap, BTreeSet};
use std::{error::Error, fmt};

use downwards_ai::{DirectProbeBudgetLimit, SearchStats};
use downwards_lab::{
    LANDING_PRECISION_VERSION, LandingPrecisionReport, LandingSupportKind,
    ROUTE_DIFFICULTY_VECTOR_VERSION, RoomAblationKind, RouteDiversityReport, route_diversity,
};
use downwards_validation::WitnessFingerprint;

use super::{
    AblationOutcomeSummary, BoundedIncompleteLoadout, CONTROLLER_DEMAND_POLICY_VERSION,
    CORPUS_ROOM_ANALYSIS_VERSION, CORPUS_ROUTE_MEASUREMENT_VERSION,
    CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY, CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY,
    CanonicalMatrixCellStatus, ControllerDemand, ControllerDemandCoordinates, CorpusRoomAnalysis,
    CorpusRoomAnalysisConfigError, CorpusRoomAnalysisConfigRecord,
    EasiestKnownRouteSelectionStatus, EvaluationLoadout, FusedRouteCandidate,
    FusedRouteCandidateProvenance, FusedRouteCellAssessment, LoadoutControllerAuditStatus,
    PositiveTerrainCoverage, ROUTE_CONTROLLER_TRACE_VERSION, RoomId, RouteControllerAssessment,
    RouteControllerAuditCompleteness,
};

/// Version of the room-metric schema and aggregation policy.
pub const ROOM_METRIC_SUMMARY_VERSION: u32 = 3;

/// Interpretation boundary for [`RoomMetricSummary`].
pub const ROOM_METRIC_SUMMARY_DISCLAIMER: &str = "room metrics retain directed route/loadout evidence and uncertainty; positive counts are not reachability rates, completed direct-controller audits cover only the configured finite vocabulary, incomparable fused fronts remain explicit not-applicable evidence in single-value controller aggregates, exact landing geometry is not proof of a narrow input window or mandatory landing, unmeasured landings are never assigned zero margin, ablation survival applies only to exact stored controllers, and operational cost is not player difficulty";

/// Complete transparent aggregation for one analyzed corpus room.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomMetricSummary {
    pub version: u32,
    pub source_analysis_version: u32,
    pub source_analysis_config: CorpusRoomAnalysisConfigRecord,
    pub room_id: RoomId,
    pub canonical_routes: CanonicalRouteMetricSummary,
    /// Exact geometry of authoritative landing events along canonical
    /// positive replays. This is deliberately separate from temporal
    /// robustness and controller demand.
    pub landing_precision: LandingPrecisionMetricSummary,
    pub direct_controllers: DirectControllerMetricSummary,
    /// Easiest-known exact-controller comparisons selected from the fused
    /// direct + canonical candidate cells. Canonical remains a fallback, and
    /// a lower-demand direct positive can supersede it.
    pub directional_asymmetry: Vec<DirectionalAsymmetryMetric>,
    pub terrain: TerrainMetricSummary,
    /// Solver work is isolated here and does not participate in any demand,
    /// diversity, coverage, or asymmetry calculation.
    pub operational_cost: OperationalCostSummary,
}

/// Why a value cannot currently be calculated from the retained evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingMetricReason {
    MissingDirectedRouteAssessment,
    MissingDirectControllerAudit,
    BoundedDirectControllerAuditWithoutPositive,
    NoPositiveTerrainController,
    ReverseDirectionsRequireTwoKnownPositiveControllers,
}

/// Why a value has no meaningful denominator in the supplied evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotApplicableMetricReason {
    NoDirectedRoutes,
    NoKnownPositiveControllerInCompleteFiniteVocabulary,
    NoSuccessfulRoutesForLoadout,
    /// At least one exact positive cell has multiple incomparable members on
    /// its fused nondominated front, so no single representative can
    /// honestly contribute to a controller-demand aggregate.
    AmbiguousNondominatedFront,
    NoInteriorTerrainComponents,
    NoInteriorTerrainTiles,
    NoAblationVariants,
    NoExactControllersForAblation,
}

/// Explicit availability wrapper used whenever absence must not become zero.
#[derive(Clone, Debug, PartialEq)]
pub enum MetricEvidence<T> {
    Observed(T),
    Missing { reason: MissingMetricReason },
    NotApplicable { reason: NotApplicableMetricReason },
}

/// Exact fraction retained as integer numerator and denominator.
///
/// Instances always have a non-zero denominator.  A zero denominator is
/// represented by [`MetricEvidence::Missing`] or
/// [`MetricEvidence::NotApplicable`] around the fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactFraction {
    pub numerator: usize,
    pub denominator: usize,
}

impl ExactFraction {
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    fn new(numerator: usize, denominator: usize) -> Self {
        debug_assert!(denominator > 0);
        debug_assert!(numerator <= denominator);
        Self {
            numerator,
            denominator,
        }
    }
}

/// Positive and bounded-inconclusive canonical route matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalRouteMetricSummary {
    pub directed_route_count: usize,
    pub loadout_route_cell_count: usize,
    pub positive_route_count: usize,
    pub bounded_inconclusive_route_count: usize,
    pub directed_loadout_routes: Vec<CanonicalDirectedLoadoutRouteMetric>,
    pub by_loadout: Vec<CanonicalLoadoutMetricSummary>,
    pub behavior_diversity: RouteDiversityReport,
}

/// One canonical matrix cell under its exact physics loadout.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalDirectedLoadoutRouteMetric {
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub evidence: CanonicalRouteMetricEvidence,
}

/// The canonical analysis retains exact measurements only for positive rows.
#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalRouteMetricEvidence {
    Positive(CanonicalPositiveRouteMetric),
    /// The original bounded-inconclusive reason lives in the evaluated route
    /// matrix, which is not embedded in [`CorpusRoomAnalysis`].  This state is
    /// therefore explicit without inventing a more specific reason.
    BoundedInconclusiveReasonNotRetained,
}

/// Selected behavior coordinates of one exact canonical positive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPositiveRouteMetric {
    pub witness_fingerprint: WitnessFingerprint,
    pub completion_ticks: usize,
    pub semantic_spans: usize,
    pub meaningful_input_transitions: usize,
    pub accepted_wall_jumps: usize,
    pub accepted_dashes: usize,
}

/// Canonical positive/inconclusive counts and diversity for one loadout.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalLoadoutMetricSummary {
    pub loadout: EvaluationLoadout,
    pub route_cell_count: usize,
    pub positive_route_count: usize,
    pub bounded_inconclusive_route_count: usize,
    pub behavior_diversity: RouteDiversityReport,
}

/// Direct-controller evidence, preserving each route/loadout cell.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectControllerMetricSummary {
    pub directed_route_count: usize,
    pub directed_routes: Vec<DirectedRouteControllerMetric>,
    pub by_loadout: Vec<DirectControllerLoadoutMetricSummary>,
    pub ability_bypasses: AbilityBypassMetricSummary,
}

/// Easiest-known controller evidence for one directed route.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectedRouteControllerMetric {
    pub source_door_id: String,
    pub target_door_id: String,
    pub audit_completeness: MetricEvidence<RouteAuditCompleteness>,
    pub overall_easiest_known: MetricEvidence<EasiestKnownControllerMetric>,
    pub exact_loadouts: Vec<DirectedRouteLoadoutControllerMetric>,
    pub positive_bypasses: Vec<DirectedRouteAbilityBypassMetric>,
}

/// Aggregate completeness copied from one directed route assessment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteAuditCompleteness {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        incomplete_loadouts: Vec<BoundedLoadoutMetric>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedLoadoutMetric {
    pub loadout: EvaluationLoadout,
    pub limit: DirectProbeBudgetLimit,
}

/// Direct-controller audit result for one exact route/loadout cell.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectedRouteLoadoutControllerMetric {
    pub loadout: EvaluationLoadout,
    pub audit: DirectControllerAuditMetric,
    pub easiest_known: MetricEvidence<EasiestKnownControllerMetric>,
}

/// Audit completeness and positive counts for an exact physics loadout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectControllerAuditMetric {
    CompleteFiniteVocabulary {
        raw_positive_witnesses: usize,
        retained_semantic_witnesses: usize,
    },
    BoundedIncomplete {
        limit: DirectProbeBudgetLimit,
        raw_positive_witnesses: usize,
        retained_semantic_witnesses: usize,
    },
    MissingDirectedRouteAssessment,
    MissingLoadoutAudit,
}

/// Controller observations for the deterministic representative of the
/// fused nondominated front. Inspect `selection_status` before treating the
/// representative as unique.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EasiestKnownControllerMetric {
    pub successful_loadout: EvaluationLoadout,
    pub demand: ControllerDemand,
    pub coordinates: ControllerDemandCoordinates,
    pub nondominated_front_size: usize,
    pub selection_status: EasiestKnownRouteSelectionStatus,
}

/// One exact lower-loadout positive bypass for a directed route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectedRouteAbilityBypassMetric {
    pub successful_loadout: EvaluationLoadout,
    pub retained_semantic_witnesses: usize,
}

/// Aggregate direct-controller metrics for one exact successful loadout.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectControllerLoadoutMetricSummary {
    pub loadout: EvaluationLoadout,
    /// Shared source/loadout audits; these are the actual audit work units.
    pub source_audit_completeness: AuditCompletenessCounts,
    /// Route/loadout cells referring to the shared audits.
    pub route_audit_completeness: AuditCompletenessCounts,
    /// Cells with an easiest-known fused positive. This includes canonical
    /// fallback positives when the direct vocabulary found none.
    pub known_positive_directed_routes: usize,
    /// Positive cells withheld from the single-value controller aggregates
    /// because their fused nondominated front is not unique.
    pub ambiguous_nondominated_front_directed_routes: usize,
    pub no_positive_in_complete_finite_vocabulary: usize,
    pub inconclusive_without_positive: usize,
    pub missing_route_or_audit: usize,
    pub easiest_controller_fractions: MetricEvidence<EasiestControllerFractionSummary>,
    pub demand_coordinates: MetricEvidence<ControllerDemandCoordinateSummary>,
}

/// Counts that make aggregate completeness auditable without a percentage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditCompletenessCounts {
    pub expected: usize,
    pub complete_finite_vocabulary: usize,
    pub bounded_incomplete: usize,
    pub missing: usize,
    pub state: AggregateAuditCompleteness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateAuditCompleteness {
    CompleteFiniteVocabulary,
    BoundedIncomplete,
    Missing,
    NotApplicableNoAuditsExpected,
}

/// Fractions among directed routes with a known positive under one exact
/// loadout.  Run-only and monotone-simple flags are not inferred for routes
/// without a positive controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EasiestControllerFractionSummary {
    pub successful_route_count: usize,
    /// Exclusive coordinate class 0.
    pub run_only_class_fraction: ExactFraction,
    /// Exclusive coordinate class 1 (monotone-simple but not run-only).
    pub monotone_simple_class_fraction: ExactFraction,
    /// Exclusive coordinate class 2.
    pub other_controller_class_fraction: ExactFraction,
    pub run_only_fraction: ExactFraction,
    /// Uses the `monotone_simple` flag, which also holds for run-only
    /// controllers under the current controller-demand policy.
    pub monotone_simple_fraction: ExactFraction,
}

/// Exact order-statistic representation of an integer coordinate.
///
/// The median is `(median_lower + median_upper) / 2`; retaining both central
/// observations avoids rounding half-integer medians.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegerCoordinateDistribution {
    pub sample_count: usize,
    pub minimum: usize,
    pub median_lower: usize,
    pub median_upper: usize,
    pub maximum: usize,
    pub spread: usize,
}

/// Exact order-statistic representation of a signed integer coordinate.
///
/// Landing edge margins are signed: a negative observation is an overhang,
/// zero is exactly flush with an edge, and a positive observation has support
/// beneath both player edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedIntegerCoordinateDistribution {
    pub sample_count: usize,
    pub minimum: i32,
    pub median_lower: i32,
    pub median_upper: i32,
    pub maximum: i32,
    pub spread: u32,
}

/// Room-wide and per-loadout exact landing evidence from canonical positive
/// route witnesses.
#[derive(Clone, Debug, PartialEq)]
pub struct LandingPrecisionMetricSummary {
    /// Sorted unique source schema versions. An empty vector means that no
    /// canonical positive route measurement was available.
    pub source_landing_precision_versions: Vec<u32>,
    pub aggregate: LandingPrecisionAggregateMetric,
    pub by_loadout: Vec<LandingPrecisionLoadoutMetricSummary>,
}

/// Landing evidence for one exact physics loadout.
#[derive(Clone, Debug, PartialEq)]
pub struct LandingPrecisionLoadoutMetricSummary {
    pub loadout: EvaluationLoadout,
    pub aggregate: LandingPrecisionAggregateMetric,
}

/// Transparent counts and exact landing-coordinate distributions.
///
/// Route-state counts intentionally overlap where their names say so. For
/// example a route may contribute to both `routes_with_measured_landings` and
/// `routes_with_unmeasured_landings`. `routes_with_only_unmeasured_landings`
/// identifies the important all-missing subset explicitly.
#[derive(Clone, Debug, PartialEq)]
pub struct LandingPrecisionAggregateMetric {
    pub canonical_positive_route_count: usize,
    pub inspected_ticks: usize,
    pub routes_with_landing_events: usize,
    pub routes_without_landing_events: usize,
    pub routes_with_measured_landings: usize,
    pub routes_with_unmeasured_landings: usize,
    pub routes_with_only_unmeasured_landings: usize,
    pub landing_event_count: usize,
    pub measured_landing_count: usize,
    pub unmeasured_landing_count: usize,
    /// Distribution of each measured sample's smaller signed edge margin.
    pub minimum_edge_margin_pixels: LandingCoordinateEvidence<SignedIntegerCoordinateDistribution>,
    pub footprint_overlap_pixels: LandingCoordinateEvidence<IntegerCoordinateDistribution>,
    pub support_width_pixels: LandingCoordinateEvidence<IntegerCoordinateDistribution>,
    pub edge_overhang_landings: usize,
    pub one_way_or_mixed_landings: usize,
}

/// Exact reason why landing-coordinate order statistics have no sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingCoordinateNotApplicableReason {
    NoCanonicalPositiveRoutes,
    NoLandingEvents,
    NoMeasuredLandings,
}

/// Availability of a coordinate derived from measured landing samples.
///
/// This narrower type prevents an exact, deterministic lack of samples from
/// being confused with missing or bounded search evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LandingCoordinateEvidence<T> {
    Observed(T),
    NotApplicable {
        reason: LandingCoordinateNotApplicableReason,
    },
}

/// Min/median/max/spread for every controller-demand coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerDemandCoordinateSummary {
    pub controller_class: IntegerCoordinateDistribution,
    pub ability_events: IntegerCoordinateDistribution,
    pub horizontal_reversals: IntegerCoordinateDistribution,
    pub vertical_decisions: IntegerCoordinateDistribution,
    pub semantic_spans: IntegerCoordinateDistribution,
    pub semantic_transitions: IntegerCoordinateDistribution,
    pub duration_ticks: IntegerCoordinateDistribution,
}

/// Counts of exact positive lower-loadout bypasses.  These are positive
/// evidence only and are not complemented into ability-requirement claims.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbilityBypassMetricSummary {
    pub directed_routes_with_any_bypass: usize,
    pub directed_routes_with_wall_jump_bypass: usize,
    pub directed_routes_with_dash_bypass: usize,
    pub directed_route_loadout_bypasses: usize,
    pub retained_semantic_bypass_witnesses: usize,
    pub by_successful_loadout: Vec<AbilityBypassLoadoutMetric>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityBypassLoadoutMetric {
    pub successful_loadout: EvaluationLoadout,
    pub directed_route_bypasses: usize,
    pub retained_semantic_witnesses: usize,
}

/// Comparison of the two directions for one unordered door pair and exact
/// loadout. `door_a` is lexicographically before `door_b`.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectionalAsymmetryMetric {
    pub door_a: String,
    pub door_b: String,
    pub loadout: EvaluationLoadout,
    pub a_to_b: MetricEvidence<EasiestKnownControllerMetric>,
    pub b_to_a: MetricEvidence<EasiestKnownControllerMetric>,
    pub comparison: MetricEvidence<DirectionalAsymmetryValues>,
}

/// Directional differences.  Both directional values are retained beside
/// the absolute difference and the direction with the larger value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalAsymmetryValues {
    pub duration_ticks: DirectionalCountDifference,
    pub semantic_spans: DirectionalCountDifference,
    pub semantic_transitions: DirectionalCountDifference,
    pub ability_events: DirectionalCountDifference,
    pub ability_use: DirectionalAbilityUseComparison,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalCountDifference {
    pub a_to_b: usize,
    pub b_to_a: usize,
    pub absolute_difference: usize,
    pub larger_direction: LargerDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LargerDirection {
    Equal,
    AToB,
    BToA,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalAbilityUseComparison {
    pub a_to_b: AbilityUseMetric,
    pub b_to_a: AbilityUseMetric,
    pub wall_jump_use_differs: bool,
    pub dash_use_differs: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityUseMetric {
    pub wall_jump_events: usize,
    pub dash_events: usize,
}

/// Terrain counts, coverage fractions, and exact-controller ablation survival.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMetricSummary {
    pub source_terrain_audit_version: u32,
    pub coverage: PositiveTerrainCoverage,
    pub coverage_fractions: TerrainCoverageFractionSummary,
    pub ablations: Vec<ExactControllerAblationSurvivalMetric>,
    /// Counts feature removals by what happened to the finite set of stored
    /// exact controllers. "All survived" is positive redundancy evidence for
    /// those controllers only; it is not proof that the feature is useless.
    pub ablation_utility: ExactControllerAblationUtilitySummary,
    pub aggregate_ablation_survival_fraction: MetricEvidence<ExactFraction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExactControllerAblationUtilitySummary {
    pub variant_count: usize,
    pub all_stored_controllers_survived: usize,
    pub at_least_one_stored_controller_affected: usize,
    pub no_stored_controllers: usize,
    pub removed_tiles_all_survived: usize,
    pub removed_tiles_some_affected: usize,
    pub removed_tiles_no_controllers: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainCoverageFractionSummary {
    pub structurally_attributed_components: MetricEvidence<ExactFraction>,
    pub structurally_attributed_tiles: MetricEvidence<ExactFraction>,
    pub traversal_near_components: MetricEvidence<ExactFraction>,
    pub traversal_near_tiles: MetricEvidence<ExactFraction>,
    pub positively_corroborated_components: MetricEvidence<ExactFraction>,
    pub positively_corroborated_tiles: MetricEvidence<ExactFraction>,
}

/// Survival of exact stored controllers after one canonical feature removal.
#[derive(Clone, Debug, PartialEq)]
pub struct ExactControllerAblationSurvivalMetric {
    pub kind: RoomAblationKind,
    pub outcomes: AblationOutcomeSummary,
    pub survival_fraction: MetricEvidence<ExactFraction>,
}

/// Search cost, explicitly excluded from player-facing aggregation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationalCostSummary {
    /// Authoritative total stored by the analysis layer.
    pub direct_controller_reported_total: OperationalCost,
    /// Independently recomputed once per shared source/loadout audit.
    pub direct_controller_recomputed_total: OperationalCost,
    pub direct_controller_audit_units: Vec<DirectControllerOperationalCostMetric>,
    pub canonical_positive_route_total: OperationalCost,
    pub canonical_positive_routes: Vec<CanonicalRouteOperationalCostMetric>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperationalCost {
    pub expanded_nodes: usize,
    pub generated_nodes: usize,
    pub simulated_ticks: usize,
    pub deepest_path_ticks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectControllerOperationalCostMetric {
    pub source_door_id: String,
    pub loadout: EvaluationLoadout,
    pub status: LoadoutControllerAuditStatus,
    pub cost: OperationalCost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRouteOperationalCostMetric {
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub cost: OperationalCost,
}

/// Malformed analysis data which would make an aggregate ambiguous.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomMetricSummaryError {
    UnsupportedAnalysisVersion {
        expected: u32,
        actual: u32,
    },
    InvalidAnalysisConfig {
        source: CorpusRoomAnalysisConfigError,
    },
    InvalidAnalysisContract {
        detail: String,
    },
    DuplicateSourceAssessmentBatch {
        source_door_id: String,
    },
    DuplicateDirectedRouteAssessment {
        source_door_id: String,
        target_door_id: String,
    },
    RouteAssessmentSourceMismatch {
        batch_source_door_id: String,
        route_source_door_id: String,
        target_door_id: String,
    },
    DuplicateCanonicalRouteMeasurement {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    DuplicateRouteLoadoutAudit {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    DuplicateSharedLoadoutAudit {
        source_door_id: String,
        loadout: EvaluationLoadout,
    },
}

impl fmt::Display for RoomMetricSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAnalysisVersion { expected, actual } => write!(
                formatter,
                "room analysis version {actual} is unsupported; expected {expected}"
            ),
            Self::InvalidAnalysisConfig { source } => {
                write!(
                    formatter,
                    "room analysis has invalid config identity: {source}"
                )
            }
            Self::InvalidAnalysisContract { detail } => {
                write!(
                    formatter,
                    "room analysis violates its exact policy contract: {detail}"
                )
            }
            Self::DuplicateSourceAssessmentBatch { source_door_id } => write!(
                formatter,
                "duplicate direct-controller source batch for {source_door_id:?}"
            ),
            Self::DuplicateDirectedRouteAssessment {
                source_door_id,
                target_door_id,
            } => write!(
                formatter,
                "duplicate direct-controller assessment for {source_door_id:?} -> {target_door_id:?}"
            ),
            Self::RouteAssessmentSourceMismatch {
                batch_source_door_id,
                route_source_door_id,
                target_door_id,
            } => write!(
                formatter,
                "source batch {batch_source_door_id:?} contains route {route_source_door_id:?} -> {target_door_id:?}"
            ),
            Self::DuplicateCanonicalRouteMeasurement {
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "duplicate canonical measurement for {} {source_door_id:?} -> {target_door_id:?}",
                loadout.slug()
            ),
            Self::DuplicateRouteLoadoutAudit {
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "duplicate {} audit for direct route {source_door_id:?} -> {target_door_id:?}",
                loadout.slug()
            ),
            Self::DuplicateSharedLoadoutAudit {
                source_door_id,
                loadout,
            } => write!(
                formatter,
                "duplicate shared {} audit for source {source_door_id:?}",
                loadout.slug()
            ),
        }
    }
}

impl Error for RoomMetricSummaryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAnalysisConfig { source } => Some(source),
            _ => None,
        }
    }
}

/// Aggregate one complete in-memory analysis without collapsing its route
/// matrix or uncertainty states.
pub fn summarize_room_metrics(
    analysis: &CorpusRoomAnalysis,
) -> Result<RoomMetricSummary, RoomMetricSummaryError> {
    if analysis.version != CORPUS_ROOM_ANALYSIS_VERSION {
        return Err(RoomMetricSummaryError::UnsupportedAnalysisVersion {
            expected: CORPUS_ROOM_ANALYSIS_VERSION,
            actual: analysis.version,
        });
    }
    analysis
        .config
        .validate()
        .map_err(|source| RoomMetricSummaryError::InvalidAnalysisConfig { source })?;
    validate_analysis_contract(analysis)?;
    let indexed = IndexedAnalysis::new(analysis)?;
    let canonical_routes = canonical_route_summary(analysis, &indexed);
    let landing_precision = landing_precision_summary(analysis);
    let direct_controllers = direct_controller_summary(&indexed);
    let directional_asymmetry = directional_asymmetry(&indexed, &direct_controllers);
    let terrain = terrain_summary(analysis);
    let operational_cost = operational_cost_summary(analysis);

    Ok(RoomMetricSummary {
        version: ROOM_METRIC_SUMMARY_VERSION,
        source_analysis_version: analysis.version,
        source_analysis_config: analysis.config.clone(),
        room_id: analysis.room_id.clone(),
        canonical_routes,
        landing_precision,
        direct_controllers,
        directional_asymmetry,
        terrain,
        operational_cost,
    })
}

fn validate_analysis_contract(analysis: &CorpusRoomAnalysis) -> Result<(), RoomMetricSummaryError> {
    let invalid = |detail: String| RoomMetricSummaryError::InvalidAnalysisContract { detail };
    let source_ids = analysis
        .source_route_assessments
        .iter()
        .map(|batch| batch.source_door_id.clone())
        .collect::<Vec<_>>();
    if source_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(
            "source assessment batches are not in strict canonical door order".to_owned(),
        ));
    }
    let source_id_set = source_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut recomputed_total = SearchStats::default();
    for batch in &analysis.source_route_assessments {
        if batch.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY {
            return Err(invalid(format!(
                "source {:?} uses a stale route-controller policy",
                batch.source_door_id
            )));
        }
        if batch.authoritative_loadout != EvaluationLoadout::Both
            || batch.expected_subset_loadouts != EvaluationLoadout::ALL
        {
            return Err(invalid(format!(
                "source {:?} does not retain the exact Both/all-subsets contract",
                batch.source_door_id
            )));
        }
        let expected_targets = source_ids
            .iter()
            .filter(|target| **target != batch.source_door_id)
            .cloned()
            .collect::<Vec<_>>();
        if batch.target_door_ids != expected_targets || batch.routes.len() != expected_targets.len()
        {
            return Err(invalid(format!(
                "source {:?} does not retain every canonical target exactly once",
                batch.source_door_id
            )));
        }
        if batch
            .shared_audits
            .iter()
            .map(|audit| audit.loadout)
            .ne(EvaluationLoadout::ALL)
        {
            return Err(invalid(format!(
                "source {:?} shared audits are not the four canonical loadouts",
                batch.source_door_id
            )));
        }
        for shared in &batch.shared_audits {
            if shared.target_count != expected_targets.len()
                || shared.raw_positive_witnesses < shared.retained_semantic_witnesses
            {
                return Err(invalid(format!(
                    "source {:?} {} shared audit has inconsistent target/witness counts",
                    batch.source_door_id,
                    shared.loadout.slug()
                )));
            }
        }
        accumulate_analysis_stats(&mut recomputed_total, batch.total_operational_stats());

        for (route, expected_target) in batch.routes.iter().zip(&expected_targets) {
            if route.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY
                || route.source_door_id != batch.source_door_id
                || route.target_door_id != *expected_target
                || route.authoritative_loadout != EvaluationLoadout::Both
                || route.expected_subset_loadouts != EvaluationLoadout::ALL
            {
                return Err(invalid(format!(
                    "route {:?}->{:?} has stale policy or noncanonical identity/loadout metadata",
                    route.source_door_id, route.target_door_id
                )));
            }
            if route
                .audits
                .iter()
                .map(|audit| audit.loadout)
                .ne(EvaluationLoadout::ALL)
            {
                return Err(invalid(format!(
                    "route {:?}->{:?} audits are not the four canonical loadouts",
                    route.source_door_id, route.target_door_id
                )));
            }
            let incomplete = route
                .audits
                .iter()
                .filter_map(|audit| match audit.status {
                    LoadoutControllerAuditStatus::CompleteFiniteVocabulary => None,
                    LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                        Some(BoundedIncompleteLoadout {
                            loadout: audit.loadout,
                            limit,
                        })
                    }
                })
                .collect::<Vec<_>>();
            let expected_completeness = if incomplete.is_empty() {
                RouteControllerAuditCompleteness::CompleteFiniteVocabulary
            } else {
                RouteControllerAuditCompleteness::BoundedIncomplete {
                    incomplete_loadouts: incomplete,
                }
            };
            if route.completeness != expected_completeness {
                return Err(invalid(format!(
                    "route {:?}->{:?} completeness disagrees with its exact audits",
                    route.source_door_id, route.target_door_id
                )));
            }
            for (audit, shared) in route.audits.iter().zip(&batch.shared_audits) {
                let retained = route
                    .easiest_first_witnesses
                    .iter()
                    .filter(|witness| witness.loadout == audit.loadout)
                    .count();
                if audit.status != shared.status
                    || audit.operational_stats != shared.operational_stats
                    || audit.retained_semantic_witnesses != retained
                    || audit.raw_positive_witnesses < audit.retained_semantic_witnesses
                {
                    return Err(invalid(format!(
                        "route {:?}->{:?} {} audit disagrees with shared status/stats or retained witnesses",
                        route.source_door_id,
                        route.target_door_id,
                        audit.loadout.slug()
                    )));
                }
            }
            if route
                .easiest_known_front
                .iter()
                .any(|index| *index >= route.easiest_first_witnesses.len())
            {
                return Err(invalid(format!(
                    "route {:?}->{:?} easiest-known front has an invalid witness index",
                    route.source_door_id, route.target_door_id
                )));
            }
            for bypass in &route.positive_bypasses {
                if bypass.loadout == EvaluationLoadout::Both
                    || bypass.witness_indices.is_empty()
                    || bypass.witness_indices.iter().any(|index| {
                        route
                            .easiest_first_witnesses
                            .get(*index)
                            .is_none_or(|witness| witness.loadout != bypass.loadout)
                    })
                {
                    return Err(invalid(format!(
                        "route {:?}->{:?} has malformed positive-bypass indices",
                        route.source_door_id, route.target_door_id
                    )));
                }
            }
        }
    }
    if analysis.direct_controller_operational_stats != recomputed_total {
        return Err(invalid(
            "reported direct-controller total differs from exact per-source shared audits"
                .to_owned(),
        ));
    }

    let mut expected_fused_keys = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        for source in &source_ids {
            for target in source_ids.iter().filter(|target| *target != source) {
                expected_fused_keys.push((source.clone(), target.clone(), loadout));
            }
        }
    }
    let actual_fused_keys = analysis
        .fused_route_cells
        .iter()
        .map(|cell| {
            (
                cell.source_door_id.clone(),
                cell.target_door_id.clone(),
                cell.loadout,
            )
        })
        .collect::<Vec<_>>();
    if actual_fused_keys != expected_fused_keys {
        return Err(invalid(
            "fused route cells are not the complete canonical loadout/source/target grid"
                .to_owned(),
        ));
    }
    for cell in &analysis.fused_route_cells {
        let selection_shape_valid = match (cell.selected, cell.nondominated_front.len()) {
            (None, 0) => cell.candidates.is_empty(),
            (Some(selection), 1) => {
                selection.status == EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate
                    && cell.nondominated_front.first().copied() == Some(selection.candidate_index)
            }
            (Some(selection), front_size) => {
                front_size > 1
                    && selection.status
                        == EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                            front_size,
                        }
                    && cell.nondominated_front.first().copied() == Some(selection.candidate_index)
            }
            (None, _) => false,
        };
        if cell.policy != CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY
            || cell.source_door_id == cell.target_door_id
            || !selection_shape_valid
            || cell
                .nondominated_front
                .iter()
                .any(|index| *index >= cell.candidates.len())
            || cell.selected.is_some_and(|selection| {
                cell.candidates
                    .get(selection.candidate_index)
                    .is_none_or(|candidate| {
                        candidate.replay_identity != selection.replay_identity
                            || !cell.nondominated_front.contains(&selection.candidate_index)
                    })
            })
            || (cell.candidates.is_empty() != cell.selected.is_none())
        {
            return Err(invalid(format!(
                "fused route {:?}->{:?} under {} has stale policy or malformed front/selection",
                cell.source_door_id,
                cell.target_door_id,
                cell.loadout.slug()
            )));
        }
        let route = analysis
            .source_route_assessments
            .iter()
            .find(|batch| batch.source_door_id == cell.source_door_id)
            .and_then(|batch| {
                batch
                    .routes
                    .iter()
                    .find(|route| route.target_door_id == cell.target_door_id)
            })
            .expect("complete fused grid was derived from the canonical direct route grid");
        let audit = route
            .audits
            .iter()
            .find(|audit| audit.loadout == cell.loadout)
            .expect("validated direct routes retain every exact loadout audit");
        if cell.direct_audit_status != audit.status.into()
            || cell.raw_direct_positive_witnesses != audit.raw_positive_witnesses
            || cell.retained_direct_positive_witnesses != audit.retained_semantic_witnesses
        {
            return Err(invalid(format!(
                "fused route {:?}->{:?} under {} disagrees with its direct audit",
                cell.source_door_id,
                cell.target_door_id,
                cell.loadout.slug()
            )));
        }
        let mut direct_indices = cell
            .candidates
            .iter()
            .flat_map(|candidate| &candidate.provenance)
            .filter_map(|provenance| match provenance {
                FusedRouteCandidateProvenance::DirectWitness { witness_index } => {
                    Some(*witness_index)
                }
                FusedRouteCandidateProvenance::CanonicalMatrix { .. } => None,
            })
            .collect::<Vec<_>>();
        direct_indices.sort_unstable();
        let expected_direct_indices = route
            .easiest_first_witnesses
            .iter()
            .enumerate()
            .filter_map(|(index, witness)| (witness.loadout == cell.loadout).then_some(index))
            .collect::<Vec<_>>();
        if direct_indices != expected_direct_indices {
            return Err(invalid(format!(
                "fused route {:?}->{:?} under {} does not retain every exact direct witness once",
                cell.source_door_id,
                cell.target_door_id,
                cell.loadout.slug()
            )));
        }
        let canonical = analysis
            .canonical_route_measurements
            .iter()
            .find(|measurement| {
                measurement.source_door_id == cell.source_door_id
                    && measurement.target_door_id == cell.target_door_id
                    && measurement.loadout == cell.loadout
            });
        match (cell.canonical_matrix_status, canonical) {
            (
                CanonicalMatrixCellStatus::Positive {
                    witness_fingerprint,
                },
                Some(measurement),
            ) if measurement.witness_fingerprint == witness_fingerprint => {}
            (CanonicalMatrixCellStatus::BoundedInconclusive { .. }, None) => {}
            _ => {
                return Err(invalid(format!(
                    "fused route {:?}->{:?} under {} disagrees with canonical measurement provenance",
                    cell.source_door_id,
                    cell.target_door_id,
                    cell.loadout.slug()
                )));
            }
        }
    }

    for measurement in &analysis.canonical_route_measurements {
        if measurement.version != CORPUS_ROUTE_MEASUREMENT_VERSION
            || measurement.landing_precision.version != LANDING_PRECISION_VERSION
            || measurement.vector.version != ROUTE_DIFFICULTY_VECTOR_VERSION
        {
            return Err(invalid(format!(
                "canonical measurement {:?}->{:?} under {} carries a stale policy version",
                measurement.source_door_id,
                measurement.target_door_id,
                measurement.loadout.slug()
            )));
        }
        if !source_id_set.contains(&measurement.source_door_id)
            || !source_id_set.contains(&measurement.target_door_id)
            || measurement.source_door_id == measurement.target_door_id
            || measurement.observation.reached_exit_id != measurement.target_door_id
            || measurement.exact_difficulty.exit_id != measurement.target_door_id
            || measurement.vector.target_id != measurement.target_door_id
            || measurement.shaky_hand.is_some()
        {
            return Err(invalid(format!(
                "canonical measurement {:?}->{:?} has an invalid identity or noncanonical shaky-hand payload",
                measurement.source_door_id, measurement.target_door_id
            )));
        }
    }
    let expected_policy = CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY;
    if expected_policy.semantic_trace_version != ROUTE_CONTROLLER_TRACE_VERSION
        || expected_policy.controller_demand_version != CONTROLLER_DEMAND_POLICY_VERSION
    {
        return Err(invalid(
            "compiled route-controller policy constants are internally inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn accumulate_analysis_stats(total: &mut SearchStats, additional: SearchStats) {
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
}

fn landing_precision_summary(analysis: &CorpusRoomAnalysis) -> LandingPrecisionMetricSummary {
    let source_landing_precision_versions = analysis
        .canonical_route_measurements
        .iter()
        .map(|measurement| measurement.landing_precision.version)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let aggregate = landing_precision_aggregate(
        analysis
            .canonical_route_measurements
            .iter()
            .map(|measurement| &measurement.landing_precision),
    );
    let by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| LandingPrecisionLoadoutMetricSummary {
            loadout,
            aggregate: landing_precision_aggregate(
                analysis
                    .canonical_route_measurements
                    .iter()
                    .filter(move |measurement| measurement.loadout == loadout)
                    .map(|measurement| &measurement.landing_precision),
            ),
        })
        .collect();
    LandingPrecisionMetricSummary {
        source_landing_precision_versions,
        aggregate,
        by_loadout,
    }
}

fn landing_precision_aggregate<'a>(
    reports: impl IntoIterator<Item = &'a LandingPrecisionReport>,
) -> LandingPrecisionAggregateMetric {
    let reports = reports.into_iter().collect::<Vec<_>>();
    let canonical_positive_route_count = reports.len();
    let inspected_ticks = reports.iter().map(|report| report.inspected_ticks).sum();
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
    let landing_event_count = reports
        .iter()
        .map(|report| report.landing_event_count)
        .sum();
    let measured_landing_count = reports.iter().map(|report| report.samples.len()).sum();
    let unmeasured_landing_count = reports
        .iter()
        .map(|report| report.unmeasured_landing_ticks.len())
        .sum();
    debug_assert_eq!(
        landing_event_count,
        measured_landing_count + unmeasured_landing_count,
        "validated landing reports partition authoritative events into measured and unmeasured"
    );

    let samples = reports
        .iter()
        .flat_map(|report| report.samples.iter())
        .collect::<Vec<_>>();
    let unavailable_reason = if canonical_positive_route_count == 0 {
        LandingCoordinateNotApplicableReason::NoCanonicalPositiveRoutes
    } else if landing_event_count == 0 {
        LandingCoordinateNotApplicableReason::NoLandingEvents
    } else {
        LandingCoordinateNotApplicableReason::NoMeasuredLandings
    };
    let minimum_edge_margin_pixels = if samples.is_empty() {
        LandingCoordinateEvidence::NotApplicable {
            reason: unavailable_reason,
        }
    } else {
        LandingCoordinateEvidence::Observed(signed_integer_distribution(
            samples
                .iter()
                .map(|sample| sample.minimum_edge_margin_pixels()),
        ))
    };
    let footprint_overlap_pixels =
        if samples.is_empty() {
            LandingCoordinateEvidence::NotApplicable {
                reason: unavailable_reason,
            }
        } else {
            LandingCoordinateEvidence::Observed(integer_distribution(samples.iter().map(
                |sample| usize::try_from(sample.footprint_overlap_pixels).unwrap_or(usize::MAX),
            )))
        };
    let support_width_pixels =
        if samples.is_empty() {
            LandingCoordinateEvidence::NotApplicable {
                reason: unavailable_reason,
            }
        } else {
            LandingCoordinateEvidence::Observed(integer_distribution(samples.iter().map(
                |sample| usize::try_from(sample.support_width_pixels()).unwrap_or(usize::MAX),
            )))
        };
    let edge_overhang_landings = samples
        .iter()
        .filter(|sample| sample.has_edge_overhang())
        .count();
    let one_way_or_mixed_landings = samples
        .iter()
        .filter(|sample| sample.support_kind != LandingSupportKind::Solid)
        .count();

    LandingPrecisionAggregateMetric {
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
        edge_overhang_landings,
        one_way_or_mixed_landings,
    }
}

type RouteKey = (String, String);
type RouteLoadoutKey = (String, String, EvaluationLoadout);

struct IndexedAnalysis<'a> {
    door_ids: Vec<String>,
    route_keys: Vec<RouteKey>,
    source_batches: BTreeMap<String, &'a super::SourceRouteControllerAssessmentBatch>,
    routes: BTreeMap<RouteKey, &'a RouteControllerAssessment>,
    canonical: BTreeMap<RouteLoadoutKey, &'a super::CorpusRouteMeasurement>,
    fused: BTreeMap<RouteLoadoutKey, &'a FusedRouteCellAssessment>,
}

impl<'a> IndexedAnalysis<'a> {
    fn new(analysis: &'a CorpusRoomAnalysis) -> Result<Self, RoomMetricSummaryError> {
        let mut door_ids = BTreeSet::new();
        let mut source_batches = BTreeMap::new();
        let mut routes = BTreeMap::new();
        for batch in &analysis.source_route_assessments {
            door_ids.insert(batch.source_door_id.clone());
            door_ids.extend(batch.target_door_ids.iter().cloned());
            if source_batches
                .insert(batch.source_door_id.clone(), batch)
                .is_some()
            {
                return Err(RoomMetricSummaryError::DuplicateSourceAssessmentBatch {
                    source_door_id: batch.source_door_id.clone(),
                });
            }
            for route in &batch.routes {
                if route.source_door_id != batch.source_door_id {
                    return Err(RoomMetricSummaryError::RouteAssessmentSourceMismatch {
                        batch_source_door_id: batch.source_door_id.clone(),
                        route_source_door_id: route.source_door_id.clone(),
                        target_door_id: route.target_door_id.clone(),
                    });
                }
                door_ids.insert(route.source_door_id.clone());
                door_ids.insert(route.target_door_id.clone());
                let key = (route.source_door_id.clone(), route.target_door_id.clone());
                if routes.insert(key.clone(), route).is_some() {
                    return Err(RoomMetricSummaryError::DuplicateDirectedRouteAssessment {
                        source_door_id: key.0,
                        target_door_id: key.1,
                    });
                }
                unique_route_audits(route)?;
            }
            unique_shared_audits(batch)?;
        }

        let mut canonical = BTreeMap::new();
        for measurement in &analysis.canonical_route_measurements {
            door_ids.insert(measurement.source_door_id.clone());
            door_ids.insert(measurement.target_door_id.clone());
            let key = (
                measurement.source_door_id.clone(),
                measurement.target_door_id.clone(),
                measurement.loadout,
            );
            if canonical.insert(key.clone(), measurement).is_some() {
                return Err(RoomMetricSummaryError::DuplicateCanonicalRouteMeasurement {
                    source_door_id: key.0,
                    target_door_id: key.1,
                    loadout: key.2,
                });
            }
        }
        let mut fused = BTreeMap::new();
        for cell in &analysis.fused_route_cells {
            door_ids.insert(cell.source_door_id.clone());
            door_ids.insert(cell.target_door_id.clone());
            let key = (
                cell.source_door_id.clone(),
                cell.target_door_id.clone(),
                cell.loadout,
            );
            if fused.insert(key.clone(), cell).is_some() {
                return Err(RoomMetricSummaryError::InvalidAnalysisContract {
                    detail: format!(
                        "duplicate fused route cell for {:?}->{:?} under {}",
                        key.0,
                        key.1,
                        key.2.slug()
                    ),
                });
            }
        }
        for witness in &analysis.terrain_audit.positive_witnesses {
            door_ids.insert(witness.controller.source_door_id.clone());
            if let super::PositiveControllerTarget::Door(target) = &witness.controller.target {
                door_ids.insert(target.clone());
            }
        }

        let door_ids = door_ids.into_iter().collect::<Vec<_>>();
        let route_keys = door_ids
            .iter()
            .flat_map(|source| {
                door_ids
                    .iter()
                    .filter(move |target| *target != source)
                    .map(move |target| (source.clone(), target.clone()))
            })
            .collect();
        Ok(Self {
            door_ids,
            route_keys,
            source_batches,
            routes,
            canonical,
            fused,
        })
    }
}

fn unique_route_audits(route: &RouteControllerAssessment) -> Result<(), RoomMetricSummaryError> {
    let mut seen = BTreeSet::new();
    for audit in &route.audits {
        if !seen.insert(audit.loadout) {
            return Err(RoomMetricSummaryError::DuplicateRouteLoadoutAudit {
                source_door_id: route.source_door_id.clone(),
                target_door_id: route.target_door_id.clone(),
                loadout: audit.loadout,
            });
        }
    }
    Ok(())
}

fn unique_shared_audits(
    batch: &super::SourceRouteControllerAssessmentBatch,
) -> Result<(), RoomMetricSummaryError> {
    let mut seen = BTreeSet::new();
    for audit in &batch.shared_audits {
        if !seen.insert(audit.loadout) {
            return Err(RoomMetricSummaryError::DuplicateSharedLoadoutAudit {
                source_door_id: batch.source_door_id.clone(),
                loadout: audit.loadout,
            });
        }
    }
    Ok(())
}

fn canonical_route_summary(
    analysis: &CorpusRoomAnalysis,
    indexed: &IndexedAnalysis<'_>,
) -> CanonicalRouteMetricSummary {
    let mut directed_loadout_routes = Vec::new();
    let mut by_loadout = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        let observations = analysis
            .canonical_route_measurements
            .iter()
            .filter(|measurement| measurement.loadout == loadout)
            .map(|measurement| measurement.observation.clone())
            .collect::<Vec<_>>();
        let positive_route_count = observations.len();
        by_loadout.push(CanonicalLoadoutMetricSummary {
            loadout,
            route_cell_count: indexed.route_keys.len(),
            positive_route_count,
            bounded_inconclusive_route_count: indexed
                .route_keys
                .len()
                .saturating_sub(positive_route_count),
            behavior_diversity: route_diversity(&observations),
        });
        for (source_door_id, target_door_id) in &indexed.route_keys {
            let key = (source_door_id.clone(), target_door_id.clone(), loadout);
            let evidence = indexed.canonical.get(&key).map_or(
                CanonicalRouteMetricEvidence::BoundedInconclusiveReasonNotRetained,
                |measurement| {
                    CanonicalRouteMetricEvidence::Positive(CanonicalPositiveRouteMetric {
                        witness_fingerprint: measurement.witness_fingerprint,
                        completion_ticks: measurement.observation.completion_ticks,
                        semantic_spans: measurement.observation.actions.spans.len(),
                        meaningful_input_transitions: measurement
                            .vector
                            .control
                            .meaningful_input_transitions,
                        accepted_wall_jumps: measurement
                            .vector
                            .control
                            .accepted_movement
                            .wall_jumps,
                        accepted_dashes: measurement.vector.control.accepted_movement.dashes,
                    })
                },
            );
            directed_loadout_routes.push(CanonicalDirectedLoadoutRouteMetric {
                source_door_id: source_door_id.clone(),
                target_door_id: target_door_id.clone(),
                loadout,
                evidence,
            });
        }
    }
    let positive_route_count = analysis.canonical_route_measurements.len();
    let loadout_route_cell_count = indexed
        .route_keys
        .len()
        .saturating_mul(EvaluationLoadout::ALL.len());
    let observations = analysis
        .canonical_route_measurements
        .iter()
        .map(|measurement| measurement.observation.clone())
        .collect::<Vec<_>>();
    CanonicalRouteMetricSummary {
        directed_route_count: indexed.route_keys.len(),
        loadout_route_cell_count,
        positive_route_count,
        bounded_inconclusive_route_count: loadout_route_cell_count
            .saturating_sub(positive_route_count),
        directed_loadout_routes,
        by_loadout,
        behavior_diversity: route_diversity(&observations),
    }
}

fn direct_controller_summary(indexed: &IndexedAnalysis<'_>) -> DirectControllerMetricSummary {
    let directed_routes = indexed
        .route_keys
        .iter()
        .map(|key| directed_route_controller_metric(key, indexed.routes.get(key).copied(), indexed))
        .collect::<Vec<_>>();
    let by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| direct_loadout_summary(loadout, indexed, &directed_routes))
        .collect();
    let ability_bypasses = ability_bypass_summary(&directed_routes);
    DirectControllerMetricSummary {
        directed_route_count: indexed.route_keys.len(),
        directed_routes,
        by_loadout,
        ability_bypasses,
    }
}

fn directed_route_controller_metric(
    key: &RouteKey,
    route: Option<&RouteControllerAssessment>,
    indexed: &IndexedAnalysis<'_>,
) -> DirectedRouteControllerMetric {
    let Some(route) = route else {
        return DirectedRouteControllerMetric {
            source_door_id: key.0.clone(),
            target_door_id: key.1.clone(),
            audit_completeness: MetricEvidence::Missing {
                reason: MissingMetricReason::MissingDirectedRouteAssessment,
            },
            overall_easiest_known: MetricEvidence::Missing {
                reason: MissingMetricReason::MissingDirectedRouteAssessment,
            },
            exact_loadouts: EvaluationLoadout::ALL
                .into_iter()
                .map(|loadout| DirectedRouteLoadoutControllerMetric {
                    loadout,
                    audit: DirectControllerAuditMetric::MissingDirectedRouteAssessment,
                    easiest_known: MetricEvidence::Missing {
                        reason: MissingMetricReason::MissingDirectedRouteAssessment,
                    },
                })
                .collect(),
            positive_bypasses: Vec::new(),
        };
    };

    let audit_completeness = MetricEvidence::Observed(match &route.completeness {
        RouteControllerAuditCompleteness::CompleteFiniteVocabulary => {
            RouteAuditCompleteness::CompleteFiniteVocabulary
        }
        RouteControllerAuditCompleteness::BoundedIncomplete {
            incomplete_loadouts,
        } => RouteAuditCompleteness::BoundedIncomplete {
            incomplete_loadouts: incomplete_loadouts
                .iter()
                .map(|incomplete| BoundedLoadoutMetric {
                    loadout: incomplete.loadout,
                    limit: incomplete.limit,
                })
                .collect(),
        },
    });
    let overall_easiest = EvaluationLoadout::ALL
        .into_iter()
        .filter_map(|loadout| {
            indexed
                .fused
                .get(&(key.0.clone(), key.1.clone(), loadout))
                .and_then(|cell| {
                    cell.selected_representative()
                        .map(|candidate| (loadout, *cell, candidate))
                })
        })
        .min_by(|(left_loadout, _, left), (right_loadout, _, right)| {
            left.controller_coordinates
                .cmp(&right.controller_coordinates)
                .then_with(|| left_loadout.cmp(right_loadout))
                .then_with(|| left.replay_identity.cmp(&right.replay_identity))
        });
    let overall_easiest_known = overall_easiest.map_or_else(
        || match &route.completeness {
            RouteControllerAuditCompleteness::CompleteFiniteVocabulary => {
                MetricEvidence::NotApplicable {
                    reason:
                        NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary,
                }
            }
            RouteControllerAuditCompleteness::BoundedIncomplete { .. } => {
                MetricEvidence::Missing {
                    reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive,
                }
            }
        },
        |(loadout, cell, candidate)| {
            MetricEvidence::Observed(easiest_controller_metric(loadout, cell, candidate))
        },
    );
    let exact_loadouts = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| {
            let fused = indexed
                .fused
                .get(&(key.0.clone(), key.1.clone(), loadout))
                .copied();
            route_loadout_controller_metric(route, loadout, fused)
        })
        .collect();
    let positive_bypasses = route
        .positive_bypasses
        .iter()
        .map(|bypass| DirectedRouteAbilityBypassMetric {
            successful_loadout: bypass.loadout,
            retained_semantic_witnesses: bypass.witness_indices.len(),
        })
        .collect();
    DirectedRouteControllerMetric {
        source_door_id: key.0.clone(),
        target_door_id: key.1.clone(),
        audit_completeness,
        overall_easiest_known,
        exact_loadouts,
        positive_bypasses,
    }
}

fn route_loadout_controller_metric(
    route: &RouteControllerAssessment,
    loadout: EvaluationLoadout,
    fused: Option<&FusedRouteCellAssessment>,
) -> DirectedRouteLoadoutControllerMetric {
    let audit = route.audits.iter().find(|audit| audit.loadout == loadout);
    let audit_metric = match audit {
        Some(audit) => match audit.status {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                DirectControllerAuditMetric::CompleteFiniteVocabulary {
                    raw_positive_witnesses: audit.raw_positive_witnesses,
                    retained_semantic_witnesses: audit.retained_semantic_witnesses,
                }
            }
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                DirectControllerAuditMetric::BoundedIncomplete {
                    limit,
                    raw_positive_witnesses: audit.raw_positive_witnesses,
                    retained_semantic_witnesses: audit.retained_semantic_witnesses,
                }
            }
        },
        None => DirectControllerAuditMetric::MissingLoadoutAudit,
    };
    let easiest = fused.and_then(|cell| {
        cell.selected_representative()
            .map(|candidate| (cell, candidate))
    });
    let easiest_known = easiest.map_or_else(
        || match audit_metric {
            DirectControllerAuditMetric::CompleteFiniteVocabulary { .. } => {
                MetricEvidence::NotApplicable {
                    reason:
                        NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary,
                }
            }
            DirectControllerAuditMetric::BoundedIncomplete { .. } => MetricEvidence::Missing {
                reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive,
            },
            DirectControllerAuditMetric::MissingDirectedRouteAssessment => {
                MetricEvidence::Missing {
                    reason: MissingMetricReason::MissingDirectedRouteAssessment,
                }
            }
            DirectControllerAuditMetric::MissingLoadoutAudit => MetricEvidence::Missing {
                reason: MissingMetricReason::MissingDirectControllerAudit,
            },
        },
        |(cell, candidate)| {
            MetricEvidence::Observed(easiest_controller_metric(loadout, cell, candidate))
        },
    );
    DirectedRouteLoadoutControllerMetric {
        loadout,
        audit: audit_metric,
        easiest_known,
    }
}

fn easiest_controller_metric(
    loadout: EvaluationLoadout,
    cell: &FusedRouteCellAssessment,
    candidate: &FusedRouteCandidate,
) -> EasiestKnownControllerMetric {
    let selection = cell
        .selected
        .expect("a selected representative accompanies the supplied candidate");
    EasiestKnownControllerMetric {
        successful_loadout: loadout,
        demand: candidate.controller_demand,
        coordinates: candidate.controller_coordinates,
        nondominated_front_size: cell.nondominated_front.len(),
        selection_status: selection.status,
    }
}

fn direct_loadout_summary(
    loadout: EvaluationLoadout,
    indexed: &IndexedAnalysis<'_>,
    directed_routes: &[DirectedRouteControllerMetric],
) -> DirectControllerLoadoutMetricSummary {
    let source_counts = source_audit_counts(loadout, indexed);
    let exact_cells = directed_routes
        .iter()
        .map(|route| {
            route
                .exact_loadouts
                .iter()
                .find(|metric| metric.loadout == loadout)
                .expect("every directed metric contains all evaluation loadouts")
        })
        .collect::<Vec<_>>();
    let route_counts = audit_counts(exact_cells.len(), exact_cells.iter().map(|cell| cell.audit));
    let successes = exact_cells
        .iter()
        .filter_map(|cell| match cell.easiest_known {
            MetricEvidence::Observed(value) => Some(value),
            MetricEvidence::Missing { .. } | MetricEvidence::NotApplicable { .. } => None,
        })
        .collect::<Vec<_>>();
    let known_positive_directed_routes = successes.len();
    let ambiguous_nondominated_front_directed_routes = successes
        .iter()
        .filter(|success| {
            matches!(
                success.selection_status,
                EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { .. }
            )
        })
        .count();
    let no_positive_in_complete_finite_vocabulary = exact_cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.easiest_known,
                MetricEvidence::NotApplicable {
                    reason: NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary
                }
            )
        })
        .count();
    let inconclusive_without_positive = exact_cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.easiest_known,
                MetricEvidence::Missing {
                    reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive
                }
            )
        })
        .count();
    let missing_route_or_audit = exact_cells
        .iter()
        .filter(|cell| {
            matches!(
                cell.easiest_known,
                MetricEvidence::Missing {
                    reason: MissingMetricReason::MissingDirectedRouteAssessment
                        | MissingMetricReason::MissingDirectControllerAudit
                }
            )
        })
        .count();
    let unavailable = successful_aggregate_unavailable_reason(
        indexed.route_keys.len(),
        inconclusive_without_positive,
        missing_route_or_audit,
    );
    let (easiest_controller_fractions, demand_coordinates) =
        controller_aggregates(&successes, unavailable);
    DirectControllerLoadoutMetricSummary {
        loadout,
        source_audit_completeness: source_counts,
        route_audit_completeness: route_counts,
        known_positive_directed_routes,
        ambiguous_nondominated_front_directed_routes,
        no_positive_in_complete_finite_vocabulary,
        inconclusive_without_positive,
        missing_route_or_audit,
        easiest_controller_fractions,
        demand_coordinates,
    }
}

fn controller_aggregates(
    successes: &[EasiestKnownControllerMetric],
    unavailable_when_empty: MetricEvidence<()>,
) -> (
    MetricEvidence<EasiestControllerFractionSummary>,
    MetricEvidence<ControllerDemandCoordinateSummary>,
) {
    if successes.iter().any(|success| {
        matches!(
            success.selection_status,
            EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { .. }
        )
    }) {
        return (
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
            },
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
            },
        );
    }
    if successes.is_empty() {
        return (
            unavailable_when_empty.clone().map(|_| unreachable!()),
            unavailable_when_empty.map(|_| unreachable!()),
        );
    }
    (
        MetricEvidence::Observed(controller_fractions(successes)),
        MetricEvidence::Observed(controller_coordinate_summary(successes)),
    )
}

trait MapUnavailable<T> {
    fn map<U>(self, observed: impl FnOnce(T) -> U) -> MetricEvidence<U>;
}

impl<T> MapUnavailable<T> for MetricEvidence<T> {
    fn map<U>(self, observed: impl FnOnce(T) -> U) -> MetricEvidence<U> {
        match self {
            MetricEvidence::Observed(value) => MetricEvidence::Observed(observed(value)),
            MetricEvidence::Missing { reason } => MetricEvidence::Missing { reason },
            MetricEvidence::NotApplicable { reason } => MetricEvidence::NotApplicable { reason },
        }
    }
}

fn successful_aggregate_unavailable_reason(
    route_count: usize,
    inconclusive: usize,
    missing: usize,
) -> MetricEvidence<()> {
    if route_count == 0 {
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoDirectedRoutes,
        }
    } else if missing > 0 {
        MetricEvidence::Missing {
            reason: MissingMetricReason::MissingDirectControllerAudit,
        }
    } else if inconclusive > 0 {
        MetricEvidence::Missing {
            reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive,
        }
    } else {
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoSuccessfulRoutesForLoadout,
        }
    }
}

fn source_audit_counts(
    loadout: EvaluationLoadout,
    indexed: &IndexedAnalysis<'_>,
) -> AuditCompletenessCounts {
    let mut statuses = Vec::with_capacity(indexed.door_ids.len());
    // `door_ids` is the canonical source universe. A missing source batch is
    // therefore counted as missing rather than silently shrinking expected.
    for source in &indexed.door_ids {
        let Some(batch) = indexed.source_batches.get(source) else {
            statuses.push(None);
            continue;
        };
        let status = batch
            .shared_audits
            .iter()
            .find(|audit| audit.loadout == loadout)
            .map(|audit| audit.status);
        statuses.push(status);
    }
    audit_counts(indexed.door_ids.len(), statuses)
}

fn audit_counts(
    expected: usize,
    statuses: impl IntoIterator<Item = impl IntoAuditStatus>,
) -> AuditCompletenessCounts {
    let mut complete_finite_vocabulary = 0;
    let mut bounded_incomplete = 0;
    let mut missing = 0;
    for status in statuses {
        match status.into_audit_status() {
            Some(LoadoutControllerAuditStatus::CompleteFiniteVocabulary) => {
                complete_finite_vocabulary += 1;
            }
            Some(LoadoutControllerAuditStatus::BoundedIncomplete { .. }) => {
                bounded_incomplete += 1;
            }
            None => missing += 1,
        }
    }
    let state = if expected == 0 {
        AggregateAuditCompleteness::NotApplicableNoAuditsExpected
    } else if missing > 0 {
        AggregateAuditCompleteness::Missing
    } else if bounded_incomplete > 0 {
        AggregateAuditCompleteness::BoundedIncomplete
    } else {
        AggregateAuditCompleteness::CompleteFiniteVocabulary
    };
    AuditCompletenessCounts {
        expected,
        complete_finite_vocabulary,
        bounded_incomplete,
        missing,
        state,
    }
}

trait IntoAuditStatus {
    fn into_audit_status(self) -> Option<LoadoutControllerAuditStatus>;
}

impl IntoAuditStatus for Option<LoadoutControllerAuditStatus> {
    fn into_audit_status(self) -> Option<LoadoutControllerAuditStatus> {
        self
    }
}

impl IntoAuditStatus for DirectControllerAuditMetric {
    fn into_audit_status(self) -> Option<LoadoutControllerAuditStatus> {
        match self {
            Self::CompleteFiniteVocabulary { .. } => {
                Some(LoadoutControllerAuditStatus::CompleteFiniteVocabulary)
            }
            Self::BoundedIncomplete { limit, .. } => {
                Some(LoadoutControllerAuditStatus::BoundedIncomplete { limit })
            }
            Self::MissingDirectedRouteAssessment | Self::MissingLoadoutAudit => None,
        }
    }
}

fn controller_fractions(
    successes: &[EasiestKnownControllerMetric],
) -> EasiestControllerFractionSummary {
    let denominator = successes.len();
    let run_only = successes
        .iter()
        .filter(|metric| metric.demand.run_only)
        .count();
    let monotone = successes
        .iter()
        .filter(|metric| metric.demand.monotone_simple)
        .count();
    let class_0 = successes
        .iter()
        .filter(|metric| metric.coordinates.controller_class == 0)
        .count();
    let class_1 = successes
        .iter()
        .filter(|metric| metric.coordinates.controller_class == 1)
        .count();
    let class_2 = denominator.saturating_sub(class_0.saturating_add(class_1));
    EasiestControllerFractionSummary {
        successful_route_count: denominator,
        run_only_class_fraction: ExactFraction::new(class_0, denominator),
        monotone_simple_class_fraction: ExactFraction::new(class_1, denominator),
        other_controller_class_fraction: ExactFraction::new(class_2, denominator),
        run_only_fraction: ExactFraction::new(run_only, denominator),
        monotone_simple_fraction: ExactFraction::new(monotone, denominator),
    }
}

fn controller_coordinate_summary(
    successes: &[EasiestKnownControllerMetric],
) -> ControllerDemandCoordinateSummary {
    ControllerDemandCoordinateSummary {
        controller_class: integer_distribution(
            successes
                .iter()
                .map(|metric| usize::from(metric.coordinates.controller_class)),
        ),
        ability_events: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.ability_events),
        ),
        horizontal_reversals: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.horizontal_reversals),
        ),
        vertical_decisions: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.vertical_decisions),
        ),
        semantic_spans: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.semantic_spans),
        ),
        semantic_transitions: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.semantic_transitions),
        ),
        duration_ticks: integer_distribution(
            successes
                .iter()
                .map(|metric| metric.coordinates.duration_ticks),
        ),
    }
}

fn integer_distribution(values: impl IntoIterator<Item = usize>) -> IntegerCoordinateDistribution {
    let mut values = values.into_iter().collect::<Vec<_>>();
    debug_assert!(!values.is_empty());
    values.sort_unstable();
    let sample_count = values.len();
    let upper = sample_count / 2;
    let lower = (sample_count - 1) / 2;
    let minimum = values[0];
    let maximum = values[sample_count - 1];
    IntegerCoordinateDistribution {
        sample_count,
        minimum,
        median_lower: values[lower],
        median_upper: values[upper],
        maximum,
        spread: maximum - minimum,
    }
}

fn signed_integer_distribution(
    values: impl IntoIterator<Item = i32>,
) -> SignedIntegerCoordinateDistribution {
    let mut values = values.into_iter().collect::<Vec<_>>();
    debug_assert!(!values.is_empty());
    values.sort_unstable();
    let sample_count = values.len();
    let upper = sample_count / 2;
    let lower = (sample_count - 1) / 2;
    let minimum = values[0];
    let maximum = values[sample_count - 1];
    SignedIntegerCoordinateDistribution {
        sample_count,
        minimum,
        median_lower: values[lower],
        median_upper: values[upper],
        maximum,
        spread: minimum.abs_diff(maximum),
    }
}

fn ability_bypass_summary(routes: &[DirectedRouteControllerMetric]) -> AbilityBypassMetricSummary {
    let directed_routes_with_any_bypass = routes
        .iter()
        .filter(|route| !route.positive_bypasses.is_empty())
        .count();
    let directed_routes_with_wall_jump_bypass = routes
        .iter()
        .filter(|route| {
            route
                .positive_bypasses
                .iter()
                .any(|bypass| !bypass.successful_loadout.abilities().wall_jump)
        })
        .count();
    let directed_routes_with_dash_bypass = routes
        .iter()
        .filter(|route| {
            route
                .positive_bypasses
                .iter()
                .any(|bypass| !bypass.successful_loadout.abilities().dash)
        })
        .count();
    let directed_route_loadout_bypasses = routes
        .iter()
        .map(|route| route.positive_bypasses.len())
        .sum();
    let retained_semantic_bypass_witnesses = routes
        .iter()
        .flat_map(|route| &route.positive_bypasses)
        .map(|bypass| bypass.retained_semantic_witnesses)
        .sum();
    let by_successful_loadout = EvaluationLoadout::ALL
        .into_iter()
        .filter(|loadout| *loadout != EvaluationLoadout::Both)
        .map(|successful_loadout| {
            let matching = routes
                .iter()
                .flat_map(|route| &route.positive_bypasses)
                .filter(|bypass| bypass.successful_loadout == successful_loadout)
                .collect::<Vec<_>>();
            AbilityBypassLoadoutMetric {
                successful_loadout,
                directed_route_bypasses: matching.len(),
                retained_semantic_witnesses: matching
                    .iter()
                    .map(|bypass| bypass.retained_semantic_witnesses)
                    .sum(),
            }
        })
        .collect();
    AbilityBypassMetricSummary {
        directed_routes_with_any_bypass,
        directed_routes_with_wall_jump_bypass,
        directed_routes_with_dash_bypass,
        directed_route_loadout_bypasses,
        retained_semantic_bypass_witnesses,
        by_successful_loadout,
    }
}

fn directional_asymmetry(
    indexed: &IndexedAnalysis<'_>,
    direct: &DirectControllerMetricSummary,
) -> Vec<DirectionalAsymmetryMetric> {
    let routes = direct
        .directed_routes
        .iter()
        .map(|route| {
            (
                (route.source_door_id.as_str(), route.target_door_id.as_str()),
                route,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for left in 0..indexed.door_ids.len() {
        for right in left + 1..indexed.door_ids.len() {
            let door_a = &indexed.door_ids[left];
            let door_b = &indexed.door_ids[right];
            let a_to_b_route = routes.get(&(door_a.as_str(), door_b.as_str())).copied();
            let b_to_a_route = routes.get(&(door_b.as_str(), door_a.as_str())).copied();
            for loadout in EvaluationLoadout::ALL {
                let a_to_b = directional_route_evidence(a_to_b_route, loadout);
                let b_to_a = directional_route_evidence(b_to_a_route, loadout);
                let comparison = compare_directions(&a_to_b, &b_to_a);
                result.push(DirectionalAsymmetryMetric {
                    door_a: door_a.clone(),
                    door_b: door_b.clone(),
                    loadout,
                    a_to_b,
                    b_to_a,
                    comparison,
                });
            }
        }
    }
    result
}

fn directional_route_evidence(
    route: Option<&DirectedRouteControllerMetric>,
    loadout: EvaluationLoadout,
) -> MetricEvidence<EasiestKnownControllerMetric> {
    route.map_or(
        MetricEvidence::Missing {
            reason: MissingMetricReason::MissingDirectedRouteAssessment,
        },
        |route| {
            route
                .exact_loadouts
                .iter()
                .find(|metric| metric.loadout == loadout)
                .map_or(
                    MetricEvidence::Missing {
                        reason: MissingMetricReason::MissingDirectControllerAudit,
                    },
                    |metric| metric.easiest_known.clone(),
                )
        },
    )
}

fn compare_directions(
    a_to_b: &MetricEvidence<EasiestKnownControllerMetric>,
    b_to_a: &MetricEvidence<EasiestKnownControllerMetric>,
) -> MetricEvidence<DirectionalAsymmetryValues> {
    let (MetricEvidence::Observed(a_to_b), MetricEvidence::Observed(b_to_a)) = (a_to_b, b_to_a)
    else {
        return MetricEvidence::Missing {
            reason: MissingMetricReason::ReverseDirectionsRequireTwoKnownPositiveControllers,
        };
    };
    if matches!(
        a_to_b.selection_status,
        EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { .. }
    ) || matches!(
        b_to_a.selection_status,
        EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { .. }
    ) {
        return MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::AmbiguousNondominatedFront,
        };
    }
    MetricEvidence::Observed(DirectionalAsymmetryValues {
        duration_ticks: count_difference(
            a_to_b.demand.duration_ticks,
            b_to_a.demand.duration_ticks,
        ),
        semantic_spans: count_difference(
            a_to_b.demand.semantic_spans,
            b_to_a.demand.semantic_spans,
        ),
        semantic_transitions: count_difference(
            a_to_b.demand.semantic_transitions,
            b_to_a.demand.semantic_transitions,
        ),
        ability_events: count_difference(
            a_to_b.demand.ability_events(),
            b_to_a.demand.ability_events(),
        ),
        ability_use: DirectionalAbilityUseComparison {
            a_to_b: ability_use(a_to_b.demand),
            b_to_a: ability_use(b_to_a.demand),
            wall_jump_use_differs: (a_to_b.demand.wall_jump_events > 0)
                != (b_to_a.demand.wall_jump_events > 0),
            dash_use_differs: (a_to_b.demand.dash_events > 0) != (b_to_a.demand.dash_events > 0),
        },
    })
}

fn count_difference(a_to_b: usize, b_to_a: usize) -> DirectionalCountDifference {
    DirectionalCountDifference {
        a_to_b,
        b_to_a,
        absolute_difference: a_to_b.abs_diff(b_to_a),
        larger_direction: match a_to_b.cmp(&b_to_a) {
            std::cmp::Ordering::Less => LargerDirection::BToA,
            std::cmp::Ordering::Equal => LargerDirection::Equal,
            std::cmp::Ordering::Greater => LargerDirection::AToB,
        },
    }
}

fn ability_use(demand: ControllerDemand) -> AbilityUseMetric {
    AbilityUseMetric {
        wall_jump_events: demand.wall_jump_events,
        dash_events: demand.dash_events,
    }
}

fn terrain_summary(analysis: &CorpusRoomAnalysis) -> TerrainMetricSummary {
    let audit = &analysis.terrain_audit;
    let coverage = audit.coverage;
    let positive_controllers = coverage.positive_controller_count;
    let component_fraction = |numerator| {
        coverage_fraction(
            numerator,
            coverage.interior_component_count,
            NotApplicableMetricReason::NoInteriorTerrainComponents,
        )
    };
    let tile_fraction = |numerator| {
        coverage_fraction(
            numerator,
            coverage.interior_tile_count,
            NotApplicableMetricReason::NoInteriorTerrainTiles,
        )
    };
    let traversal_near_components =
        if positive_controllers == 0 && coverage.interior_component_count > 0 {
            MetricEvidence::Missing {
                reason: MissingMetricReason::NoPositiveTerrainController,
            }
        } else {
            component_fraction(coverage.traversal_near_component_count)
        };
    let traversal_near_tiles = if positive_controllers == 0 && coverage.interior_tile_count > 0 {
        MetricEvidence::Missing {
            reason: MissingMetricReason::NoPositiveTerrainController,
        }
    } else {
        tile_fraction(coverage.traversal_near_tile_count)
    };
    let coverage_fractions = TerrainCoverageFractionSummary {
        structurally_attributed_components: component_fraction(
            coverage.structurally_attributed_component_count,
        ),
        structurally_attributed_tiles: tile_fraction(coverage.structurally_attributed_tile_count),
        traversal_near_components,
        traversal_near_tiles,
        positively_corroborated_components: component_fraction(
            coverage.positively_corroborated_component_count,
        ),
        positively_corroborated_tiles: tile_fraction(coverage.positively_corroborated_tile_count),
    };
    let ablations = audit
        .ablations
        .iter()
        .map(|ablation| ExactControllerAblationSurvivalMetric {
            kind: ablation.kind,
            outcomes: ablation.summary,
            survival_fraction: ablation_survival_fraction(ablation.summary),
        })
        .collect::<Vec<_>>();
    let total_controllers = ablations
        .iter()
        .map(|ablation| ablation.outcomes.controller_count)
        .sum::<usize>();
    let total_succeeded = ablations
        .iter()
        .map(|ablation| ablation.outcomes.succeeded)
        .sum::<usize>();
    let aggregate_ablation_survival_fraction = if ablations.is_empty() {
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoAblationVariants,
        }
    } else if total_controllers == 0 {
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoExactControllersForAblation,
        }
    } else {
        MetricEvidence::Observed(ExactFraction::new(total_succeeded, total_controllers))
    };
    let mut ablation_utility = ExactControllerAblationUtilitySummary {
        variant_count: ablations.len(),
        ..ExactControllerAblationUtilitySummary::default()
    };
    for ablation in &ablations {
        let removed_tiles = match ablation.kind {
            RoomAblationKind::InteriorTerrainComponent { tile_count, .. }
            | RoomAblationKind::StaticHazardComponent { tile_count, .. } => tile_count,
            RoomAblationKind::TimedHazard { .. } => 0,
        };
        if ablation.outcomes.controller_count == 0 {
            ablation_utility.no_stored_controllers += 1;
            ablation_utility.removed_tiles_no_controllers += removed_tiles;
        } else if ablation.outcomes.succeeded == ablation.outcomes.controller_count {
            ablation_utility.all_stored_controllers_survived += 1;
            ablation_utility.removed_tiles_all_survived += removed_tiles;
        } else {
            ablation_utility.at_least_one_stored_controller_affected += 1;
            ablation_utility.removed_tiles_some_affected += removed_tiles;
        }
    }
    TerrainMetricSummary {
        source_terrain_audit_version: audit.version,
        coverage,
        coverage_fractions,
        ablations,
        ablation_utility,
        aggregate_ablation_survival_fraction,
    }
}

fn coverage_fraction(
    numerator: usize,
    denominator: usize,
    empty_reason: NotApplicableMetricReason,
) -> MetricEvidence<ExactFraction> {
    if denominator == 0 {
        MetricEvidence::NotApplicable {
            reason: empty_reason,
        }
    } else {
        MetricEvidence::Observed(ExactFraction::new(numerator, denominator))
    }
}

fn ablation_survival_fraction(summary: AblationOutcomeSummary) -> MetricEvidence<ExactFraction> {
    if summary.controller_count == 0 {
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoExactControllersForAblation,
        }
    } else {
        MetricEvidence::Observed(ExactFraction::new(
            summary.succeeded,
            summary.controller_count,
        ))
    }
}

fn operational_cost_summary(analysis: &CorpusRoomAnalysis) -> OperationalCostSummary {
    let mut direct_controller_audit_units = Vec::new();
    for batch in &analysis.source_route_assessments {
        for audit in &batch.shared_audits {
            direct_controller_audit_units.push(DirectControllerOperationalCostMetric {
                source_door_id: batch.source_door_id.clone(),
                loadout: audit.loadout,
                status: audit.status,
                cost: audit.operational_stats.into(),
            });
        }
    }
    let direct_controller_recomputed_total = direct_controller_audit_units
        .iter()
        .map(|metric| metric.cost)
        .fold(OperationalCost::default(), aggregate_cost);
    let canonical_positive_routes = analysis
        .canonical_route_measurements
        .iter()
        .map(|measurement| CanonicalRouteOperationalCostMetric {
            source_door_id: measurement.source_door_id.clone(),
            target_door_id: measurement.target_door_id.clone(),
            loadout: measurement.loadout,
            cost: OperationalCost {
                expanded_nodes: measurement.vector.operational_solver_cost.expanded_nodes,
                generated_nodes: measurement.vector.operational_solver_cost.generated_nodes,
                simulated_ticks: measurement.vector.operational_solver_cost.simulated_ticks,
                deepest_path_ticks: measurement
                    .vector
                    .operational_solver_cost
                    .deepest_path_ticks,
            },
        })
        .collect::<Vec<_>>();
    let canonical_positive_route_total = canonical_positive_routes
        .iter()
        .map(|metric| metric.cost)
        .fold(OperationalCost::default(), aggregate_cost);
    OperationalCostSummary {
        direct_controller_reported_total: analysis.direct_controller_operational_stats.into(),
        direct_controller_recomputed_total,
        direct_controller_audit_units,
        canonical_positive_route_total,
        canonical_positive_routes,
    }
}

fn aggregate_cost(mut total: OperationalCost, additional: OperationalCost) -> OperationalCost {
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

impl From<SearchStats> for OperationalCost {
    fn from(value: SearchStats) -> Self {
        Self {
            expanded_nodes: value.expanded_nodes,
            generated_nodes: value.generated_nodes,
            simulated_ticks: value.simulated_ticks,
            deepest_path_ticks: value.deepest_path_ticks,
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{DifficultyConfig, SolverConfig};
    use downwards_core::Rect;
    use downwards_lab::{LANDING_PRECISION_VERSION, LandingSample};

    use super::*;
    use crate::corpus::{
        CorpusBuildConfigV1, CorpusRoomAnalysisConfig, analyze_corpus_room,
        evaluate_route_matrices, generate_seed_block,
    };

    fn demand(
        duration_ticks: usize,
        semantic_spans: usize,
        semantic_transitions: usize,
        wall_jump_events: usize,
        dash_events: usize,
    ) -> ControllerDemand {
        ControllerDemand {
            run_only: false,
            monotone_simple: false,
            ordinary_jump_events: 0,
            wall_jump_events,
            dash_events,
            jump_press_edges: 0,
            dash_press_edges: 0,
            horizontal_reversals: 0,
            vertical_input_changes: 0,
            dash_direction_changes: 0,
            vertical_decisions: 0,
            semantic_spans,
            semantic_transitions,
            duration_ticks,
        }
    }

    fn easiest(demand: ControllerDemand) -> MetricEvidence<EasiestKnownControllerMetric> {
        MetricEvidence::Observed(EasiestKnownControllerMetric {
            successful_loadout: EvaluationLoadout::Both,
            coordinates: demand.coordinates(),
            demand,
            nondominated_front_size: 1,
            selection_status: EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate,
        })
    }

    fn landing_report(
        samples: Vec<LandingSample>,
        unmeasured_landing_ticks: Vec<usize>,
    ) -> LandingPrecisionReport {
        let landing_event_count = samples.len() + unmeasured_landing_ticks.len();
        LandingPrecisionReport {
            version: LANDING_PRECISION_VERSION,
            inspected_ticks: 20,
            landing_event_count,
            samples: samples.into_boxed_slice(),
            unmeasured_landing_ticks: unmeasured_landing_ticks.into_boxed_slice(),
            minimum_footprint_overlap_pixels: None,
            minimum_edge_margin_pixels: None,
            narrowest_support_width_pixels: None,
            edge_overhang_landings: 0,
            one_way_or_mixed_landings: 0,
        }
    }

    #[test]
    fn synthetic_reverse_routes_report_direction_and_ability_asymmetry() {
        let a_to_b = easiest(demand(20, 3, 2, 0, 0));
        let b_to_a = easiest(demand(35, 8, 7, 2, 1));
        let MetricEvidence::Observed(comparison) = compare_directions(&a_to_b, &b_to_a) else {
            panic!("two known positives must be comparable");
        };
        assert_eq!(comparison.duration_ticks.absolute_difference, 15);
        assert_eq!(
            comparison.duration_ticks.larger_direction,
            LargerDirection::BToA
        );
        assert_eq!(comparison.semantic_spans.absolute_difference, 5);
        assert_eq!(comparison.semantic_transitions.absolute_difference, 5);
        assert_eq!(comparison.ability_events.a_to_b, 0);
        assert_eq!(comparison.ability_events.b_to_a, 3);
        assert!(comparison.ability_use.wall_jump_use_differs);
        assert!(comparison.ability_use.dash_use_differs);

        let missing_reverse = MetricEvidence::Missing {
            reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive,
        };
        assert!(matches!(
            compare_directions(&a_to_b, &missing_reverse),
            MetricEvidence::Missing {
                reason: MissingMetricReason::ReverseDirectionsRequireTwoKnownPositiveControllers
            }
        ));

        let MetricEvidence::Observed(mut ambiguous) = a_to_b else {
            unreachable!();
        };
        ambiguous.nondominated_front_size = 2;
        ambiguous.selection_status =
            EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { front_size: 2 };
        assert!(matches!(
            compare_directions(&MetricEvidence::Observed(ambiguous), &b_to_a),
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::AmbiguousNondominatedFront
            }
        ));
    }

    #[test]
    fn integer_distributions_retain_exact_half_integer_medians() {
        assert_eq!(
            integer_distribution([1, 4, 9, 12]),
            IntegerCoordinateDistribution {
                sample_count: 4,
                minimum: 1,
                median_lower: 4,
                median_upper: 9,
                maximum: 12,
                spread: 11,
            }
        );
        assert_eq!(
            signed_integer_distribution([-5, -1, 0, 7]),
            SignedIntegerCoordinateDistribution {
                sample_count: 4,
                minimum: -5,
                median_lower: -1,
                median_upper: 0,
                maximum: 7,
                spread: 12,
            }
        );
    }

    #[test]
    fn mixed_unique_and_ambiguous_fronts_do_not_shrink_aggregate_denominators() {
        let MetricEvidence::Observed(unique) = easiest(demand(20, 3, 2, 0, 0)) else {
            unreachable!();
        };
        let mut ambiguous = unique;
        ambiguous.nondominated_front_size = 2;
        ambiguous.selection_status =
            EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { front_size: 2 };

        let (fractions, coordinates) = controller_aggregates(
            &[unique, ambiguous],
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::NoSuccessfulRoutesForLoadout,
            },
        );
        assert!(matches!(
            fractions,
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::AmbiguousNondominatedFront
            }
        ));
        assert!(matches!(
            coordinates,
            MetricEvidence::NotApplicable {
                reason: NotApplicableMetricReason::AmbiguousNondominatedFront
            }
        ));
    }

    #[test]
    fn landing_aggregate_distinguishes_no_event_unmeasured_and_signed_geometry() {
        let empty = landing_precision_aggregate(std::iter::empty());
        assert!(matches!(
            empty.minimum_edge_margin_pixels,
            LandingCoordinateEvidence::NotApplicable {
                reason: LandingCoordinateNotApplicableReason::NoCanonicalPositiveRoutes
            }
        ));

        let no_event_report = landing_report(Vec::new(), Vec::new());
        let no_event = landing_precision_aggregate([&no_event_report]);
        assert!(matches!(
            no_event.minimum_edge_margin_pixels,
            LandingCoordinateEvidence::NotApplicable {
                reason: LandingCoordinateNotApplicableReason::NoLandingEvents
            }
        ));
        assert_eq!(no_event.routes_without_landing_events, 1);

        let unmeasured_report = landing_report(Vec::new(), vec![4]);
        let unmeasured = landing_precision_aggregate([&unmeasured_report]);
        assert!(matches!(
            unmeasured.minimum_edge_margin_pixels,
            LandingCoordinateEvidence::NotApplicable {
                reason: LandingCoordinateNotApplicableReason::NoMeasuredLandings
            }
        ));
        assert_eq!(unmeasured.routes_with_only_unmeasured_landings, 1);

        let measured_report = landing_report(
            vec![
                LandingSample {
                    replay_tick: 3,
                    player_bounds: Rect::new(8, 8, 8, 12),
                    surface_y: 20,
                    support_left: 10,
                    support_right: 30,
                    support_kind: LandingSupportKind::OneWay,
                    footprint_overlap_pixels: 6,
                    left_edge_margin_pixels: -2,
                    right_edge_margin_pixels: 14,
                },
                LandingSample {
                    replay_tick: 7,
                    player_bounds: Rect::new(10, 8, 8, 12),
                    surface_y: 20,
                    support_left: 10,
                    support_right: 18,
                    support_kind: LandingSupportKind::Solid,
                    footprint_overlap_pixels: 8,
                    left_edge_margin_pixels: 0,
                    right_edge_margin_pixels: 0,
                },
            ],
            vec![11],
        );
        let measured = landing_precision_aggregate([&measured_report]);
        assert_eq!(measured.landing_event_count, 3);
        assert_eq!(measured.measured_landing_count, 2);
        assert_eq!(measured.unmeasured_landing_count, 1);
        assert_eq!(measured.edge_overhang_landings, 1);
        assert_eq!(measured.one_way_or_mixed_landings, 1);
        assert!(matches!(
            measured.minimum_edge_margin_pixels,
            LandingCoordinateEvidence::Observed(SignedIntegerCoordinateDistribution {
                sample_count: 2,
                minimum: -2,
                median_lower: -2,
                median_upper: 0,
                maximum: 0,
                spread: 2,
            })
        ));
    }

    #[test]
    fn real_room_summary_is_deterministic_and_keeps_matrix_denominators() {
        let mut generated =
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        generated.rooms.truncate(1);
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
        let first = summarize_room_metrics(&analysis).unwrap();
        let second = summarize_room_metrics(&analysis).unwrap();
        assert_eq!(first, second);

        let door_count = evaluated.generated.variants[0].generated.room.doors().len();
        let directed_routes = door_count * door_count.saturating_sub(1);
        assert_eq!(first.canonical_routes.directed_route_count, directed_routes);
        assert_eq!(
            first.canonical_routes.loadout_route_cell_count,
            directed_routes * EvaluationLoadout::ALL.len()
        );
        assert_eq!(
            first.canonical_routes.positive_route_count
                + first.canonical_routes.bounded_inconclusive_route_count,
            first.canonical_routes.loadout_route_cell_count
        );
        assert_eq!(
            first.canonical_routes.behavior_diversity.route_count,
            first.canonical_routes.positive_route_count
        );
        assert_eq!(
            first
                .landing_precision
                .aggregate
                .canonical_positive_route_count,
            first.canonical_routes.positive_route_count
        );
        assert_eq!(
            first.landing_precision.by_loadout.len(),
            EvaluationLoadout::ALL.len()
        );
        assert_eq!(
            first.landing_precision.aggregate.landing_event_count,
            first.landing_precision.aggregate.measured_landing_count
                + first.landing_precision.aggregate.unmeasured_landing_count
        );
        match &first.landing_precision.aggregate.minimum_edge_margin_pixels {
            LandingCoordinateEvidence::Observed(distribution) => assert_eq!(
                distribution.sample_count,
                first.landing_precision.aggregate.measured_landing_count
            ),
            LandingCoordinateEvidence::NotApplicable { .. } => {
                assert_eq!(first.landing_precision.aggregate.measured_landing_count, 0);
            }
        }
        assert_eq!(
            first.direct_controllers.directed_route_count,
            directed_routes
        );
        assert_eq!(
            first.operational_cost.direct_controller_reported_total,
            first.operational_cost.direct_controller_recomputed_total
        );
        for loadout in &first.direct_controllers.by_loadout {
            assert_eq!(
                loadout.route_audit_completeness.complete_finite_vocabulary
                    + loadout.route_audit_completeness.bounded_incomplete
                    + loadout.route_audit_completeness.missing,
                directed_routes
            );
            if let MetricEvidence::Observed(fractions) = loadout.easiest_controller_fractions {
                assert_eq!(
                    fractions.run_only_fraction.denominator,
                    loadout.known_positive_directed_routes
                );
            }
        }
        for ablation in &first.terrain.ablations {
            if let MetricEvidence::Observed(fraction) = ablation.survival_fraction {
                assert_eq!(fraction.denominator, ablation.outcomes.controller_count);
                assert_eq!(fraction.numerator, ablation.outcomes.succeeded);
            }
        }
        let utility = first.terrain.ablation_utility;
        assert_eq!(utility.variant_count, first.terrain.ablations.len());
        assert_eq!(
            utility.all_stored_controllers_survived
                + utility.at_least_one_stored_controller_affected
                + utility.no_stored_controllers,
            utility.variant_count
        );

        // A canonical positive remains reportable when the exact direct
        // vocabulary contributes no positive for that cell.
        let mut fallback_analysis = analysis.clone();
        let cell_index = fallback_analysis
            .fused_route_cells
            .iter()
            .position(|cell| {
                cell.candidates.iter().any(|candidate| {
                    candidate.provenance.iter().any(|source| {
                        matches!(
                            source,
                            FusedRouteCandidateProvenance::CanonicalMatrix { .. }
                        )
                    })
                })
            })
            .expect("real analysis retains a canonical positive");
        let source = fallback_analysis.fused_route_cells[cell_index]
            .source_door_id
            .clone();
        let target = fallback_analysis.fused_route_cells[cell_index]
            .target_door_id
            .clone();
        let loadout = fallback_analysis.fused_route_cells[cell_index].loadout;
        let route = fallback_analysis
            .source_route_assessments
            .iter_mut()
            .find(|batch| batch.source_door_id == source)
            .and_then(|batch| {
                batch
                    .routes
                    .iter_mut()
                    .find(|route| route.target_door_id == target)
            })
            .unwrap();
        let mut next_witness_index = 0;
        let witness_index_remap = route
            .easiest_first_witnesses
            .iter()
            .map(|witness| {
                if witness.loadout == loadout {
                    None
                } else {
                    let remapped = next_witness_index;
                    next_witness_index += 1;
                    Some(remapped)
                }
            })
            .collect::<Vec<_>>();
        route
            .easiest_first_witnesses
            .retain(|witness| witness.loadout != loadout);
        route.easiest_known_front.clear();
        route.positive_bypasses.clear();
        let audit = route
            .audits
            .iter_mut()
            .find(|audit| audit.loadout == loadout)
            .unwrap();
        audit.raw_positive_witnesses = 0;
        audit.retained_semantic_witnesses = 0;

        for cell in fallback_analysis
            .fused_route_cells
            .iter_mut()
            .filter(|cell| {
                cell.source_door_id == source
                    && cell.target_door_id == target
                    && cell.loadout != loadout
            })
        {
            for provenance in cell
                .candidates
                .iter_mut()
                .flat_map(|candidate| &mut candidate.provenance)
            {
                if let FusedRouteCandidateProvenance::DirectWitness { witness_index } = provenance {
                    *witness_index = witness_index_remap[*witness_index]
                        .expect("another exact loadout's witness remains retained");
                }
            }
        }

        let fallback_cell = &mut fallback_analysis.fused_route_cells[cell_index];
        fallback_cell.raw_direct_positive_witnesses = 0;
        fallback_cell.retained_direct_positive_witnesses = 0;
        fallback_cell.candidates.retain_mut(|candidate| {
            candidate.provenance.retain(|source| {
                matches!(
                    source,
                    FusedRouteCandidateProvenance::CanonicalMatrix { .. }
                )
            });
            !candidate.provenance.is_empty()
        });
        assert_eq!(fallback_cell.candidates.len(), 1);
        fallback_cell.nondominated_front = vec![0];
        fallback_cell.ease_evidence.clear();
        fallback_cell.selected = Some(crate::corpus::EasiestKnownRouteSelection {
            candidate_index: 0,
            replay_identity: fallback_cell.candidates[0].replay_identity,
            status: crate::corpus::EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate,
        });
        let expected_coordinates = fallback_cell.candidates[0].controller_coordinates;

        let fallback_summary = summarize_room_metrics(&fallback_analysis).unwrap();
        let fallback_metric = fallback_summary
            .direct_controllers
            .directed_routes
            .iter()
            .find(|route| route.source_door_id == source && route.target_door_id == target)
            .and_then(|route| {
                route
                    .exact_loadouts
                    .iter()
                    .find(|cell| cell.loadout == loadout)
            })
            .unwrap();
        assert!(matches!(
            &fallback_metric.easiest_known,
            MetricEvidence::Observed(metric) if metric.coordinates == expected_coordinates
        ));
    }
}
