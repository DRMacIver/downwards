//! Observed route-choice diversity for exact directed route/loadout cells.
//!
//! Generator graph cycles are hypotheses about possible routes. This module
//! instead starts from retained positive controllers, replays every direct
//! controller under its own exact physics loadout, and compares what actually
//! happened. A matching canonical matrix positive may be added to the same
//! cell because that observation was already replay-certified under the same
//! source, target, and loadout.
//!
//! Alternative identity deliberately ignores action-span durations,
//! traversal-span sample counts, and event ticks. Timing-only variants are
//! aliases, not additional route choices. Spatial trajectory, semantic input
//! sequence, accepted event order, and coarse authored-path/gate attribution
//! all have to agree before two observations are folded together.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use downwards_ai::{DirectProbeBudgetLimit, ReachedTarget, SearchStats, SearchTarget};
use downwards_core::{BoundarySide, DashDirection, Simulation, WallSide};
use downwards_gen::{
    GeneratedLevel,
    experimental::{BoundaryPort, RoutePlan, RouteVerb, SupportSpec},
};
use downwards_lab::{
    SemanticAction, SemanticEvent, SuccessfulWitnessObservation, TraversalCell,
    WitnessObservationError, observe_successful_replay, semantic_action_distance,
    traversal_distance,
};
use downwards_validation::BoundedTargetEvidence;

use super::{
    CORPUS_ROOM_ANALYSIS_VERSION, CORPUS_ROUTE_MEASUREMENT_VERSION, CorpusCandidate,
    CorpusMetricInputV2Error, CorpusRoomAnalysis, CorpusRoomAnalysisConfigError,
    CorpusRoomAnalysisConfigRecord, EvaluatedCorpusRoom, EvaluatedCorpusRoomV2, EvaluationLoadout,
    LoadoutControllerAudit, LoadoutControllerAuditStatus, LoadoutRouteMatrix, RoomId,
    RouteControllerAssessment, resolve_corpus_metric_candidate_v2,
};

/// Version of alternative identity, structural attribution, and aggregation.
pub const ROUTE_CHOICE_DIVERSITY_VERSION: u32 = 1;

/// Interpretation boundary for every report produced by this module.
pub const ROUTE_CHOICE_DIVERSITY_DISCLAIMER: &str = "route alternatives are positives observed in the canonical matrix and configured finite direct-controller vocabulary; complete means only that this finite vocabulary was exhausted, bounded absence is inconclusive, coarse route-plan attribution is descriptive rather than proof of an authored edge, timing-only aliases are collapsed, and operational work is not player difficulty";

/// Complete observed route-choice evidence for one analyzed room.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomRouteChoiceDiversity {
    pub version: u32,
    pub source_analysis_version: u32,
    pub source_analysis_config: CorpusRoomAnalysisConfigRecord,
    pub room_id: RoomId,
    /// Loadout-major, then lexicographic source/target order.
    pub cells: Vec<DirectedRouteChoiceCell>,
    pub by_loadout: Vec<LoadoutRouteChoiceSummary>,
    /// Pooled within-cell evidence across every loadout. Distances are never
    /// taken between routes with different endpoints or physics.
    pub room: RouteChoiceAggregate,
    /// Search and measurement work is kept outside every diversity axis.
    pub operational_cost: RouteChoiceOperationalCost,
}

/// One directed door pair under one exact physics loadout.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectedRouteChoiceCell {
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub evidence: RouteChoiceCellEvidence,
}

/// Positive alternatives or an explicit reason no positive can be reported.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteChoiceCellEvidence {
    Positive(Box<RouteAlternativeSet>),
    /// The finite direct-controller vocabulary was exhausted without a
    /// positive, and no matching canonical matrix positive was retained.
    NoPositiveInCompleteFiniteVocabulary,
    /// Search stopped at a configured bound. This is not unreachability.
    BoundedInconclusiveWithoutPositive {
        limit: DirectProbeBudgetLimit,
    },
    MissingDirectedRouteAssessment,
    MissingLoadoutAudit,
}

/// Completeness of direct-controller enumeration for a positive cell.
///
/// A cell may still be positive when this is missing or bounded because the
/// canonical matrix supplied an independently certified positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositiveAlternativeAuditStatus {
    CompleteFiniteVocabulary,
    BoundedIncomplete { limit: DirectProbeBudgetLimit },
    MissingDirectedRouteAssessment,
    MissingLoadoutAudit,
}

/// All materially distinct positive choices observed for one exact cell.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteAlternativeSet {
    pub direct_audit_status: PositiveAlternativeAuditStatus,
    /// Exact action-prefix positives before the route-assessment layer's
    /// timing-sensitive semantic deduplication.
    pub raw_direct_positive_witnesses: usize,
    /// Direct positives retained by route assessment for this loadout.
    pub retained_direct_positive_witnesses: usize,
    /// Zero or one positive measurement from the canonical route matrix.
    pub canonical_positive_observations: usize,
    pub observed_positive_count: usize,
    /// Joint classes after durations, sample counts, and event ticks are
    /// removed from identity.
    pub positive_alternative_count: usize,
    pub timing_or_exact_aliases_collapsed: usize,
    pub spatial_path_classes: usize,
    pub semantic_action_classes: usize,
    pub accepted_event_sequence_classes: usize,
    pub gate_path_style_classes: usize,
    /// Deterministic evidence order, not easiest-first ordering. Direct
    /// controller classes precede a novel canonical class, but canonical
    /// positives never participate in easiest-controller claims.
    pub alternatives: Vec<RouteAlternativeClass>,
    pub distances: RouteAlternativeDistanceReport,
}

/// Auditable identity and provenance of one timing-insensitive joint class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteAlternativeClass {
    pub members: usize,
    pub direct_controller_members: usize,
    pub canonical_matrix_members: usize,
    pub minimum_completion_ticks: usize,
    pub maximum_completion_ticks: usize,
    pub spatial_path: SpatialPathSignature,
    pub semantic_actions: Vec<SemanticAction>,
    pub accepted_events: Vec<SemanticEvent>,
    pub gate_path_style: GatePathStyleSignature,
}

/// Timing-insensitive coarse trajectory identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpatialPathSignature {
    pub grid_columns: u16,
    pub grid_rows: u16,
    /// Consecutive duplicate cells and their sample counts are removed.
    pub ordered_cells: Vec<TraversalCell>,
    pub visited_cells: Vec<TraversalCell>,
}

/// Observational path style plus coarse authored-route attribution.
///
/// Route-plan nodes are attributed when the observed player-center cell lies
/// over a generated support. Source and target port nodes are pinned exactly.
/// `candidate_edges` lists compatible authored edges between consecutive
/// attributed nodes; it does not claim the controller used one of them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GatePathStyleSignature {
    pub source_side: BoundarySide,
    pub target_side: BoundarySide,
    pub structural_node_sequence: Vec<u16>,
    pub structural_steps: Vec<StructuralPathStep>,
    pub accepted_gate_sequence: Vec<AcceptedGateEvent>,
    pub horizontal_motion_pattern: Vec<i8>,
    pub vertical_motion_pattern: Vec<i8>,
    pub vertical_excursion: VerticalExcursion,
}

/// Candidate graph edges compatible with one observed support transition.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StructuralPathStep {
    pub from_node: u16,
    pub to_node: u16,
    pub candidate_edges: Vec<OrientedRouteVerb>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrientedRouteVerb {
    pub verb: RouteVerb,
    pub orientation: AuthoredEdgeOrientation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AuthoredEdgeOrientation {
    Forward,
    Reverse,
}

/// Successful traversal-ability events, in authoritative event order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AcceptedGateEvent {
    WallJump(WallSide),
    Dash(DashDirection),
}

/// Excursion outside the vertical band between the endpoint support cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VerticalExcursion {
    WithinEndpointBand,
    AboveEndpointBand,
    BelowEndpointBand,
    AboveAndBelowEndpointBand,
}

/// Separate distance axes; there is deliberately no combined fun score.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteAlternativeDistanceReport {
    pub spatial_trajectory: WithinCellDistanceDistribution,
    pub semantic_actions: WithinCellDistanceDistribution,
    pub accepted_event_sequence: WithinCellDistanceDistribution,
    pub gate_path_style: WithinCellDistanceDistribution,
}

/// Pairwise and per-alternative nearest-neighbour distributions in `[0, 1]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WithinCellDistanceDistribution {
    pub pairwise: NormalizedDistanceSamples,
    pub nearest_neighbor: NormalizedDistanceSamples,
}

/// Deterministic empirical distribution. Percentiles use nearest-rank indices
/// `floor((n - 1) * p / 100)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NormalizedDistanceSamples {
    pub samples: usize,
    pub minimum: Option<f64>,
    pub p10: Option<f64>,
    pub median: Option<f64>,
    pub p90: Option<f64>,
    pub maximum: Option<f64>,
    pub mean: Option<f64>,
}

/// Per-loadout aggregation of within-cell route choices.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadoutRouteChoiceSummary {
    pub loadout: EvaluationLoadout,
    pub aggregate: RouteChoiceAggregate,
    pub operational_cost: RouteChoiceOperationalCost,
}

/// Transparent room/loadout aggregation. Count histograms preserve the shape
/// of route-choice evidence instead of collapsing it into one score.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteChoiceAggregate {
    pub expected_cells: usize,
    pub positive_cells: usize,
    pub positive_complete_audit_cells: usize,
    pub positive_bounded_audit_cells: usize,
    pub positive_missing_route_or_audit_cells: usize,
    pub complete_without_positive_cells: usize,
    pub bounded_inconclusive_without_positive_cells: usize,
    pub missing_route_cells: usize,
    pub missing_audit_cells: usize,
    pub cells_with_multiple_alternatives: usize,
    pub observed_positive_count: usize,
    pub positive_alternative_count: usize,
    pub timing_or_exact_aliases_collapsed: usize,
    pub alternative_count_histogram: BTreeMap<usize, usize>,
    pub spatial_path_class_histogram: BTreeMap<usize, usize>,
    pub semantic_action_class_histogram: BTreeMap<usize, usize>,
    pub accepted_event_class_histogram: BTreeMap<usize, usize>,
    pub gate_path_style_class_histogram: BTreeMap<usize, usize>,
    /// Pooled only from pairs and nearest neighbours inside the same cell.
    pub distances: RouteAlternativeDistanceReport,
}

