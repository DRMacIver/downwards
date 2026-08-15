//! Generator-neutral fusion of exact positive route witnesses.
//!
//! Direct-controller audits and the canonical route matrix answer different
//! bounded questions.  This module combines their *positive* replays for one
//! exact directed-door/loadout cell without treating either search process as
//! authoritative difficulty evidence.  Every replay is checked again under
//! the declared physics, measured through the same difficulty pipeline, and
//! retained with its discovery provenance.
//!
//! The versioned rule retains the easiest-known nondominated front. A
//! single-replay consumer may use its deterministic representative, but that
//! representative carries no ease claim when the front is ambiguous. Neither
//! a unique front nor an ambiguous one is a global optimum: a bounded direct
//! audit or bounded matrix search can always have missed another positive.

use std::{cmp::Ordering, collections::BTreeMap, error::Error, fmt};

use downwards_ai::{
    DifficultyConfig, DifficultyError, DifficultyReport, InconclusiveReason, ReachedTarget, Replay,
    SearchStats, SearchTarget, Solution,
};
use downwards_core::{Action, DoorEntryError, Simulation};
use downwards_gen::GeneratedLevel;
use downwards_lab::{
    DifficultyComparisonTolerances, LandingPrecisionError, LandingPrecisionReport,
    PERFECT_CONTROL_ROUTE_DIFFICULTY_COMPARISON_VERSION, ROUTE_DIFFICULTY_VECTOR_VERSION,
    RouteDifficultyComparison, RouteDifficultyVector, RouteDifficultyVectorError,
    SuccessfulWitnessObservation, TraversalGrid, WitnessObservationError, analyze_replay_landings,
    compare_perfect_control_route_difficulty, observe_solution, route_difficulty_vector,
};
use downwards_validation::{BoundedTargetEvidence, DoorRouteEvidence, WitnessFingerprint};

use super::route_assessment::controller_demand;
use super::{
    CONTROLLER_DEMAND_POLICY_VERSION, CORPUS_ROUTE_MEASUREMENT_VERSION,
    CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, ControllerDemand, ControllerDemandCoordinates,
    EvaluationLoadout, LoadoutControllerAuditStatus, LoadoutRouteMatrix, RouteControllerAssessment,
};

/// Version of candidate collection, exact replay deduplication, and evidence
/// retained by [`FusedRouteCellAssessment`].
pub const EASIEST_KNOWN_ROUTE_FUSION_VERSION: u32 = 1;

/// Version of the easiest-known selection rule.
///
/// Strict controller-coordinate dominance defines the primary Pareto front.
/// Among candidates with equal controller coordinates, the shared
/// perfect-control-only route-difficulty comparator may remove a clearly
/// harder candidate. Incomparable or equivalent candidates remain on the
/// front. A stable representative is ordered by lexicographic controller
/// coordinates, semantic spans, then complete replay bytes, but that
/// representative carries no ease claim when multiple front members remain.
pub const EASIEST_KNOWN_ROUTE_SELECTION_VERSION: u32 = 1;

/// Version of the compact stable replay identity and collision-safe complete
/// replay ordering used by this module.
pub const EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION: u32 = 1;

pub const EASIEST_KNOWN_ROUTE_DISCLAIMER: &str = "easiest-known selection compares only exact positive candidates retained by the canonical matrix and configured finite direct-controller audit; it is not a global optimum, and bounded non-success is not evidence that a route is unreachable";

/// Exact policy identities required to interpret a fused cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EasiestKnownRouteFusionPolicy {
    pub fusion_version: u32,
    pub selection_version: u32,
    pub replay_identity_version: u32,
    pub controller_demand_version: u32,
    pub route_measurement_version: u32,
    pub route_difficulty_vector_version: u32,
    pub perfect_control_comparison_version: u32,
}

pub const CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY: EasiestKnownRouteFusionPolicy =
    EasiestKnownRouteFusionPolicy {
        fusion_version: EASIEST_KNOWN_ROUTE_FUSION_VERSION,
        selection_version: EASIEST_KNOWN_ROUTE_SELECTION_VERSION,
        replay_identity_version: EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION,
        controller_demand_version: CONTROLLER_DEMAND_POLICY_VERSION,
        route_measurement_version: CORPUS_ROUTE_MEASUREMENT_VERSION,
        route_difficulty_vector_version: ROUTE_DIFFICULTY_VECTOR_VERSION,
        perfect_control_comparison_version: PERFECT_CONTROL_ROUTE_DIFFICULTY_COMPARISON_VERSION,
    };

/// Completeness of the finite direct-controller portion of a cell.
///
/// The canonical matrix contributes at most its one retained positive.  Its
/// bounded search is described independently by [`CanonicalMatrixCellStatus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FusedDirectAuditStatus {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        limit: downwards_ai::DirectProbeBudgetLimit,
    },
}

impl From<LoadoutControllerAuditStatus> for FusedDirectAuditStatus {
    fn from(value: LoadoutControllerAuditStatus) -> Self {
        match value {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                Self::CompleteFiniteVocabulary
            }
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                Self::BoundedIncomplete { limit }
            }
        }
    }
}

/// The matrix evidence for this exact cell.  Solver effort is deliberately
/// omitted: it does not participate in candidate measurement or selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalMatrixCellStatus {
    Positive {
        witness_fingerprint: WitnessFingerprint,
    },
    BoundedInconclusive {
        reason: InconclusiveReason,
    },
}

/// Where an exact candidate replay was found.
///
/// One candidate can carry both variants when the direct audit and canonical
/// matrix retained the same exact successful replay prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FusedRouteCandidateProvenance {
    DirectWitness {
        /// Index into `RouteControllerAssessment::easiest_first_witnesses`.
        witness_index: usize,
    },
    CanonicalMatrix {
        witness_fingerprint: WitnessFingerprint,
    },
}

/// Exact, solver-neutral measurements of one candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedRouteCandidateMeasurement {
    pub observation: SuccessfulWitnessObservation,
    pub landing_precision: LandingPrecisionReport,
    pub exact_difficulty: DifficultyReport,
    pub vector: RouteDifficultyVector,
}

/// One deduplicated exact positive replay.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedRouteCandidate {
    /// Stable, non-cryptographic display/cache identity.  Exact equality and
    /// deduplication also compare the complete replay, so hash collisions do
    /// not collapse candidates.
    pub replay_identity: u64,
    pub replay: Replay,
    pub provenance: Vec<FusedRouteCandidateProvenance>,
    pub controller_demand: ControllerDemand,
    pub controller_coordinates: ControllerDemandCoordinates,
    pub measurement: FusedRouteCandidateMeasurement,
}

