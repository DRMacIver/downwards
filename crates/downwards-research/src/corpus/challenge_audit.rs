//! Batch-level challenge sanity diagnostics for corpus pilots.
//!
//! This module deliberately reports a vector of raw observations rather than
//! a room score, difficulty band, or claim about fun.  Every expected directed
//! route/loadout cell is retained as an exact known positive, a completed
//! finite-vocabulary audit without a positive, a bounded audit without a
//! positive, or missing evidence.  In particular, neither bounded nor complete
//! direct-controller non-success proves that a route is unreachable.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Write as _},
};

use downwards_ai::DirectProbeBudgetLimit;
use serde::{Deserialize, Serialize};

use super::model::loadout_from_abilities;
use super::{
    CorpusRoomAnalysis, DirectControllerAuditMetric, EvaluatedCorpusBatch, EvaluatedCorpusRoom,
    EvaluationLoadout, MetricEvidence, MissingMetricReason, NotApplicableMetricReason,
    RoomMetricSummary, RoomMetricSummaryError, summarize_room_metrics,
};

/// Version of the route-cell partition, aggregation, and diagnostic text.
pub const CHALLENGE_SANITY_AUDIT_VERSION: u32 = 1;

/// Interpretation boundary for every report produced by this module.
pub const CHALLENGE_SANITY_AUDIT_DISCLAIMER: &str = "challenge sanity thresholds are configurable pilot diagnostics over easiest-known finite controller evidence; they are not difficulty bands, fun scores, reachability proofs, or evidence that an unobserved controller does not exist";

/// A stable local representation of the direct-probe bound that stopped an
/// audit.  This avoids serializing a dependency's non-serde enum by debug text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectProbeLimitRecord {
    ExpandedNodes,
    SimulatedTicks,
}

impl From<DirectProbeBudgetLimit> for DirectProbeLimitRecord {
    fn from(value: DirectProbeBudgetLimit) -> Self {
        match value {
            DirectProbeBudgetLimit::ExpandedNodes => Self::ExpandedNodes,
            DirectProbeBudgetLimit::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

/// Exclusive class of the easiest controller known for one exact cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EasiestControllerClass {
    RunOnly,
    MonotoneSimpleOnly,
    GenuinelyOther,
}

/// Controller-demand observations copied from one exact positive witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerDemandRecord {
    pub semantic_spans: usize,
    pub semantic_transitions: usize,
    pub horizontal_reversals: usize,
    pub vertical_decisions: usize,
    pub ability_events: usize,
    pub wall_jump_events: usize,
    pub dash_events: usize,
    pub duration_ticks: usize,
}

/// Completeness of the finite-vocabulary audit which contained a positive.
/// A replay-certified positive remains exact even if later probes were cut off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PositiveAuditStatus {
    CompleteFiniteVocabulary,
    BoundedIncomplete { limit: DirectProbeLimitRecord },
    MissingAuditMetadata,
}

/// Why an expected challenge cell is absent from the supplied analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeCellMissingReason {
    RoomAnalysisNotSupplied,
    DirectedRouteAssessmentMissing,
    DirectControllerAuditMissing,
    RouteLoadoutCellNotSupplied,
}

/// Exhaustive evidence state for one directed route under one exact loadout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChallengeCellEvidence {
    KnownPositive {
        class: EasiestControllerClass,
        demand: ControllerDemandRecord,
        audit_status: PositiveAuditStatus,
    },
    NoPositiveInCompleteFiniteVocabulary,
    BoundedWithoutPositive {
        limit: DirectProbeLimitRecord,
    },
    Missing {
        reason: ChallengeCellMissingReason,
    },
}

/// One deterministic route/loadout evidence cell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteLoadoutChallengeCell {
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub evidence: ChallengeCellEvidence,
}

/// Status of one canonical route or pickup hard-gate component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardGateStatus {
    Pass,
    BoundedEvidenceDoesNotPass,
    MissingMatrix,
}

/// Transparent positive/inconclusive counts for one hard-gate component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardGateComponentRecord {
    pub expected_cells: usize,
    pub positive_cells: usize,
    pub bounded_inconclusive_cells: usize,
    pub status: HardGateStatus,
}

impl HardGateComponentRecord {
    fn missing() -> Self {
        Self {
            expected_cells: 0,
            positive_cells: 0,
            bounded_inconclusive_cells: 0,
            status: HardGateStatus::MissingMatrix,
        }
    }
}

/// Construction-loadout and complete-kit hard-gate evidence for one room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomHardGateRecord {
    pub construction_loadout: EvaluationLoadout,
    pub construction_routes: HardGateComponentRecord,
    pub construction_pickups: HardGateComponentRecord,
    pub complete_kit_routes: HardGateComponentRecord,
    pub complete_kit_pickups: HardGateComponentRecord,
}

/// Normalized, serde-ready evidence for one room.  This is also the input to
/// the pure aggregation path used by synthetic tests and artifact consumers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRoomEvidence {
    pub room_id: String,
    pub door_ids: Vec<String>,
    pub route_loadout_cells: Vec<RouteLoadoutChallengeCell>,
    pub hard_gates: RoomHardGateRecord,
}

/// Exact order statistics for one nonnegative demand coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemandDistribution {
    pub sample_count: usize,
    pub minimum: usize,
    pub median_lower: usize,
    pub median_upper: usize,
    pub maximum: usize,
    pub spread: usize,
    pub total: usize,
}

/// Explicit absence wrapper for a demand distribution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DemandDistributionEvidence {
    Observed { distribution: DemandDistribution },
    NoKnownPositiveControllers,
}

/// Demand-coordinate distributions among exact known positives only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerDemandDistributions {
    pub semantic_spans: DemandDistributionEvidence,
    pub semantic_transitions: DemandDistributionEvidence,
    pub horizontal_reversals: DemandDistributionEvidence,
    pub vertical_decisions: DemandDistributionEvidence,
    pub ability_events: DemandDistributionEvidence,
    pub duration_ticks: DemandDistributionEvidence,
}

/// Exhaustive denominators for an expected set of route/loadout cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeCellDenominators {
    pub expected_cells: usize,
    pub known_positive_cells: usize,
    pub known_run_only_cells: usize,
    pub known_monotone_simple_only_cells: usize,
    pub known_genuinely_other_cells: usize,
    pub positives_from_complete_audits: usize,
    pub positives_from_bounded_audits: usize,
    pub positives_with_missing_audit_metadata: usize,
    pub no_positive_in_complete_finite_vocabulary: usize,
    pub bounded_without_positive: usize,
    pub missing: usize,
}

impl ChallengeCellDenominators {
    fn observe(&mut self, evidence: &ChallengeCellEvidence) {
        self.expected_cells = self.expected_cells.saturating_add(1);
        match evidence {
            ChallengeCellEvidence::KnownPositive {
                class,
                audit_status,
                ..
            } => {
                self.known_positive_cells = self.known_positive_cells.saturating_add(1);
                match class {
                    EasiestControllerClass::RunOnly => {
                        self.known_run_only_cells = self.known_run_only_cells.saturating_add(1);
                    }
                    EasiestControllerClass::MonotoneSimpleOnly => {
                        self.known_monotone_simple_only_cells =
                            self.known_monotone_simple_only_cells.saturating_add(1);
                    }
                    EasiestControllerClass::GenuinelyOther => {
                        self.known_genuinely_other_cells =
                            self.known_genuinely_other_cells.saturating_add(1);
                    }
                }
                match audit_status {
                    PositiveAuditStatus::CompleteFiniteVocabulary => {
                        self.positives_from_complete_audits =
                            self.positives_from_complete_audits.saturating_add(1);
                    }
                    PositiveAuditStatus::BoundedIncomplete { .. } => {
                        self.positives_from_bounded_audits =
                            self.positives_from_bounded_audits.saturating_add(1);
                    }
                    PositiveAuditStatus::MissingAuditMetadata => {
                        self.positives_with_missing_audit_metadata =
                            self.positives_with_missing_audit_metadata.saturating_add(1);
                    }
                }
            }
            ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary => {
                self.no_positive_in_complete_finite_vocabulary = self
                    .no_positive_in_complete_finite_vocabulary
                    .saturating_add(1);
            }
            ChallengeCellEvidence::BoundedWithoutPositive { .. } => {
                self.bounded_without_positive = self.bounded_without_positive.saturating_add(1);
            }
            ChallengeCellEvidence::Missing { .. } => {
                self.missing = self.missing.saturating_add(1);
            }
        }
    }
}

/// Per-loadout or all-loadout challenge summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeControllerSummary {
    pub denominators: ChallengeCellDenominators,
    pub demand: ControllerDemandDistributions,
}

/// One absolute directional difference while retaining both source values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionalCountDifferenceRecord {
    pub a_to_b: usize,
    pub b_to_a: usize,
    pub absolute_difference: usize,
}

/// Full controller-demand asymmetry for one comparable pair/loadout cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionalDemandDifferenceRecord {
    pub controller_class: DirectionalCountDifferenceRecord,
    pub semantic_spans: DirectionalCountDifferenceRecord,
    pub semantic_transitions: DirectionalCountDifferenceRecord,
    pub horizontal_reversals: DirectionalCountDifferenceRecord,
    pub vertical_decisions: DirectionalCountDifferenceRecord,
    pub ability_events: DirectionalCountDifferenceRecord,
    pub duration_ticks: DirectionalCountDifferenceRecord,
    pub wall_jump_use_differs: bool,
    pub dash_use_differs: bool,
}

