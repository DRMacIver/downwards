//! Deterministic shaky-hand evidence for easiest-known exact controllers.
//!
//! This layer is deliberately downstream of [`CorpusRoomAnalysis`]. For each
//! directed door pair and exact physics loadout it consumes the versioned
//! direct + canonical fusion cell and replays that cell's deterministic
//! representative. If the nondominated front is ambiguous, that fact remains
//! explicit and the representative carries no ease claim. A witness from
//! another loadout is never replayed under different physics, and shaky-hand
//! outcomes never participate in the upstream selection.
//!
//! Bounded direct-controller non-success remains bounded evidence. The result
//! therefore retains complete no-positive, bounded no-positive, and missing
//! analysis states separately rather than turning any of them into a numeric
//! robustness value.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::{error::Error, fmt};

use downwards_ai::{
    DirectProbeBudgetLimit, NoiseFamily, ReachedTarget, SHAKY_HAND_CONFIG_VERSION,
    SHAKY_HAND_EVIDENCE_DISCLAIMER, SHAKY_HAND_POLICY_VERSION, SearchStats, SearchTarget,
    ShakyHandConfig, ShakyHandError, TargetSolution, evaluate_shaky_hand,
};
use downwards_core::{Action, DoorEntryError, Simulation};
use serde::{Deserialize, Serialize};

use super::{
    CORPUS_ROOM_ANALYSIS_VERSION, ControllerDemandCoordinates, CorpusCandidate,
    CorpusMetricInputV2Error, CorpusRoomAnalysis, EasiestKnownRouteSelectionStatus,
    EvaluatedCorpusRoom, EvaluatedCorpusRoomV2, EvaluationLoadout, FusedDirectAuditStatus,
    FusedRouteCandidateProvenance, FusedRouteCellAssessment, LoadoutControllerAuditStatus, RoomId,
    RouteControllerAssessment, resolve_corpus_metric_candidate_v2,
};

/// Version of the corpus-layer grid, selection, and curve-compaction policy.
pub const CORPUS_SHAKY_HAND_ANALYSIS_VERSION: u32 = 2;

/// Version of the stable route-derived seed encoding.
pub const SHAKY_HAND_ROUTE_SEED_VERSION: u32 = 2;

/// Interpretation boundary for persisted corpus shaky-hand evidence.
pub const CORPUS_SHAKY_HAND_DISCLAIMER: &str = "shaky-hand curves are deterministic blind-continuation controller diagnostics, not calibrated human success probabilities; bounded non-success is not proof that a target is unreachable";

/// Shared settings for a corpus shaky-hand batch.
///
/// `base_seed` is namespaced independently for every exact
/// `(room, source, target, loadout, replay)` identity. The resulting route
/// seed is stored in every positive cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusShakyHandConfig {
    pub base_seed: u64,
    pub trials_per_curve_point: usize,
    pub grace_ticks: usize,
    pub correlated_boundaries: usize,
    pub convergence_confirmation_ticks: usize,
}

impl Default for CorpusShakyHandConfig {
    fn default() -> Self {
        let config = ShakyHandConfig::default();
        Self {
            base_seed: config.seed,
            trials_per_curve_point: config.trials_per_curve_point,
            grace_ticks: config.grace_ticks,
            correlated_boundaries: config.correlated_boundaries,
            convergence_confirmation_ticks: config.convergence_confirmation_ticks,
        }
    }
}

impl CorpusShakyHandConfig {
    fn validate(self) -> Result<(), CorpusShakyHandConfigError> {
        if self.trials_per_curve_point == 0 {
            return Err(CorpusShakyHandConfigError::ZeroTrialsPerCurvePoint);
        }
        if self.correlated_boundaries < 2 {
            return Err(CorpusShakyHandConfigError::CorrelatedBoundariesLessThanTwo);
        }
        if self.convergence_confirmation_ticks == 0 {
            return Err(CorpusShakyHandConfigError::ZeroConvergenceConfirmationTicks);
        }
        Ok(())
    }

    const fn for_route(self, seed: u64) -> ShakyHandConfig {
        ShakyHandConfig {
            seed,
            trials_per_curve_point: self.trials_per_curve_point,
            grace_ticks: self.grace_ticks,
            correlated_boundaries: self.correlated_boundaries,
            convergence_confirmation_ticks: self.convergence_confirmation_ticks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusShakyHandConfigError {
    ZeroTrialsPerCurvePoint,
    CorrelatedBoundariesLessThanTwo,
    ZeroConvergenceConfirmationTicks,
}

impl fmt::Display for CorpusShakyHandConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroTrialsPerCurvePoint => "trials_per_curve_point must be greater than zero",
            Self::CorrelatedBoundariesLessThanTwo => "correlated_boundaries must be at least two",
            Self::ZeroConvergenceConfirmationTicks => {
                "convergence_confirmation_ticks must be greater than zero"
            }
        })
    }
}

impl Error for CorpusShakyHandConfigError {}

/// Complete directed/loadout shaky-hand grid for one room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusShakyHandAnalysis {
    pub version: u32,
    pub source_analysis_version: u32,
    pub seed_derivation_version: u32,
    pub ai_policy_version: u32,
    pub ai_config_version: u32,
    pub room_id: RoomId,
    pub config: CorpusShakyHandConfig,
    pub evidence_disclaimer: String,
    pub cells: Vec<DirectedLoadoutShakyHandCell>,
}

/// One exact `(source door, target door, physics loadout)` cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectedLoadoutShakyHandCell {
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub evidence: CorpusShakyHandEvidence,
}