/// Operational cost segregated from player-facing route-choice evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteChoiceOperationalCost {
    /// Existing direct-probe work, counted once per source/loadout audit.
    pub inherited_direct_controller_audit: SearchStats,
    /// Exact replay observations performed by this pass.
    pub replayed_direct_witnesses: usize,
    pub replayed_direct_ticks: usize,
    /// Canonical observations were already replayed during room analysis and
    /// are reused rather than charged as new simulation work here.
    pub reused_canonical_observations: usize,
}

/// Build observed alternatives for every directed door pair and exact loadout.
pub fn analyze_route_choice_diversity(
    evaluated: &EvaluatedCorpusRoom,
    analysis: &CorpusRoomAnalysis,
) -> Result<RoomRouteChoiceDiversity, RouteChoiceDiversityError> {
    if evaluated.generated.id != analysis.room_id {
        return Err(RouteChoiceDiversityError::RoomIdMismatch {
            evaluated: evaluated.generated.id.clone(),
            analysis: analysis.room_id.clone(),
        });
    }
    let candidate = evaluated.generated.variants.first().ok_or_else(|| {
        RouteChoiceDiversityError::MissingCanonicalVariant {
            room_id: analysis.room_id.clone(),
        }
    })?;
    analyze_route_choice_diversity_common(
        &evaluated.generated.id,
        &candidate.generated,
        &candidate.route_plan,
        &candidate.boundary_ports,
        &evaluated.matrices,
        analysis,
    )
}

/// Analyze route choices for a final-path room using its validated
/// post-feasibility canonical native candidate.
pub fn analyze_route_choice_diversity_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
) -> Result<RoomRouteChoiceDiversity, RouteChoiceDiversityError> {
    let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        RouteChoiceDiversityError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    analyze_route_choice_diversity_common(
        &evaluated.generated.id,
        candidate.generated(),
        candidate.route_plan(),
        candidate.boundary_ports(),
        &evaluated.matrices,
        analysis,
    )
}

/// Explicit-candidate final-path entry point for independent regenerators and
/// artifact verifiers. The candidate must exactly equal the validated
/// post-feasibility canonical retained candidate.
pub fn analyze_route_choice_diversity_for_corpus_candidate(
    candidate: &CorpusCandidate,
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
) -> Result<RoomRouteChoiceDiversity, RouteChoiceDiversityError> {
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        RouteChoiceDiversityError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    if candidate != canonical {
        return Err(RouteChoiceDiversityError::CorpusCandidateMismatch {
            room_id: evaluated.generated.id.clone(),
        });
    }
    if analysis.room_id != evaluated.generated.id {
        return Err(RouteChoiceDiversityError::RoomIdMismatch {
            evaluated: evaluated.generated.id.clone(),
            analysis: analysis.room_id.clone(),
        });
    }
    analyze_route_choice_diversity_common(
        &evaluated.generated.id,
        candidate.generated(),
        candidate.route_plan(),
        candidate.boundary_ports(),
        &evaluated.matrices,
        analysis,
    )
}

fn analyze_route_choice_diversity_common(
    room_id: &RoomId,
    generated: &GeneratedLevel,
    route_plan: &RoutePlan,
    boundary_ports: &[BoundaryPort],
    matrices: &[LoadoutRouteMatrix],
    analysis: &CorpusRoomAnalysis,
) -> Result<RoomRouteChoiceDiversity, RouteChoiceDiversityError> {
    if *room_id != analysis.room_id {
        return Err(RouteChoiceDiversityError::RoomIdMismatch {
            evaluated: room_id.clone(),
            analysis: analysis.room_id.clone(),
        });
    }
    if analysis.version != CORPUS_ROOM_ANALYSIS_VERSION {
        return Err(RouteChoiceDiversityError::AnalysisVersion {
            expected: CORPUS_ROOM_ANALYSIS_VERSION,
            actual: analysis.version,
        });
    }
    analysis
        .config
        .validate()
        .map_err(|source| RouteChoiceDiversityError::AnalysisConfig { source })?;
    let context = PathContext::new(generated, route_plan, boundary_ports)?;
    validate_canonical_measurement_bindings(&generated.room, matrices, analysis)?;
    let indexed = IndexedAnalysis::new(analysis, &context.door_ids)?;

    let mut cells = Vec::with_capacity(
        context
            .door_ids
            .len()
            .saturating_mul(context.door_ids.len().saturating_sub(1))
            .saturating_mul(EvaluationLoadout::ALL.len()),
    );
    let mut samples_by_cell = Vec::with_capacity(cells.capacity());
    let mut replay_cost_by_loadout =
        BTreeMap::<EvaluationLoadout, RouteChoiceOperationalCost>::new();

    for loadout in EvaluationLoadout::ALL {
        for source_door_id in &context.door_ids {
            for target_door_id in context
                .door_ids
                .iter()
                .filter(|target| *target != source_door_id)
            {
                let key = (source_door_id.clone(), target_door_id.clone(), loadout);
                let route_key = (source_door_id.clone(), target_door_id.clone());
                let route = indexed.routes.get(&route_key).copied();
                let audit = route
                    .and_then(|route| route.audits.iter().find(|audit| audit.loadout == loadout));
                let canonical = indexed.canonical.get(&key).copied();
                let built = build_cell(
                    &context,
                    source_door_id,
                    target_door_id,
                    loadout,
                    route,
                    audit,
                    canonical,
                )?;
                let cost = replay_cost_by_loadout.entry(loadout).or_default();
                cost.replayed_direct_witnesses = cost
                    .replayed_direct_witnesses
                    .saturating_add(built.replayed_direct_witnesses);
                cost.replayed_direct_ticks = cost
                    .replayed_direct_ticks
                    .saturating_add(built.replayed_direct_ticks);
                cost.reused_canonical_observations = cost
                    .reused_canonical_observations
                    .saturating_add(built.reused_canonical_observations);
                cells.push(DirectedRouteChoiceCell {
                    source_door_id: source_door_id.clone(),
                    target_door_id: target_door_id.clone(),
                    loadout,
                    evidence: built.evidence,
                });
                samples_by_cell.push(built.samples);
            }
        }
    }

    let mut direct_cost_by_loadout = BTreeMap::<EvaluationLoadout, SearchStats>::new();
    for batch in &analysis.source_route_assessments {
        for audit in &batch.shared_audits {
            accumulate_search_stats(
                direct_cost_by_loadout.entry(audit.loadout).or_default(),
                audit.operational_stats,
            );
        }
    }
    let recomputed_direct_cost = direct_cost_by_loadout.values().copied().fold(
        SearchStats::default(),
        |mut total, additional| {
            accumulate_search_stats(&mut total, additional);
            total
        },
    );
    if recomputed_direct_cost != analysis.direct_controller_operational_stats {
        return Err(RouteChoiceDiversityError::OperationalCostMismatch {
            reported: analysis.direct_controller_operational_stats,
            recomputed: recomputed_direct_cost,
        });
    }

    let by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| {
            let selected = cells
                .iter()
                .zip(&samples_by_cell)
                .filter(|(cell, _)| cell.loadout == loadout)
                .collect::<Vec<_>>();
            let mut operational_cost = replay_cost_by_loadout
                .get(&loadout)
                .copied()
                .unwrap_or_default();
            operational_cost.inherited_direct_controller_audit = direct_cost_by_loadout
                .get(&loadout)
                .copied()
                .unwrap_or_default();
            LoadoutRouteChoiceSummary {
                loadout,
                aggregate: aggregate_cells(&selected),
                operational_cost,
            }
        })
        .collect::<Vec<_>>();
    let all_cells = cells.iter().zip(&samples_by_cell).collect::<Vec<_>>();
    let room = aggregate_cells(&all_cells);
    let replayed_direct_witnesses = replay_cost_by_loadout
        .values()
        .map(|cost| cost.replayed_direct_witnesses)
        .sum();
    let replayed_direct_ticks = replay_cost_by_loadout
        .values()
        .map(|cost| cost.replayed_direct_ticks)
        .sum();
    let reused_canonical_observations = replay_cost_by_loadout
        .values()
        .map(|cost| cost.reused_canonical_observations)
        .sum();
    let operational_cost = RouteChoiceOperationalCost {
        inherited_direct_controller_audit: analysis.direct_controller_operational_stats,
        replayed_direct_witnesses,
        replayed_direct_ticks,
        reused_canonical_observations,
    };

    Ok(RoomRouteChoiceDiversity {
        version: ROUTE_CHOICE_DIVERSITY_VERSION,
        source_analysis_version: analysis.version,
        source_analysis_config: analysis.config.clone(),
        room_id: analysis.room_id.clone(),
        cells,
        by_loadout,
        room,
        operational_cost,
    })
}