/// Controller coordinate result recorded for every deterministic pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerEaseOrdering {
    LeftStrictlyDominates,
    Equal,
    Tradeoff,
    RightStrictlyDominates,
}

/// Transparent pairwise evidence used to construct the front.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusedRouteEaseEvidence {
    pub left_candidate_index: usize,
    pub right_candidate_index: usize,
    pub controller_ordering: ControllerEaseOrdering,
    /// Present only when controller coordinates are equal.  A difficulty
    /// comparison is never allowed to override lower controller demand.
    pub comparable_difficulty: Option<RouteDifficultyComparison>,
}

/// A deterministic representative of one exact cell's nondominated front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EasiestKnownRouteSelection {
    pub candidate_index: usize,
    pub replay_identity: u64,
    pub status: EasiestKnownRouteSelectionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasiestKnownRouteSelectionStatus {
    UniqueNondominatedCandidate,
    /// The chosen replay is only a stable single-replay representative. No
    /// ease ordering is asserted among the surviving front members.
    AmbiguousNondominatedFront {
        front_size: usize,
    },
}

/// Auditable direct + canonical candidate assessment for one exact cell.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedRouteCellAssessment {
    pub policy: EasiestKnownRouteFusionPolicy,
    pub evidence_disclaimer: &'static str,
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub direct_audit_status: FusedDirectAuditStatus,
    pub raw_direct_positive_witnesses: usize,
    pub retained_direct_positive_witnesses: usize,
    pub canonical_matrix_status: CanonicalMatrixCellStatus,
    /// Candidates sorted by controller coordinates and then complete replay
    /// bytes.  Search effort and discovery order are absent from this order.
    pub candidates: Vec<FusedRouteCandidate>,
    /// Indices into `candidates`.  Equal-demand candidates are pruned only by
    /// a decisive shared difficulty comparison; trade-offs and missing
    /// evidence remain on the front.
    pub nondominated_front: Vec<usize>,
    pub ease_evidence: Vec<FusedRouteEaseEvidence>,
    pub selected: Option<EasiestKnownRouteSelection>,
}

impl FusedRouteCellAssessment {
    #[must_use]
    pub fn selected_representative(&self) -> Option<&FusedRouteCandidate> {
        self.selected
            .and_then(|selection| self.candidates.get(selection.candidate_index))
    }
}

/// Fuse one exact matrix row with all retained direct positives for the same
/// directed-door/loadout coordinates.
pub fn fuse_easiest_known_route_cell(
    generated: &GeneratedLevel,
    matrix: &LoadoutRouteMatrix,
    route: &RouteControllerAssessment,
    source_door_id: &str,
    target_door_id: &str,
    difficulty_config: &DifficultyConfig,
) -> Result<FusedRouteCellAssessment, RouteFusionError> {
    if source_door_id == target_door_id {
        return Err(RouteFusionError::SameSourceAndTarget {
            door_id: source_door_id.to_owned(),
        });
    }
    if matrix.evidence.loadout() != matrix.loadout.abilities() {
        return Err(RouteFusionError::MatrixLoadoutMismatch {
            declared: matrix.loadout,
            evidence: matrix.evidence.loadout(),
        });
    }
    let rows = matrix
        .evidence
        .door_routes()
        .iter()
        .filter(|row| row.source_door_id == source_door_id && row.target_door_id == target_door_id)
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return Err(RouteFusionError::MatrixCellCardinality {
            source_door_id: source_door_id.to_owned(),
            target_door_id: target_door_id.to_owned(),
            loadout: matrix.loadout,
            count: rows.len(),
        });
    };
    fuse_easiest_known_route_row(generated, matrix.loadout, row, route, difficulty_config)
}

fn fuse_easiest_known_route_row(
    generated: &GeneratedLevel,
    loadout: EvaluationLoadout,
    row: &DoorRouteEvidence,
    route: &RouteControllerAssessment,
    difficulty_config: &DifficultyConfig,
) -> Result<FusedRouteCellAssessment, RouteFusionError> {
    if route.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY {
        return Err(RouteFusionError::RoutePolicyMismatch);
    }
    if route.source_door_id != row.source_door_id || route.target_door_id != row.target_door_id {
        return Err(RouteFusionError::RouteCoordinateMismatch {
            matrix_source_door_id: row.source_door_id.clone(),
            matrix_target_door_id: row.target_door_id.clone(),
            route_source_door_id: route.source_door_id.clone(),
            route_target_door_id: route.target_door_id.clone(),
        });
    }
    if !route.expected_subset_loadouts.contains(&loadout) {
        return Err(RouteFusionError::RouteDoesNotCoverLoadout { loadout });
    }
    let audits = route
        .audits
        .iter()
        .filter(|audit| audit.loadout == loadout)
        .collect::<Vec<_>>();
    let [audit] = audits.as_slice() else {
        return Err(RouteFusionError::DirectAuditCardinality {
            source_door_id: row.source_door_id.clone(),
            target_door_id: row.target_door_id.clone(),
            loadout,
            count: audits.len(),
        });
    };
    let exact_direct_witnesses = route
        .easiest_first_witnesses
        .iter()
        .enumerate()
        .filter(|(_, witness)| witness.loadout == loadout)
        .collect::<Vec<_>>();
    if exact_direct_witnesses.len() != audit.retained_semantic_witnesses
        || audit.raw_positive_witnesses < audit.retained_semantic_witnesses
    {
        return Err(RouteFusionError::DirectWitnessCountMismatch {
            source_door_id: row.source_door_id.clone(),
            target_door_id: row.target_door_id.clone(),
            loadout,
            audit_raw: audit.raw_positive_witnesses,
            audit_retained: audit.retained_semantic_witnesses,
            stored_retained: exact_direct_witnesses.len(),
        });
    }

    let initial = Simulation::enter_via_door(
        generated.room.clone(),
        loadout.abilities(),
        &row.source_door_id,
    )
    .map_err(|source| RouteFusionError::DoorEntry {
        source_door_id: row.source_door_id.clone(),
        loadout,
        source: Box::new(source),
    })?;

    let mut pending = MeasuredCandidateAccumulator::with_capacity(
        exact_direct_witnesses.len()
            + usize::from(matches!(row.evidence, BoundedTargetEvidence::Positive(_))),
    );
    for (witness_index, witness) in exact_direct_witnesses {
        let candidate = certify_and_normalize_candidate(
            &initial,
            &row.target_door_id,
            &witness.replay,
            RouteFusionCandidateOrigin::DirectWitness { witness_index },
        )?;
        let candidate_index = pending.retain(
            &initial,
            &row.target_door_id,
            candidate,
            difficulty_config,
            FusedRouteCandidateProvenance::DirectWitness { witness_index },
        )?;
        let retained = &pending.candidates[candidate_index];
        if retained.measurement.observation.actions != witness.semantic_trace {
            return Err(RouteFusionError::StoredDirectTraceMismatch {
                source_door_id: row.source_door_id.clone(),
                target_door_id: row.target_door_id.clone(),
                loadout,
                witness_index,
            });
        }
        if retained.controller_demand != witness.demand {
            return Err(RouteFusionError::StoredDirectDemandMismatch {
                source_door_id: row.source_door_id.clone(),
                target_door_id: row.target_door_id.clone(),
                loadout,
                witness_index,
            });
        }
    }

    let canonical_matrix_status = match &row.evidence {
        BoundedTargetEvidence::Positive(evidence) => {
            let solution = evidence.solution();
            if solution.target != SearchTarget::door(&row.target_door_id)
                || solution.reached != ReachedTarget::Door(row.target_door_id.clone())
            {
                return Err(RouteFusionError::CanonicalTargetMismatch {
                    source_door_id: row.source_door_id.clone(),
                    target_door_id: row.target_door_id.clone(),
                    loadout,
                    target: solution.target.clone(),
                    reached: solution.reached.clone(),
                });
            }
            let fingerprint = evidence.witness_fingerprint();
            let candidate = certify_and_normalize_candidate(
                &initial,
                &row.target_door_id,
                &solution.replay,
                RouteFusionCandidateOrigin::CanonicalMatrix,
            )?;
            pending.retain(
                &initial,
                &row.target_door_id,
                candidate,
                difficulty_config,
                FusedRouteCandidateProvenance::CanonicalMatrix {
                    witness_fingerprint: fingerprint,
                },
            )?;
            CanonicalMatrixCellStatus::Positive {
                witness_fingerprint: fingerprint,
            }
        }
        BoundedTargetEvidence::Inconclusive(evidence) => {
            CanonicalMatrixCellStatus::BoundedInconclusive {
                reason: evidence.reason,
            }
        }
    };

    let mut pending = pending.into_candidates();
    pending.sort_by(compare_candidates);
    let (ease_evidence, dominated) = pairwise_ease_evidence(&pending);
    let nondominated_front = dominated
        .iter()
        .enumerate()
        .filter_map(|(index, dominated)| (!*dominated).then_some(index))
        .collect::<Vec<_>>();
    let selected = select_representative(&pending, &nondominated_front);

    Ok(FusedRouteCellAssessment {
        policy: CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY,
        evidence_disclaimer: EASIEST_KNOWN_ROUTE_DISCLAIMER,
        source_door_id: row.source_door_id.clone(),
        target_door_id: row.target_door_id.clone(),
        loadout,
        direct_audit_status: audit.status.into(),
        raw_direct_positive_witnesses: audit.raw_positive_witnesses,
        retained_direct_positive_witnesses: audit.retained_semantic_witnesses,
        canonical_matrix_status,
        candidates: pending,
        nondominated_front,
        ease_evidence,
        selected,
    })
}