/// Positive evidence and every distinct reason it may be unavailable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CorpusShakyHandEvidence {
    Observed(CorpusShakyHandObservation),
    /// The configured finite direct-controller vocabulary was exhausted and
    /// yielded no positive for this exact loadout. This is not a general
    /// unreachability claim.
    NoPositiveCompleteFiniteVocabulary,
    /// The finite-vocabulary audit hit a bound before yielding a positive.
    NoPositiveBoundedIncomplete {
        limit: CorpusProbeBudgetLimit,
    },
    /// The supplied analysis lacks the row needed to decide this cell.
    Missing {
        reason: CorpusShakyHandMissingReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusProbeBudgetLimit {
    ExpandedNodes,
    SimulatedTicks,
}

impl From<DirectProbeBudgetLimit> for CorpusProbeBudgetLimit {
    fn from(value: DirectProbeBudgetLimit) -> Self {
        match value {
            DirectProbeBudgetLimit::ExpandedNodes => Self::ExpandedNodes,
            DirectProbeBudgetLimit::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusShakyHandMissingReason {
    MissingSourceAssessment,
    MissingDirectedRouteAssessment,
    MissingLoadoutAudit,
    MissingFusedRouteCell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PositiveControllerAuditStatus {
    CompleteFiniteVocabulary,
    BoundedIncomplete { limit: CorpusProbeBudgetLimit },
}

/// Compact positive evidence suitable for persistence and QD projections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusShakyHandObservation {
    /// Index into the exact cell's deterministic fused candidate set.
    pub selected_candidate_index: usize,
    pub selected_provenance: Vec<CorpusSelectedCandidateProvenance>,
    pub nondominated_front_size: usize,
    pub representative_status: CorpusRouteRepresentativeStatus,
    pub selected_witness_demand: ControllerDemandCoordinatesRecord,
    pub audit_status: PositiveControllerAuditStatus,
    pub identity: CorpusShakyHandStudyIdentity,
    pub exact_control_succeeded: bool,
    pub curves: Vec<CompactShakyHandCurve>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CorpusRouteRepresentativeStatus {
    UniqueNondominatedCandidate,
    AmbiguousNondominatedFront { front_size: usize },
}

impl From<EasiestKnownRouteSelectionStatus> for CorpusRouteRepresentativeStatus {
    fn from(value: EasiestKnownRouteSelectionStatus) -> Self {
        match value {
            EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate => {
                Self::UniqueNondominatedCandidate
            }
            EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { front_size } => {
                Self::AmbiguousNondominatedFront { front_size }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CorpusSelectedCandidateProvenance {
    DirectWitness { witness_index: usize },
    CanonicalMatrix { witness_fingerprint: String },
}

/// Serialization-native controller coordinates of the exact representative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerDemandCoordinatesRecord {
    pub controller_class: u8,
    pub ability_events: usize,
    pub horizontal_reversals: usize,
    pub vertical_decisions: usize,
    pub semantic_spans: usize,
    pub semantic_transitions: usize,
    pub duration_ticks: usize,
}

impl From<ControllerDemandCoordinates> for ControllerDemandCoordinatesRecord {
    fn from(value: ControllerDemandCoordinates) -> Self {
        Self {
            controller_class: value.controller_class,
            ability_events: value.ability_events,
            horizontal_reversals: value.horizontal_reversals,
            vertical_decisions: value.vertical_decisions,
            semantic_spans: value.semantic_spans,
            semantic_transitions: value.semantic_transitions,
            duration_ticks: value.duration_ticks,
        }
    }
}

/// Stable identity of the exact replay and study settings used by one cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusShakyHandStudyIdentity {
    pub corpus_analysis_version: u32,
    pub seed_derivation_version: u32,
    pub ai_policy_version: u32,
    pub ai_config_version: u32,
    pub ai_config_digest: u64,
    pub route_seed: u64,
    pub replay_fingerprint: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusNoiseFamily {
    Exact,
    BoundaryTiming,
    CorrelatedTiming,
    HoldRelease,
    DropRepeatFrame,
}

impl From<NoiseFamily> for CorpusNoiseFamily {
    fn from(value: NoiseFamily) -> Self {
        match value {
            NoiseFamily::Exact => Self::Exact,
            NoiseFamily::BoundaryTiming => Self::BoundaryTiming,
            NoiseFamily::CorrelatedTiming => Self::CorrelatedTiming,
            NoiseFamily::HoldRelease => Self::HoldRelease,
            NoiseFamily::DropRepeatFrame => Self::DropRepeatFrame,
        }
    }
}

/// Integer-only curve summary. No family is collapsed into a scalar score.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactShakyHandCurve {
    pub family: CorpusNoiseFamily,
    pub strength_ticks: u8,
    pub requested_trials: usize,
    pub applicable_trials: usize,
    pub not_applicable_trials: usize,
    pub successes: usize,
    pub death_events: u64,
    pub trials_with_death: usize,
    pub successes_after_death: usize,
    pub wrong_target_outcomes: usize,
    pub other_door_outcomes: usize,
    pub other_exit_outcomes: usize,
    pub timeouts: usize,
    pub divergent_successes: usize,
    pub exact_state_convergences: usize,
}

/// Build the full directed-door × exact-loadout shaky-hand grid.
///
/// The generated room embedded in `evaluated` is authoritative. The analysis
/// is allowed to be structurally incomplete so missing rows remain explicit,
/// but duplicate or internally inconsistent rows are rejected.
pub fn assess_corpus_shaky_hand(
    analysis: &CorpusRoomAnalysis,
    evaluated: &EvaluatedCorpusRoom,
    config: CorpusShakyHandConfig,
) -> Result<CorpusShakyHandAnalysis, CorpusShakyHandAnalysisError> {
    let Some(canonical) = evaluated.generated.variants.first() else {
        return Err(CorpusShakyHandAnalysisError::MissingCanonicalVariant {
            room_id: evaluated.generated.id.clone(),
        });
    };
    assess_corpus_shaky_hand_common(
        analysis,
        &evaluated.generated.id,
        &canonical.generated.room,
        config,
    )
}

/// Assess shaky-hand robustness for a final-path room using its validated
/// post-feasibility canonical native candidate.
pub fn assess_corpus_shaky_hand_v2(
    analysis: &CorpusRoomAnalysis,
    evaluated: &EvaluatedCorpusRoomV2,
    config: CorpusShakyHandConfig,
) -> Result<CorpusShakyHandAnalysis, CorpusShakyHandAnalysisError> {
    let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        CorpusShakyHandAnalysisError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    assess_corpus_shaky_hand_common(
        analysis,
        &evaluated.generated.id,
        &candidate.generated().room,
        config,
    )
}

/// Explicit-candidate final-path entry point. The supplied candidate must be
/// exactly the validated retained canonical candidate.
pub fn assess_corpus_shaky_hand_for_corpus_candidate(
    analysis: &CorpusRoomAnalysis,
    candidate: &CorpusCandidate,
    evaluated: &EvaluatedCorpusRoomV2,
    config: CorpusShakyHandConfig,
) -> Result<CorpusShakyHandAnalysis, CorpusShakyHandAnalysisError> {
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        CorpusShakyHandAnalysisError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    if candidate != canonical {
        return Err(CorpusShakyHandAnalysisError::CorpusCandidateMismatch {
            room_id: evaluated.generated.id.clone(),
        });
    }
    assess_corpus_shaky_hand_common(
        analysis,
        &evaluated.generated.id,
        &candidate.generated().room,
        config,
    )
}

fn assess_corpus_shaky_hand_common(
    analysis: &CorpusRoomAnalysis,
    room_id: &RoomId,
    room: &downwards_core::Room,
    config: CorpusShakyHandConfig,
) -> Result<CorpusShakyHandAnalysis, CorpusShakyHandAnalysisError> {
    config
        .validate()
        .map_err(CorpusShakyHandAnalysisError::InvalidConfig)?;
    if analysis.version != CORPUS_ROOM_ANALYSIS_VERSION {
        return Err(CorpusShakyHandAnalysisError::AnalysisVersion {
            actual: analysis.version,
            expected: CORPUS_ROOM_ANALYSIS_VERSION,
        });
    }
    if analysis.room_id != *room_id {
        return Err(CorpusShakyHandAnalysisError::RoomIdentity {
            analysis: analysis.room_id.clone(),
            evaluated: room_id.clone(),
        });
    }

    let mut door_ids = room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    door_ids.sort_unstable();
    if let Some(door_id) = first_duplicate(&door_ids) {
        return Err(CorpusShakyHandAnalysisError::DuplicateDoorId {
            door_id: door_id.to_owned(),
        });
    }

    let source_batches = index_source_batches(analysis, &door_ids)?;
    let fused_cells = index_fused_cells(analysis, &door_ids)?;
    let mut cells = Vec::with_capacity(
        door_ids
            .len()
            .saturating_mul(door_ids.len().saturating_sub(1))
            .saturating_mul(EvaluationLoadout::ALL.len()),
    );
    for source_door_id in &door_ids {
        for target_door_id in door_ids
            .iter()
            .filter(|target_door_id| *target_door_id != source_door_id)
        {
            let route = source_batches
                .get(source_door_id)
                .and_then(|routes| routes.get(target_door_id));
            for loadout in EvaluationLoadout::ALL {
                let evidence = match route {
                    None if !source_batches.contains_key(source_door_id) => {
                        CorpusShakyHandEvidence::Missing {
                            reason: CorpusShakyHandMissingReason::MissingSourceAssessment,
                        }
                    }
                    None => CorpusShakyHandEvidence::Missing {
                        reason: CorpusShakyHandMissingReason::MissingDirectedRouteAssessment,
                    },
                    Some(route) => assess_loadout(
                        room_id,
                        room,
                        route,
                        loadout,
                        fused_cells
                            .get(&(source_door_id.clone(), target_door_id.clone(), loadout))
                            .copied(),
                        config,
                    )?,
                };
                cells.push(DirectedLoadoutShakyHandCell {
                    source_door_id: source_door_id.clone(),
                    target_door_id: target_door_id.clone(),
                    loadout,
                    evidence,
                });
            }
        }
    }

    Ok(CorpusShakyHandAnalysis {
        version: CORPUS_SHAKY_HAND_ANALYSIS_VERSION,
        source_analysis_version: analysis.version,
        seed_derivation_version: SHAKY_HAND_ROUTE_SEED_VERSION,
        ai_policy_version: SHAKY_HAND_POLICY_VERSION,
        ai_config_version: SHAKY_HAND_CONFIG_VERSION,
        room_id: analysis.room_id.clone(),
        config,
        evidence_disclaimer: format!(
            "{CORPUS_SHAKY_HAND_DISCLAIMER}; {SHAKY_HAND_EVIDENCE_DISCLAIMER}"
        ),
        cells,
    })
}

type FusedCellKey = (String, String, EvaluationLoadout);

fn index_fused_cells<'a>(
    analysis: &'a CorpusRoomAnalysis,
    expected_doors: &[String],
) -> Result<BTreeMap<FusedCellKey, &'a FusedRouteCellAssessment>, CorpusShakyHandAnalysisError> {
    let expected_doors = expected_doors
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut indexed = BTreeMap::new();
    for cell in &analysis.fused_route_cells {
        if !expected_doors.contains(cell.source_door_id.as_str()) {
            return Err(CorpusShakyHandAnalysisError::UnknownDoorInAnalysis {
                door_id: cell.source_door_id.clone(),
            });
        }
        if !expected_doors.contains(cell.target_door_id.as_str()) {
            return Err(CorpusShakyHandAnalysisError::UnknownDoorInAnalysis {
                door_id: cell.target_door_id.clone(),
            });
        }
        if cell.source_door_id == cell.target_door_id {
            return Err(CorpusShakyHandAnalysisError::SameSourceAndTarget {
                door_id: cell.source_door_id.clone(),
            });
        }
        let key = (
            cell.source_door_id.clone(),
            cell.target_door_id.clone(),
            cell.loadout,
        );
        if indexed.insert(key.clone(), cell).is_some() {
            return Err(CorpusShakyHandAnalysisError::DuplicateFusedRouteCell {
                source_door_id: key.0,
                target_door_id: key.1,
                loadout: key.2,
            });
        }
    }
    Ok(indexed)
}

fn index_source_batches<'a>(
    analysis: &'a CorpusRoomAnalysis,
    expected_doors: &[String],
) -> Result<
    BTreeMap<String, BTreeMap<String, &'a RouteControllerAssessment>>,
    CorpusShakyHandAnalysisError,
> {
    let expected_doors = expected_doors
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut indexed = BTreeMap::new();
    for batch in &analysis.source_route_assessments {
        if !expected_doors.contains(batch.source_door_id.as_str()) {
            return Err(CorpusShakyHandAnalysisError::UnknownDoorInAnalysis {
                door_id: batch.source_door_id.clone(),
            });
        }
        if indexed.contains_key(&batch.source_door_id) {
            return Err(CorpusShakyHandAnalysisError::DuplicateSourceAssessment {
                source_door_id: batch.source_door_id.clone(),
            });
        }
        let mut routes = BTreeMap::new();
        for route in &batch.routes {
            if route.source_door_id != batch.source_door_id {
                return Err(CorpusShakyHandAnalysisError::RouteSourceMismatch {
                    batch_source_door_id: batch.source_door_id.clone(),
                    route_source_door_id: route.source_door_id.clone(),
                    target_door_id: route.target_door_id.clone(),
                });
            }
            if !expected_doors.contains(route.target_door_id.as_str()) {
                return Err(CorpusShakyHandAnalysisError::UnknownDoorInAnalysis {
                    door_id: route.target_door_id.clone(),
                });
            }
            if route.source_door_id == route.target_door_id {
                return Err(CorpusShakyHandAnalysisError::SameSourceAndTarget {
                    door_id: route.source_door_id.clone(),
                });
            }
            if routes.insert(route.target_door_id.clone(), route).is_some() {
                return Err(CorpusShakyHandAnalysisError::DuplicateDirectedRoute {
                    source_door_id: route.source_door_id.clone(),
                    target_door_id: route.target_door_id.clone(),
                });
            }
        }
        indexed.insert(batch.source_door_id.clone(), routes);
    }
    Ok(indexed)
}

fn assess_loadout(
    room_id: &RoomId,
    room: &downwards_core::Room,
    route: &RouteControllerAssessment,
    loadout: EvaluationLoadout,
    fused: Option<&FusedRouteCellAssessment>,
    config: CorpusShakyHandConfig,
) -> Result<CorpusShakyHandEvidence, CorpusShakyHandAnalysisError> {
    let matching_audits = route
        .audits
        .iter()
        .filter(|audit| audit.loadout == loadout)
        .collect::<Vec<_>>();
    let Some(audit) = matching_audits.first() else {
        return Ok(CorpusShakyHandEvidence::Missing {
            reason: CorpusShakyHandMissingReason::MissingLoadoutAudit,
        });
    };
    if matching_audits.len() != 1 {
        return Err(CorpusShakyHandAnalysisError::DuplicateLoadoutAudit {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
        });
    }

    let exact_witnesses = route
        .easiest_first_witnesses
        .iter()
        .enumerate()
        .filter(|(_, witness)| witness.loadout == loadout)
        .collect::<Vec<_>>();
    if exact_witnesses.len() != audit.retained_semantic_witnesses {
        return Err(CorpusShakyHandAnalysisError::RetainedWitnessCountMismatch {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
            audit_count: audit.retained_semantic_witnesses,
            actual_count: exact_witnesses.len(),
        });
    }
    let audit_status = match audit.status {
        LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
            PositiveControllerAuditStatus::CompleteFiniteVocabulary
        }
        LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
            PositiveControllerAuditStatus::BoundedIncomplete {
                limit: limit.into(),
            }
        }
    };
    let Some(fused) = fused else {
        return Ok(CorpusShakyHandEvidence::Missing {
            reason: CorpusShakyHandMissingReason::MissingFusedRouteCell,
        });
    };
    let expected_fused_status = match audit.status {
        LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
            FusedDirectAuditStatus::CompleteFiniteVocabulary
        }
        LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
            FusedDirectAuditStatus::BoundedIncomplete { limit }
        }
    };
    if fused.source_door_id != route.source_door_id
        || fused.target_door_id != route.target_door_id
        || fused.loadout != loadout
        || fused.direct_audit_status != expected_fused_status
        || fused.raw_direct_positive_witnesses != audit.raw_positive_witnesses
        || fused.retained_direct_positive_witnesses != audit.retained_semantic_witnesses
    {
        return Err(CorpusShakyHandAnalysisError::FusedRouteCellMismatch {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
        });
    }
    let Some(selection) = fused.selected else {
        return Ok(match audit_status {
            PositiveControllerAuditStatus::CompleteFiniteVocabulary => {
                CorpusShakyHandEvidence::NoPositiveCompleteFiniteVocabulary
            }
            PositiveControllerAuditStatus::BoundedIncomplete { limit } => {
                CorpusShakyHandEvidence::NoPositiveBoundedIncomplete { limit }
            }
        });
    };
    let Some(candidate) = fused.candidates.get(selection.candidate_index) else {
        return Err(CorpusShakyHandAnalysisError::FusedRouteCellMismatch {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
        });
    };
    let selection_status_matches = match selection.status {
        EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate => {
            fused.nondominated_front.len() == 1
        }
        EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { front_size } => {
            front_size > 1 && fused.nondominated_front.len() == front_size
        }
    };
    if candidate.replay_identity != selection.replay_identity
        || fused.nondominated_front.first().copied() != Some(selection.candidate_index)
        || !selection_status_matches
    {
        return Err(CorpusShakyHandAnalysisError::FusedRouteCellMismatch {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
        });
    }

    let route_seed = route_seed(
        config.base_seed,
        room_id,
        &route.source_door_id,
        &route.target_door_id,
        loadout,
        &candidate.replay,
    );
    let initial =
        Simulation::enter_via_door(room.clone(), loadout.abilities(), &route.source_door_id)
            .map_err(|source| CorpusShakyHandAnalysisError::DoorEntry {
                source_door_id: route.source_door_id.clone(),
                target_door_id: route.target_door_id.clone(),
                loadout,
                source: Box::new(source),
            })?;
    let solution = TargetSolution {
        target: SearchTarget::door(&route.target_door_id),
        reached: ReachedTarget::Door(route.target_door_id.clone()),
        replay: candidate.replay.clone(),
        stats: SearchStats::default(),
    };
    let report = evaluate_shaky_hand(&initial, &solution, config.for_route(route_seed)).map_err(
        |source| CorpusShakyHandAnalysisError::ShakyHand {
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            loadout,
            source: Box::new(source),
        },
    )?;
    let identity = CorpusShakyHandStudyIdentity {
        corpus_analysis_version: CORPUS_SHAKY_HAND_ANALYSIS_VERSION,
        seed_derivation_version: SHAKY_HAND_ROUTE_SEED_VERSION,
        ai_policy_version: report.study.identity.policy_version,
        ai_config_version: report.study.identity.config_version,
        ai_config_digest: report.study.identity.config_digest,
        route_seed: report.study.identity.seed,
        replay_fingerprint: report.study.replay_fingerprint,
    };
    let curves = report
        .curves
        .into_iter()
        .map(|curve| CompactShakyHandCurve {
            family: curve.family.into(),
            strength_ticks: curve.strength_ticks,
            requested_trials: curve.requested_trials,
            applicable_trials: curve.trials,
            not_applicable_trials: curve.not_applicable_trials,
            successes: curve.successes,
            death_events: curve.death_events,
            trials_with_death: curve.trials_with_death,
            successes_after_death: curve.successes_after_death,
            wrong_target_outcomes: curve.wrong_target_outcomes,
            other_door_outcomes: curve.other_door_outcomes,
            other_exit_outcomes: curve.other_exit_outcomes,
            timeouts: curve.timeouts,
            divergent_successes: curve.divergent_successes,
            exact_state_convergences: curve.exact_state_convergences,
        })
        .collect();
    Ok(CorpusShakyHandEvidence::Observed(
        CorpusShakyHandObservation {
            selected_candidate_index: selection.candidate_index,
            selected_provenance: candidate
                .provenance
                .iter()
                .map(|provenance| match provenance {
                    FusedRouteCandidateProvenance::DirectWitness { witness_index } => {
                        CorpusSelectedCandidateProvenance::DirectWitness {
                            witness_index: *witness_index,
                        }
                    }
                    FusedRouteCandidateProvenance::CanonicalMatrix {
                        witness_fingerprint,
                    } => CorpusSelectedCandidateProvenance::CanonicalMatrix {
                        witness_fingerprint: witness_fingerprint.to_string(),
                    },
                })
                .collect(),
            nondominated_front_size: fused.nondominated_front.len(),
            representative_status: selection.status.into(),
            selected_witness_demand: candidate.controller_coordinates.into(),
            audit_status,
            identity,
            exact_control_succeeded: report.exact_control_succeeded,
            curves,
        },
    ))
}