/// Compact availability state retained for either direction of an
/// incomparable door-pair/loadout cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DirectionalRouteAvailability {
    KnownPositive { class: EasiestControllerClass },
    NoPositiveInCompleteFiniteVocabulary,
    BoundedWithoutPositive { limit: DirectProbeLimitRecord },
    Missing { reason: ChallengeCellMissingReason },
}

/// Whether both directions have exact positives and can be compared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DirectionalPairEvidence {
    Comparable {
        differences: DirectionalDemandDifferenceRecord,
    },
    Incomplete {
        a_to_b: DirectionalRouteAvailability,
        b_to_a: DirectionalRouteAvailability,
    },
}

/// One unordered door pair under one exact physics loadout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionalPairChallengeRecord {
    pub room_id: String,
    pub door_a: String,
    pub door_b: String,
    pub loadout: EvaluationLoadout,
    pub evidence: DirectionalPairEvidence,
}

/// Aggregate directional-comparison denominators and difference samples.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionalAsymmetrySummary {
    pub expected_pair_cells: usize,
    pub comparable_pair_cells: usize,
    pub incomplete_pair_cells: usize,
    pub pairs_with_any_measured_difference: usize,
    pub pairs_with_different_wall_jump_use: usize,
    pub pairs_with_different_dash_use: usize,
    pub semantic_span_absolute_difference: DemandDistributionEvidence,
    pub semantic_transition_absolute_difference: DemandDistributionEvidence,
    pub horizontal_reversal_absolute_difference: DemandDistributionEvidence,
    pub vertical_decision_absolute_difference: DemandDistributionEvidence,
    pub ability_event_absolute_difference: DemandDistributionEvidence,
    pub duration_tick_absolute_difference: DemandDistributionEvidence,
}

/// Completeness of a room's complete-kit direct-controller cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomControllerEvidenceState {
    CompleteFiniteVocabulary,
    BoundedIncomplete,
    Missing,
}

/// Composition of the room's known complete-kit easiest controllers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompleteKitKnownRouteComposition {
    NoKnownPositive,
    AllKnownRunOrMonotone,
    MixedRunOrMonotoneAndGenuinelyOther,
    AllKnownGenuinelyOther,
}

/// Complete-kit route composition and uncertainty for one room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomCompleteKitChallengeRecord {
    pub room_id: String,
    pub denominators: ChallengeCellDenominators,
    pub evidence_state: RoomControllerEvidenceState,
    pub known_route_composition: CompleteKitKnownRouteComposition,
}

/// Deterministic room ID groups for complete-kit composition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomCompositionSummary {
    pub no_known_positive_room_ids: Vec<String>,
    pub all_known_run_or_monotone_room_ids: Vec<String>,
    pub mixed_room_ids: Vec<String>,
    pub all_known_genuinely_other_room_ids: Vec<String>,
    pub complete_evidence_room_ids: Vec<String>,
    pub bounded_evidence_room_ids: Vec<String>,
    pub missing_evidence_room_ids: Vec<String>,
}

/// Number of rooms in each status for one hard-gate component.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardGateStatusSummary {
    pub room_count: usize,
    pub pass_count: usize,
    pub bounded_does_not_pass_count: usize,
    pub missing_matrix_count: usize,
    pub nonpassing_room_ids: Vec<String>,
}

/// Batch hard-gate summary, without folding route and pickup evidence together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchHardGateSummary {
    pub construction_routes: HardGateStatusSummary,
    pub construction_pickups: HardGateStatusSummary,
    pub complete_kit_routes: HardGateStatusSummary,
    pub complete_kit_pickups: HardGateStatusSummary,
}

/// Exact rational threshold.  Cross multiplication is performed in `u128`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FractionFloor {
    pub numerator: usize,
    pub denominator: usize,
}

/// Optional pilot-only challenge guardrails.  `None` disables a diagnostic;
/// no implicit difficulty target is supplied by this module.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeSanityThresholds {
    pub minimum_complete_kit_genuinely_other_routes: Option<usize>,
    pub minimum_complete_kit_genuinely_other_fraction_of_known_positives: Option<FractionFloor>,
    pub minimum_complete_kit_genuinely_other_fraction_of_expected_routes: Option<FractionFloor>,
    pub maximum_rooms_with_all_known_complete_kit_routes_run_or_monotone: Option<usize>,
    pub require_all_construction_route_gates: bool,
    pub require_all_construction_pickup_gates: bool,
    pub require_all_complete_kit_route_gates: bool,
    pub require_all_complete_kit_pickup_gates: bool,
}

/// Outcome of one configured pilot threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PilotDiagnosticOutcome {
    MeetsThreshold,
    Deficit,
    Unavailable,
}

/// One deterministic, human-readable threshold observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotDiagnostic {
    pub id: String,
    pub outcome: PilotDiagnosticOutcome,
    pub observed: String,
    pub configured_threshold: String,
    /// Rendered verbatim in `ChallengeSanityAudit::threshold_deficits` when
    /// the outcome is not `MeetsThreshold`.
    pub message: String,
}

/// Complete non-scalar challenge audit for one evaluated/analyzed batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeSanityAudit {
    pub version: u32,
    pub disclaimer: String,
    pub thresholds: ChallengeSanityThresholds,
    pub room_count: usize,
    pub route_loadout_cells: Vec<RouteLoadoutChallengeCellRecord>,
    pub all_loadouts: ChallengeControllerSummary,
    pub by_loadout: Vec<LoadoutChallengeControllerSummary>,
    pub directional_pairs: Vec<DirectionalPairChallengeRecord>,
    pub directional_asymmetry: DirectionalAsymmetrySummary,
    pub directional_asymmetry_by_loadout: Vec<LoadoutDirectionalAsymmetrySummary>,
    pub complete_kit_rooms: Vec<RoomCompleteKitChallengeRecord>,
    pub room_composition: RoomCompositionSummary,
    pub hard_gates: BatchHardGateSummary,
    pub diagnostics: Vec<PilotDiagnostic>,
    /// Exact diagnostic messages copied without rewriting.
    pub threshold_deficits: Vec<String>,
    /// Exact cell/matrix incompleteness descriptions, separate from threshold
    /// failures so uncertainty never silently changes a numerator.
    pub incomplete_evidence: Vec<String>,
}

/// Batch-qualified route/loadout cell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteLoadoutChallengeCellRecord {
    pub room_id: String,
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub evidence: ChallengeCellEvidence,
}

/// One exact-loadout slice of the batch controller summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadoutChallengeControllerSummary {
    pub loadout: EvaluationLoadout,
    pub summary: ChallengeControllerSummary,
}

/// One exact-loadout slice of directional asymmetry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadoutDirectionalAsymmetrySummary {
    pub loadout: EvaluationLoadout,
    pub summary: DirectionalAsymmetrySummary,
}

/// Malformed or ambiguously joined batch evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChallengeSanityAuditError {
    InvalidFractionFloor {
        field: &'static str,
        numerator: usize,
        denominator: usize,
    },
    DuplicateEvaluatedRoom {
        room_id: String,
    },
    DuplicateAnalysisRoom {
        room_id: String,
    },
    AnalysisWithoutEvaluatedRoom {
        room_id: String,
    },
    MissingCanonicalVariant {
        room_id: String,
    },
    DuplicateDoorId {
        room_id: String,
        door_id: String,
    },
    DuplicateLoadoutMatrix {
        room_id: String,
        loadout: EvaluationLoadout,
    },
    DuplicateRouteLoadoutCell {
        room_id: String,
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    UnexpectedRouteLoadoutCell {
        room_id: String,
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    InconsistentHardGateCounts {
        room_id: String,
        component: &'static str,
        expected: usize,
        positive: usize,
        bounded: usize,
    },
    RoomMetrics {
        room_id: String,
        source: RoomMetricSummaryError,
    },
}

impl fmt::Display for ChallengeSanityAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFractionFloor {
                field,
                numerator,
                denominator,
            } => write!(
                formatter,
                "invalid fraction floor {field}: {numerator}/{denominator} (denominator must be positive and numerator cannot exceed it)"
            ),
            Self::DuplicateEvaluatedRoom { room_id } => {
                write!(formatter, "duplicate evaluated room {room_id:?}")
            }
            Self::DuplicateAnalysisRoom { room_id } => {
                write!(formatter, "duplicate analyzed room {room_id:?}")
            }
            Self::AnalysisWithoutEvaluatedRoom { room_id } => write!(
                formatter,
                "analysis was supplied for room {room_id:?}, which is absent from the evaluated batch"
            ),
            Self::MissingCanonicalVariant { room_id } => {
                write!(
                    formatter,
                    "evaluated room {room_id:?} has no canonical variant"
                )
            }
            Self::DuplicateDoorId { room_id, door_id } => {
                write!(
                    formatter,
                    "room {room_id:?} has duplicate door ID {door_id:?}"
                )
            }
            Self::DuplicateLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "room {room_id:?} has duplicate {} route matrices",
                loadout.slug()
            ),
            Self::DuplicateRouteLoadoutCell {
                room_id,
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "room {room_id:?} has duplicate {} challenge cell {source_door_id:?} -> {target_door_id:?}",
                loadout.slug()
            ),
            Self::UnexpectedRouteLoadoutCell {
                room_id,
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "room {room_id:?} has unexpected {} challenge cell {source_door_id:?} -> {target_door_id:?}",
                loadout.slug()
            ),
            Self::InconsistentHardGateCounts {
                room_id,
                component,
                expected,
                positive,
                bounded,
            } => write!(
                formatter,
                "room {room_id:?} hard-gate component {component} partitions {positive} positive + {bounded} bounded cells, expected {expected}"
            ),
            Self::RoomMetrics { room_id, source } => {
                write!(
                    formatter,
                    "cannot summarize challenge metrics for {room_id:?}: {source}"
                )
            }
        }
    }
}