#[derive(Clone, Copy, Debug)]
pub enum RouteFusionCandidateOrigin {
    DirectWitness { witness_index: usize },
    CanonicalMatrix,
}

impl fmt::Display for RouteFusionCandidateOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectWitness { witness_index } => {
                write!(formatter, "direct witness {witness_index}")
            }
            Self::CanonicalMatrix => formatter.write_str("canonical matrix witness"),
        }
    }
}

/// A supplied replay that has been independently verified, observed through
/// first target contact, and normalized to that exact successful prefix.
///
/// Landing and difficulty analysis are intentionally absent. Exact normalized
/// replay deduplication happens on this representation so those expensive
/// measurements run once per unique candidate rather than once per source.
#[derive(Clone, Debug)]
struct CertifiedRouteCandidate {
    replay_identity: u64,
    replay: Replay,
    controller_demand: ControllerDemand,
    controller_coordinates: ControllerDemandCoordinates,
    observation: SuccessfulWitnessObservation,
    /// The first source retaining this normalized replay. Measurement errors
    /// use it for stable diagnostics; all discovery sources remain in
    /// `provenance` on successful output.
    measurement_origin: RouteFusionCandidateOrigin,
}

fn certify_and_normalize_candidate(
    initial: &Simulation,
    target_door_id: &str,
    replay: &Replay,
    origin: RouteFusionCandidateOrigin,
) -> Result<CertifiedRouteCandidate, RouteFusionError> {
    // `observe_solution` verifies every supplied frame, identifies first
    // target contact, and rejects a replay that reaches another exit.
    let supplied_solution = Solution {
        exit_id: target_door_id.to_owned(),
        replay: replay.clone(),
        // Candidate measurement intentionally normalizes discovery effort.
        // The difficulty vector retains this zero cost, and neither selection
        // nor pairwise comparison ever consults operational work.
        stats: SearchStats::default(),
    };
    let supplied_observation =
        observe_solution(initial, &supplied_solution, TraversalGrid::default()).map_err(
            |source| RouteFusionError::ReplayObservation {
                origin,
                source: Box::new(source),
            },
        )?;
    if supplied_observation.reached_exit_id != target_door_id {
        return Err(RouteFusionError::ReachedUnexpectedDoor {
            origin,
            expected_door_id: target_door_id.to_owned(),
            actual_door_id: supplied_observation.reached_exit_id,
        });
    }

    #[cfg(test)]
    SUCCESSFUL_CANDIDATE_CERTIFICATIONS.with(|count| count.set(count.get().saturating_add(1)));

    // Normalize aliases which differ only by inert frames after first target
    // contact before exact replay deduplication and all measurements.
    let replay = Replay {
        initial_digest: replay.initial_digest,
        frames: replay.frames[..supplied_observation.completion_ticks].to_vec(),
    };
    debug_assert_eq!(replay.frames.len(), supplied_observation.completion_ticks);
    // The observation already contains only the prefix through first target
    // contact. Re-observing the trimmed replay would verify and simulate that
    // same prefix a second time without changing any retained evidence.
    let observation = supplied_observation;
    let controller_demand = controller_demand(&observation.actions);
    Ok(CertifiedRouteCandidate {
        replay_identity: replay_identity(&replay),
        replay,
        controller_coordinates: controller_demand.coordinates(),
        controller_demand,
        observation,
        measurement_origin: origin,
    })
}