/// Compare success fractions only when curve identity and sampling
/// denominators are exactly comparable. `Greater` means `left` was more
/// robust. Death and failure diagnostics remain independent coordinates and
/// do not break ties here.
#[must_use]
pub fn compare_comparable_curve_robustness(
    left: &CompactShakyHandCurve,
    right: &CompactShakyHandCurve,
) -> Option<Ordering> {
    if left.family != right.family
        || left.strength_ticks != right.strength_ticks
        || left.requested_trials != right.requested_trials
        || left.applicable_trials == 0
        || left.applicable_trials != right.applicable_trials
        || left.not_applicable_trials != right.not_applicable_trials
    {
        return None;
    }
    let left_scaled = (left.successes as u128) * (right.applicable_trials as u128);
    let right_scaled = (right.successes as u128) * (left.applicable_trials as u128);
    Some(left_scaled.cmp(&right_scaled))
}

/// Pareto-style comparison across the complete family-specific curves.
///
/// Results are comparable only under the same AI policy/config digest and
/// with exactly matching comparable curve keys and denominators. `Greater`
/// means every success fraction is at least as robust and at least one is
/// strictly more robust; mixed directions are incomparable.
#[must_use]
pub fn compare_shaky_hand_robustness(
    left: &CorpusShakyHandObservation,
    right: &CorpusShakyHandObservation,
) -> Option<Ordering> {
    if left.identity.ai_policy_version != right.identity.ai_policy_version
        || left.identity.ai_config_version != right.identity.ai_config_version
        || left.identity.ai_config_digest != right.identity.ai_config_digest
        || left.identity.corpus_analysis_version != right.identity.corpus_analysis_version
        || left.identity.seed_derivation_version != right.identity.seed_derivation_version
        || left.curves.len() != right.curves.len()
    {
        return None;
    }
    let mut direction = Ordering::Equal;
    for (left_curve, right_curve) in left.curves.iter().zip(&right.curves) {
        let comparison = compare_comparable_curve_robustness(left_curve, right_curve)?;
        match (direction, comparison) {
            (Ordering::Equal, ordering) => direction = ordering,
            (Ordering::Less, Ordering::Greater) | (Ordering::Greater, Ordering::Less) => {
                return None;
            }
            _ => {}
        }
    }
    Some(direction)
}