impl Error for ChallengeSanityAuditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RoomMetrics { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Build a challenge audit directly from evaluated rooms and in-memory room
/// analyses.  Evaluated rooms without a supplied analysis remain in the
/// report with every controller cell marked missing.
pub fn audit_challenge_sanity(
    evaluated: &EvaluatedCorpusBatch,
    analyses: &[CorpusRoomAnalysis],
    thresholds: ChallengeSanityThresholds,
) -> Result<ChallengeSanityAudit, ChallengeSanityAuditError> {
    let mut summaries = Vec::with_capacity(analyses.len());
    for analysis in analyses {
        summaries.push(summarize_room_metrics(analysis).map_err(|source| {
            ChallengeSanityAuditError::RoomMetrics {
                room_id: analysis.room_id.0.clone(),
                source,
            }
        })?);
    }
    audit_challenge_sanity_from_metrics(evaluated, &summaries, thresholds)
}

/// Build a challenge audit from evaluated rooms and precomputed room metric
/// summaries.  This is the inexpensive adapter for deep-shard consumers.
pub fn audit_challenge_sanity_from_metrics(
    evaluated: &EvaluatedCorpusBatch,
    metrics: &[RoomMetricSummary],
    thresholds: ChallengeSanityThresholds,
) -> Result<ChallengeSanityAudit, ChallengeSanityAuditError> {
    let mut evaluated_ids = BTreeSet::new();
    for room in &evaluated.rooms {
        if !evaluated_ids.insert(room.generated.id.0.clone()) {
            return Err(ChallengeSanityAuditError::DuplicateEvaluatedRoom {
                room_id: room.generated.id.0.clone(),
            });
        }
    }
    let mut metric_by_room = BTreeMap::new();
    for metric in metrics {
        let room_id = metric.room_id.0.clone();
        if !evaluated_ids.contains(&room_id) {
            return Err(ChallengeSanityAuditError::AnalysisWithoutEvaluatedRoom { room_id });
        }
        if metric_by_room.insert(room_id.clone(), metric).is_some() {
            return Err(ChallengeSanityAuditError::DuplicateAnalysisRoom { room_id });
        }
    }

    let mut evidence = evaluated
        .rooms
        .iter()
        .map(|room| {
            challenge_room_evidence(room, metric_by_room.get(&room.generated.id.0).copied())
        })
        .collect::<Result<Vec<_>, _>>()?;
    evidence.sort_by(|left, right| left.room_id.cmp(&right.room_id));
    audit_challenge_evidence(&evidence, thresholds)
}

/// Normalize one evaluated room and optional deep metric summary into the
/// independent serde-ready evidence shape.
pub fn challenge_room_evidence(
    evaluated: &EvaluatedCorpusRoom,
    metrics: Option<&RoomMetricSummary>,
) -> Result<ChallengeRoomEvidence, ChallengeSanityAuditError> {
    let room_id = evaluated.generated.id.0.clone();
    let canonical = evaluated.generated.variants.first().ok_or_else(|| {
        ChallengeSanityAuditError::MissingCanonicalVariant {
            room_id: room_id.clone(),
        }
    })?;
    let mut door_ids = canonical
        .generated
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    if let Some(duplicate) = door_ids.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(ChallengeSanityAuditError::DuplicateDoorId {
            room_id,
            door_id: duplicate[0].clone(),
        });
    }
    let construction_loadout = loadout_from_abilities(canonical.key.source.profile.abilities);
    let hard_gates = RoomHardGateRecord {
        construction_loadout,
        construction_routes: matrix_component(
            evaluated,
            construction_loadout,
            MatrixComponent::Doors,
        )?,
        construction_pickups: matrix_component(
            evaluated,
            construction_loadout,
            MatrixComponent::Pickups,
        )?,
        complete_kit_routes: matrix_component(
            evaluated,
            EvaluationLoadout::Both,
            MatrixComponent::Doors,
        )?,
        complete_kit_pickups: matrix_component(
            evaluated,
            EvaluationLoadout::Both,
            MatrixComponent::Pickups,
        )?,
    };

    let mut supplied = BTreeMap::new();
    if let Some(metrics) = metrics {
        for route in &metrics.direct_controllers.directed_routes {
            for exact in &route.exact_loadouts {
                let key = (
                    route.source_door_id.clone(),
                    route.target_door_id.clone(),
                    exact.loadout,
                );
                if supplied
                    .insert(key.clone(), metric_cell_evidence(exact))
                    .is_some()
                {
                    return Err(ChallengeSanityAuditError::DuplicateRouteLoadoutCell {
                        room_id: evaluated.generated.id.0.clone(),
                        source_door_id: key.0,
                        target_door_id: key.1,
                        loadout: key.2,
                    });
                }
            }
        }
    }

    let expected_keys = expected_route_loadout_keys(&door_ids);
    let expected_set = expected_keys.iter().cloned().collect::<BTreeSet<_>>();
    if let Some(unexpected) = supplied.keys().find(|key| !expected_set.contains(*key)) {
        return Err(ChallengeSanityAuditError::UnexpectedRouteLoadoutCell {
            room_id: evaluated.generated.id.0.clone(),
            source_door_id: unexpected.0.clone(),
            target_door_id: unexpected.1.clone(),
            loadout: unexpected.2,
        });
    }
    let missing_reason = if metrics.is_some() {
        ChallengeCellMissingReason::RouteLoadoutCellNotSupplied
    } else {
        ChallengeCellMissingReason::RoomAnalysisNotSupplied
    };
    let route_loadout_cells = expected_keys
        .into_iter()
        .map(
            |(source_door_id, target_door_id, loadout)| RouteLoadoutChallengeCell {
                evidence: supplied
                    .remove(&(source_door_id.clone(), target_door_id.clone(), loadout))
                    .unwrap_or(ChallengeCellEvidence::Missing {
                        reason: missing_reason,
                    }),
                source_door_id,
                target_door_id,
                loadout,
            },
        )
        .collect();

    Ok(ChallengeRoomEvidence {
        room_id: evaluated.generated.id.0.clone(),
        door_ids,
        route_loadout_cells,
        hard_gates,
    })
}

#[derive(Clone, Copy)]
enum MatrixComponent {
    Doors,
    Pickups,
}

fn matrix_component(
    room: &EvaluatedCorpusRoom,
    loadout: EvaluationLoadout,
    component: MatrixComponent,
) -> Result<HardGateComponentRecord, ChallengeSanityAuditError> {
    let mut matrices = room
        .matrices
        .iter()
        .filter(|matrix| matrix.loadout == loadout);
    let Some(matrix) = matrices.next() else {
        return Ok(HardGateComponentRecord::missing());
    };
    if matrices.next().is_some() {
        return Err(ChallengeSanityAuditError::DuplicateLoadoutMatrix {
            room_id: room.generated.id.0.clone(),
            loadout,
        });
    }
    let (expected_cells, positive_cells, bounded_inconclusive_cells) = match component {
        MatrixComponent::Doors => (
            matrix.summary.door_rows,
            matrix.summary.positive_door_rows,
            matrix.summary.inconclusive_door_rows,
        ),
        MatrixComponent::Pickups => (
            matrix.summary.pickup_rows,
            matrix.summary.positive_pickup_rows,
            matrix.summary.inconclusive_pickup_rows,
        ),
    };
    Ok(HardGateComponentRecord {
        expected_cells,
        positive_cells,
        bounded_inconclusive_cells,
        status: if bounded_inconclusive_cells == 0 {
            HardGateStatus::Pass
        } else {
            HardGateStatus::BoundedEvidenceDoesNotPass
        },
    })
}