fn measure_certified_candidate(
    initial: &Simulation,
    target_door_id: &str,
    candidate: CertifiedRouteCandidate,
    difficulty_config: &DifficultyConfig,
) -> Result<FusedRouteCandidate, RouteFusionError> {
    let CertifiedRouteCandidate {
        replay_identity,
        replay,
        controller_demand,
        controller_coordinates,
        observation,
        measurement_origin: origin,
    } = candidate;
    let solution = Solution {
        exit_id: target_door_id.to_owned(),
        replay: replay.clone(),
        stats: SearchStats::default(),
    };

    #[cfg(test)]
    EXPENSIVE_CANDIDATE_MEASUREMENTS.with(|count| count.set(count.get().saturating_add(1)));

    let landing_precision = analyze_replay_landings(initial, &replay, observation.completion_ticks)
        .map_err(|source| RouteFusionError::LandingPrecision {
            origin,
            source: Box::new(source),
        })?;
    let exact_difficulty = downwards_ai::analyze_solution(initial, &solution, difficulty_config)
        .map_err(|source| RouteFusionError::Difficulty {
            origin,
            source: Box::new(source),
        })?;
    let vector =
        route_difficulty_vector(&observation, &exact_difficulty, None).map_err(|source| {
            RouteFusionError::DifficultyVector {
                origin,
                source: Box::new(source),
            }
        })?;
    Ok(FusedRouteCandidate {
        replay_identity,
        replay,
        provenance: Vec::new(),
        controller_coordinates,
        controller_demand,
        measurement: FusedRouteCandidateMeasurement {
            observation,
            landing_precision,
            exact_difficulty,
            vector,
        },
    })
}

/// Source-ordered measured-candidate set with an exact replay equality check
/// inside every stable-identity bucket.
///
/// Each supplied source is certified before lookup. A new normalized replay
/// is measured immediately, preserving the old source-by-source error
/// precedence; an exact duplicate merges provenance without repeating
/// landing or difficulty work. The identity accelerates lookup but never
/// establishes equality, so even an adversarial collision retains and
/// measures both candidates.
#[derive(Default)]
struct MeasuredCandidateAccumulator {
    candidates: Vec<FusedRouteCandidate>,
    identity_buckets: BTreeMap<u64, Vec<usize>>,
}

impl MeasuredCandidateAccumulator {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            candidates: Vec::with_capacity(capacity),
            identity_buckets: BTreeMap::new(),
        }
    }

    fn retain(
        &mut self,
        initial: &Simulation,
        target_door_id: &str,
        candidate: CertifiedRouteCandidate,
        difficulty_config: &DifficultyConfig,
        provenance: FusedRouteCandidateProvenance,
    ) -> Result<usize, RouteFusionError> {
        self.retain_in_identity_bucket(
            candidate.replay_identity,
            initial,
            target_door_id,
            candidate,
            difficulty_config,
            provenance,
        )
    }

    fn retain_in_identity_bucket(
        &mut self,
        bucket_identity: u64,
        initial: &Simulation,
        target_door_id: &str,
        candidate: CertifiedRouteCandidate,
        difficulty_config: &DifficultyConfig,
        provenance: FusedRouteCandidateProvenance,
    ) -> Result<usize, RouteFusionError> {
        let existing_index = self
            .identity_buckets
            .get(&bucket_identity)
            .and_then(|indices| {
                indices
                    .iter()
                    .copied()
                    .find(|&index| self.candidates[index].replay == candidate.replay)
            });
        if let Some(existing_index) = existing_index {
            let existing = &mut self.candidates[existing_index];
            debug_assert_eq!(existing.replay_identity, candidate.replay_identity);
            debug_assert_eq!(existing.controller_demand, candidate.controller_demand);
            debug_assert_eq!(
                existing.controller_coordinates,
                candidate.controller_coordinates
            );
            debug_assert_eq!(existing.measurement.observation, candidate.observation);
            existing.provenance.push(provenance);
            existing.provenance.sort_unstable();
            existing.provenance.dedup();
            Ok(existing_index)
        } else {
            let mut candidate =
                measure_certified_candidate(initial, target_door_id, candidate, difficulty_config)?;
            candidate.provenance.push(provenance);
            let candidate_index = self.candidates.len();
            self.candidates.push(candidate);
            self.identity_buckets
                .entry(bucket_identity)
                .or_default()
                .push(candidate_index);
            Ok(candidate_index)
        }
    }

    fn into_candidates(self) -> Vec<FusedRouteCandidate> {
        self.candidates
    }
}