fn route_seed(
    base_seed: u64,
    room_id: &RoomId,
    source_door_id: &str,
    target_door_id: &str,
    loadout: EvaluationLoadout,
    replay: &downwards_ai::Replay,
) -> u64 {
    let mut digest = StableDigest::new(b"downwards-corpus-shaky-hand-route-seed");
    digest.u32(SHAKY_HAND_ROUTE_SEED_VERSION);
    digest.u64(base_seed);
    digest.string(&room_id.0);
    digest.string(source_door_id);
    digest.string(target_door_id);
    digest.byte(loadout_tag(loadout));
    digest.u64(replay.initial_digest.0);
    digest.usize(replay.frames.len());
    for frame in &replay.frames {
        digest.action(frame.action);
        digest.u64(frame.expected_digest.0);
        digest.u64(frame.expected_event_digest.0);
    }
    digest.finish()
}

const fn loadout_tag(loadout: EvaluationLoadout) -> u8 {
    match loadout {
        EvaluationLoadout::Baseline => 0,
        EvaluationLoadout::WallJump => 1,
        EvaluationLoadout::Dash => 2,
        EvaluationLoadout::Both => 3,
    }
}

fn first_duplicate(values: &[String]) -> Option<&str> {
    values
        .windows(2)
        .find(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].as_str())
}