fn metric_cell_evidence(
    metric: &super::DirectedRouteLoadoutControllerMetric,
) -> ChallengeCellEvidence {
    match &metric.easiest_known {
        MetricEvidence::Observed(known) => ChallengeCellEvidence::KnownPositive {
            class: if known.demand.run_only {
                EasiestControllerClass::RunOnly
            } else if known.demand.monotone_simple {
                EasiestControllerClass::MonotoneSimpleOnly
            } else {
                EasiestControllerClass::GenuinelyOther
            },
            demand: ControllerDemandRecord {
                semantic_spans: known.demand.semantic_spans,
                semantic_transitions: known.demand.semantic_transitions,
                horizontal_reversals: known.demand.horizontal_reversals,
                vertical_decisions: known.demand.vertical_decisions,
                ability_events: known.demand.ability_events(),
                wall_jump_events: known.demand.wall_jump_events,
                dash_events: known.demand.dash_events,
                duration_ticks: known.demand.duration_ticks,
            },
            audit_status: match metric.audit {
                DirectControllerAuditMetric::CompleteFiniteVocabulary { .. } => {
                    PositiveAuditStatus::CompleteFiniteVocabulary
                }
                DirectControllerAuditMetric::BoundedIncomplete { limit, .. } => {
                    PositiveAuditStatus::BoundedIncomplete {
                        limit: limit.into(),
                    }
                }
                DirectControllerAuditMetric::MissingDirectedRouteAssessment
                | DirectControllerAuditMetric::MissingLoadoutAudit => {
                    PositiveAuditStatus::MissingAuditMetadata
                }
            },
        },
        MetricEvidence::NotApplicable {
            reason: NotApplicableMetricReason::NoKnownPositiveControllerInCompleteFiniteVocabulary,
        } => ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary,
        MetricEvidence::Missing {
            reason: MissingMetricReason::BoundedDirectControllerAuditWithoutPositive,
        } => match metric.audit {
            DirectControllerAuditMetric::BoundedIncomplete { limit, .. } => {
                ChallengeCellEvidence::BoundedWithoutPositive {
                    limit: limit.into(),
                }
            }
            DirectControllerAuditMetric::CompleteFiniteVocabulary { .. }
            | DirectControllerAuditMetric::MissingDirectedRouteAssessment
            | DirectControllerAuditMetric::MissingLoadoutAudit => ChallengeCellEvidence::Missing {
                reason: ChallengeCellMissingReason::DirectControllerAuditMissing,
            },
        },
        MetricEvidence::Missing {
            reason: MissingMetricReason::MissingDirectedRouteAssessment,
        } => ChallengeCellEvidence::Missing {
            reason: ChallengeCellMissingReason::DirectedRouteAssessmentMissing,
        },
        MetricEvidence::Missing {
            reason: MissingMetricReason::MissingDirectControllerAudit,
        }
        | MetricEvidence::Missing { .. }
        | MetricEvidence::NotApplicable { .. } => ChallengeCellEvidence::Missing {
            reason: ChallengeCellMissingReason::DirectControllerAuditMissing,
        },
    }
}

fn expected_route_loadout_keys(door_ids: &[String]) -> Vec<(String, String, EvaluationLoadout)> {
    let mut result = Vec::new();
    for source in door_ids {
        for target in door_ids {
            if source == target {
                continue;
            }
            for loadout in EvaluationLoadout::ALL {
                result.push((source.clone(), target.clone(), loadout));
            }
        }
    }
    result
}

/// Pure deterministic aggregation over normalized room evidence.
pub fn audit_challenge_evidence(
    rooms: &[ChallengeRoomEvidence],
    thresholds: ChallengeSanityThresholds,
) -> Result<ChallengeSanityAudit, ChallengeSanityAuditError> {
    validate_thresholds(&thresholds)?;
    let mut rooms = rooms.to_vec();
    rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
    for pair in rooms.windows(2) {
        if pair[0].room_id == pair[1].room_id {
            return Err(ChallengeSanityAuditError::DuplicateEvaluatedRoom {
                room_id: pair[0].room_id.clone(),
            });
        }
    }

    let mut route_loadout_cells = Vec::new();
    let mut complete_kit_rooms = Vec::new();
    let mut incomplete_evidence = Vec::new();
    for room in &rooms {
        let normalized = normalize_room_cells(room)?;
        let mut complete_kit = ChallengeCellDenominators::default();
        for cell in normalized {
            if cell.loadout == EvaluationLoadout::Both {
                complete_kit.observe(&cell.evidence);
            }
            append_incomplete_cell(&mut incomplete_evidence, &room.room_id, &cell);
            route_loadout_cells.push(RouteLoadoutChallengeCellRecord {
                room_id: room.room_id.clone(),
                source_door_id: cell.source_door_id,
                target_door_id: cell.target_door_id,
                loadout: cell.loadout,
                evidence: cell.evidence,
            });
        }
        let evidence_state = room_evidence_state(complete_kit);
        let known_route_composition = room_known_composition(complete_kit);
        complete_kit_rooms.push(RoomCompleteKitChallengeRecord {
            room_id: room.room_id.clone(),
            denominators: complete_kit,
            evidence_state,
            known_route_composition,
        });
        append_incomplete_gates(&mut incomplete_evidence, room);
    }

    let all_loadouts = summarize_cells(route_loadout_cells.iter());
    let by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| LoadoutChallengeControllerSummary {
            loadout,
            summary: summarize_cells(
                route_loadout_cells
                    .iter()
                    .filter(move |cell| cell.loadout == loadout),
            ),
        })
        .collect::<Vec<_>>();
    let directional_pairs = directional_pairs(&rooms, &route_loadout_cells);
    let directional_asymmetry = summarize_directional_pairs(directional_pairs.iter());
    let directional_asymmetry_by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| LoadoutDirectionalAsymmetrySummary {
            loadout,
            summary: summarize_directional_pairs(
                directional_pairs
                    .iter()
                    .filter(move |pair| pair.loadout == loadout),
            ),
        })
        .collect::<Vec<_>>();
    let room_composition = summarize_room_composition(&complete_kit_rooms);
    let hard_gates = summarize_hard_gates(&rooms);
    let diagnostics = pilot_diagnostics(&thresholds, &by_loadout, &room_composition, &hard_gates);
    let threshold_deficits = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.outcome != PilotDiagnosticOutcome::MeetsThreshold)
        .map(|diagnostic| diagnostic.message.clone())
        .collect();

    Ok(ChallengeSanityAudit {
        version: CHALLENGE_SANITY_AUDIT_VERSION,
        disclaimer: CHALLENGE_SANITY_AUDIT_DISCLAIMER.to_owned(),
        thresholds,
        room_count: rooms.len(),
        route_loadout_cells,
        all_loadouts,
        by_loadout,
        directional_pairs,
        directional_asymmetry,
        directional_asymmetry_by_loadout,
        complete_kit_rooms,
        room_composition,
        hard_gates,
        diagnostics,
        threshold_deficits,
        incomplete_evidence,
    })
}

fn validate_thresholds(
    thresholds: &ChallengeSanityThresholds,
) -> Result<(), ChallengeSanityAuditError> {
    for (field, floor) in [
        (
            "minimum_complete_kit_genuinely_other_fraction_of_known_positives",
            thresholds.minimum_complete_kit_genuinely_other_fraction_of_known_positives,
        ),
        (
            "minimum_complete_kit_genuinely_other_fraction_of_expected_routes",
            thresholds.minimum_complete_kit_genuinely_other_fraction_of_expected_routes,
        ),
    ] {
        if let Some(floor) = floor
            && (floor.denominator == 0 || floor.numerator > floor.denominator)
        {
            return Err(ChallengeSanityAuditError::InvalidFractionFloor {
                field,
                numerator: floor.numerator,
                denominator: floor.denominator,
            });
        }
    }
    Ok(())
}

fn normalize_room_cells(
    room: &ChallengeRoomEvidence,
) -> Result<Vec<RouteLoadoutChallengeCell>, ChallengeSanityAuditError> {
    let mut door_ids = room.door_ids.clone();
    door_ids.sort_unstable();
    if let Some(duplicate) = door_ids.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(ChallengeSanityAuditError::DuplicateDoorId {
            room_id: room.room_id.clone(),
            door_id: duplicate[0].clone(),
        });
    }
    let expected = expected_route_loadout_keys(&door_ids);
    let expected_set = expected.iter().cloned().collect::<BTreeSet<_>>();
    let mut supplied = BTreeMap::new();
    for cell in &room.route_loadout_cells {
        let key = (
            cell.source_door_id.clone(),
            cell.target_door_id.clone(),
            cell.loadout,
        );
        if !expected_set.contains(&key) {
            return Err(ChallengeSanityAuditError::UnexpectedRouteLoadoutCell {
                room_id: room.room_id.clone(),
                source_door_id: key.0,
                target_door_id: key.1,
                loadout: key.2,
            });
        }
        if supplied
            .insert(key.clone(), cell.evidence.clone())
            .is_some()
        {
            return Err(ChallengeSanityAuditError::DuplicateRouteLoadoutCell {
                room_id: room.room_id.clone(),
                source_door_id: key.0,
                target_door_id: key.1,
                loadout: key.2,
            });
        }
    }
    validate_hard_gates(room)?;
    Ok(expected
        .into_iter()
        .map(
            |(source_door_id, target_door_id, loadout)| RouteLoadoutChallengeCell {
                evidence: supplied
                    .remove(&(source_door_id.clone(), target_door_id.clone(), loadout))
                    .unwrap_or(ChallengeCellEvidence::Missing {
                        reason: ChallengeCellMissingReason::RouteLoadoutCellNotSupplied,
                    }),
                source_door_id,
                target_door_id,
                loadout,
            },
        )
        .collect())
}