fn validate_canonical_measurement_bindings(
    room: &downwards_core::Room,
    matrices: &[LoadoutRouteMatrix],
    analysis: &CorpusRoomAnalysis,
) -> Result<(), RouteChoiceDiversityError> {
    let door_ids = room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<BTreeSet<_>>();
    if door_ids.len() != room.doors().len() {
        return Err(RouteChoiceDiversityError::DuplicateDoorId {
            door_id: room
                .doors()
                .iter()
                .find_map(|door| {
                    (room
                        .doors()
                        .iter()
                        .filter(|other| other.id == door.id)
                        .count()
                        > 1)
                    .then(|| door.id.clone())
                })
                .unwrap_or_default(),
        });
    }
    let expected = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect::<BTreeSet<_>>();
    let mut consumed_measurements = BTreeSet::new();
    for loadout in EvaluationLoadout::ALL {
        let matching = matrices
            .iter()
            .filter(|matrix| matrix.loadout == loadout)
            .collect::<Vec<_>>();
        let [matrix] = matching.as_slice() else {
            return Err(RouteChoiceDiversityError::MatrixCardinality {
                loadout,
                count: matching.len(),
            });
        };
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(RouteChoiceDiversityError::MatrixLoadoutMismatch { loadout });
        }
        let actual = matrix
            .evidence
            .door_routes()
            .iter()
            .map(|row| (row.source_door_id.clone(), row.target_door_id.clone()))
            .collect::<BTreeSet<_>>();
        if actual.len() != matrix.evidence.door_routes().len() || actual != expected {
            return Err(RouteChoiceDiversityError::MatrixDoorCoordinates { loadout });
        }
        for row in matrix.evidence.door_routes() {
            let BoundedTargetEvidence::Positive(positive) = &row.evidence else {
                continue;
            };
            let solution = positive.solution();
            if solution.target != SearchTarget::door(&row.target_door_id)
                || solution.reached != ReachedTarget::Door(row.target_door_id.clone())
            {
                return Err(RouteChoiceDiversityError::MatrixPositiveTargetMismatch {
                    source: row.source_door_id.clone(),
                    target: row.target_door_id.clone(),
                    loadout,
                });
            }
            let matches = analysis
                .canonical_route_measurements
                .iter()
                .enumerate()
                .filter(|(_, measurement)| {
                    measurement.loadout == loadout
                        && measurement.source_door_id == row.source_door_id
                        && measurement.target_door_id == row.target_door_id
                        && measurement.witness_fingerprint == positive.witness_fingerprint()
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [measurement_index] = matches.as_slice() else {
                return Err(RouteChoiceDiversityError::CanonicalMeasurementBinding {
                    source: row.source_door_id.clone(),
                    target: row.target_door_id.clone(),
                    loadout,
                    count: matches.len(),
                });
            };
            consumed_measurements.insert(*measurement_index);
        }
    }
    if consumed_measurements.len() != analysis.canonical_route_measurements.len() {
        return Err(RouteChoiceDiversityError::UnboundCanonicalMeasurements {
            consumed: consumed_measurements.len(),
            total: analysis.canonical_route_measurements.len(),
        });
    }
    Ok(())
}

type RouteKey = (String, String);
type RouteLoadoutKey = (String, String, EvaluationLoadout);

struct IndexedAnalysis<'a> {
    routes: BTreeMap<RouteKey, &'a RouteControllerAssessment>,
    canonical: BTreeMap<RouteLoadoutKey, &'a super::CorpusRouteMeasurement>,
}

impl<'a> IndexedAnalysis<'a> {
    fn new(
        analysis: &'a CorpusRoomAnalysis,
        expected_doors: &[String],
    ) -> Result<Self, RouteChoiceDiversityError> {
        let expected = expected_doors
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut routes = BTreeMap::new();
        let mut source_batches = BTreeSet::new();
        for batch in &analysis.source_route_assessments {
            if !expected.contains(batch.source_door_id.as_str()) {
                return Err(RouteChoiceDiversityError::UnknownDoorInAnalysis {
                    door_id: batch.source_door_id.clone(),
                });
            }
            if !source_batches.insert(batch.source_door_id.clone()) {
                return Err(RouteChoiceDiversityError::DuplicateSourceAssessmentBatch {
                    source: batch.source_door_id.clone(),
                });
            }
            let mut shared_loadouts = BTreeSet::new();
            for audit in &batch.shared_audits {
                if !shared_loadouts.insert(audit.loadout) {
                    return Err(RouteChoiceDiversityError::DuplicateSharedLoadoutAudit {
                        source: batch.source_door_id.clone(),
                        loadout: audit.loadout,
                    });
                }
            }
            for route in &batch.routes {
                if route.source_door_id != batch.source_door_id {
                    return Err(RouteChoiceDiversityError::RouteSourceMismatch {
                        batch_source: batch.source_door_id.clone(),
                        route_source: route.source_door_id.clone(),
                        target: route.target_door_id.clone(),
                    });
                }
                if !expected.contains(route.target_door_id.as_str()) {
                    return Err(RouteChoiceDiversityError::UnknownDoorInAnalysis {
                        door_id: route.target_door_id.clone(),
                    });
                }
                if route.source_door_id == route.target_door_id {
                    return Err(RouteChoiceDiversityError::SameSourceAndTarget {
                        door_id: route.source_door_id.clone(),
                    });
                }
                let mut audit_loadouts = BTreeSet::new();
                for audit in &route.audits {
                    if !audit_loadouts.insert(audit.loadout) {
                        return Err(RouteChoiceDiversityError::DuplicateLoadoutAudit {
                            source: route.source_door_id.clone(),
                            target: route.target_door_id.clone(),
                            loadout: audit.loadout,
                        });
                    }
                }
                let key = (route.source_door_id.clone(), route.target_door_id.clone());
                if routes.insert(key.clone(), route).is_some() {
                    return Err(RouteChoiceDiversityError::DuplicateRouteAssessment {
                        source: key.0,
                        target: key.1,
                    });
                }
            }
        }

        let mut canonical = BTreeMap::new();
        for measurement in &analysis.canonical_route_measurements {
            if measurement.version != CORPUS_ROUTE_MEASUREMENT_VERSION {
                return Err(RouteChoiceDiversityError::CanonicalMeasurementVersion {
                    expected: CORPUS_ROUTE_MEASUREMENT_VERSION,
                    actual: measurement.version,
                });
            }
            if !expected.contains(measurement.source_door_id.as_str()) {
                return Err(RouteChoiceDiversityError::UnknownDoorInAnalysis {
                    door_id: measurement.source_door_id.clone(),
                });
            }
            if !expected.contains(measurement.target_door_id.as_str()) {
                return Err(RouteChoiceDiversityError::UnknownDoorInAnalysis {
                    door_id: measurement.target_door_id.clone(),
                });
            }
            if measurement.source_door_id == measurement.target_door_id {
                return Err(RouteChoiceDiversityError::SameSourceAndTarget {
                    door_id: measurement.source_door_id.clone(),
                });
            }
            if measurement.observation.reached_exit_id != measurement.target_door_id {
                return Err(RouteChoiceDiversityError::CanonicalTargetMismatch {
                    source: measurement.source_door_id.clone(),
                    target: measurement.target_door_id.clone(),
                    loadout: measurement.loadout,
                    reached: measurement.observation.reached_exit_id.clone(),
                });
            }
            if measurement.vector.target_id != measurement.target_door_id {
                return Err(RouteChoiceDiversityError::CanonicalVectorTargetMismatch {
                    source: measurement.source_door_id.clone(),
                    target: measurement.target_door_id.clone(),
                    loadout: measurement.loadout,
                    vector_target: measurement.vector.target_id.clone(),
                });
            }
            let key = (
                measurement.source_door_id.clone(),
                measurement.target_door_id.clone(),
                measurement.loadout,
            );
            if canonical.insert(key.clone(), measurement).is_some() {
                return Err(RouteChoiceDiversityError::DuplicateCanonicalPositive {
                    source: key.0,
                    target: key.1,
                    loadout: key.2,
                });
            }
        }
        Ok(Self { routes, canonical })
    }
}

struct PathContext<'a> {
    room: &'a downwards_core::Room,
    route_plan: &'a RoutePlan,
    door_ids: Vec<String>,
    doors: BTreeMap<String, DoorPathEndpoint>,
    node_support_cells: BTreeMap<u16, SupportCell>,
}

#[derive(Clone, Copy)]
struct DoorPathEndpoint {
    side: BoundarySide,
    node_id: u16,
}

#[derive(Clone, Copy)]
struct SupportCell {
    minimum_x: u16,
    maximum_x: u16,
    standing_y: u16,
    center_x: u16,
}

impl<'a> PathContext<'a> {
    fn new(
        generated: &'a GeneratedLevel,
        route_plan: &'a RoutePlan,
        boundary_ports: &'a [BoundaryPort],
    ) -> Result<Self, RouteChoiceDiversityError> {
        let mut door_ids = generated
            .room
            .doors()
            .iter()
            .map(|door| door.id.clone())
            .collect::<Vec<_>>();
        door_ids.sort_unstable();
        if let Some(pair) = door_ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(RouteChoiceDiversityError::DuplicateDoorId {
                door_id: pair[0].clone(),
            });
        }

        let mut route_nodes = BTreeMap::new();
        for node in &route_plan.nodes {
            if route_nodes.insert(node.id, node.support).is_some() {
                return Err(RouteChoiceDiversityError::DuplicateRouteNodeId { node_id: node.id });
            }
        }
        let room_doors = generated
            .room
            .doors()
            .iter()
            .map(|door| (door.id.as_str(), door))
            .collect::<BTreeMap<_, _>>();
        let mut doors = BTreeMap::new();
        for port in boundary_ports {
            let Some(room_door) = room_doors.get(port.door.id.as_str()) else {
                return Err(RouteChoiceDiversityError::UnknownBoundaryPortDoor {
                    door_id: port.door.id.clone(),
                });
            };
            if **room_door != port.door {
                return Err(RouteChoiceDiversityError::BoundaryPortGeometryMismatch {
                    door_id: port.door.id.clone(),
                });
            }
            if !route_nodes.contains_key(&port.node_id) {
                return Err(RouteChoiceDiversityError::PortReferencesUnknownNode {
                    door_id: port.door.id.clone(),
                    node_id: port.node_id,
                });
            }
            if doors
                .insert(
                    port.door.id.clone(),
                    DoorPathEndpoint {
                        side: port.door.side,
                        node_id: port.node_id,
                    },
                )
                .is_some()
            {
                return Err(RouteChoiceDiversityError::DuplicateBoundaryPort {
                    door_id: port.door.id.clone(),
                });
            }
        }
        for door_id in &door_ids {
            if !doors.contains_key(door_id) {
                return Err(RouteChoiceDiversityError::MissingBoundaryPort {
                    door_id: door_id.clone(),
                });
            }
        }