#[cfg(test)]
std::thread_local! {
    static SUCCESSFUL_CANDIDATE_CERTIFICATIONS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
    static EXPENSIVE_CANDIDATE_MEASUREMENTS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

fn compare_candidates(left: &FusedRouteCandidate, right: &FusedRouteCandidate) -> Ordering {
    left.controller_coordinates
        .cmp(&right.controller_coordinates)
        .then_with(|| semantic_span_key(left).cmp(&semantic_span_key(right)))
        .then_with(|| replay_bytes(&left.replay).cmp(&replay_bytes(&right.replay)))
        .then_with(|| left.provenance.cmp(&right.provenance))
}

fn select_representative(
    candidates: &[FusedRouteCandidate],
    nondominated_front: &[usize],
) -> Option<EasiestKnownRouteSelection> {
    nondominated_front
        .first()
        .map(|&candidate_index| EasiestKnownRouteSelection {
            candidate_index,
            replay_identity: candidates[candidate_index].replay_identity,
            status: if nondominated_front.len() == 1 {
                EasiestKnownRouteSelectionStatus::UniqueNondominatedCandidate
            } else {
                EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront {
                    front_size: nondominated_front.len(),
                }
            },
        })
}

fn semantic_span_key(
    candidate: &FusedRouteCandidate,
) -> Vec<(downwards_lab::SemanticAction, usize)> {
    candidate
        .measurement
        .observation
        .actions
        .spans
        .iter()
        .map(|span| (span.action, span.ticks))
        .collect()
}

fn pairwise_ease_evidence(
    candidates: &[FusedRouteCandidate],
) -> (Vec<FusedRouteEaseEvidence>, Vec<bool>) {
    let mut evidence = Vec::with_capacity(
        candidates
            .len()
            .saturating_mul(candidates.len().saturating_sub(1))
            / 2,
    );
    let mut dominated = vec![false; candidates.len()];
    for left_index in 0..candidates.len() {
        for right_index in left_index + 1..candidates.len() {
            let left = &candidates[left_index];
            let right = &candidates[right_index];
            let (controller_ordering, comparable_difficulty) = match controller_dominance(
                left.controller_coordinates,
                right.controller_coordinates,
            ) {
                ControllerEaseOrdering::LeftStrictlyDominates => {
                    dominated[right_index] = true;
                    (ControllerEaseOrdering::LeftStrictlyDominates, None)
                }
                ControllerEaseOrdering::RightStrictlyDominates => {
                    dominated[left_index] = true;
                    (ControllerEaseOrdering::RightStrictlyDominates, None)
                }
                ControllerEaseOrdering::Equal => {
                    let comparison = compare_perfect_control_route_difficulty(
                        &left.measurement.vector,
                        &right.measurement.vector,
                        DifficultyComparisonTolerances {
                            ticks: 0.0,
                            counts: 0.0,
                            coarse_distance_cells: 0.0,
                            hazard_pressure: 0.0,
                            probabilities: 0.0,
                        },
                    );
                    match &comparison {
                        RouteDifficultyComparison::LeftClearlyHarder { .. } => {
                            dominated[left_index] = true;
                        }
                        RouteDifficultyComparison::RightClearlyHarder { .. } => {
                            dominated[right_index] = true;
                        }
                        RouteDifficultyComparison::EquivalentWithinTolerance
                        | RouteDifficultyComparison::Incomparable { .. }
                        | RouteDifficultyComparison::InsufficientEvidence { .. } => {}
                    }
                    (ControllerEaseOrdering::Equal, Some(comparison))
                }
                ControllerEaseOrdering::Tradeoff => (ControllerEaseOrdering::Tradeoff, None),
            };
            evidence.push(FusedRouteEaseEvidence {
                left_candidate_index: left_index,
                right_candidate_index: right_index,
                controller_ordering,
                comparable_difficulty,
            });
        }
    }
    (evidence, dominated)
}

fn controller_dominance(
    left: ControllerDemandCoordinates,
    right: ControllerDemandCoordinates,
) -> ControllerEaseOrdering {
    let pairs = [
        (
            usize::from(left.controller_class),
            usize::from(right.controller_class),
        ),
        (left.ability_events, right.ability_events),
        (left.horizontal_reversals, right.horizontal_reversals),
        (left.vertical_decisions, right.vertical_decisions),
        (left.semantic_spans, right.semantic_spans),
        (left.semantic_transitions, right.semantic_transitions),
        (left.duration_ticks, right.duration_ticks),
    ];
    let left_no_harder = pairs.iter().all(|(left, right)| left <= right);
    let right_no_harder = pairs.iter().all(|(left, right)| right <= left);
    match (left_no_harder, right_no_harder) {
        (true, true) => ControllerEaseOrdering::Equal,
        (true, false) => ControllerEaseOrdering::LeftStrictlyDominates,
        (false, true) => ControllerEaseOrdering::RightStrictlyDominates,
        (false, false) => ControllerEaseOrdering::Tradeoff,
    }
}

fn replay_identity(replay: &Replay) -> u64 {
    let mut hash = StableReplayHash::new();
    hash.bytes(b"downwards-easiest-known-route-replay");
    hash.u32(EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION);
    hash.bytes(&replay_bytes(replay));
    hash.finish()
}

fn replay_bytes(replay: &Replay) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + replay.frames.len().saturating_mul(22));
    bytes.extend_from_slice(&replay.initial_digest.0.to_le_bytes());
    bytes.extend_from_slice(&(replay.frames.len() as u64).to_le_bytes());
    for frame in &replay.frames {
        append_action(&mut bytes, frame.action);
        bytes.extend_from_slice(&frame.expected_digest.0.to_le_bytes());
        bytes.extend_from_slice(&frame.expected_event_digest.0.to_le_bytes());
    }
    bytes
}

fn append_action(bytes: &mut Vec<u8>, action: Action) {
    bytes.push(action.move_x as u8);
    bytes.push(action.move_y as u8);
    bytes.push(u8::from(action.jump));
    bytes.push(u8::from(action.dash));
    bytes.push(u8::from(action.restart));
}

struct StableReplayHash(u64);

impl StableReplayHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn bytes(&mut self, values: &[u8]) {
        for &value in values {
            self.byte(value);
        }
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub enum RouteFusionError {
    SameSourceAndTarget {
        door_id: String,
    },
    MatrixLoadoutMismatch {
        declared: EvaluationLoadout,
        evidence: downwards_core::AbilitySet,
    },
    MatrixCellCardinality {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        count: usize,
    },
    RoutePolicyMismatch,
    RouteCoordinateMismatch {
        matrix_source_door_id: String,
        matrix_target_door_id: String,
        route_source_door_id: String,
        route_target_door_id: String,
    },
    RouteDoesNotCoverLoadout {
        loadout: EvaluationLoadout,
    },
    DirectAuditCardinality {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        count: usize,
    },
    DirectWitnessCountMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        audit_raw: usize,
        audit_retained: usize,
        stored_retained: usize,
    },
    DoorEntry {
        source_door_id: String,
        loadout: EvaluationLoadout,
        source: Box<DoorEntryError>,
    },
    CanonicalTargetMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        target: SearchTarget,
        reached: ReachedTarget,
    },
    ReplayObservation {
        origin: RouteFusionCandidateOrigin,
        source: Box<WitnessObservationError>,
    },
    ReachedUnexpectedDoor {
        origin: RouteFusionCandidateOrigin,
        expected_door_id: String,
        actual_door_id: String,
    },
    StoredDirectTraceMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        witness_index: usize,
    },
    StoredDirectDemandMismatch {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        witness_index: usize,
    },
    LandingPrecision {
        origin: RouteFusionCandidateOrigin,
        source: Box<LandingPrecisionError>,
    },
    Difficulty {
        origin: RouteFusionCandidateOrigin,
        source: Box<DifficultyError>,
    },
    DifficultyVector {
        origin: RouteFusionCandidateOrigin,
        source: Box<RouteDifficultyVectorError>,
    },
}