fn validate_hard_gates(room: &ChallengeRoomEvidence) -> Result<(), ChallengeSanityAuditError> {
    for (name, component) in [
        ("construction_routes", room.hard_gates.construction_routes),
        ("construction_pickups", room.hard_gates.construction_pickups),
        ("complete_kit_routes", room.hard_gates.complete_kit_routes),
        ("complete_kit_pickups", room.hard_gates.complete_kit_pickups),
    ] {
        if component.status != HardGateStatus::MissingMatrix
            && component
                .positive_cells
                .saturating_add(component.bounded_inconclusive_cells)
                != component.expected_cells
        {
            return Err(ChallengeSanityAuditError::InconsistentHardGateCounts {
                room_id: room.room_id.clone(),
                component: name,
                expected: component.expected_cells,
                positive: component.positive_cells,
                bounded: component.bounded_inconclusive_cells,
            });
        }
    }
    Ok(())
}

fn summarize_cells<'a>(
    cells: impl IntoIterator<Item = &'a RouteLoadoutChallengeCellRecord>,
) -> ChallengeControllerSummary {
    let cells = cells.into_iter().collect::<Vec<_>>();
    let mut denominators = ChallengeCellDenominators::default();
    let mut demands = Vec::new();
    for cell in cells {
        denominators.observe(&cell.evidence);
        if let ChallengeCellEvidence::KnownPositive { demand, .. } = cell.evidence {
            demands.push(demand);
        }
    }
    ChallengeControllerSummary {
        denominators,
        demand: demand_distributions(&demands),
    }
}

fn demand_distributions(demands: &[ControllerDemandRecord]) -> ControllerDemandDistributions {
    ControllerDemandDistributions {
        semantic_spans: distribution_evidence(demands.iter().map(|demand| demand.semantic_spans)),
        semantic_transitions: distribution_evidence(
            demands.iter().map(|demand| demand.semantic_transitions),
        ),
        horizontal_reversals: distribution_evidence(
            demands.iter().map(|demand| demand.horizontal_reversals),
        ),
        vertical_decisions: distribution_evidence(
            demands.iter().map(|demand| demand.vertical_decisions),
        ),
        ability_events: distribution_evidence(demands.iter().map(|demand| demand.ability_events)),
        duration_ticks: distribution_evidence(demands.iter().map(|demand| demand.duration_ticks)),
    }
}

fn distribution_evidence(samples: impl IntoIterator<Item = usize>) -> DemandDistributionEvidence {
    let mut samples = samples.into_iter().collect::<Vec<_>>();
    if samples.is_empty() {
        DemandDistributionEvidence::NoKnownPositiveControllers
    } else {
        samples.sort_unstable();
        let sample_count = samples.len();
        DemandDistributionEvidence::Observed {
            distribution: DemandDistribution {
                sample_count,
                minimum: samples[0],
                median_lower: samples[(sample_count - 1) / 2],
                median_upper: samples[sample_count / 2],
                maximum: samples[sample_count - 1],
                spread: samples[sample_count - 1].saturating_sub(samples[0]),
                total: samples.iter().copied().sum(),
            },
        }
    }
}

fn room_evidence_state(denominators: ChallengeCellDenominators) -> RoomControllerEvidenceState {
    if denominators.missing > 0 || denominators.positives_with_missing_audit_metadata > 0 {
        RoomControllerEvidenceState::Missing
    } else if denominators.bounded_without_positive > 0
        || denominators.positives_from_bounded_audits > 0
    {
        RoomControllerEvidenceState::BoundedIncomplete
    } else {
        RoomControllerEvidenceState::CompleteFiniteVocabulary
    }
}

fn room_known_composition(
    denominators: ChallengeCellDenominators,
) -> CompleteKitKnownRouteComposition {
    let simple = denominators
        .known_run_only_cells
        .saturating_add(denominators.known_monotone_simple_only_cells);
    let other = denominators.known_genuinely_other_cells;
    if denominators.known_positive_cells == 0 {
        CompleteKitKnownRouteComposition::NoKnownPositive
    } else if other == 0 {
        CompleteKitKnownRouteComposition::AllKnownRunOrMonotone
    } else if simple == 0 {
        CompleteKitKnownRouteComposition::AllKnownGenuinelyOther
    } else {
        CompleteKitKnownRouteComposition::MixedRunOrMonotoneAndGenuinelyOther
    }
}

fn summarize_room_composition(rooms: &[RoomCompleteKitChallengeRecord]) -> RoomCompositionSummary {
    let mut summary = RoomCompositionSummary::default();
    for room in rooms {
        match room.known_route_composition {
            CompleteKitKnownRouteComposition::NoKnownPositive => {
                summary
                    .no_known_positive_room_ids
                    .push(room.room_id.clone());
            }
            CompleteKitKnownRouteComposition::AllKnownRunOrMonotone => {
                summary
                    .all_known_run_or_monotone_room_ids
                    .push(room.room_id.clone());
            }
            CompleteKitKnownRouteComposition::MixedRunOrMonotoneAndGenuinelyOther => {
                summary.mixed_room_ids.push(room.room_id.clone());
            }
            CompleteKitKnownRouteComposition::AllKnownGenuinelyOther => {
                summary
                    .all_known_genuinely_other_room_ids
                    .push(room.room_id.clone());
            }
        }
        match room.evidence_state {
            RoomControllerEvidenceState::CompleteFiniteVocabulary => {
                summary
                    .complete_evidence_room_ids
                    .push(room.room_id.clone());
            }
            RoomControllerEvidenceState::BoundedIncomplete => {
                summary.bounded_evidence_room_ids.push(room.room_id.clone());
            }
            RoomControllerEvidenceState::Missing => {
                summary.missing_evidence_room_ids.push(room.room_id.clone());
            }
        }
    }
    summary
}

fn directional_pairs(
    rooms: &[ChallengeRoomEvidence],
    cells: &[RouteLoadoutChallengeCellRecord],
) -> Vec<DirectionalPairChallengeRecord> {
    let cell_map = cells
        .iter()
        .map(|cell| {
            (
                (
                    cell.room_id.as_str(),
                    cell.source_door_id.as_str(),
                    cell.target_door_id.as_str(),
                    cell.loadout,
                ),
                &cell.evidence,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for room in rooms {
        let mut doors = room.door_ids.clone();
        doors.sort_unstable();
        doors.dedup();
        for (a_index, door_a) in doors.iter().enumerate() {
            for door_b in &doors[a_index + 1..] {
                for loadout in EvaluationLoadout::ALL {
                    let a_to_b = cell_map
                        .get(&(
                            room.room_id.as_str(),
                            door_a.as_str(),
                            door_b.as_str(),
                            loadout,
                        ))
                        .expect("normalized route cells cover both directions");
                    let b_to_a = cell_map
                        .get(&(
                            room.room_id.as_str(),
                            door_b.as_str(),
                            door_a.as_str(),
                            loadout,
                        ))
                        .expect("normalized route cells cover both directions");
                    let evidence = match (*a_to_b, *b_to_a) {
                        (
                            ChallengeCellEvidence::KnownPositive {
                                class: class_a,
                                demand: demand_a,
                                ..
                            },
                            ChallengeCellEvidence::KnownPositive {
                                class: class_b,
                                demand: demand_b,
                                ..
                            },
                        ) => DirectionalPairEvidence::Comparable {
                            differences: demand_differences(
                                *class_a, *demand_a, *class_b, *demand_b,
                            ),
                        },
                        (a_to_b, b_to_a) => DirectionalPairEvidence::Incomplete {
                            a_to_b: route_availability(a_to_b),
                            b_to_a: route_availability(b_to_a),
                        },
                    };
                    result.push(DirectionalPairChallengeRecord {
                        room_id: room.room_id.clone(),
                        door_a: door_a.clone(),
                        door_b: door_b.clone(),
                        loadout,
                        evidence,
                    });
                }
            }
        }
    }
    result
}

fn route_availability(evidence: &ChallengeCellEvidence) -> DirectionalRouteAvailability {
    match evidence {
        ChallengeCellEvidence::KnownPositive { class, .. } => {
            DirectionalRouteAvailability::KnownPositive { class: *class }
        }
        ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary => {
            DirectionalRouteAvailability::NoPositiveInCompleteFiniteVocabulary
        }
        ChallengeCellEvidence::BoundedWithoutPositive { limit } => {
            DirectionalRouteAvailability::BoundedWithoutPositive { limit: *limit }
        }
        ChallengeCellEvidence::Missing { reason } => {
            DirectionalRouteAvailability::Missing { reason: *reason }
        }
    }
}

fn demand_differences(
    class_a: EasiestControllerClass,
    demand_a: ControllerDemandRecord,
    class_b: EasiestControllerClass,
    demand_b: ControllerDemandRecord,
) -> DirectionalDemandDifferenceRecord {
    DirectionalDemandDifferenceRecord {
        controller_class: count_difference(class_ordinal(class_a), class_ordinal(class_b)),
        semantic_spans: count_difference(demand_a.semantic_spans, demand_b.semantic_spans),
        semantic_transitions: count_difference(
            demand_a.semantic_transitions,
            demand_b.semantic_transitions,
        ),
        horizontal_reversals: count_difference(
            demand_a.horizontal_reversals,
            demand_b.horizontal_reversals,
        ),
        vertical_decisions: count_difference(
            demand_a.vertical_decisions,
            demand_b.vertical_decisions,
        ),
        ability_events: count_difference(demand_a.ability_events, demand_b.ability_events),
        duration_ticks: count_difference(demand_a.duration_ticks, demand_b.duration_ticks),
        wall_jump_use_differs: (demand_a.wall_jump_events > 0) != (demand_b.wall_jump_events > 0),
        dash_use_differs: (demand_a.dash_events > 0) != (demand_b.dash_events > 0),
    }
}

const fn class_ordinal(class: EasiestControllerClass) -> usize {
    match class {
        EasiestControllerClass::RunOnly => 0,
        EasiestControllerClass::MonotoneSimpleOnly => 1,
        EasiestControllerClass::GenuinelyOther => 2,
    }
}

const fn count_difference(left: usize, right: usize) -> DirectionalCountDifferenceRecord {
    DirectionalCountDifferenceRecord {
        a_to_b: left,
        b_to_a: right,
        absolute_difference: left.abs_diff(right),
    }
}

fn summarize_directional_pairs<'a>(
    pairs: impl IntoIterator<Item = &'a DirectionalPairChallengeRecord>,
) -> DirectionalAsymmetrySummary {
    let pairs = pairs.into_iter().collect::<Vec<_>>();
    let differences = pairs
        .iter()
        .filter_map(|pair| match pair.evidence {
            DirectionalPairEvidence::Comparable { differences } => Some(differences),
            DirectionalPairEvidence::Incomplete { .. } => None,
        })
        .collect::<Vec<_>>();
    let pairs_with_any_measured_difference = differences
        .iter()
        .filter(|difference| {
            difference.controller_class.absolute_difference > 0
                || difference.semantic_spans.absolute_difference > 0
                || difference.semantic_transitions.absolute_difference > 0
                || difference.horizontal_reversals.absolute_difference > 0
                || difference.vertical_decisions.absolute_difference > 0
                || difference.ability_events.absolute_difference > 0
                || difference.duration_ticks.absolute_difference > 0
                || difference.wall_jump_use_differs
                || difference.dash_use_differs
        })
        .count();
    DirectionalAsymmetrySummary {
        expected_pair_cells: pairs.len(),
        comparable_pair_cells: differences.len(),
        incomplete_pair_cells: pairs.len().saturating_sub(differences.len()),
        pairs_with_any_measured_difference,
        pairs_with_different_wall_jump_use: differences
            .iter()
            .filter(|difference| difference.wall_jump_use_differs)
            .count(),
        pairs_with_different_dash_use: differences
            .iter()
            .filter(|difference| difference.dash_use_differs)
            .count(),
        semantic_span_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.semantic_spans.absolute_difference),
        ),
        semantic_transition_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.semantic_transitions.absolute_difference),
        ),
        horizontal_reversal_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.horizontal_reversals.absolute_difference),
        ),
        vertical_decision_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.vertical_decisions.absolute_difference),
        ),
        ability_event_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.ability_events.absolute_difference),
        ),
        duration_tick_absolute_difference: distribution_evidence(
            differences
                .iter()
                .map(|difference| difference.duration_ticks.absolute_difference),
        ),
    }
}