        let grid = downwards_lab::TraversalGrid::default();
        let node_support_cells = route_nodes
            .into_iter()
            .map(|(node_id, support)| {
                (
                    node_id,
                    support_cell(
                        support,
                        generated.room.tile_size(),
                        generated.room.width(),
                        generated.room.height(),
                        grid,
                    ),
                )
            })
            .collect();
        Ok(Self {
            room: &generated.room,
            route_plan,
            door_ids,
            doors,
            node_support_cells,
        })
    }

    fn style(
        &self,
        source_door_id: &str,
        target_door_id: &str,
        observation: &SuccessfulWitnessObservation,
    ) -> GatePathStyleSignature {
        let source = self.doors[source_door_id];
        let target = self.doors[target_door_id];
        let spatial = spatial_signature(observation);
        let mut structural_node_sequence = vec![source.node_id];
        for &cell in &spatial.ordered_cells {
            if let Some(node_id) = self.closest_support_node(cell)
                && structural_node_sequence.last().copied() != Some(node_id)
            {
                structural_node_sequence.push(node_id);
            }
        }
        if structural_node_sequence.last().copied() != Some(target.node_id) {
            structural_node_sequence.push(target.node_id);
        }
        let structural_steps = structural_node_sequence
            .windows(2)
            .map(|pair| StructuralPathStep {
                from_node: pair[0],
                to_node: pair[1],
                candidate_edges: candidate_edges(self.route_plan, pair[0], pair[1]),
            })
            .collect();
        let accepted_gate_sequence = observation
            .actions
            .events
            .iter()
            .filter_map(|event| match event.event {
                SemanticEvent::WallJump(side) => Some(AcceptedGateEvent::WallJump(side)),
                SemanticEvent::Dash(direction) => Some(AcceptedGateEvent::Dash(direction)),
                _ => None,
            })
            .collect();
        let horizontal_motion_pattern = motion_pattern(&spatial.ordered_cells, |cell| cell.x);
        let vertical_motion_pattern = motion_pattern(&spatial.ordered_cells, |cell| cell.y);
        let source_y = self.node_support_cells[&source.node_id].standing_y;
        let target_y = self.node_support_cells[&target.node_id].standing_y;
        let band_min = source_y.min(target_y);
        let band_max = source_y.max(target_y);
        let above = spatial.ordered_cells.iter().any(|cell| cell.y < band_min);
        let below = spatial.ordered_cells.iter().any(|cell| cell.y > band_max);
        let vertical_excursion = match (above, below) {
            (false, false) => VerticalExcursion::WithinEndpointBand,
            (true, false) => VerticalExcursion::AboveEndpointBand,
            (false, true) => VerticalExcursion::BelowEndpointBand,
            (true, true) => VerticalExcursion::AboveAndBelowEndpointBand,
        };

        GatePathStyleSignature {
            source_side: source.side,
            target_side: target.side,
            structural_node_sequence,
            structural_steps,
            accepted_gate_sequence,
            horizontal_motion_pattern,
            vertical_motion_pattern,
            vertical_excursion,
        }
    }

    fn closest_support_node(&self, cell: TraversalCell) -> Option<u16> {
        self.node_support_cells
            .iter()
            .filter(|(_, support)| {
                cell.y == support.standing_y
                    && cell.x >= support.minimum_x
                    && cell.x <= support.maximum_x
            })
            .min_by_key(|(node_id, support)| (cell.x.abs_diff(support.center_x), **node_id))
            .map(|(&node_id, _)| node_id)
    }
}

fn support_cell(
    support: SupportSpec,
    tile_size: i32,
    room_width_tiles: u16,
    room_height_tiles: u16,
    grid: downwards_lab::TraversalGrid,
) -> SupportCell {
    let room_width = i64::from(room_width_tiles) * i64::from(tile_size);
    let room_height = i64::from(room_height_tiles) * i64::from(tile_size);
    let to_grid_x = |pixel: i64| {
        ((pixel.clamp(0, room_width - 1) * i64::from(grid.columns())) / room_width) as u16
    };
    let standing_center_y = i64::from(support.row) * i64::from(tile_size)
        - i64::from(downwards_core::PLAYER_HEIGHT / 2);
    let standing_y = ((standing_center_y.clamp(0, room_height - 1) * i64::from(grid.rows()))
        / room_height) as u16;
    let minimum_x = to_grid_x(i64::from(support.start_x) * i64::from(tile_size));
    let maximum_x = to_grid_x(i64::from(support.end_x) * i64::from(tile_size) - 1);
    SupportCell {
        minimum_x,
        maximum_x,
        standing_y,
        center_x: minimum_x + maximum_x.saturating_sub(minimum_x) / 2,
    }
}