impl fmt::Display for RouteFusionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameSourceAndTarget { door_id } => {
                write!(
                    formatter,
                    "route fusion source and target are both {door_id:?}"
                )
            }
            Self::MatrixLoadoutMismatch { declared, evidence } => write!(
                formatter,
                "{} matrix carries exact evidence for {evidence:?}",
                declared.slug()
            ),
            Self::MatrixCellCardinality {
                source_door_id,
                target_door_id,
                loadout,
                count,
            } => write!(
                formatter,
                "{} matrix contains {count} rows for {source_door_id:?}->{target_door_id:?}",
                loadout.slug()
            ),
            Self::RoutePolicyMismatch => {
                formatter.write_str("route fusion received a stale direct-controller policy")
            }
            Self::RouteCoordinateMismatch {
                matrix_source_door_id,
                matrix_target_door_id,
                route_source_door_id,
                route_target_door_id,
            } => write!(
                formatter,
                "matrix cell {matrix_source_door_id:?}->{matrix_target_door_id:?} received direct route {route_source_door_id:?}->{route_target_door_id:?}"
            ),
            Self::RouteDoesNotCoverLoadout { loadout } => write!(
                formatter,
                "direct route assessment does not cover exact {} physics",
                loadout.slug()
            ),
            Self::DirectAuditCardinality {
                source_door_id,
                target_door_id,
                loadout,
                count,
            } => write!(
                formatter,
                "direct route {source_door_id:?}->{target_door_id:?} has {count} {} audits",
                loadout.slug()
            ),
            Self::DirectWitnessCountMismatch {
                source_door_id,
                target_door_id,
                loadout,
                audit_raw,
                audit_retained,
                stored_retained,
            } => write!(
                formatter,
                "direct route {source_door_id:?}->{target_door_id:?} {} audit reports raw={audit_raw}, retained={audit_retained}, but stores {stored_retained} exact witnesses",
                loadout.slug()
            ),
            Self::DoorEntry {
                source_door_id,
                loadout,
                source,
            } => write!(
                formatter,
                "cannot enter {source_door_id:?} under {} for route fusion: {source}",
                loadout.slug()
            ),
            Self::CanonicalTargetMismatch {
                source_door_id,
                target_door_id,
                loadout,
                target,
                reached,
            } => write!(
                formatter,
                "canonical {} row {source_door_id:?}->{target_door_id:?} contains target={target:?}, reached={reached:?}",
                loadout.slug()
            ),
            Self::ReplayObservation { origin, source } => {
                write!(formatter, "cannot replay {origin}: {source}")
            }
            Self::ReachedUnexpectedDoor {
                origin,
                expected_door_id,
                actual_door_id,
            } => write!(
                formatter,
                "{origin} reached {actual_door_id:?}, expected {expected_door_id:?}"
            ),
            Self::StoredDirectTraceMismatch {
                source_door_id,
                target_door_id,
                loadout,
                witness_index,
            } => write!(
                formatter,
                "direct witness {witness_index} for {source_door_id:?}->{target_door_id:?} {} disagrees with its replayed semantic trace",
                loadout.slug()
            ),
            Self::StoredDirectDemandMismatch {
                source_door_id,
                target_door_id,
                loadout,
                witness_index,
            } => write!(
                formatter,
                "direct witness {witness_index} for {source_door_id:?}->{target_door_id:?} {} disagrees with its replayed controller demand",
                loadout.slug()
            ),
            Self::LandingPrecision { origin, source } => {
                write!(
                    formatter,
                    "cannot measure {origin} landing geometry: {source}"
                )
            }
            Self::Difficulty { origin, source } => {
                write!(
                    formatter,
                    "cannot measure {origin} exact difficulty: {source}"
                )
            }
            Self::DifficultyVector { origin, source } => {
                write!(
                    formatter,
                    "cannot assemble {origin} difficulty vector: {source}"
                )
            }
        }
    }
}