fn summarize_hard_gates(rooms: &[ChallengeRoomEvidence]) -> BatchHardGateSummary {
    BatchHardGateSummary {
        construction_routes: hard_gate_status_summary(rooms, |room| {
            room.hard_gates.construction_routes
        }),
        construction_pickups: hard_gate_status_summary(rooms, |room| {
            room.hard_gates.construction_pickups
        }),
        complete_kit_routes: hard_gate_status_summary(rooms, |room| {
            room.hard_gates.complete_kit_routes
        }),
        complete_kit_pickups: hard_gate_status_summary(rooms, |room| {
            room.hard_gates.complete_kit_pickups
        }),
    }
}

fn hard_gate_status_summary(
    rooms: &[ChallengeRoomEvidence],
    component: impl Fn(&ChallengeRoomEvidence) -> HardGateComponentRecord,
) -> HardGateStatusSummary {
    let mut summary = HardGateStatusSummary {
        room_count: rooms.len(),
        ..HardGateStatusSummary::default()
    };
    for room in rooms {
        match component(room).status {
            HardGateStatus::Pass => summary.pass_count += 1,
            HardGateStatus::BoundedEvidenceDoesNotPass => {
                summary.bounded_does_not_pass_count += 1;
                summary.nonpassing_room_ids.push(room.room_id.clone());
            }
            HardGateStatus::MissingMatrix => {
                summary.missing_matrix_count += 1;
                summary.nonpassing_room_ids.push(room.room_id.clone());
            }
        }
    }
    summary
}

fn pilot_diagnostics(
    thresholds: &ChallengeSanityThresholds,
    by_loadout: &[LoadoutChallengeControllerSummary],
    rooms: &RoomCompositionSummary,
    hard_gates: &BatchHardGateSummary,
) -> Vec<PilotDiagnostic> {
    let complete_kit = &by_loadout
        .iter()
        .find(|summary| summary.loadout == EvaluationLoadout::Both)
        .expect("all loadouts are always summarized")
        .summary
        .denominators;
    let mut diagnostics = Vec::new();
    if let Some(minimum) = thresholds.minimum_complete_kit_genuinely_other_routes {
        let observed = complete_kit.known_genuinely_other_cells;
        diagnostics.push(count_floor_diagnostic(
            "minimum_complete_kit_genuinely_other_routes",
            observed,
            minimum,
            format!(
                "complete-kit genuinely-other known routes: observed {observed}, configured minimum {minimum}"
            ),
        ));
    }
    if let Some(floor) = thresholds.minimum_complete_kit_genuinely_other_fraction_of_known_positives
    {
        diagnostics.push(fraction_floor_diagnostic(
            "minimum_complete_kit_genuinely_other_fraction_of_known_positives",
            complete_kit.known_genuinely_other_cells,
            complete_kit.known_positive_cells,
            floor,
            "complete-kit genuinely-other fraction among exact known positives",
        ));
    }
    if let Some(floor) = thresholds.minimum_complete_kit_genuinely_other_fraction_of_expected_routes
    {
        diagnostics.push(fraction_floor_diagnostic(
            "minimum_complete_kit_genuinely_other_fraction_of_expected_routes",
            complete_kit.known_genuinely_other_cells,
            complete_kit.expected_cells,
            floor,
            "complete-kit genuinely-other raw fraction among all expected directed routes",
        ));
    }
    if let Some(maximum) =
        thresholds.maximum_rooms_with_all_known_complete_kit_routes_run_or_monotone
    {
        let observed = rooms.all_known_run_or_monotone_room_ids.len();
        let outcome = if observed <= maximum {
            PilotDiagnosticOutcome::MeetsThreshold
        } else {
            PilotDiagnosticOutcome::Deficit
        };
        diagnostics.push(PilotDiagnostic {
            id: "maximum_rooms_with_all_known_complete_kit_routes_run_or_monotone".to_owned(),
            outcome,
            observed: observed.to_string(),
            configured_threshold: format!("maximum {maximum}"),
            message: format!(
                "rooms whose known complete-kit routes are all run-only or monotone-simple-only: observed {observed}, configured maximum {maximum}; room IDs [{}]",
                rooms.all_known_run_or_monotone_room_ids.join(", ")
            ),
        });
    }
    for (enabled, id, label, summary) in [
        (
            thresholds.require_all_construction_route_gates,
            "require_all_construction_route_gates",
            "construction route gates",
            &hard_gates.construction_routes,
        ),
        (
            thresholds.require_all_construction_pickup_gates,
            "require_all_construction_pickup_gates",
            "construction pickup gates",
            &hard_gates.construction_pickups,
        ),
        (
            thresholds.require_all_complete_kit_route_gates,
            "require_all_complete_kit_route_gates",
            "complete-kit route gates",
            &hard_gates.complete_kit_routes,
        ),
        (
            thresholds.require_all_complete_kit_pickup_gates,
            "require_all_complete_kit_pickup_gates",
            "complete-kit pickup gates",
            &hard_gates.complete_kit_pickups,
        ),
    ] {
        if enabled {
            let nonpassing = summary
                .bounded_does_not_pass_count
                .saturating_add(summary.missing_matrix_count);
            diagnostics.push(PilotDiagnostic {
                id: id.to_owned(),
                outcome: if nonpassing == 0 {
                    PilotDiagnosticOutcome::MeetsThreshold
                } else {
                    PilotDiagnosticOutcome::Deficit
                },
                observed: format!("{}/{} pass", summary.pass_count, summary.room_count),
                configured_threshold: "all rooms pass".to_owned(),
                message: format!(
                    "{label}: {}/{} rooms pass; nonpassing room IDs [{}]",
                    summary.pass_count,
                    summary.room_count,
                    summary.nonpassing_room_ids.join(", ")
                ),
            });
        }
    }
    diagnostics
}

fn count_floor_diagnostic(
    id: &str,
    observed: usize,
    minimum: usize,
    message: String,
) -> PilotDiagnostic {
    PilotDiagnostic {
        id: id.to_owned(),
        outcome: if observed >= minimum {
            PilotDiagnosticOutcome::MeetsThreshold
        } else {
            PilotDiagnosticOutcome::Deficit
        },
        observed: observed.to_string(),
        configured_threshold: format!("minimum {minimum}"),
        message,
    }
}