fn candidate_edges(plan: &RoutePlan, from: u16, to: u16) -> Vec<OrientedRouteVerb> {
    let mut result = plan
        .edges
        .iter()
        .filter_map(|edge| {
            if edge.from == from && edge.to == to {
                Some(OrientedRouteVerb {
                    verb: edge.verb,
                    orientation: AuthoredEdgeOrientation::Forward,
                })
            } else if edge.from == to && edge.to == from {
                Some(OrientedRouteVerb {
                    verb: edge.verb,
                    orientation: AuthoredEdgeOrientation::Reverse,
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result.dedup();
    result
}

fn motion_pattern(cells: &[TraversalCell], coordinate: impl Fn(TraversalCell) -> u16) -> Vec<i8> {
    let mut result = Vec::new();
    for pair in cells.windows(2) {
        let direction = coordinate(pair[1]).cmp(&coordinate(pair[0])) as i8;
        if direction != 0 && result.last().copied() != Some(direction) {
            result.push(direction);
        }
    }
    result
}

struct BuiltCell {
    evidence: RouteChoiceCellEvidence,
    samples: RouteAlternativeDistanceSamples,
    replayed_direct_witnesses: usize,
    replayed_direct_ticks: usize,
    reused_canonical_observations: usize,
}

fn build_cell(
    context: &PathContext<'_>,
    source_door_id: &str,
    target_door_id: &str,
    loadout: EvaluationLoadout,
    route: Option<&RouteControllerAssessment>,
    audit: Option<&LoadoutControllerAudit>,
    canonical: Option<&super::CorpusRouteMeasurement>,
) -> Result<BuiltCell, RouteChoiceDiversityError> {
    let direct_witnesses = route
        .into_iter()
        .flat_map(|route| &route.easiest_first_witnesses)
        .filter(|witness| witness.loadout == loadout)
        .collect::<Vec<_>>();
    if audit.is_none() && !direct_witnesses.is_empty() {
        return Err(RouteChoiceDiversityError::WitnessesWithoutAudit {
            source: source_door_id.to_owned(),
            target: target_door_id.to_owned(),
            loadout,
            witnesses: direct_witnesses.len(),
        });
    }
    if let Some(audit) = audit
        && (audit.retained_semantic_witnesses != direct_witnesses.len()
            || audit.raw_positive_witnesses < audit.retained_semantic_witnesses)
    {
        return Err(RouteChoiceDiversityError::DirectWitnessCountMismatch {
            source: source_door_id.to_owned(),
            target: target_door_id.to_owned(),
            loadout,
            audit_raw: audit.raw_positive_witnesses,
            audit_retained: audit.retained_semantic_witnesses,
            stored_retained: direct_witnesses.len(),
        });
    }

    let audit_status = match (route, audit) {
        (None, _) => PositiveAlternativeAuditStatus::MissingDirectedRouteAssessment,
        (Some(_), None) => PositiveAlternativeAuditStatus::MissingLoadoutAudit,
        (Some(_), Some(audit)) => match audit.status {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                PositiveAlternativeAuditStatus::CompleteFiniteVocabulary
            }
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                PositiveAlternativeAuditStatus::BoundedIncomplete { limit }
            }
        },
    };

    let mut observed =
        Vec::with_capacity(direct_witnesses.len() + usize::from(canonical.is_some()));
    if !direct_witnesses.is_empty() {
        let initial =
            Simulation::enter_via_door(context.room.clone(), loadout.abilities(), source_door_id)
                .map_err(|source| RouteChoiceDiversityError::DoorEntry {
                source_door_id: source_door_id.to_owned(),
                loadout,
                source: Box::new(source),
            })?;
        for witness in direct_witnesses {
            let observation = observe_successful_replay(
                &initial,
                &witness.replay,
                downwards_lab::TraversalGrid::default(),
            )
            .map_err(
                |source| RouteChoiceDiversityError::DirectReplayObservation {
                    source_door_id: source_door_id.to_owned(),
                    target_door_id: target_door_id.to_owned(),
                    loadout,
                    source: Box::new(source),
                },
            )?;
            if observation.reached_exit_id != target_door_id {
                return Err(RouteChoiceDiversityError::DirectTargetMismatch {
                    source: source_door_id.to_owned(),
                    target: target_door_id.to_owned(),
                    loadout,
                    reached: observation.reached_exit_id,
                });
            }
            if observation.actions != witness.semantic_trace {
                return Err(RouteChoiceDiversityError::StoredSemanticTraceMismatch {
                    source: source_door_id.to_owned(),
                    target: target_door_id.to_owned(),
                    loadout,
                });
            }
            observed.push(ObservedPositive {
                provenance: ObservationProvenance::DirectController,
                observation,
            });
        }
    }
    if let Some(canonical) = canonical {
        observed.push(ObservedPositive {
            provenance: ObservationProvenance::CanonicalMatrix,
            observation: canonical.observation.clone(),
        });
    }

    let replayed_direct_witnesses = observed
        .iter()
        .filter(|observation| observation.provenance == ObservationProvenance::DirectController)
        .count();
    let replayed_direct_ticks = observed
        .iter()
        .filter(|observation| observation.provenance == ObservationProvenance::DirectController)
        .map(|observation| observation.observation.completion_ticks)
        .sum();
    let reused_canonical_observations = usize::from(canonical.is_some());

    if observed.is_empty() {
        let evidence = match (route, audit) {
            (None, _) => RouteChoiceCellEvidence::MissingDirectedRouteAssessment,
            (Some(_), None) => RouteChoiceCellEvidence::MissingLoadoutAudit,
            (Some(_), Some(audit)) => match audit.status {
                LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                    RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary
                }
                LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                    RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive { limit }
                }
            },
        };
        return Ok(BuiltCell {
            evidence,
            samples: RouteAlternativeDistanceSamples::default(),
            replayed_direct_witnesses,
            replayed_direct_ticks,
            reused_canonical_observations,
        });
    }

    let (set, samples) = build_positive_set(
        context,
        source_door_id,
        target_door_id,
        audit_status,
        audit,
        observed,
    );
    Ok(BuiltCell {
        evidence: RouteChoiceCellEvidence::Positive(Box::new(set)),
        samples,
        replayed_direct_witnesses,
        replayed_direct_ticks,
        reused_canonical_observations,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationProvenance {
    DirectController,
    CanonicalMatrix,
}

struct ObservedPositive {
    provenance: ObservationProvenance,
    observation: SuccessfulWitnessObservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AlternativeKey {
    spatial_path: SpatialPathSignature,
    semantic_actions: Vec<SemanticAction>,
    accepted_events: Vec<SemanticEvent>,
    gate_path_style: GatePathStyleSignature,
}

struct WorkingAlternative {
    class: RouteAlternativeClass,
    representative: SuccessfulWitnessObservation,
    key: AlternativeKey,
}

fn build_positive_set(
    context: &PathContext<'_>,
    source_door_id: &str,
    target_door_id: &str,
    audit_status: PositiveAlternativeAuditStatus,
    audit: Option<&LoadoutControllerAudit>,
    observed: Vec<ObservedPositive>,
) -> (RouteAlternativeSet, RouteAlternativeDistanceSamples) {
    let observed_positive_count = observed.len();
    let retained_direct_positive_witnesses = observed
        .iter()
        .filter(|observation| observation.provenance == ObservationProvenance::DirectController)
        .count();
    let canonical_positive_observations =
        observed_positive_count.saturating_sub(retained_direct_positive_witnesses);
    let mut alternatives = Vec::<WorkingAlternative>::new();
    for observed in observed {
        let spatial_path = spatial_signature(&observed.observation);
        let semantic_actions = observed
            .observation
            .actions
            .spans
            .iter()
            .map(|span| span.action)
            .collect::<Vec<_>>();
        let accepted_events = observed
            .observation
            .actions
            .events
            .iter()
            .map(|event| event.event)
            .collect::<Vec<_>>();
        let gate_path_style = context.style(source_door_id, target_door_id, &observed.observation);
        let key = AlternativeKey {
            spatial_path: spatial_path.clone(),
            semantic_actions: semantic_actions.clone(),
            accepted_events: accepted_events.clone(),
            gate_path_style: gate_path_style.clone(),
        };
        if let Some(existing) = alternatives
            .iter_mut()
            .find(|alternative| alternative.key == key)
        {
            existing.class.members += 1;
            match observed.provenance {
                ObservationProvenance::DirectController => {
                    existing.class.direct_controller_members += 1;
                }
                ObservationProvenance::CanonicalMatrix => {
                    existing.class.canonical_matrix_members += 1;
                }
            }
            existing.class.minimum_completion_ticks = existing
                .class
                .minimum_completion_ticks
                .min(observed.observation.completion_ticks);
            existing.class.maximum_completion_ticks = existing
                .class
                .maximum_completion_ticks
                .max(observed.observation.completion_ticks);
            continue;
        }
        let (direct_controller_members, canonical_matrix_members) = match observed.provenance {
            ObservationProvenance::DirectController => (1, 0),
            ObservationProvenance::CanonicalMatrix => (0, 1),
        };
        alternatives.push(WorkingAlternative {
            class: RouteAlternativeClass {
                members: 1,
                direct_controller_members,
                canonical_matrix_members,
                minimum_completion_ticks: observed.observation.completion_ticks,
                maximum_completion_ticks: observed.observation.completion_ticks,
                spatial_path,
                semantic_actions,
                accepted_events,
                gate_path_style,
            },
            representative: observed.observation,
            key,
        });
    }

    let spatial_path_classes =
        class_count(&alternatives, |alternative| &alternative.key.spatial_path);
    let semantic_action_classes = class_count(&alternatives, |alternative| {
        &alternative.key.semantic_actions
    });
    let accepted_event_sequence_classes = class_count(&alternatives, |alternative| {
        &alternative.key.accepted_events
    });
    let gate_path_style_classes = class_count(&alternatives, |alternative| {
        &alternative.key.gate_path_style
    });
    let samples = alternative_distances(&alternatives);
    let distances = samples.summarize();
    let positive_alternative_count = alternatives.len();
    let timing_or_exact_aliases_collapsed =
        observed_positive_count.saturating_sub(positive_alternative_count);
    let classes = alternatives
        .into_iter()
        .map(|alternative| alternative.class)
        .collect();
    (
        RouteAlternativeSet {
            direct_audit_status: audit_status,
            raw_direct_positive_witnesses: audit.map_or(0, |audit| audit.raw_positive_witnesses),
            retained_direct_positive_witnesses,
            canonical_positive_observations,
            observed_positive_count,
            positive_alternative_count,
            timing_or_exact_aliases_collapsed,
            spatial_path_classes,
            semantic_action_classes,
            accepted_event_sequence_classes,
            gate_path_style_classes,
            alternatives: classes,
            distances,
        },
        samples,
    )
}

fn spatial_signature(observation: &SuccessfulWitnessObservation) -> SpatialPathSignature {
    SpatialPathSignature {
        grid_columns: observation.traversal.grid.columns(),
        grid_rows: observation.traversal.grid.rows(),
        ordered_cells: observation
            .traversal
            .spans
            .iter()
            .map(|span| span.cell)
            .collect(),
        visited_cells: observation.traversal.visited_cells.to_vec(),
    }
}

fn class_count<'a, T: PartialEq + ?Sized + 'a>(
    alternatives: &'a [WorkingAlternative],
    projection: impl Fn(&'a WorkingAlternative) -> &'a T,
) -> usize {
    let mut representatives = Vec::<usize>::new();
    for (index, candidate) in alternatives.iter().enumerate() {
        if representatives.iter().all(|&representative| {
            projection(candidate) != projection(&alternatives[representative])
        }) {
            representatives.push(index);
        }
    }
    representatives.len()
}

#[derive(Clone, Debug, Default)]
struct AxisSamples {
    pairwise: Vec<f64>,
    nearest: Vec<f64>,
}

impl AxisSamples {
    fn summarize(&self) -> WithinCellDistanceDistribution {
        WithinCellDistanceDistribution {
            pairwise: summarize_samples(&self.pairwise),
            nearest_neighbor: summarize_samples(&self.nearest),
        }
    }

    fn extend(&mut self, other: &Self) {
        self.pairwise.extend_from_slice(&other.pairwise);
        self.nearest.extend_from_slice(&other.nearest);
    }
}

#[derive(Clone, Debug, Default)]
struct RouteAlternativeDistanceSamples {
    spatial: AxisSamples,
    actions: AxisSamples,
    events: AxisSamples,
    style: AxisSamples,
}

impl RouteAlternativeDistanceSamples {
    fn summarize(&self) -> RouteAlternativeDistanceReport {
        RouteAlternativeDistanceReport {
            spatial_trajectory: self.spatial.summarize(),
            semantic_actions: self.actions.summarize(),
            accepted_event_sequence: self.events.summarize(),
            gate_path_style: self.style.summarize(),
        }
    }

    fn extend(&mut self, other: &Self) {
        self.spatial.extend(&other.spatial);
        self.actions.extend(&other.actions);
        self.events.extend(&other.events);
        self.style.extend(&other.style);
    }
}

fn alternative_distances(alternatives: &[WorkingAlternative]) -> RouteAlternativeDistanceSamples {
    if alternatives.len() < 2 {
        return RouteAlternativeDistanceSamples::default();
    }
    let mut samples = RouteAlternativeDistanceSamples::default();
    let mut spatial_nearest = vec![f64::INFINITY; alternatives.len()];
    let mut action_nearest = vec![f64::INFINITY; alternatives.len()];
    let mut event_nearest = vec![f64::INFINITY; alternatives.len()];
    let mut style_nearest = vec![f64::INFINITY; alternatives.len()];
    for left in 0..alternatives.len() {
        for right in left + 1..alternatives.len() {
            let left_observation = &alternatives[left].representative;
            let right_observation = &alternatives[right].representative;
            let traversal =
                traversal_distance(&left_observation.traversal, &right_observation.traversal);
            // The lab's time-resampled ordered-path component is useful for
            // pacing studies but would let waiting duration affect route
            // diversity. Use its grid/set components plus a timing-free edit
            // distance over the coarse cell sequence instead.
            let ordered_cells = normalized_edit_distance(
                &alternatives[left].key.spatial_path.ordered_cells,
                &alternatives[right].key.spatial_path.ordered_cells,
            );
            let spatial = (traversal.grid + traversal.visited_cells + ordered_cells) / 3.0;
            let semantic =
                semantic_action_distance(&left_observation.actions, &right_observation.actions);
            // Time-aligned sampled input and duration belong to pacing, not
            // timing-insensitive route-choice identity.
            let actions = semantic.transitions;
            let events = semantic.events;
            let style = gate_path_style_distance(
                &alternatives[left].key.gate_path_style,
                &alternatives[right].key.gate_path_style,
            );
            samples.spatial.pairwise.push(spatial);
            samples.actions.pairwise.push(actions);
            samples.events.pairwise.push(events);
            samples.style.pairwise.push(style);
            spatial_nearest[left] = spatial_nearest[left].min(spatial);
            spatial_nearest[right] = spatial_nearest[right].min(spatial);
            action_nearest[left] = action_nearest[left].min(actions);
            action_nearest[right] = action_nearest[right].min(actions);
            event_nearest[left] = event_nearest[left].min(events);
            event_nearest[right] = event_nearest[right].min(events);
            style_nearest[left] = style_nearest[left].min(style);
            style_nearest[right] = style_nearest[right].min(style);
        }
    }
    samples.spatial.nearest = spatial_nearest;
    samples.actions.nearest = action_nearest;
    samples.events.nearest = event_nearest;
    samples.style.nearest = style_nearest;
    samples
}

fn gate_path_style_distance(left: &GatePathStyleSignature, right: &GatePathStyleSignature) -> f64 {
    let endpoints = (usize::from(left.source_side != right.source_side)
        + usize::from(left.target_side != right.target_side)) as f64
        / 2.0;
    let nodes = normalized_edit_distance(
        &left.structural_node_sequence,
        &right.structural_node_sequence,
    );
    let steps = normalized_edit_distance(&left.structural_steps, &right.structural_steps);
    let gates =
        normalized_edit_distance(&left.accepted_gate_sequence, &right.accepted_gate_sequence);
    let horizontal = normalized_edit_distance(
        &left.horizontal_motion_pattern,
        &right.horizontal_motion_pattern,
    );
    let vertical = normalized_edit_distance(
        &left.vertical_motion_pattern,
        &right.vertical_motion_pattern,
    );
    let excursion = f64::from(left.vertical_excursion != right.vertical_excursion);
    (endpoints + nodes + steps + gates + horizontal + vertical + excursion) / 7.0
}

fn normalized_edit_distance<T: Eq>(left: &[T], right: &[T]) -> f64 {
    let denominator = left.len().max(right.len());
    if denominator == 0 {
        return 0.0;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_item) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_item) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index]
                + usize::from(left_item != right_item))
            .min(current[right_index] + 1)
            .min(previous[right_index + 1] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()] as f64 / denominator as f64
}

fn summarize_samples(values: &[f64]) -> NormalizedDistanceSamples {
    if values.is_empty() {
        return NormalizedDistanceSamples::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    debug_assert!(
        sorted
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    );
    NormalizedDistanceSamples {
        samples: sorted.len(),
        minimum: sorted.first().copied(),
        p10: percentile(&sorted, 10),
        median: percentile(&sorted, 50),
        p90: percentile(&sorted, 90),
        maximum: sorted.last().copied(),
        mean: Some(sorted.iter().sum::<f64>() / sorted.len() as f64),
    }
}

fn percentile(sorted: &[f64], percentile: usize) -> Option<f64> {
    (!sorted.is_empty()).then(|| sorted[(sorted.len() - 1) * percentile / 100])
}

fn aggregate_cells(
    cells: &[(&DirectedRouteChoiceCell, &RouteAlternativeDistanceSamples)],
) -> RouteChoiceAggregate {
    let mut aggregate = RouteChoiceAggregate {
        expected_cells: cells.len(),
        ..RouteChoiceAggregate::default()
    };
    let mut samples = RouteAlternativeDistanceSamples::default();
    for (cell, cell_samples) in cells {
        match &cell.evidence {
            RouteChoiceCellEvidence::Positive(set) => {
                aggregate.positive_cells += 1;
                match set.direct_audit_status {
                    PositiveAlternativeAuditStatus::CompleteFiniteVocabulary => {
                        aggregate.positive_complete_audit_cells += 1;
                    }
                    PositiveAlternativeAuditStatus::BoundedIncomplete { .. } => {
                        aggregate.positive_bounded_audit_cells += 1;
                    }
                    PositiveAlternativeAuditStatus::MissingDirectedRouteAssessment
                    | PositiveAlternativeAuditStatus::MissingLoadoutAudit => {
                        aggregate.positive_missing_route_or_audit_cells += 1;
                    }
                }
                aggregate.cells_with_multiple_alternatives +=
                    usize::from(set.positive_alternative_count > 1);
                aggregate.observed_positive_count = aggregate
                    .observed_positive_count
                    .saturating_add(set.observed_positive_count);
                aggregate.positive_alternative_count = aggregate
                    .positive_alternative_count
                    .saturating_add(set.positive_alternative_count);
                aggregate.timing_or_exact_aliases_collapsed = aggregate
                    .timing_or_exact_aliases_collapsed
                    .saturating_add(set.timing_or_exact_aliases_collapsed);
                increment_histogram(
                    &mut aggregate.alternative_count_histogram,
                    set.positive_alternative_count,
                );
                increment_histogram(
                    &mut aggregate.spatial_path_class_histogram,
                    set.spatial_path_classes,
                );
                increment_histogram(
                    &mut aggregate.semantic_action_class_histogram,
                    set.semantic_action_classes,
                );
                increment_histogram(
                    &mut aggregate.accepted_event_class_histogram,
                    set.accepted_event_sequence_classes,
                );
                increment_histogram(
                    &mut aggregate.gate_path_style_class_histogram,
                    set.gate_path_style_classes,
                );
                samples.extend(cell_samples);
            }
            RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary => {
                aggregate.complete_without_positive_cells += 1;
            }
            RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive { .. } => {
                aggregate.bounded_inconclusive_without_positive_cells += 1;
            }
            RouteChoiceCellEvidence::MissingDirectedRouteAssessment => {
                aggregate.missing_route_cells += 1;
            }
            RouteChoiceCellEvidence::MissingLoadoutAudit => {
                aggregate.missing_audit_cells += 1;
            }
        }
    }
    aggregate.distances = samples.summarize();
    aggregate
}

fn increment_histogram(histogram: &mut BTreeMap<usize, usize>, value: usize) {
    *histogram.entry(value).or_default() += 1;
}

fn accumulate_search_stats(total: &mut SearchStats, additional: SearchStats) {
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

#[derive(Debug)]
pub enum RouteChoiceDiversityError {
    CorpusV2Identity {
        source: Box<CorpusMetricInputV2Error>,
    },
    CorpusCandidateMismatch {
        room_id: RoomId,
    },
    AnalysisVersion {
        expected: u32,
        actual: u32,
    },
    AnalysisConfig {
        source: CorpusRoomAnalysisConfigError,
    },
    MatrixCardinality {
        loadout: EvaluationLoadout,
        count: usize,
    },
    MatrixLoadoutMismatch {
        loadout: EvaluationLoadout,
    },
    MatrixDoorCoordinates {
        loadout: EvaluationLoadout,
    },
    MatrixPositiveTargetMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
    },
    CanonicalMeasurementBinding {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        count: usize,
    },
    UnboundCanonicalMeasurements {
        consumed: usize,
        total: usize,
    },
    RoomIdMismatch {
        evaluated: RoomId,
        analysis: RoomId,
    },
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    DuplicateDoorId {
        door_id: String,
    },
    DuplicateRouteNodeId {
        node_id: u16,
    },
    PortReferencesUnknownNode {
        door_id: String,
        node_id: u16,
    },
    DuplicateBoundaryPort {
        door_id: String,
    },
    MissingBoundaryPort {
        door_id: String,
    },
    UnknownBoundaryPortDoor {
        door_id: String,
    },
    BoundaryPortGeometryMismatch {
        door_id: String,
    },
    UnknownDoorInAnalysis {
        door_id: String,
    },
    RouteSourceMismatch {
        batch_source: String,
        route_source: String,
        target: String,
    },
    SameSourceAndTarget {
        door_id: String,
    },
    DuplicateSourceAssessmentBatch {
        source: String,
    },
    DuplicateSharedLoadoutAudit {
        source: String,
        loadout: EvaluationLoadout,
    },
    DuplicateRouteAssessment {
        source: String,
        target: String,
    },
    DuplicateLoadoutAudit {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
    },
    DuplicateCanonicalPositive {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
    },
    CanonicalTargetMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        reached: String,
    },
    CanonicalMeasurementVersion {
        expected: u32,
        actual: u32,
    },
    CanonicalVectorTargetMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        vector_target: String,
    },
    WitnessesWithoutAudit {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        witnesses: usize,
    },
    DirectWitnessCountMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        audit_raw: usize,
        audit_retained: usize,
        stored_retained: usize,
    },
    DoorEntry {
        source_door_id: String,
        loadout: EvaluationLoadout,
        source: Box<downwards_core::DoorEntryError>,
    },
    DirectReplayObservation {
        source_door_id: String,
        target_door_id: String,
        loadout: EvaluationLoadout,
        source: Box<WitnessObservationError>,
    },
    DirectTargetMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
        reached: String,
    },
    StoredSemanticTraceMismatch {
        source: String,
        target: String,
        loadout: EvaluationLoadout,
    },
    OperationalCostMismatch {
        reported: SearchStats,
        recomputed: SearchStats,
    },
}