impl Error for RouteFusionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorEntry { source, .. } => Some(source.as_ref()),
            Self::ReplayObservation { source, .. } => Some(source.as_ref()),
            Self::LandingPrecision { source, .. } => Some(source.as_ref()),
            Self::Difficulty { source, .. } => Some(source.as_ref()),
            Self::DifficultyVector { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{ActionMacro, SolverConfig};
    use downwards_core::{AbilitySet, BoundarySide, Door, Point, Rect, Room, Tile};
    use downwards_gen::{
        AbilityTier, GENERATION_VERSION, GeneratedMetadata, GenerationStats, LayoutFamily,
    };
    use downwards_validation::{ValidationConfig, evaluate_generated_door_targets_for_loadout};

    use super::*;
    use crate::corpus::{
        RouteControllerAuditCompleteness, RouteMatrixSummary, assess_easiest_known_route,
    };

    const WIDTH: usize = 32;
    const HEIGHT: usize = 18;

    fn flat_generated() -> GeneratedLevel {
        let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        for tile in &mut tiles[16 * WIDTH..17 * WIDTH] {
            *tile = Tile::Solid;
        }
        let room = Room::new(
            "route-fusion-flat",
            "Route fusion flat",
            WIDTH as u16,
            HEIGHT as u16,
            10,
            tiles,
            Point::new(30, 148),
            vec![],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".into(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 140, 4, 20),
                arrival: Point::new(30, 148),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".into(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 140, 4, 20),
                arrival: Point::new(270, 148),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap();
        GeneratedLevel {
            room,
            metadata: GeneratedMetadata {
                generation_version: GENERATION_VERSION,
                seed: 0,
                layout_family: LayoutFamily::HazardRun,
                ability_tier: AbilityTier::Baseline,
                intended_abilities: AbilitySet::NONE,
                stats: GenerationStats::default(),
            },
        }
    }

    fn hard_canonical_matrix(generated: &GeneratedLevel) -> LoadoutRouteMatrix {
        let solver = SolverConfig {
            probe_direct_routes: false,
            max_expanded_nodes: 1_000,
            max_simulated_ticks: 100_000,
            max_ticks_per_path: 240,
            beam_width: 8,
            macros: vec![ActionMacro::held(
                "held-jump-right",
                Action {
                    move_x: 1,
                    jump: true,
                    ..Action::default()
                },
                4,
            )],
            ..SolverConfig::default()
        };
        let evidence = evaluate_generated_door_targets_for_loadout(
            generated,
            AbilitySet::NONE,
            &ValidationConfig {
                solver,
                difficulty: DifficultyConfig::default(),
            },
        )
        .unwrap();
        let positive_door_rows = evidence
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        LoadoutRouteMatrix {
            loadout: EvaluationLoadout::Baseline,
            summary: RouteMatrixSummary {
                door_rows: evidence.door_routes().len(),
                positive_door_rows,
                inconclusive_door_rows: evidence.door_routes().len() - positive_door_rows,
                pickup_rows: evidence.pickup_routes().len(),
                positive_pickup_rows: 0,
                inconclusive_pickup_rows: evidence.pickup_routes().len(),
            },
            evidence,
        }
    }

    fn direct_route(generated: &GeneratedLevel) -> RouteControllerAssessment {
        assess_easiest_known_route(
            &generated.room,
            "west",
            "east",
            EvaluationLoadout::Baseline,
            &SolverConfig {
                max_expanded_nodes: 10_000,
                max_simulated_ticks: 1_000_000,
                max_ticks_per_path: 240,
                ..SolverConfig::default()
            },
        )
        .unwrap()
    }

    /// Reference implementation of the pre-staging measurement path. It
    /// deliberately observes both the supplied replay and its normalized
    /// prefix so the optimized path can prove exact retained-output
    /// equivalence while avoiding that second observation in production.
    fn legacy_measure_candidate(
        initial: &Simulation,
        target_door_id: &str,
        replay: &Replay,
        difficulty_config: &DifficultyConfig,
    ) -> FusedRouteCandidate {
        let supplied_solution = Solution {
            exit_id: target_door_id.to_owned(),
            replay: replay.clone(),
            stats: SearchStats::default(),
        };
        let supplied_observation =
            observe_solution(initial, &supplied_solution, TraversalGrid::default()).unwrap();
        assert_eq!(supplied_observation.reached_exit_id, target_door_id);
        let replay = Replay {
            initial_digest: replay.initial_digest,
            frames: replay.frames[..supplied_observation.completion_ticks].to_vec(),
        };
        let solution = Solution {
            exit_id: target_door_id.to_owned(),
            replay: replay.clone(),
            stats: SearchStats::default(),
        };
        let observation = observe_solution(initial, &solution, TraversalGrid::default()).unwrap();
        let landing_precision =
            analyze_replay_landings(initial, &replay, observation.completion_ticks).unwrap();
        let exact_difficulty =
            downwards_ai::analyze_solution(initial, &solution, difficulty_config).unwrap();
        let vector = route_difficulty_vector(&observation, &exact_difficulty, None).unwrap();
        let controller_demand = controller_demand(&observation.actions);
        FusedRouteCandidate {
            replay_identity: replay_identity(&replay),
            replay,
            provenance: Vec::new(),
            controller_coordinates: controller_demand.coordinates(),
            controller_demand,
            measurement: FusedRouteCandidateMeasurement {
                observation,
                landing_precision,
                exact_difficulty,
                vector,
            },
        }
    }

    fn canonical_west_east_positive(matrix: &LoadoutRouteMatrix) -> (&Replay, WitnessFingerprint) {
        let row = matrix
            .evidence
            .door_routes()
            .iter()
            .find(|row| row.source_door_id == "west" && row.target_door_id == "east")
            .expect("west->east row");
        let BoundedTargetEvidence::Positive(evidence) = &row.evidence else {
            panic!("west->east canonical row must be positive");
        };
        (&evidence.solution().replay, evidence.witness_fingerprint())
    }

    #[test]
    fn hard_canonical_and_easy_direct_selects_direct_without_solver_effort() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let route = direct_route(&generated);
        let fused = fuse_easiest_known_route_cell(
            &generated,
            &matrix,
            &route,
            "west",
            "east",
            &DifficultyConfig::default(),
        )
        .unwrap();

        let selected = fused
            .selected_representative()
            .expect("the cell is positive");
        assert!(selected.controller_demand.run_only);
        assert!(
            selected.provenance.iter().any(|source| matches!(
                source,
                FusedRouteCandidateProvenance::DirectWitness { .. }
            ))
        );
        let canonical = fused
            .candidates
            .iter()
            .find(|candidate| {
                candidate.provenance.iter().any(|source| {
                    matches!(
                        source,
                        FusedRouteCandidateProvenance::CanonicalMatrix { .. }
                    )
                })
            })
            .expect("the canonical matrix positive is retained");
        assert!(!canonical.controller_demand.run_only);
        assert!(
            selected.controller_coordinates < canonical.controller_coordinates,
            "the explicit versioned selection rule prefers lower controller coordinates"
        );
        assert!(fused.candidates.iter().all(|candidate| {
            candidate.measurement.vector.operational_solver_cost == Default::default()
        }));
    }

    #[test]
    fn canonical_positive_is_fallback_when_direct_audit_has_no_positive() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let mut route = direct_route(&generated);
        route.easiest_first_witnesses.clear();
        route.easiest_known_front.clear();
        route.positive_bypasses.clear();
        let audit = route.audits.first_mut().unwrap();
        audit.raw_positive_witnesses = 0;
        audit.retained_semantic_witnesses = 0;
        audit.status = LoadoutControllerAuditStatus::CompleteFiniteVocabulary;
        route.completeness = RouteControllerAuditCompleteness::CompleteFiniteVocabulary;

        let fused = fuse_easiest_known_route_cell(
            &generated,
            &matrix,
            &route,
            "west",
            "east",
            &DifficultyConfig::default(),
        )
        .unwrap();
        let selected = fused
            .selected_representative()
            .expect("canonical is a fallback");
        assert_eq!(fused.candidates.len(), 1);
        assert!(matches!(
            selected.provenance.as_slice(),
            [FusedRouteCandidateProvenance::CanonicalMatrix { .. }]
        ));
        assert_eq!(
            fused.direct_audit_status,
            FusedDirectAuditStatus::CompleteFiniteVocabulary
        );
    }

    #[test]
    fn coordinate_and_loadout_mismatches_are_rejected() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let route = direct_route(&generated);
        let mut wrong_target = route.clone();
        wrong_target.target_door_id = "west".to_owned();
        assert!(matches!(
            fuse_easiest_known_route_cell(
                &generated,
                &matrix,
                &wrong_target,
                "west",
                "east",
                &DifficultyConfig::default(),
            ),
            Err(RouteFusionError::RouteCoordinateMismatch { .. })
        ));

        let mut wrong_loadout = matrix.clone();
        wrong_loadout.loadout = EvaluationLoadout::WallJump;
        assert!(matches!(
            fuse_easiest_known_route_cell(
                &generated,
                &wrong_loadout,
                &route,
                "west",
                "east",
                &DifficultyConfig::default(),
            ),
            Err(RouteFusionError::MatrixLoadoutMismatch { .. })
        ));
    }

    #[test]
    fn candidate_and_pareto_front_identity_are_repeatable() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let route = direct_route(&generated);
        let assess = || {
            fuse_easiest_known_route_cell(
                &generated,
                &matrix,
                &route,
                "west",
                "east",
                &DifficultyConfig::default(),
            )
            .unwrap()
        };
        let first = assess();
        let second = assess();
        assert_eq!(first, second);
        assert_eq!(first.nondominated_front, vec![0]);
        assert_eq!(
            first.selected.unwrap().replay_identity,
            first.candidates[first.nondominated_front[0]].replay_identity
        );
    }

    #[test]
    fn normalized_dedup_is_output_equivalent_provenance_complete_and_collision_safe() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let route = direct_route(&generated);
        let (witness_index, witness) = route
            .easiest_first_witnesses
            .iter()
            .enumerate()
            .find(|(_, witness)| witness.loadout == EvaluationLoadout::Baseline)
            .expect("baseline direct positive");
        let (canonical_replay, fingerprint) = canonical_west_east_positive(&matrix);
        let initial =
            Simulation::enter_via_door(generated.room.clone(), AbilitySet::NONE, "west").unwrap();
        let config = DifficultyConfig::default();

        let direct_provenance = FusedRouteCandidateProvenance::DirectWitness { witness_index };
        let canonical_provenance = FusedRouteCandidateProvenance::CanonicalMatrix {
            witness_fingerprint: fingerprint,
        };
        SUCCESSFUL_CANDIDATE_CERTIFICATIONS.with(|count| count.set(0));
        let direct = certify_and_normalize_candidate(
            &initial,
            "east",
            &witness.replay,
            RouteFusionCandidateOrigin::DirectWitness { witness_index },
        )
        .unwrap();
        let post_contact_alias = Replay::record(
            &initial,
            witness
                .replay
                .actions()
                .chain(std::iter::once(Action::default())),
        );
        assert_ne!(
            replay_bytes(&witness.replay),
            replay_bytes(&post_contact_alias)
        );
        let mut expected = legacy_measure_candidate(&initial, "east", &witness.replay, &config);
        let legacy_alias = legacy_measure_candidate(&initial, "east", &post_contact_alias, &config);
        assert_eq!(expected, legacy_alias);
        expected.provenance = vec![direct_provenance, canonical_provenance];
        expected.provenance.sort_unstable();

        let canonical_alias = certify_and_normalize_candidate(
            &initial,
            "east",
            &post_contact_alias,
            RouteFusionCandidateOrigin::CanonicalMatrix,
        )
        .unwrap();
        let successful_certifications =
            SUCCESSFUL_CANDIDATE_CERTIFICATIONS.with(std::cell::Cell::get);
        assert_eq!(successful_certifications, 2);
        assert_eq!(direct.replay, canonical_alias.replay);
        assert_eq!(direct.observation, canonical_alias.observation);

        let mut invalid_post_contact_alias = post_contact_alias.clone();
        invalid_post_contact_alias
            .frames
            .last_mut()
            .expect("appended post-contact frame")
            .expected_digest
            .0 ^= 1;
        assert!(matches!(
            certify_and_normalize_candidate(
                &initial,
                "east",
                &invalid_post_contact_alias,
                RouteFusionCandidateOrigin::CanonicalMatrix,
            ),
            Err(RouteFusionError::ReplayObservation {
                origin: RouteFusionCandidateOrigin::CanonicalMatrix,
                ..
            })
        ));
        assert_eq!(
            SUCCESSFUL_CANDIDATE_CERTIFICATIONS.with(std::cell::Cell::get),
            2,
            "an invalid discarded tail is rejected rather than normalized away"
        );

        let collision_direct = direct.clone();
        EXPENSIVE_CANDIDATE_MEASUREMENTS.with(|count| count.set(0));
        let mut candidates = MeasuredCandidateAccumulator::with_capacity(2);
        candidates
            .retain(&initial, "east", direct, &config, direct_provenance)
            .unwrap();
        candidates
            .retain(
                &initial,
                "east",
                canonical_alias,
                &config,
                canonical_provenance,
            )
            .unwrap();
        let [measured] = candidates.into_candidates().try_into().unwrap();
        let expensive_measurements = EXPENSIVE_CANDIDATE_MEASUREMENTS.with(std::cell::Cell::get);

        assert_eq!(expensive_measurements, 1);
        assert_eq!(measured.provenance, expected.provenance);
        assert_eq!(
            replay_bytes(&measured.replay),
            replay_bytes(&expected.replay)
        );
        assert_eq!(measured, expected);

        let mut canonical = certify_and_normalize_candidate(
            &initial,
            "east",
            canonical_replay,
            RouteFusionCandidateOrigin::CanonicalMatrix,
        )
        .unwrap();
        assert_ne!(collision_direct.replay, canonical.replay);

        // Force both structurally different candidates into one identity
        // bucket, exactly modeling a stable-hash collision. Full replay
        // equality, not the hash, must decide whether they merge.
        let collision_identity = collision_direct.replay_identity;
        canonical.replay_identity = collision_identity;
        EXPENSIVE_CANDIDATE_MEASUREMENTS.with(|count| count.set(0));
        let mut candidates = MeasuredCandidateAccumulator::with_capacity(2);
        candidates
            .retain_in_identity_bucket(
                collision_identity,
                &initial,
                "east",
                collision_direct,
                &config,
                direct_provenance,
            )
            .unwrap();
        candidates
            .retain_in_identity_bucket(
                collision_identity,
                &initial,
                "east",
                canonical,
                &config,
                canonical_provenance,
            )
            .unwrap();
        let measured_collision = candidates.into_candidates();
        assert_eq!(
            EXPENSIVE_CANDIDATE_MEASUREMENTS.with(std::cell::Cell::get),
            2,
            "hash-colliding structural candidates are each measured"
        );
        assert_eq!(measured_collision.len(), 2);
        assert_ne!(measured_collision[0].replay, measured_collision[1].replay);
        assert_eq!(measured_collision[0].provenance, vec![direct_provenance]);
        assert_eq!(measured_collision[1].provenance, vec![canonical_provenance]);
    }

    #[test]
    fn perfect_control_evidence_prunes_only_decisive_equal_controller_cases() {
        let generated = flat_generated();
        let matrix = hard_canonical_matrix(&generated);
        let route = direct_route(&generated);
        let fused = fuse_easiest_known_route_cell(
            &generated,
            &matrix,
            &route,
            "west",
            "east",
            &DifficultyConfig::default(),
        )
        .unwrap();
        let mut safer = fused.candidates[0].clone();
        safer.measurement.vector.hazards.minimum_clearance =
            downwards_lab::HazardClearanceEvidence::NotApplicableNoRelevantHazard;
        safer.measurement.vector.hazards.pressure = 0.0;

        let mut riskier = safer.clone();
        riskier.measurement.vector.hazards.minimum_clearance =
            downwards_lab::HazardClearanceEvidence::Observed {
                pixels: 0,
                replay_tick: 1,
            };
        riskier.measurement.vector.hazards.pressure = 1.0;

        let (decisive_evidence, decisive_dominated) =
            pairwise_ease_evidence(&[safer.clone(), riskier.clone()]);
        assert_eq!(decisive_dominated, vec![false, true]);
        assert!(matches!(
            decisive_evidence[0],
            FusedRouteEaseEvidence {
                controller_ordering: ControllerEaseOrdering::Equal,
                comparable_difficulty: Some(RouteDifficultyComparison::RightClearlyHarder { .. }),
                ..
            }
        ));

        safer.measurement.vector.traversal.horizontal_travel_cells += 1.0;
        let tradeoff_candidates = [safer, riskier];
        let (tradeoff_evidence, tradeoff_dominated) = pairwise_ease_evidence(&tradeoff_candidates);
        assert_eq!(tradeoff_dominated, vec![false, false]);
        assert!(matches!(
            tradeoff_evidence[0],
            FusedRouteEaseEvidence {
                controller_ordering: ControllerEaseOrdering::Equal,
                comparable_difficulty: Some(RouteDifficultyComparison::Incomparable { .. }),
                ..
            }
        ));

        let selection = select_representative(&tradeoff_candidates, &[0, 1]).unwrap();
        assert_eq!(selection.candidate_index, 0);
        assert_eq!(
            selection.status,
            EasiestKnownRouteSelectionStatus::AmbiguousNondominatedFront { front_size: 2 }
        );
    }
}