fn fraction_floor_diagnostic(
    id: &str,
    numerator: usize,
    denominator: usize,
    floor: FractionFloor,
    label: &str,
) -> PilotDiagnostic {
    if denominator == 0 {
        return PilotDiagnostic {
            id: id.to_owned(),
            outcome: PilotDiagnosticOutcome::Unavailable,
            observed: "unavailable (zero denominator)".to_owned(),
            configured_threshold: format!("minimum {}/{}", floor.numerator, floor.denominator),
            message: format!(
                "{label}: unavailable because there are no cells in the configured denominator; configured minimum {}/{}",
                floor.numerator, floor.denominator
            ),
        };
    }
    let meets = (numerator as u128).saturating_mul(floor.denominator as u128)
        >= (floor.numerator as u128).saturating_mul(denominator as u128);
    PilotDiagnostic {
        id: id.to_owned(),
        outcome: if meets {
            PilotDiagnosticOutcome::MeetsThreshold
        } else {
            PilotDiagnosticOutcome::Deficit
        },
        observed: format!("{numerator}/{denominator}"),
        configured_threshold: format!("minimum {}/{}", floor.numerator, floor.denominator),
        message: format!(
            "{label}: observed {numerator}/{denominator}, configured minimum {}/{}",
            floor.numerator, floor.denominator
        ),
    }
}

fn append_incomplete_cell(
    output: &mut Vec<String>,
    room_id: &str,
    cell: &RouteLoadoutChallengeCell,
) {
    match cell.evidence {
        ChallengeCellEvidence::KnownPositive {
            audit_status: PositiveAuditStatus::BoundedIncomplete { limit },
            ..
        } => output.push(format!(
            "room {room_id} {} route {} -> {} has an exact positive, but its finite-vocabulary audit was bounded by {limit:?}",
            cell.loadout.slug(), cell.source_door_id, cell.target_door_id
        )),
        ChallengeCellEvidence::KnownPositive {
            audit_status: PositiveAuditStatus::MissingAuditMetadata,
            ..
        } => output.push(format!(
            "room {room_id} {} route {} -> {} has an exact positive with missing audit metadata",
            cell.loadout.slug(), cell.source_door_id, cell.target_door_id
        )),
        ChallengeCellEvidence::BoundedWithoutPositive { limit } => output.push(format!(
            "room {room_id} {} route {} -> {} has no known positive after an audit bounded by {limit:?}; this is not an unreachability claim",
            cell.loadout.slug(), cell.source_door_id, cell.target_door_id
        )),
        ChallengeCellEvidence::Missing { reason } => output.push(format!(
            "room {room_id} {} route {} -> {} is missing challenge evidence: {reason:?}",
            cell.loadout.slug(), cell.source_door_id, cell.target_door_id
        )),
        ChallengeCellEvidence::KnownPositive {
            audit_status: PositiveAuditStatus::CompleteFiniteVocabulary,
            ..
        }
        | ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary => {}
    }
}

fn append_incomplete_gates(output: &mut Vec<String>, room: &ChallengeRoomEvidence) {
    for (label, component) in [
        ("construction routes", room.hard_gates.construction_routes),
        ("construction pickups", room.hard_gates.construction_pickups),
        ("complete-kit routes", room.hard_gates.complete_kit_routes),
        ("complete-kit pickups", room.hard_gates.complete_kit_pickups),
    ] {
        match component.status {
            HardGateStatus::Pass => {}
            HardGateStatus::BoundedEvidenceDoesNotPass => output.push(format!(
                "room {} {label} hard gate does not pass: {}/{} positive, {} bounded-inconclusive",
                room.room_id,
                component.positive_cells,
                component.expected_cells,
                component.bounded_inconclusive_cells
            )),
            HardGateStatus::MissingMatrix => output.push(format!(
                "room {} {label} hard gate is missing its loadout matrix",
                room.room_id
            )),
        }
    }
}

/// Render a stable, compact plain-text report.  JSON users should serialize
/// [`ChallengeSanityAudit`] directly; both forms contain the same raw counts.
#[must_use]
pub fn render_challenge_sanity_text(audit: &ChallengeSanityAudit) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "challenge-sanity-audit-v{} rooms={}",
        audit.version, audit.room_count
    )
    .expect("writing to a String cannot fail");
    writeln!(output, "disclaimer: {}", audit.disclaimer).expect("writing to a String cannot fail");
    write_controller_summary(&mut output, "all-loadouts", &audit.all_loadouts);
    for summary in &audit.by_loadout {
        write_controller_summary(&mut output, summary.loadout.slug(), &summary.summary);
    }
    writeln!(
        output,
        "directional-pairs expected={} comparable={} incomplete={} any-difference={}",
        audit.directional_asymmetry.expected_pair_cells,
        audit.directional_asymmetry.comparable_pair_cells,
        audit.directional_asymmetry.incomplete_pair_cells,
        audit
            .directional_asymmetry
            .pairs_with_any_measured_difference
    )
    .expect("writing to a String cannot fail");
    writeln!(
        output,
        "complete-kit-room-composition none={} all-run-or-monotone={} mixed={} all-other={}",
        audit.room_composition.no_known_positive_room_ids.len(),
        audit
            .room_composition
            .all_known_run_or_monotone_room_ids
            .len(),
        audit.room_composition.mixed_room_ids.len(),
        audit
            .room_composition
            .all_known_genuinely_other_room_ids
            .len()
    )
    .expect("writing to a String cannot fail");
    for diagnostic in &audit.diagnostics {
        writeln!(
            output,
            "diagnostic {} {:?}: {}",
            diagnostic.id, diagnostic.outcome, diagnostic.message
        )
        .expect("writing to a String cannot fail");
    }
    for deficit in &audit.threshold_deficits {
        writeln!(output, "deficit: {deficit}").expect("writing to a String cannot fail");
    }
    for incomplete in &audit.incomplete_evidence {
        writeln!(output, "incomplete: {incomplete}").expect("writing to a String cannot fail");
    }
    output
}