impl fmt::Display for RouteChoiceDiversityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorpusV2Identity { source } => source.fmt(formatter),
            Self::CorpusCandidateMismatch { room_id } => write!(
                formatter,
                "explicit route-choice candidate does not equal the validated corpus-v2 canonical candidate for {}",
                room_id.0
            ),
            Self::AnalysisVersion { expected, actual } => write!(
                formatter,
                "route-choice analysis version {actual} is unsupported; expected {expected}"
            ),
            Self::AnalysisConfig { source } => {
                write!(
                    formatter,
                    "route-choice analysis has invalid config identity: {source}"
                )
            }
            Self::MatrixCardinality { loadout, count } => write!(
                formatter,
                "route-choice input has {count} {} matrices instead of one",
                loadout.slug()
            ),
            Self::MatrixLoadoutMismatch { loadout } => write!(
                formatter,
                "route-choice {} matrix carries different exact physics abilities",
                loadout.slug()
            ),
            Self::MatrixDoorCoordinates { loadout } => write!(
                formatter,
                "route-choice {} matrix rows do not exactly equal the room's directed door coordinates",
                loadout.slug()
            ),
            Self::MatrixPositiveTargetMismatch {
                source,
                target,
                loadout,
            } => write!(
                formatter,
                "{} matrix positive {source:?}->{target:?} carries a different typed target",
                loadout.slug()
            ),
            Self::CanonicalMeasurementBinding {
                source,
                target,
                loadout,
                count,
            } => write!(
                formatter,
                "{} canonical matrix positive {source:?}->{target:?} has {count} matching route measurements instead of one",
                loadout.slug()
            ),
            Self::UnboundCanonicalMeasurements { consumed, total } => write!(
                formatter,
                "{consumed} of {total} canonical route measurements bind to exact matrix positives"
            ),
            Self::RoomIdMismatch {
                evaluated,
                analysis,
            } => write!(
                formatter,
                "route-choice room mismatch: evaluated {}, analysis {}",
                evaluated.0, analysis.0
            ),
            Self::MissingCanonicalVariant { room_id } => {
                write!(
                    formatter,
                    "evaluated room {} has no canonical variant",
                    room_id.0
                )
            }
            Self::DuplicateDoorId { door_id } => write!(formatter, "duplicate door ID {door_id:?}"),
            Self::DuplicateRouteNodeId { node_id } => {
                write!(formatter, "route plan repeats node ID {node_id}")
            }
            Self::PortReferencesUnknownNode { door_id, node_id } => write!(
                formatter,
                "boundary port {door_id:?} references unknown route node {node_id}"
            ),
            Self::DuplicateBoundaryPort { door_id } => {
                write!(formatter, "duplicate boundary-port mapping for {door_id:?}")
            }
            Self::MissingBoundaryPort { door_id } => {
                write!(
                    formatter,
                    "room door {door_id:?} has no boundary-port mapping"
                )
            }
            Self::UnknownBoundaryPortDoor { door_id } => write!(
                formatter,
                "boundary-port mapping references unknown room door {door_id:?}"
            ),
            Self::BoundaryPortGeometryMismatch { door_id } => write!(
                formatter,
                "boundary-port geometry for {door_id:?} differs from the canonical room door"
            ),
            Self::UnknownDoorInAnalysis { door_id } => {
                write!(
                    formatter,
                    "analysis references unknown room door {door_id:?}"
                )
            }
            Self::RouteSourceMismatch {
                batch_source,
                route_source,
                target,
            } => write!(
                formatter,
                "source batch {batch_source:?} contains route {route_source:?}->{target:?}"
            ),
            Self::SameSourceAndTarget { door_id } => write!(
                formatter,
                "route-choice evidence uses the same source and target door {door_id:?}"
            ),
            Self::DuplicateSourceAssessmentBatch { source } => {
                write!(
                    formatter,
                    "duplicate source assessment batch for {source:?}"
                )
            }
            Self::DuplicateSharedLoadoutAudit { source, loadout } => write!(
                formatter,
                "duplicate shared {} direct audit for source {source:?}",
                loadout.slug()
            ),
            Self::DuplicateRouteAssessment { source, target } => {
                write!(
                    formatter,
                    "duplicate route assessment {source:?}->{target:?}"
                )
            }
            Self::DuplicateLoadoutAudit {
                source,
                target,
                loadout,
            } => write!(
                formatter,
                "duplicate {} audit for {source:?}->{target:?}",
                loadout.slug()
            ),
            Self::DuplicateCanonicalPositive {
                source,
                target,
                loadout,
            } => write!(
                formatter,
                "duplicate {} canonical positive for {source:?}->{target:?}",
                loadout.slug()
            ),
            Self::CanonicalTargetMismatch {
                source,
                target,
                loadout,
                reached,
            } => write!(
                formatter,
                "{} canonical positive {source:?}->{target:?} reached {reached:?}",
                loadout.slug()
            ),
            Self::CanonicalMeasurementVersion { expected, actual } => write!(
                formatter,
                "canonical route measurement version {actual} is unsupported; expected {expected}"
            ),
            Self::CanonicalVectorTargetMismatch {
                source,
                target,
                loadout,
                vector_target,
            } => write!(
                formatter,
                "{} canonical positive {source:?}->{target:?} carries vector target {vector_target:?}",
                loadout.slug()
            ),
            Self::WitnessesWithoutAudit {
                source,
                target,
                loadout,
                witnesses,
            } => write!(
                formatter,
                "{} route {source:?}->{target:?} stores {witnesses} witnesses without an audit",
                loadout.slug()
            ),
            Self::DirectWitnessCountMismatch {
                source,
                target,
                loadout,
                audit_raw,
                audit_retained,
                stored_retained,
            } => write!(
                formatter,
                "{} route {source:?}->{target:?} direct counts disagree: raw={audit_raw}, audit-retained={audit_retained}, stored-retained={stored_retained}",
                loadout.slug()
            ),
            Self::DoorEntry {
                source_door_id,
                loadout,
                source,
            } => write!(
                formatter,
                "cannot enter {source_door_id:?} under {} for route-choice replay: {source}",
                loadout.slug()
            ),
            Self::DirectReplayObservation {
                source_door_id,
                target_door_id,
                loadout,
                source,
            } => write!(
                formatter,
                "cannot observe {} direct replay {source_door_id:?}->{target_door_id:?}: {source}",
                loadout.slug()
            ),
            Self::DirectTargetMismatch {
                source,
                target,
                loadout,
                reached,
            } => write!(
                formatter,
                "{} direct replay {source:?}->{target:?} reached {reached:?}",
                loadout.slug()
            ),
            Self::StoredSemanticTraceMismatch {
                source,
                target,
                loadout,
            } => write!(
                formatter,
                "{} stored semantic trace differs from exact replay for {source:?}->{target:?}",
                loadout.slug()
            ),
            Self::OperationalCostMismatch {
                reported,
                recomputed,
            } => write!(
                formatter,
                "route-choice direct audit cost disagrees with room analysis: reported={reported:?}, recomputed={recomputed:?}"
            ),
        }
    }
}