struct StableDigest(u64);

impl StableDigest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new(domain: &[u8]) -> Self {
        let mut digest = Self(Self::OFFSET);
        digest.bytes(domain);
        digest
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        self.usize(values.len());
        for &value in values {
            self.byte(value);
        }
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.raw_bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.raw_bytes(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    fn action(&mut self, action: Action) {
        self.byte(action.move_x as u8);
        self.byte(action.move_y as u8);
        self.byte(u8::from(action.jump));
        self.byte(u8::from(action.dash));
        self.byte(u8::from(action.restart));
    }

    fn raw_bytes(&mut self, values: &[u8]) {
        for &value in values {
            self.byte(value);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub enum CorpusShakyHandAnalysisError {
    CorpusV2Identity {
        source: Box<CorpusMetricInputV2Error>,
    },
    CorpusCandidateMismatch {
        room_id: RoomId,
    },
    InvalidConfig(CorpusShakyHandConfigError),
    AnalysisVersion {
        actual: u32,
        expected: u32,
    },
    RoomIdentity {
        analysis: RoomId,
        evaluated: RoomId,
    },
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    DuplicateDoorId {
        door_id: String,
    },
    UnknownDoorInAnalysis {
        door_id: String,
    },
    SameSourceAndTarget {
        door_id: String,
    },
    DuplicateSourceAssessment {
        source_door_id: String,
    },
    RouteSourceMismatch {
        batch_source_door_id: String,
        route_source_door_id: String,
        target_door_id: String,
    },
    DuplicateDirectedRoute {
        source_door_id: String,
        target_door_id: String,
    },
    DuplicateFusedRouteCell {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    FusedRouteCellMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    DuplicateLoadoutAudit {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
    },
    RetainedWitnessCountMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        audit_count: usize,
        actual_count: usize,
    },
    DoorEntry {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        source: Box<DoorEntryError>,
    },
    ShakyHand {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        source: Box<ShakyHandError>,
    },
}

impl fmt::Display for CorpusShakyHandAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorpusV2Identity { source } => source.fmt(formatter),
            Self::CorpusCandidateMismatch { room_id } => write!(
                formatter,
                "explicit shaky-hand candidate does not equal the validated corpus-v2 canonical candidate for {:?}",
                room_id.0
            ),
            Self::InvalidConfig(source) => write!(formatter, "invalid shaky-hand config: {source}"),
            Self::AnalysisVersion { actual, expected } => write!(
                formatter,
                "room analysis version {actual} is unsupported; expected {expected}"
            ),
            Self::RoomIdentity {
                analysis,
                evaluated,
            } => write!(
                formatter,
                "analysis room {:?} does not match evaluated room {:?}",
                analysis.0, evaluated.0
            ),
            Self::MissingCanonicalVariant { room_id } => {
                write!(formatter, "room {:?} has no canonical variant", room_id.0)
            }
            Self::DuplicateDoorId { door_id } => {
                write!(formatter, "generated room repeats door ID {door_id:?}")
            }
            Self::UnknownDoorInAnalysis { door_id } => {
                write!(
                    formatter,
                    "analysis references unknown room door {door_id:?}"
                )
            }
            Self::SameSourceAndTarget { door_id } => write!(
                formatter,
                "analysis contains a same-source/target route for {door_id:?}"
            ),
            Self::DuplicateSourceAssessment { source_door_id } => write!(
                formatter,
                "analysis repeats source assessment {source_door_id:?}"
            ),
            Self::RouteSourceMismatch {
                batch_source_door_id,
                route_source_door_id,
                target_door_id,
            } => write!(
                formatter,
                "batch source {batch_source_door_id:?} contains route {route_source_door_id:?}->{target_door_id:?}"
            ),
            Self::DuplicateDirectedRoute {
                source_door_id,
                target_door_id,
            } => write!(
                formatter,
                "analysis repeats directed route {source_door_id:?}->{target_door_id:?}"
            ),
            Self::DuplicateFusedRouteCell {
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "analysis repeats fused route cell {source_door_id:?}->{target_door_id:?} under {}",
                loadout.slug()
            ),
            Self::FusedRouteCellMismatch {
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "fused route cell {source_door_id:?}->{target_door_id:?} under {} disagrees with its direct audit or selected candidate",
                loadout.slug()
            ),
            Self::DuplicateLoadoutAudit {
                source_door_id,
                target_door_id,
                loadout,
            } => write!(
                formatter,
                "route {source_door_id:?}->{target_door_id:?} repeats {} audit",
                loadout.slug()
            ),
            Self::RetainedWitnessCountMismatch {
                source_door_id,
                target_door_id,
                loadout,
                audit_count,
                actual_count,
            } => write!(
                formatter,
                "route {source_door_id:?}->{target_door_id:?} {} audit retains {audit_count} witnesses but contains {actual_count}",
                loadout.slug()
            ),
            Self::DoorEntry {
                source_door_id,
                target_door_id,
                loadout,
                source,
            } => write!(
                formatter,
                "cannot enter {source_door_id:?} for {source_door_id:?}->{target_door_id:?} {} shaky-hand replay: {source}",
                loadout.slug()
            ),
            Self::ShakyHand {
                source_door_id,
                target_door_id,
                loadout,
                source,
            } => write!(
                formatter,
                "shaky-hand replay failed for {source_door_id:?}->{target_door_id:?} {}: {source}",
                loadout.slug()
            ),
        }
    }
}

impl Error for CorpusShakyHandAnalysisError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CorpusV2Identity { source } => Some(source.as_ref()),
            Self::InvalidConfig(source) => Some(source),
            Self::DoorEntry { source, .. } => Some(source.as_ref()),
            Self::ShakyHand { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use downwards_ai::{DifficultyConfig, SolverConfig};

    use super::*;
    use crate::corpus::{
        CorpusBuildConfigV1, CorpusRoomAnalysisConfig, analyze_corpus_room,
        evaluate_route_matrices, generate_seed_block,
    };

    fn real_two_door_fixture() -> (EvaluatedCorpusRoom, CorpusRoomAnalysis) {
        let mut generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1))
            .expect("seed-zero generation is stable");
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
        let config = CorpusRoomAnalysisConfig {
            direct_controller_solver: SolverConfig {
                max_expanded_nodes: 10_000,
                max_simulated_ticks: 2_000_000,
                max_ticks_per_path: 240,
                ..SolverConfig::default()
            },
            canonical_witness_difficulty: DifficultyConfig::default(),
        };
        let analysis = analyze_corpus_room(&evaluated, &config).expect("real room analyzes");
        (evaluated, analysis)
    }

    fn tiny_config() -> CorpusShakyHandConfig {
        CorpusShakyHandConfig {
            base_seed: 0x1234_5678_9abc_def0,
            trials_per_curve_point: 2,
            grace_ticks: 4,
            correlated_boundaries: 2,
            convergence_confirmation_ticks: 1,
        }
    }

    fn observed_cells(
        report: &CorpusShakyHandAnalysis,
    ) -> impl Iterator<Item = &DirectedLoadoutShakyHandCell> {
        report
            .cells
            .iter()
            .filter(|cell| matches!(cell.evidence, CorpusShakyHandEvidence::Observed(_)))
    }

    #[test]
    fn batch_replays_exact_source_target_and_loadout_repeatably() {
        let (evaluated, analysis) = real_two_door_fixture();
        let first = assess_corpus_shaky_hand(&analysis, &evaluated, tiny_config()).unwrap();
        let second = assess_corpus_shaky_hand(&analysis, &evaluated, tiny_config()).unwrap();
        assert_eq!(first, second);
        let persisted = serde_json::to_vec(&first).unwrap();
        let restored: CorpusShakyHandAnalysis = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(restored, first);
        assert_eq!(first.cells.len(), 2 * EvaluationLoadout::ALL.len());

        let observed = observed_cells(&first).collect::<Vec<_>>();
        assert!(!observed.is_empty());
        let mut route_seeds = BTreeSet::new();
        for cell in observed {
            let CorpusShakyHandEvidence::Observed(observation) = &cell.evidence else {
                unreachable!()
            };
            assert!(observation.exact_control_succeeded);
            let exact = observation
                .curves
                .iter()
                .find(|curve| curve.family == CorpusNoiseFamily::Exact)
                .expect("every study has an exact control");
            assert_eq!((exact.successes, exact.applicable_trials), (1, 1));

            let fused = analysis
                .fused_route_cells
                .iter()
                .find(|fused| {
                    fused.source_door_id == cell.source_door_id
                        && fused.target_door_id == cell.target_door_id
                        && fused.loadout == cell.loadout
                })
                .unwrap();
            let selected = &fused.candidates[observation.selected_candidate_index];
            assert_eq!(
                selected.controller_coordinates,
                ControllerDemandCoordinates {
                    controller_class: observation.selected_witness_demand.controller_class,
                    ability_events: observation.selected_witness_demand.ability_events,
                    horizontal_reversals: observation.selected_witness_demand.horizontal_reversals,
                    vertical_decisions: observation.selected_witness_demand.vertical_decisions,
                    semantic_spans: observation.selected_witness_demand.semantic_spans,
                    semantic_transitions: observation.selected_witness_demand.semantic_transitions,
                    duration_ticks: observation.selected_witness_demand.duration_ticks,
                }
            );
            assert!(!observation.selected_provenance.is_empty());
            route_seeds.insert(observation.identity.route_seed);
        }
        assert_eq!(route_seeds.len(), observed_cells(&first).count());
    }

    #[test]
    fn absent_analysis_rows_remain_explicit_missing_evidence() {
        let (evaluated, mut analysis) = real_two_door_fixture();
        analysis.source_route_assessments.clear();
        let report = assess_corpus_shaky_hand(&analysis, &evaluated, tiny_config()).unwrap();
        assert!(report.cells.iter().all(|cell| {
            matches!(
                cell.evidence,
                CorpusShakyHandEvidence::Missing {
                    reason: CorpusShakyHandMissingReason::MissingSourceAssessment
                }
            )
        }));
    }

    #[test]
    fn canonical_fallback_selected_by_fusion_is_the_replay_studied() {
        let (evaluated, mut analysis) = real_two_door_fixture();
        let cell_index = analysis
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
            .expect("real fixture retains a canonical positive");
        let source = analysis.fused_route_cells[cell_index]
            .source_door_id
            .clone();
        let target = analysis.fused_route_cells[cell_index]
            .target_door_id
            .clone();
        let loadout = analysis.fused_route_cells[cell_index].loadout;

        let route = analysis
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
        route
            .easiest_first_witnesses
            .retain(|witness| witness.loadout != loadout);
        let audit = route
            .audits
            .iter_mut()
            .find(|audit| audit.loadout == loadout)
            .unwrap();
        audit.status = LoadoutControllerAuditStatus::CompleteFiniteVocabulary;
        audit.raw_positive_witnesses = 0;
        audit.retained_semantic_witnesses = 0;

        let fused = &mut analysis.fused_route_cells[cell_index];
        fused.direct_audit_status = FusedDirectAuditStatus::CompleteFiniteVocabulary;
        fused.raw_direct_positive_witnesses = 0;
        fused.retained_direct_positive_witnesses = 0;
        fused.candidates.retain_mut(|candidate| {
            candidate.provenance.retain(|source| {
                matches!(
                    source,
                    FusedRouteCandidateProvenance::CanonicalMatrix { .. }
                )
            });
            !candidate.provenance.is_empty()
        });
        assert_eq!(fused.candidates.len(), 1);
        fused.nondominated_front = vec![0];
        fused.ease_evidence.clear();
        fused.selected = Some(crate::corpus::EasiestKnownRouteSelection {
            candidate_index: 0,
            replay_identity: fused.candidates[0].replay_identity,
            status: crate::corpus::EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate,
        });

        let report = assess_corpus_shaky_hand(&analysis, &evaluated, tiny_config()).unwrap();
        let observed = report
            .cells
            .iter()
            .find(|cell| {
                cell.source_door_id == source
                    && cell.target_door_id == target
                    && cell.loadout == loadout
            })
            .and_then(|cell| match &cell.evidence {
                CorpusShakyHandEvidence::Observed(observation) => Some(observation),
                _ => None,
            })
            .expect("canonical fallback remains an observed shaky-hand cell");
        assert!(matches!(
            observed.selected_provenance.as_slice(),
            [CorpusSelectedCandidateProvenance::CanonicalMatrix { .. }]
        ));
    }

    #[test]
    fn complete_and_bounded_no_positive_are_not_collapsed() {
        let (evaluated, mut analysis) = real_two_door_fixture();
        let route = &mut analysis.source_route_assessments[0].routes[0];
        let mut changed = Vec::new();
        for (index, loadout) in [EvaluationLoadout::Baseline, EvaluationLoadout::WallJump]
            .into_iter()
            .enumerate()
        {
            route
                .easiest_first_witnesses
                .retain(|witness| witness.loadout != loadout);
            let audit = route
                .audits
                .iter_mut()
                .find(|audit| audit.loadout == loadout)
                .unwrap();
            audit.retained_semantic_witnesses = 0;
            audit.raw_positive_witnesses = 0;
            audit.status = if index == 0 {
                LoadoutControllerAuditStatus::CompleteFiniteVocabulary
            } else {
                LoadoutControllerAuditStatus::BoundedIncomplete {
                    limit: DirectProbeBudgetLimit::ExpandedNodes,
                }
            };
            changed.push((loadout, audit.status));
        }
        let source = route.source_door_id.clone();
        let target = route.target_door_id.clone();
        for (loadout, status) in &changed {
            let fused = analysis
                .fused_route_cells
                .iter_mut()
                .find(|cell| {
                    cell.source_door_id == source
                        && cell.target_door_id == target
                        && cell.loadout == *loadout
                })
                .unwrap();
            fused.direct_audit_status = (*status).into();
            fused.raw_direct_positive_witnesses = 0;
            fused.retained_direct_positive_witnesses = 0;
            fused.canonical_matrix_status =
                crate::corpus::CanonicalMatrixCellStatus::BoundedInconclusive {
                    reason: downwards_ai::InconclusiveReason::FrontierExhausted,
                };
            fused.candidates.clear();
            fused.nondominated_front.clear();
            fused.ease_evidence.clear();
            fused.selected = None;
        }
        let report = assess_corpus_shaky_hand(&analysis, &evaluated, tiny_config()).unwrap();
        let evidence = |loadout| {
            &report
                .cells
                .iter()
                .find(|cell| {
                    cell.source_door_id == source
                        && cell.target_door_id == target
                        && cell.loadout == loadout
                })
                .unwrap()
                .evidence
        };
        assert!(matches!(
            evidence(EvaluationLoadout::Baseline),
            CorpusShakyHandEvidence::NoPositiveCompleteFiniteVocabulary
        ));
        assert!(matches!(
            evidence(EvaluationLoadout::WallJump),
            CorpusShakyHandEvidence::NoPositiveBoundedIncomplete {
                limit: CorpusProbeBudgetLimit::ExpandedNodes
            }
        ));
        assert_eq!(changed.len(), 2);
    }

    fn curve(successes: usize, trials: usize) -> CompactShakyHandCurve {
        CompactShakyHandCurve {
            family: CorpusNoiseFamily::BoundaryTiming,
            strength_ticks: 2,
            requested_trials: trials,
            applicable_trials: trials,
            not_applicable_trials: 0,
            successes,
            death_events: 0,
            trials_with_death: 0,
            successes_after_death: 0,
            wrong_target_outcomes: 0,
            other_door_outcomes: 0,
            other_exit_outcomes: 0,
            timeouts: trials - successes,
            divergent_successes: 0,
            exact_state_convergences: 0,
        }
    }

    #[test]
    fn robustness_order_exists_only_for_comparable_curves() {
        let robust = curve(7, 8);
        let fragile = curve(2, 8);
        assert_eq!(
            compare_comparable_curve_robustness(&robust, &fragile),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_comparable_curve_robustness(&fragile, &robust),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_comparable_curve_robustness(&robust, &curve(7, 9)),
            None
        );

        let mut different_family = fragile.clone();
        different_family.family = CorpusNoiseFamily::CorrelatedTiming;
        assert_eq!(
            compare_comparable_curve_robustness(&robust, &different_family),
            None
        );

        let observation = |curves| CorpusShakyHandObservation {
            selected_candidate_index: 0,
            selected_provenance: vec![CorpusSelectedCandidateProvenance::DirectWitness {
                witness_index: 0,
            }],
            nondominated_front_size: 1,
            representative_status: CorpusRouteRepresentativeStatus::UniqueNondominatedCandidate,
            selected_witness_demand: ControllerDemandCoordinatesRecord {
                controller_class: 1,
                ability_events: 0,
                horizontal_reversals: 0,
                vertical_decisions: 1,
                semantic_spans: 2,
                semantic_transitions: 1,
                duration_ticks: 30,
            },
            audit_status: PositiveControllerAuditStatus::CompleteFiniteVocabulary,
            identity: CorpusShakyHandStudyIdentity {
                corpus_analysis_version: CORPUS_SHAKY_HAND_ANALYSIS_VERSION,
                seed_derivation_version: SHAKY_HAND_ROUTE_SEED_VERSION,
                ai_policy_version: SHAKY_HAND_POLICY_VERSION,
                ai_config_version: SHAKY_HAND_CONFIG_VERSION,
                ai_config_digest: 17,
                route_seed: 23,
                replay_fingerprint: 29,
            },
            exact_control_succeeded: true,
            curves,
        };
        let robust_observation = observation(vec![curve(8, 8), curve(7, 8)]);
        let fragile_observation = observation(vec![curve(6, 8), curve(2, 8)]);
        assert_eq!(
            compare_shaky_hand_robustness(&robust_observation, &fragile_observation),
            Some(Ordering::Greater)
        );
        let mixed_observation = observation(vec![curve(5, 8), curve(8, 8)]);
        assert_eq!(
            compare_shaky_hand_robustness(&robust_observation, &mixed_observation),
            None
        );
        let mut different_config = fragile_observation.clone();
        different_config.identity.ai_config_digest += 1;
        assert_eq!(
            compare_shaky_hand_robustness(&robust_observation, &different_config),
            None
        );
    }

    /// Manual release-mode smoke benchmark for one real generated room.
    /// Kept ignored so normal correctness runs do not acquire timing noise.
    #[test]
    #[ignore = "run explicitly in release mode"]
    fn release_benchmark_small_real_room() {
        let (evaluated, analysis) = real_two_door_fixture();
        let config = CorpusShakyHandConfig {
            trials_per_curve_point: 16,
            ..tiny_config()
        };
        let started = Instant::now();
        let report = assess_corpus_shaky_hand(&analysis, &evaluated, config).unwrap();
        let elapsed = started.elapsed();
        let observed = observed_cells(&report).count();
        let curves = observed_cells(&report)
            .filter_map(|cell| match &cell.evidence {
                CorpusShakyHandEvidence::Observed(observation) => Some(observation.curves.len()),
                _ => None,
            })
            .sum::<usize>();
        eprintln!(
            "shaky-hand release benchmark: room={} cells={} observed={} curves={} elapsed_ms={}",
            report.room_id.0,
            report.cells.len(),
            observed,
            curves,
            elapsed.as_millis()
        );
        assert!(observed > 0);
        assert!(curves >= observed);
    }
}