fn write_controller_summary(
    output: &mut String,
    label: &str,
    summary: &ChallengeControllerSummary,
) {
    let counts = summary.denominators;
    writeln!(
        output,
        "controllers {label}: expected={} known={} run={} monotone-only={} other={} complete-no-positive={} bounded-no-positive={} missing={}",
        counts.expected_cells,
        counts.known_positive_cells,
        counts.known_run_only_cells,
        counts.known_monotone_simple_only_cells,
        counts.known_genuinely_other_cells,
        counts.no_positive_in_complete_finite_vocabulary,
        counts.bounded_without_positive,
        counts.missing
    )
    .expect("writing to a String cannot fail");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{
        CorpusBuildConfigV1, CorpusRoomAnalysisConfig, analyze_corpus_room,
        evaluate_route_matrices, generate_seed_block,
    };
    use downwards_ai::{DifficultyConfig, SolverConfig};

    fn demand(
        spans: usize,
        transitions: usize,
        reversals: usize,
        vertical: usize,
        wall_jumps: usize,
        dashes: usize,
        duration: usize,
    ) -> ControllerDemandRecord {
        ControllerDemandRecord {
            semantic_spans: spans,
            semantic_transitions: transitions,
            horizontal_reversals: reversals,
            vertical_decisions: vertical,
            ability_events: wall_jumps + dashes,
            wall_jump_events: wall_jumps,
            dash_events: dashes,
            duration_ticks: duration,
        }
    }

    fn positive(
        class: EasiestControllerClass,
        demand: ControllerDemandRecord,
    ) -> ChallengeCellEvidence {
        ChallengeCellEvidence::KnownPositive {
            class,
            demand,
            audit_status: PositiveAuditStatus::CompleteFiniteVocabulary,
        }
    }

    fn gate(status: HardGateStatus) -> HardGateComponentRecord {
        match status {
            HardGateStatus::Pass => HardGateComponentRecord {
                expected_cells: 2,
                positive_cells: 2,
                bounded_inconclusive_cells: 0,
                status,
            },
            HardGateStatus::BoundedEvidenceDoesNotPass => HardGateComponentRecord {
                expected_cells: 2,
                positive_cells: 1,
                bounded_inconclusive_cells: 1,
                status,
            },
            HardGateStatus::MissingMatrix => HardGateComponentRecord::missing(),
        }
    }

    fn synthetic_room() -> ChallengeRoomEvidence {
        let mut route_loadout_cells = Vec::new();
        for loadout in EvaluationLoadout::ALL {
            let (a_to_b, b_to_a) = if loadout == EvaluationLoadout::Both {
                (
                    positive(
                        EasiestControllerClass::RunOnly,
                        demand(2, 1, 0, 0, 0, 0, 40),
                    ),
                    positive(
                        EasiestControllerClass::GenuinelyOther,
                        demand(9, 8, 2, 3, 1, 0, 125),
                    ),
                )
            } else if loadout == EvaluationLoadout::Dash {
                (
                    ChallengeCellEvidence::BoundedWithoutPositive {
                        limit: DirectProbeLimitRecord::SimulatedTicks,
                    },
                    ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary,
                )
            } else {
                (
                    positive(
                        EasiestControllerClass::MonotoneSimpleOnly,
                        demand(4, 3, 0, 1, 0, 0, 70),
                    ),
                    ChallengeCellEvidence::Missing {
                        reason: ChallengeCellMissingReason::DirectControllerAuditMissing,
                    },
                )
            };
            route_loadout_cells.push(RouteLoadoutChallengeCell {
                source_door_id: "a".to_owned(),
                target_door_id: "b".to_owned(),
                loadout,
                evidence: a_to_b,
            });
            route_loadout_cells.push(RouteLoadoutChallengeCell {
                source_door_id: "b".to_owned(),
                target_door_id: "a".to_owned(),
                loadout,
                evidence: b_to_a,
            });
        }
        ChallengeRoomEvidence {
            room_id: "room-z".to_owned(),
            door_ids: vec!["b".to_owned(), "a".to_owned()],
            route_loadout_cells,
            hard_gates: RoomHardGateRecord {
                construction_loadout: EvaluationLoadout::Baseline,
                construction_routes: gate(HardGateStatus::BoundedEvidenceDoesNotPass),
                construction_pickups: gate(HardGateStatus::Pass),
                complete_kit_routes: gate(HardGateStatus::Pass),
                complete_kit_pickups: gate(HardGateStatus::MissingMatrix),
            },
        }
    }

    #[test]
    fn synthetic_audit_keeps_partition_demand_asymmetry_and_deficits_explicit() {
        let thresholds = ChallengeSanityThresholds {
            minimum_complete_kit_genuinely_other_routes: Some(2),
            minimum_complete_kit_genuinely_other_fraction_of_known_positives: Some(FractionFloor {
                numerator: 2,
                denominator: 3,
            }),
            minimum_complete_kit_genuinely_other_fraction_of_expected_routes: Some(FractionFloor {
                numerator: 1,
                denominator: 2,
            }),
            maximum_rooms_with_all_known_complete_kit_routes_run_or_monotone: Some(0),
            require_all_construction_route_gates: true,
            require_all_complete_kit_pickup_gates: true,
            ..ChallengeSanityThresholds::default()
        };
        let audit = audit_challenge_evidence(&[synthetic_room()], thresholds).unwrap();

        assert_eq!(audit.room_count, 1);
        assert_eq!(audit.route_loadout_cells.len(), 8);
        let counts = audit.all_loadouts.denominators;
        assert_eq!(counts.expected_cells, 8);
        assert_eq!(counts.known_positive_cells, 4);
        assert_eq!(counts.known_run_only_cells, 1);
        assert_eq!(counts.known_monotone_simple_only_cells, 2);
        assert_eq!(counts.known_genuinely_other_cells, 1);
        assert_eq!(
            counts.known_run_only_cells
                + counts.known_monotone_simple_only_cells
                + counts.known_genuinely_other_cells,
            counts.known_positive_cells
        );
        assert_eq!(counts.no_positive_in_complete_finite_vocabulary, 1);
        assert_eq!(counts.bounded_without_positive, 1);
        assert_eq!(counts.missing, 2);
        assert_eq!(
            counts.known_positive_cells
                + counts.no_positive_in_complete_finite_vocabulary
                + counts.bounded_without_positive
                + counts.missing,
            counts.expected_cells
        );

        let complete = &audit.complete_kit_rooms[0];
        assert_eq!(
            complete.known_route_composition,
            CompleteKitKnownRouteComposition::MixedRunOrMonotoneAndGenuinelyOther
        );
        assert_eq!(
            complete.evidence_state,
            RoomControllerEvidenceState::CompleteFiniteVocabulary
        );
        assert_eq!(audit.directional_asymmetry.comparable_pair_cells, 1);
        let both_pair = audit
            .directional_pairs
            .iter()
            .find(|pair| pair.loadout == EvaluationLoadout::Both)
            .unwrap();
        let DirectionalPairEvidence::Comparable { differences } = both_pair.evidence else {
            panic!("Both should have two exact positive directions");
        };
        assert_eq!(differences.horizontal_reversals.absolute_difference, 2);
        assert_eq!(differences.duration_ticks.absolute_difference, 85);
        assert!(differences.wall_jump_use_differs);

        assert_eq!(audit.diagnostics.len(), 6);
        assert_eq!(audit.threshold_deficits.len(), 4);
        assert!(audit.threshold_deficits.iter().any(|deficit| {
            deficit == "complete-kit genuinely-other known routes: observed 1, configured minimum 2"
        }));
        assert!(audit.incomplete_evidence.iter().any(|line| {
            line.contains("bounded by SimulatedTicks")
                && line.contains("not an unreachability claim")
        }));
        assert!(
            audit
                .incomplete_evidence
                .iter()
                .any(|line| line.contains("complete-kit pickups hard gate is missing"))
        );
    }

    #[test]
    fn ordering_text_and_serde_round_trip_are_deterministic() {
        let mut second = synthetic_room();
        second.room_id = "room-a".to_owned();
        let first_order = audit_challenge_evidence(
            &[synthetic_room(), second.clone()],
            ChallengeSanityThresholds::default(),
        )
        .unwrap();
        let second_order = audit_challenge_evidence(
            &[second, synthetic_room()],
            ChallengeSanityThresholds::default(),
        )
        .unwrap();
        assert_eq!(first_order, second_order);
        assert_eq!(
            render_challenge_sanity_text(&first_order),
            render_challenge_sanity_text(&second_order)
        );
        let json = serde_json::to_string_pretty(&first_order).unwrap();
        let reparsed: ChallengeSanityAudit = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed, first_order);
    }

    #[test]
    fn zero_known_positive_fraction_is_unavailable_not_zero() {
        let mut room = synthetic_room();
        for cell in &mut room.route_loadout_cells {
            if cell.loadout == EvaluationLoadout::Both {
                cell.evidence = ChallengeCellEvidence::NoPositiveInCompleteFiniteVocabulary;
            }
        }
        let audit = audit_challenge_evidence(
            &[room],
            ChallengeSanityThresholds {
                minimum_complete_kit_genuinely_other_fraction_of_known_positives: Some(
                    FractionFloor {
                        numerator: 1,
                        denominator: 4,
                    },
                ),
                ..ChallengeSanityThresholds::default()
            },
        )
        .unwrap();
        assert_eq!(
            audit.diagnostics[0].outcome,
            PilotDiagnosticOutcome::Unavailable
        );
        assert_eq!(
            audit.threshold_deficits,
            vec![audit.diagnostics[0].message.clone()]
        );
        assert_eq!(
            audit.complete_kit_rooms[0].known_route_composition,
            CompleteKitKnownRouteComposition::NoKnownPositive
        );
    }

    #[test]
    fn real_generated_evaluated_and_analyzed_room_retains_every_expected_cell() {
        let mut generated =
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        generated.rooms.truncate(1);
        let evaluated = evaluate_route_matrices(generated).unwrap();
        let analysis = analyze_corpus_room(
            &evaluated.rooms[0],
            &CorpusRoomAnalysisConfig {
                direct_controller_solver: SolverConfig {
                    max_expanded_nodes: 10_000,
                    max_simulated_ticks: 2_000_000,
                    max_ticks_per_path: 240,
                    ..SolverConfig::default()
                },
                canonical_witness_difficulty: DifficultyConfig::default(),
            },
        )
        .unwrap();
        let audit = audit_challenge_sanity(
            &evaluated,
            &[analysis],
            ChallengeSanityThresholds::default(),
        )
        .unwrap();

        let door_count = evaluated.rooms[0].generated.variants[0]
            .generated
            .room
            .doors()
            .len();
        let directed_routes = door_count * door_count.saturating_sub(1);
        assert_eq!(audit.room_count, 1);
        assert_eq!(
            audit.route_loadout_cells.len(),
            directed_routes * EvaluationLoadout::ALL.len()
        );
        let counts = audit.all_loadouts.denominators;
        assert_eq!(counts.expected_cells, audit.route_loadout_cells.len());
        assert_eq!(
            counts.known_positive_cells
                + counts.no_positive_in_complete_finite_vocabulary
                + counts.bounded_without_positive
                + counts.missing,
            counts.expected_cells
        );
        assert_eq!(
            counts.known_run_only_cells
                + counts.known_monotone_simple_only_cells
                + counts.known_genuinely_other_cells,
            counts.known_positive_cells
        );
        assert_eq!(counts.missing, 0);
        assert_eq!(audit.complete_kit_rooms.len(), 1);
        assert_eq!(
            audit.directional_pairs.len(),
            door_count * door_count.saturating_sub(1) / 2 * EvaluationLoadout::ALL.len()
        );
        assert_eq!(audit.threshold_deficits.len(), 0);
        assert_eq!(audit.diagnostics.len(), 0);
        let complete_kit_matrix = evaluated.rooms[0]
            .matrices
            .iter()
            .find(|matrix| matrix.loadout == EvaluationLoadout::Both)
            .unwrap();
        assert_eq!(
            audit.hard_gates.complete_kit_routes.pass_count,
            usize::from(complete_kit_matrix.summary.inconclusive_door_rows == 0)
        );
        assert!(render_challenge_sanity_text(&audit).starts_with("challenge-sanity-audit-v1"));
    }
}