impl Error for RouteChoiceDiversityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CorpusV2Identity { source } => Some(source.as_ref()),
            Self::AnalysisConfig { source } => Some(source),
            Self::DoorEntry { source, .. } => Some(source.as_ref()),
            Self::DirectReplayObservation { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use downwards_ai::{DifficultyConfig, Replay, SolverConfig};
    use downwards_core::{
        AbilitySet, Action, Door, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect, Room, Tile,
    };
    use downwards_gen::{
        CompositionalFeatureSet, CompositionalKey, CompositionalProfile, GeneratedLevel,
        GeneratedMetadata, GenerationStats, LayoutFamily, StagedCompositionalCandidate,
        StagedCompositionalKey,
        experimental::{
            BoundaryPort, ChallengeIntent, GenerationStrategy, NodeRole, RouteEdge, RouteNode,
            SupportKind,
        },
    };

    use super::*;
    use crate::corpus::{
        BoundedIncompleteLoadout, CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, CorpusBuildConfigV1,
        CorpusRoomAnalysisConfig, RouteControllerAuditCompleteness, analyze_corpus_room,
        evaluate_route_matrices, generate_seed_block,
    };

    fn flat_candidate() -> StagedCompositionalCandidate {
        let width = 32;
        let height = 18;
        let tile_size = 10;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        for x in 0..width {
            tiles[usize::from(height - 1) * usize::from(width) + usize::from(x)] = Tile::Solid;
        }
        let standing_y = i32::from(height - 1) * tile_size - PLAYER_HEIGHT;
        let west = Door {
            id: "west".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, standing_y, 4, PLAYER_HEIGHT),
            arrival: Point::new(12, standing_y),
            destination_room: None,
            destination_door: None,
        };
        let east = Door {
            id: "east".to_owned(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, standing_y, 4, PLAYER_HEIGHT),
            arrival: Point::new(300, standing_y),
            destination_room: None,
            destination_door: None,
        };
        let room = Room::new(
            "route-choice-flat",
            "Route Choice Flat",
            width,
            height,
            tile_size,
            tiles,
            Point::new(12, standing_y),
            vec![],
        )
        .unwrap()
        .with_doors(vec![west.clone(), east.clone()])
        .unwrap();
        let support = SupportSpec {
            start_x: 1,
            end_x: width - 1,
            row: height - 1,
            kind: SupportKind::Solid,
        };
        let route_plan = RoutePlan {
            nodes: vec![
                RouteNode {
                    id: 0,
                    role: NodeRole::Port,
                    support,
                },
                RouteNode {
                    id: 1,
                    role: NodeRole::Port,
                    support,
                },
            ],
            edges: vec![RouteEdge {
                from: 0,
                to: 1,
                verb: RouteVerb::Run,
                critical: true,
            }],
        };
        let key = StagedCompositionalKey::new(
            CompositionalKey::new(
                0,
                CompositionalProfile::new(
                    AbilitySet::NONE,
                    GenerationStrategy::CyclicGraph,
                    ChallengeIntent::Gentle,
                ),
            ),
            CompositionalFeatureSet::TerrainOnly,
        );
        StagedCompositionalCandidate {
            key,
            generated: GeneratedLevel {
                room,
                metadata: GeneratedMetadata {
                    generation_version: 1,
                    seed: 0,
                    layout_family: LayoutFamily::HazardRun,
                    ability_tier: downwards_gen::AbilityTier::Baseline,
                    intended_abilities: AbilitySet::NONE,
                    stats: GenerationStats::default(),
                },
            },
            route_summary: route_plan.summary(),
            route_plan,
            boundary_ports: vec![
                BoundaryPort {
                    node_id: 0,
                    door: west,
                },
                BoundaryPort {
                    node_id: 1,
                    door: east,
                },
            ],
        }
    }

    fn replay_to_east(initial: &Simulation, idle_ticks: usize, jump: bool) -> Replay {
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        for tick in 0..300 {
            let action = if tick < idle_ticks {
                Action::default()
            } else {
                Action {
                    move_x: 1,
                    jump: jump && tick < idle_ticks + 10,
                    ..Action::default()
                }
            };
            simulation.step(action);
            actions.push(action);
            if simulation.reached_exit() == Some("east") {
                return Replay::record(initial, actions);
            }
        }
        panic!("synthetic controller did not reach east")
    }

    fn observed(
        candidate: &StagedCompositionalCandidate,
        idle_ticks: usize,
        jump: bool,
    ) -> ObservedPositive {
        let initial =
            Simulation::enter_via_door(candidate.generated.room.clone(), AbilitySet::NONE, "west")
                .unwrap();
        let replay = replay_to_east(&initial, idle_ticks, jump);
        ObservedPositive {
            provenance: ObservationProvenance::DirectController,
            observation: observe_successful_replay(
                &initial,
                &replay,
                downwards_lab::TraversalGrid::default(),
            )
            .unwrap(),
        }
    }

    #[test]
    fn timing_variants_do_not_fake_route_alternatives() {
        let candidate = flat_candidate();
        let context = PathContext::new(
            &candidate.generated,
            &candidate.route_plan,
            &candidate.boundary_ports,
        )
        .unwrap();
        let first = observed(&candidate, 1, false);
        let second = observed(&candidate, 4, false);
        assert_ne!(
            first.observation.completion_ticks,
            second.observation.completion_ticks
        );
        let (set, _) = build_positive_set(
            &context,
            "west",
            "east",
            PositiveAlternativeAuditStatus::CompleteFiniteVocabulary,
            None,
            vec![first, second],
        );
        assert_eq!(set.observed_positive_count, 2);
        assert_eq!(set.positive_alternative_count, 1);
        assert_eq!(set.timing_or_exact_aliases_collapsed, 1);
        assert_eq!(set.spatial_path_classes, 1);
        assert_eq!(set.semantic_action_classes, 1);
        assert_eq!(set.accepted_event_sequence_classes, 1);
        assert_eq!(set.gate_path_style_classes, 1);
        assert_eq!(set.distances.spatial_trajectory.pairwise.samples, 0);
        assert!(
            set.alternatives[0].minimum_completion_ticks
                < set.alternatives[0].maximum_completion_ticks
        );
    }

    #[test]
    fn genuinely_different_spatial_routes_remain_distinct() {
        let candidate = flat_candidate();
        let context = PathContext::new(
            &candidate.generated,
            &candidate.route_plan,
            &candidate.boundary_ports,
        )
        .unwrap();
        let run = observed(&candidate, 1, false);
        let jump = observed(&candidate, 1, true);
        assert_ne!(
            spatial_signature(&run.observation),
            spatial_signature(&jump.observation)
        );
        let (set, _) = build_positive_set(
            &context,
            "west",
            "east",
            PositiveAlternativeAuditStatus::CompleteFiniteVocabulary,
            None,
            vec![run, jump],
        );
        assert_eq!(set.positive_alternative_count, 2);
        assert_eq!(set.spatial_path_classes, 2);
        assert_eq!(set.distances.spatial_trajectory.pairwise.samples, 1);
        assert_eq!(set.distances.spatial_trajectory.nearest_neighbor.samples, 2);
        assert!(set.distances.spatial_trajectory.pairwise.minimum.unwrap() > 0.0);
    }

    fn empty_route(status: LoadoutControllerAuditStatus) -> RouteControllerAssessment {
        let completeness = match status {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                RouteControllerAuditCompleteness::CompleteFiniteVocabulary
            }
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                RouteControllerAuditCompleteness::BoundedIncomplete {
                    incomplete_loadouts: vec![BoundedIncompleteLoadout {
                        loadout: EvaluationLoadout::Baseline,
                        limit,
                    }],
                }
            }
        };
        RouteControllerAssessment {
            policy: CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY,
            source_door_id: "west".to_owned(),
            target_door_id: "east".to_owned(),
            authoritative_loadout: EvaluationLoadout::Baseline,
            expected_subset_loadouts: vec![EvaluationLoadout::Baseline],
            audits: vec![LoadoutControllerAudit {
                loadout: EvaluationLoadout::Baseline,
                status,
                operational_stats: SearchStats::default(),
                raw_positive_witnesses: 0,
                retained_semantic_witnesses: 0,
            }],
            completeness,
            easiest_first_witnesses: vec![],
            easiest_known_front: vec![],
            positive_bypasses: vec![],
        }
    }

    #[test]
    fn bounded_absence_is_not_reported_as_no_positive() {
        let candidate = flat_candidate();
        let context = PathContext::new(
            &candidate.generated,
            &candidate.route_plan,
            &candidate.boundary_ports,
        )
        .unwrap();
        let route = empty_route(LoadoutControllerAuditStatus::BoundedIncomplete {
            limit: DirectProbeBudgetLimit::ExpandedNodes,
        });
        let built = build_cell(
            &context,
            "west",
            "east",
            EvaluationLoadout::Baseline,
            Some(&route),
            Some(&route.audits[0]),
            None,
        )
        .unwrap();
        assert_eq!(
            built.evidence,
            RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive {
                limit: DirectProbeBudgetLimit::ExpandedNodes,
            }
        );

        let complete = empty_route(LoadoutControllerAuditStatus::CompleteFiniteVocabulary);
        let built = build_cell(
            &context,
            "west",
            "east",
            EvaluationLoadout::Baseline,
            Some(&complete),
            Some(&complete.audits[0]),
            None,
        )
        .unwrap();
        assert_eq!(
            built.evidence,
            RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary
        );
    }

    #[test]
    fn canonical_and_direct_provenance_stay_distinct_after_alias_collapse() {
        let candidate = flat_candidate();
        let context = PathContext::new(
            &candidate.generated,
            &candidate.route_plan,
            &candidate.boundary_ports,
        )
        .unwrap();
        let direct = observed(&candidate, 1, false);
        let canonical = ObservedPositive {
            provenance: ObservationProvenance::CanonicalMatrix,
            observation: direct.observation.clone(),
        };
        let audit = LoadoutControllerAudit {
            loadout: EvaluationLoadout::Baseline,
            status: LoadoutControllerAuditStatus::BoundedIncomplete {
                limit: DirectProbeBudgetLimit::SimulatedTicks,
            },
            operational_stats: SearchStats::default(),
            raw_positive_witnesses: 1,
            retained_semantic_witnesses: 1,
        };
        let (set, _) = build_positive_set(
            &context,
            "west",
            "east",
            PositiveAlternativeAuditStatus::BoundedIncomplete {
                limit: DirectProbeBudgetLimit::SimulatedTicks,
            },
            Some(&audit),
            vec![direct, canonical],
        );
        assert_eq!(set.observed_positive_count, 2);
        assert_eq!(set.positive_alternative_count, 1);
        assert_eq!(set.retained_direct_positive_witnesses, 1);
        assert_eq!(set.canonical_positive_observations, 1);
        assert_eq!(set.alternatives[0].direct_controller_members, 1);
        assert_eq!(set.alternatives[0].canonical_matrix_members, 1);
        assert!(matches!(
            set.direct_audit_status,
            PositiveAlternativeAuditStatus::BoundedIncomplete { .. }
        ));
    }

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
        (evaluated, analysis)
    }

    #[test]
    fn real_room_report_is_complete_repeatable_and_within_cell_only() {
        let (evaluated, analysis) = real_two_door_fixture();
        let first = analyze_route_choice_diversity(&evaluated, &analysis).unwrap();
        let second = analyze_route_choice_diversity(&evaluated, &analysis).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.cells.len(), 2 * EvaluationLoadout::ALL.len());
        assert_eq!(first.room.expected_cells, first.cells.len());
        assert_eq!(
            first.room.positive_cells
                + first.room.complete_without_positive_cells
                + first.room.bounded_inconclusive_without_positive_cells
                + first.room.missing_route_cells
                + first.room.missing_audit_cells,
            first.room.expected_cells
        );
        assert!(first.room.positive_cells > 0);
        assert_eq!(
            first.operational_cost.inherited_direct_controller_audit,
            analysis.direct_controller_operational_stats
        );
        assert_eq!(
            first.operational_cost.replayed_direct_witnesses,
            first
                .cells
                .iter()
                .filter_map(|cell| match &cell.evidence {
                    RouteChoiceCellEvidence::Positive(set) => {
                        Some(set.retained_direct_positive_witnesses)
                    }
                    _ => None,
                })
                .sum::<usize>()
        );
        for cell in &first.cells {
            let RouteChoiceCellEvidence::Positive(set) = &cell.evidence else {
                continue;
            };
            let expected_pairs = set
                .positive_alternative_count
                .saturating_mul(set.positive_alternative_count.saturating_sub(1))
                / 2;
            assert_eq!(
                set.distances.spatial_trajectory.pairwise.samples,
                expected_pairs
            );
        }
    }

    /// Manual release-mode smoke benchmark for the post-analysis pass.
    #[test]
    #[ignore = "run explicitly in release mode"]
    fn release_benchmark_small_real_room() {
        let (evaluated, analysis) = real_two_door_fixture();
        let started = Instant::now();
        let report = analyze_route_choice_diversity(&evaluated, &analysis).unwrap();
        let elapsed = started.elapsed();
        eprintln!(
            "route-choice release benchmark: room={} cells={} positives={} alternatives={} replayed={} replay_ticks={} elapsed_ms={} alternative_hist={:?} spatial_hist={:?} action_hist={:?} event_hist={:?} style_hist={:?}",
            report.room_id.0,
            report.cells.len(),
            report.room.positive_cells,
            report.room.positive_alternative_count,
            report.operational_cost.replayed_direct_witnesses,
            report.operational_cost.replayed_direct_ticks,
            elapsed.as_millis(),
            report.room.alternative_count_histogram,
            report.room.spatial_path_class_histogram,
            report.room.semantic_action_class_histogram,
            report.room.accepted_event_class_histogram,
            report.room.gate_path_style_class_histogram,
        );
        assert!(report.room.positive_cells > 0);
    }

    #[test]
    fn distance_summary_has_explicit_empty_and_nearest_denominators() {
        assert_eq!(summarize_samples(&[]), NormalizedDistanceSamples::default());
        let summary = summarize_samples(&[0.1, 0.3, 0.9]);
        assert_eq!(summary.samples, 3);
        assert_eq!(summary.minimum, Some(0.1));
        assert_eq!(summary.median, Some(0.3));
        assert_eq!(summary.maximum, Some(0.9));
    }

    #[test]
    fn flat_fixture_respects_player_and_door_geometry() {
        let candidate = flat_candidate();
        let room = &candidate.generated.room;
        assert_eq!(room.doors().len(), 2);
        assert_eq!(PLAYER_WIDTH, 8);
        assert_eq!(PLAYER_HEIGHT, 12);
    }
}
