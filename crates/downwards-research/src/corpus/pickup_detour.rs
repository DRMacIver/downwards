//! Replay-certified pickup challenge and finite optionality evidence.
//!
//! A pickup cell is one exact `(source door, loadout, pickup)` objective from
//! the bounded route matrix. Positive cells are replayed under that exact
//! loadout and measured through first contact with the typed pickup. Bounded
//! non-success remains inconclusive. Canonical door witnesses from the same
//! source and loadout are also replayed so that observed opportunistic pickup
//! collection and target-directed spatial/action differences can be reported.
//!
//! None of the finite comparisons in this module prove that a pickup is
//! mandatory, unreachable, or absent from every possible door route.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fmt,
};

use downwards_ai::{
    InconclusiveReason, ReachedTarget, Replay, ReplayDivergence, SearchStats, SearchTarget,
    TargetSolution,
};
use downwards_core::{
    DashDirection, DeathReason, DoorEntryError, JumpKind, Simulation, SimulationEvent,
};
use downwards_gen::{
    GeneratedLevel, StagedCompositionalCandidate,
    experimental::{NodeRole, RoutePlan, RouteVerb},
};
use downwards_lab::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, SemanticEventAt, TraversalCell,
    TraversalGrid, TraversalSpan, TraversalTrace,
};
use downwards_validation::{
    BoundedTargetEvidence, ReplayCertifiedTargetEvidence, WitnessFingerprint,
};

use super::{
    CorpusCandidate, CorpusMetricInputV2Error, EvaluatedCorpusRoom, EvaluatedCorpusRoomV2,
    EvaluationLoadout, LoadoutRouteMatrix, RoomId, resolve_corpus_metric_candidate_v2,
};

/// Version of the complete pickup-detour analysis policy and result shape.
pub const PICKUP_DETOUR_ANALYSIS_VERSION: u32 = 1;

/// Required interpretation warning for persisted or displayed results.
pub const PICKUP_DETOUR_EVIDENCE_DISCLAIMER: &str = "positive pickup and door evidence is exact replay evidence for one retained witness; bounded non-success is inconclusive, target-directed-only means only that no retained positive canonical door witness collected the pickup, and authored off-spine placement is not proof of a required gameplay detour";

/// Complete deterministic pickup analysis for one evaluated room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomPickupDetourAnalysis {
    pub version: u32,
    pub room_id: RoomId,
    pub traversal_grid: TraversalGrid,
    /// One row per canonical pickup ID.
    pub structural_placements: Vec<PickupStructuralPlacement>,
    /// Canonical loadout, source-door, pickup order.
    pub cells: Vec<PickupChallengeCell>,
    /// One row per source search. Search work is deliberately not part of any
    /// challenge or optionality coordinate.
    pub operational_cost: PickupAnalysisOperationalCost,
    /// Cross-loadout positive/bounded facts. These are bypass observations,
    /// never negative ability requirements.
    pub cross_loadout: Vec<PickupCrossLoadoutEvidence>,
}

/// One exact route-matrix pickup coordinate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupChallengeCell {
    pub loadout: EvaluationLoadout,
    pub source_door_id: String,
    pub pickup_id: String,
    pub evidence: PickupCellEvidence,
    pub canonical_door_context: CanonicalDoorPickupContext,
    pub operational: PickupCellOperationalCost,
}

/// Positive or bounded-inconclusive evidence for one exact pickup target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickupCellEvidence {
    Positive(Box<PickupPositiveChallenge>),
    BoundedInconclusive { reason: InconclusiveReason },
}

impl PickupCellEvidence {
    #[must_use]
    pub const fn is_positive(&self) -> bool {
        matches!(self, Self::Positive(_))
    }
}

/// Non-operational measurements from one exactly replayed positive witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupPositiveChallenge {
    pub witness_fingerprint: WitnessFingerprint,
    pub completion: PickupCompletionDemand,
    pub control: PickupControlDemand,
    pub traversal: PickupTraversalDemand,
    pub ability: PickupAbilityDemand,
    /// Full prefix observations make every aggregate and route comparison
    /// independently auditable.
    pub action_trace: SemanticActionTrace,
    pub traversal_trace: TraversalTrace,
}

/// Completion facts through first contact with the exact target pickup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupCompletionDemand {
    pub completion_ticks: usize,
    pub target_collection_tick: usize,
    pub deaths_before_completion: usize,
    pub resets_before_completion: usize,
    /// Exact IDs in event order, including the target. A target solver may
    /// encounter another pickup first without changing target typing.
    pub pickup_collection_events: Vec<PickupCollectionEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupCollectionEvent {
    pub pickup_id: String,
    pub replay_tick: usize,
}

/// Transparent timing-sensitive controller demand of the retained witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupControlDemand {
    pub run_only: bool,
    pub monotone_simple: bool,
    pub jump_press_edges: usize,
    pub dash_press_edges: usize,
    pub horizontal_reversals: usize,
    pub vertical_input_changes: usize,
    pub dash_direction_changes: usize,
    pub vertical_decisions: usize,
    pub semantic_spans: usize,
    pub semantic_transitions: usize,
    pub duration_ticks: usize,
}

/// Coarse spatial demand under the configured traversal grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupTraversalDemand {
    pub sample_count: usize,
    pub traversal_spans: usize,
    pub visited_cell_count: usize,
    pub horizontal_cell_span: u16,
    pub vertical_cell_span: u16,
    /// Manhattan distance along the timing-insensitive traversal-span path.
    pub coarse_path_steps: usize,
}

/// Ability events observed in the exact positive replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupAbilityDemand {
    pub loadout: EvaluationLoadout,
    pub ordinary_jump_events: usize,
    pub wall_jump_events: usize,
    pub dash_events: usize,
    pub used_wall_jump: bool,
    pub used_dash: bool,
    pub enabled_wall_jump_unused_by_witness: bool,
    pub enabled_dash_unused_by_witness: bool,
}

/// Finite canonical door-witness context for one pickup cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDoorPickupContext {
    pub relation: FiniteCanonicalPickupRelation,
    /// One row for every other target door in canonical target order.
    pub routes: Vec<CanonicalDoorPickupRoute>,
}

/// What the retained finite door witnesses establish about collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FiniteCanonicalPickupRelation {
    /// At least one positive canonical door witness still carries the pickup
    /// at its target door. This is positive opportunistic evidence.
    ObservedOpportunistically {
        target_door_ids: Vec<String>,
        positive_noncollecting_door_witnesses: usize,
        bounded_door_cells: usize,
    },
    /// Positive door witnesses exist, but none carries the pickup at door
    /// contact. This describes the retained witnesses only.
    TargetDirectedOnlyAmongObservedWitnesses {
        positive_door_witnesses: usize,
        bounded_door_cells: usize,
    },
    /// Every same-source door target was bounded-inconclusive, so there is no
    /// positive canonical door witness with which to compare the pickup.
    NoPositiveDoorWitnessContext { bounded_door_cells: usize },
}

/// Evidence from one same-source canonical door route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalDoorPickupRoute {
    Positive {
        target_door_id: String,
        /// True only if the pickup is in authoritative simulation state at
        /// target-door contact. A collection erased by reset is reported by
        /// `collection_events` but is not opportunistic retention.
        pickup_retained_at_door: bool,
        collection_events: Vec<PickupCollectionEvent>,
        /// Present exactly when the pickup cell itself is positive.
        difference_from_pickup_witness: Option<PickupDoorRouteDifference>,
    },
    BoundedInconclusive {
        target_door_id: String,
        reason: InconclusiveReason,
    },
}

/// Spatial and action difference between two exact same-source/loadout
/// witnesses: the pickup target and one target door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupDoorRouteDifference {
    pub spatial: PickupDoorSpatialDifference,
    pub action: PickupDoorActionDifference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupDoorSpatialDifference {
    pub shared_visited_cells: usize,
    pub pickup_only_visited_cells: usize,
    pub door_only_visited_cells: usize,
    pub union_visited_cells: usize,
    /// Timing-insensitive edit distance between traversal-cell span sequences.
    pub traversal_span_edit_distance: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupDoorActionDifference {
    pub shared_prefix_ticks: usize,
    pub pickup_duration_ticks: usize,
    pub door_duration_ticks: usize,
    pub absolute_duration_difference: usize,
    /// Timing-insensitive edit distance between semantic action symbols after
    /// run-length encoding. Span durations are deliberately excluded here and
    /// retained by the duration fields above.
    pub semantic_span_edit_distance: usize,
}

/// Explicit search effort for one pickup cell. These coordinates must not be
/// interpreted as player difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupCellOperationalCost {
    /// Total shared source-frontier effort (repeated across that source's
    /// cells for standalone interpretation).
    pub shared_source_search: SearchStats,
    /// Cumulative shared-frontier snapshot when this target was found, or the
    /// final bound for an inconclusive target.
    pub pickup_target_snapshot: SearchStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupAnalysisOperationalCost {
    /// Unique source searches, so callers can sum these without multiplying
    /// work by the number of pickup or door targets.
    pub source_searches: Vec<PickupSourceSearchCost>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupSourceSearchCost {
    pub loadout: EvaluationLoadout,
    pub source_index: usize,
    pub source_door_id: String,
    pub search: SearchStats,
}

/// Cross-loadout positive and bounded facts for one `(source, pickup)` pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupCrossLoadoutEvidence {
    pub source_door_id: String,
    pub pickup_id: String,
    pub positive_loadouts: Vec<EvaluationLoadout>,
    pub bounded_loadouts: Vec<PickupBoundedLoadout>,
    pub positive_without_wall_jump: bool,
    pub positive_without_dash: bool,
    pub witnesses_using_wall_jump: Vec<EvaluationLoadout>,
    pub witnesses_using_dash: Vec<EvaluationLoadout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupBoundedLoadout {
    pub loadout: EvaluationLoadout,
    pub reason: InconclusiveReason,
}

/// Conservative authored-graph placement for one pickup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupStructuralPlacement {
    pub pickup_id: String,
    pub mapping: PickupRoutePlanMapping,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickupRoutePlanMapping {
    Unique(PickupAuthoredNodePlacement),
    NoMatchingAuthoredPickupNode,
    AmbiguousMatchingAuthoredPickupNodes { node_ids: Vec<u16> },
}

/// Exact authored-graph facts. `on_shortest_port_spine` uses undirected,
/// unweighted authored edges and does not assert anything about simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickupAuthoredNodePlacement {
    pub node_id: u16,
    pub undirected_degree: usize,
    pub authored_leaf: bool,
    pub on_shortest_port_spine: bool,
    pub distance_to_shortest_port_spine: Option<usize>,
    pub incident_critical_edges: usize,
    pub incident_wall_climb_edges: usize,
    pub incident_dash_edges: usize,
}

/// Analyze every pickup cell and finite canonical door comparison.
pub fn analyze_pickup_detours(
    evaluated: &EvaluatedCorpusRoom,
    traversal_grid: TraversalGrid,
) -> Result<RoomPickupDetourAnalysis, PickupDetourAnalysisError> {
    let room_id = evaluated.generated.id.clone();
    let Some(candidate) = evaluated.generated.variants.first() else {
        return Err(PickupDetourAnalysisError::MissingCanonicalVariant { room_id });
    };
    analyze_pickup_detours_for_candidate(candidate, evaluated, traversal_grid)
}

/// Variant accepting the exact canonical candidate explicitly. This is useful
/// to artifact verifiers, which can regenerate the candidate independently.
pub fn analyze_pickup_detours_for_candidate(
    candidate: &StagedCompositionalCandidate,
    evaluated: &EvaluatedCorpusRoom,
    traversal_grid: TraversalGrid,
) -> Result<RoomPickupDetourAnalysis, PickupDetourAnalysisError> {
    let room_id = evaluated.generated.id.clone();
    let Some(canonical) = evaluated.generated.variants.first() else {
        return Err(PickupDetourAnalysisError::MissingCanonicalVariant { room_id });
    };
    if candidate.key != canonical.key || candidate.generated.room != canonical.generated.room {
        return Err(PickupDetourAnalysisError::CandidateMismatch {
            room_id: evaluated.generated.id.clone(),
        });
    }

    analyze_pickup_detours_common(
        &evaluated.generated.id,
        &candidate.generated,
        &candidate.route_plan,
        &evaluated.matrices,
        traversal_grid,
    )
}

/// Analyze pickup challenge and detour evidence for a final-path room using
/// its validated post-feasibility canonical native candidate.
pub fn analyze_pickup_detours_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    traversal_grid: TraversalGrid,
) -> Result<RoomPickupDetourAnalysis, PickupDetourAnalysisError> {
    let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        PickupDetourAnalysisError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    analyze_pickup_detours_common(
        &evaluated.generated.id,
        candidate.generated(),
        candidate.route_plan(),
        &evaluated.matrices,
        traversal_grid,
    )
}

/// Explicit-candidate final-path entry point for independently regenerated
/// candidates. Exact native candidate and `Room` equality are required.
pub fn analyze_pickup_detours_for_corpus_candidate(
    candidate: &CorpusCandidate,
    evaluated: &EvaluatedCorpusRoomV2,
    traversal_grid: TraversalGrid,
) -> Result<RoomPickupDetourAnalysis, PickupDetourAnalysisError> {
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        PickupDetourAnalysisError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    if candidate != canonical {
        return Err(PickupDetourAnalysisError::CandidateMismatch {
            room_id: evaluated.generated.id.clone(),
        });
    }
    analyze_pickup_detours_common(
        &evaluated.generated.id,
        candidate.generated(),
        candidate.route_plan(),
        &evaluated.matrices,
        traversal_grid,
    )
}

fn analyze_pickup_detours_common(
    room_id: &RoomId,
    generated: &GeneratedLevel,
    route_plan: &RoutePlan,
    matrices: &[LoadoutRouteMatrix],
    traversal_grid: TraversalGrid,
) -> Result<RoomPickupDetourAnalysis, PickupDetourAnalysisError> {
    let door_ids = canonical_ids(
        generated.room.doors().iter().map(|door| door.id.as_str()),
        |duplicate| PickupDetourAnalysisError::DuplicateDoorId {
            room_id: room_id.clone(),
            door_id: duplicate,
        },
    )?;
    let pickup_ids = canonical_ids(
        generated.room.pickups().iter().map(|pickup| pickup.id()),
        |duplicate| PickupDetourAnalysisError::DuplicatePickupId {
            room_id: room_id.clone(),
            pickup_id: duplicate,
        },
    )?;
    let matrices = ordered_matrices(room_id, matrices, &door_ids, &pickup_ids)?;
    let structural_placements = structural_placements(&generated.room, route_plan, &pickup_ids)?;

    let mut cells =
        Vec::with_capacity(EvaluationLoadout::ALL.len() * door_ids.len() * pickup_ids.len());
    let mut operational_cost = PickupAnalysisOperationalCost::default();

    for (loadout_index, matrix) in matrices.iter().enumerate() {
        debug_assert_eq!(matrix.loadout, EvaluationLoadout::ALL[loadout_index]);
        for (source_index, source_door_id) in door_ids.iter().enumerate() {
            let source_effort = matrix
                .evidence
                .source_search_effort()
                .get(source_index)
                .ok_or_else(|| PickupDetourAnalysisError::SourceEffortCardinality {
                    room_id: room_id.clone(),
                    loadout: matrix.loadout,
                    expected: door_ids.len(),
                    actual: matrix.evidence.source_search_effort().len(),
                })?;
            if source_effort.source_door_id != *source_door_id {
                return Err(PickupDetourAnalysisError::SourceEffortOrder {
                    room_id: room_id.clone(),
                    loadout: matrix.loadout,
                    source_index,
                    expected_source_door_id: source_door_id.clone(),
                    actual_source_door_id: source_effort.source_door_id.clone(),
                });
            }
            operational_cost
                .source_searches
                .push(PickupSourceSearchCost {
                    loadout: matrix.loadout,
                    source_index,
                    source_door_id: source_door_id.clone(),
                    search: source_effort.stats,
                });

            let initial = Simulation::enter_via_door(
                generated.room.clone(),
                matrix.loadout.abilities(),
                source_door_id,
            )
            .map_err(|source| PickupDetourAnalysisError::DoorEntry {
                room_id: room_id.clone(),
                loadout: matrix.loadout,
                source_door_id: source_door_id.clone(),
                source,
            })?;

            let door_rows = matrix
                .evidence
                .door_routes()
                .iter()
                .filter(|row| row.source_door_id == *source_door_id)
                .map(|row| {
                    let observed = match &row.evidence {
                        BoundedTargetEvidence::Positive(positive) => {
                            validate_door_solution(positive.solution(), &row.target_door_id)?;
                            Some(observe_replay_prefix(
                                &initial,
                                &positive.solution().replay,
                                ReplayObjective::Door(&row.target_door_id),
                                traversal_grid,
                            )?)
                        }
                        BoundedTargetEvidence::Inconclusive(_) => None,
                    };
                    Ok(ObservedDoorRow { row, observed })
                })
                .collect::<Result<Vec<_>, PickupDetourAnalysisError>>()?;

            for row in matrix
                .evidence
                .pickup_routes()
                .iter()
                .filter(|row| row.source_door_id == *source_door_id)
            {
                let (evidence, pickup_observation, pickup_target_snapshot) = match &row.evidence {
                    BoundedTargetEvidence::Positive(positive) => {
                        validate_pickup_solution(positive.solution(), &row.required_pickup_id)?;
                        let observation = observe_replay_prefix(
                            &initial,
                            &positive.solution().replay,
                            ReplayObjective::Pickup(&row.required_pickup_id),
                            traversal_grid,
                        )?;
                        let challenge = positive_challenge(matrix.loadout, positive, &observation);
                        (
                            PickupCellEvidence::Positive(Box::new(challenge)),
                            Some(observation),
                            positive.solution().stats,
                        )
                    }
                    BoundedTargetEvidence::Inconclusive(inconclusive) => (
                        PickupCellEvidence::BoundedInconclusive {
                            reason: inconclusive.reason,
                        },
                        None,
                        inconclusive.search_effort,
                    ),
                };
                let canonical_door_context = door_context(
                    &row.required_pickup_id,
                    pickup_observation.as_ref(),
                    &door_rows,
                );
                cells.push(PickupChallengeCell {
                    loadout: matrix.loadout,
                    source_door_id: source_door_id.clone(),
                    pickup_id: row.required_pickup_id.clone(),
                    evidence,
                    canonical_door_context,
                    operational: PickupCellOperationalCost {
                        shared_source_search: source_effort.stats,
                        pickup_target_snapshot,
                    },
                });
            }
        }
    }

    let cross_loadout = cross_loadout_evidence(&door_ids, &pickup_ids, &cells);
    Ok(RoomPickupDetourAnalysis {
        version: PICKUP_DETOUR_ANALYSIS_VERSION,
        room_id: room_id.clone(),
        traversal_grid,
        structural_placements,
        cells,
        operational_cost,
        cross_loadout,
    })
}

fn canonical_ids<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    duplicate_error: impl FnOnce(String) -> PickupDetourAnalysisError,
) -> Result<Vec<String>, PickupDetourAnalysisError> {
    let mut result = ids.into_iter().map(str::to_owned).collect::<Vec<String>>();
    result.sort_unstable();
    if let Some(duplicate) = result
        .windows(2)
        .find(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].clone())
    {
        return Err(duplicate_error(duplicate));
    }
    Ok(result)
}

fn ordered_matrices<'a>(
    room_id: &RoomId,
    matrices: &'a [LoadoutRouteMatrix],
    door_ids: &[String],
    pickup_ids: &[String],
) -> Result<Vec<&'a LoadoutRouteMatrix>, PickupDetourAnalysisError> {
    let expected_doors = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect::<Vec<_>>();
    let expected_pickups = door_ids
        .iter()
        .flat_map(|source| {
            pickup_ids
                .iter()
                .map(move |pickup| (source.clone(), pickup.clone()))
        })
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(EvaluationLoadout::ALL.len());
    for loadout in EvaluationLoadout::ALL {
        let matches = matrices
            .iter()
            .filter(|matrix| matrix.loadout == loadout)
            .collect::<Vec<_>>();
        let [matrix] = matches.as_slice() else {
            return Err(if matches.is_empty() {
                PickupDetourAnalysisError::MissingLoadoutMatrix {
                    room_id: room_id.clone(),
                    loadout,
                }
            } else {
                PickupDetourAnalysisError::DuplicateLoadoutMatrix {
                    room_id: room_id.clone(),
                    loadout,
                }
            });
        };
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(PickupDetourAnalysisError::MatrixLoadoutMismatch {
                room_id: room_id.clone(),
                loadout,
            });
        }
        if matrix.evidence.door_routes().len() != expected_doors.len() {
            return Err(PickupDetourAnalysisError::DoorRowCardinality {
                room_id: room_id.clone(),
                loadout,
                expected: expected_doors.len(),
                actual: matrix.evidence.door_routes().len(),
            });
        }
        for (row_index, (row, expected)) in matrix
            .evidence
            .door_routes()
            .iter()
            .zip(&expected_doors)
            .enumerate()
        {
            if (&row.source_door_id, &row.target_door_id) != (&expected.0, &expected.1) {
                return Err(PickupDetourAnalysisError::DoorRowOrder {
                    room_id: room_id.clone(),
                    loadout,
                    row_index,
                    expected: Box::new(expected.clone()),
                    actual: Box::new((row.source_door_id.clone(), row.target_door_id.clone())),
                });
            }
        }
        if matrix.evidence.pickup_routes().len() != expected_pickups.len() {
            return Err(PickupDetourAnalysisError::PickupRowCardinality {
                room_id: room_id.clone(),
                loadout,
                expected: expected_pickups.len(),
                actual: matrix.evidence.pickup_routes().len(),
            });
        }
        for (row_index, (row, expected)) in matrix
            .evidence
            .pickup_routes()
            .iter()
            .zip(&expected_pickups)
            .enumerate()
        {
            if (&row.source_door_id, &row.required_pickup_id) != (&expected.0, &expected.1) {
                return Err(PickupDetourAnalysisError::PickupRowOrder {
                    room_id: room_id.clone(),
                    loadout,
                    row_index,
                    expected: Box::new(expected.clone()),
                    actual: Box::new((row.source_door_id.clone(), row.required_pickup_id.clone())),
                });
            }
        }
        if matrix.evidence.source_search_effort().len() != door_ids.len() {
            return Err(PickupDetourAnalysisError::SourceEffortCardinality {
                room_id: room_id.clone(),
                loadout,
                expected: door_ids.len(),
                actual: matrix.evidence.source_search_effort().len(),
            });
        }
        ordered.push(*matrix);
    }
    Ok(ordered)
}

fn validate_pickup_solution(
    solution: &TargetSolution,
    expected_pickup_id: &str,
) -> Result<(), PickupDetourAnalysisError> {
    let target = SearchTarget::pickup(expected_pickup_id);
    let reached = ReachedTarget::Pickup(expected_pickup_id.to_owned());
    if solution.target != target || solution.reached != reached {
        return Err(PickupDetourAnalysisError::MalformedPickupPositive {
            expected_pickup_id: expected_pickup_id.to_owned(),
            target: solution.target.clone(),
            reached: solution.reached.clone(),
        });
    }
    Ok(())
}

fn validate_door_solution(
    solution: &TargetSolution,
    expected_door_id: &str,
) -> Result<(), PickupDetourAnalysisError> {
    let target = SearchTarget::door(expected_door_id);
    let reached = ReachedTarget::Door(expected_door_id.to_owned());
    if solution.target != target || solution.reached != reached {
        return Err(PickupDetourAnalysisError::MalformedDoorPositive {
            expected_door_id: expected_door_id.to_owned(),
            target: solution.target.clone(),
            reached: solution.reached.clone(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ReplayObjective<'a> {
    Pickup(&'a str),
    Door(&'a str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedReplayPrefix {
    completion_ticks: usize,
    target_collection_tick: Option<usize>,
    traversal: TraversalTrace,
    actions: SemanticActionTrace,
    collection_events: Vec<PickupCollectionEvent>,
    retained_pickups: BTreeSet<String>,
}

fn observe_replay_prefix(
    initial: &Simulation,
    replay: &Replay,
    objective: ReplayObjective<'_>,
    grid: TraversalGrid,
) -> Result<ObservedReplayPrefix, PickupDetourAnalysisError> {
    replay
        .verify(initial)
        .map_err(|source| PickupDetourAnalysisError::ReplayDiverged {
            objective: objective.label(),
            source,
        })?;

    let room_width = i32::from(initial.room().width()) * initial.room().tile_size();
    let room_height = i32::from(initial.room().height()) * initial.room().tile_size();
    let mut simulation = initial.clone();
    let initial_cell = traversal_cell(&simulation, grid, room_width, room_height);
    let mut traversal_spans = vec![TraversalSpan {
        cell: initial_cell,
        samples: 1,
    }];
    let mut visited = BTreeSet::from([initial_cell]);
    let mut action_spans = Vec::<ActionSpan>::new();
    let mut semantic_events = Vec::<SemanticEventAt>::new();
    let mut previous = SemanticAction::default();
    let mut jump_presses = 0;
    let mut dash_presses = 0;
    let mut restart_presses = 0;
    let mut successful_jumps = 0;
    let mut successful_wall_jumps = 0;
    let mut successful_dashes = 0;
    let mut deaths = 0;
    let mut pickups_collected = 0;
    let mut completion_ticks = 0;
    let mut target_collection_tick = None;
    let mut collection_events = Vec::new();
    let mut retained_pickups = BTreeSet::<String>::new();
    let mut completed = false;
    let mut unexpected_door = None;

    for frame in &replay.frames {
        if completed {
            break;
        }
        let action = SemanticAction::from(frame.action);
        if action.jump_held && !previous.jump_held {
            jump_presses += 1;
        }
        if action.dash_held && !previous.dash_held {
            dash_presses += 1;
        }
        if action.restart && !previous.restart {
            restart_presses += 1;
        }
        push_action_span(&mut action_spans, action);

        let report = simulation.step(frame.action);
        completion_ticks += 1;
        for event in report.events {
            let semantic = match event {
                SimulationEvent::Jumped(JumpKind::Grounded) => {
                    successful_jumps += 1;
                    SemanticEvent::GroundJump
                }
                SimulationEvent::Jumped(JumpKind::Coyote) => {
                    successful_jumps += 1;
                    SemanticEvent::CoyoteJump
                }
                SimulationEvent::Jumped(JumpKind::Buffered) => {
                    successful_jumps += 1;
                    SemanticEvent::BufferedJump
                }
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    successful_jumps += 1;
                    successful_wall_jumps += 1;
                    SemanticEvent::WallJump(side)
                }
                SimulationEvent::Dashed { direction } => {
                    successful_dashes += 1;
                    SemanticEvent::Dash(direction)
                }
                SimulationEvent::Landed => SemanticEvent::Land,
                SimulationEvent::Died(DeathReason::Hazard { .. }) => {
                    deaths += 1;
                    SemanticEvent::DeathFromStaticHazard
                }
                SimulationEvent::Died(DeathReason::TimedHazard { .. }) => {
                    deaths += 1;
                    SemanticEvent::DeathFromTimedHazard
                }
                SimulationEvent::PickupCollected { id } => {
                    pickups_collected += 1;
                    retained_pickups.insert(id.clone());
                    collection_events.push(PickupCollectionEvent {
                        pickup_id: id.clone(),
                        replay_tick: completion_ticks,
                    });
                    if matches!(objective, ReplayObjective::Pickup(target) if target == id) {
                        target_collection_tick = Some(completion_ticks);
                        completed = true;
                    }
                    SemanticEvent::Pickup
                }
                SimulationEvent::Reset => {
                    retained_pickups.clear();
                    SemanticEvent::Reset
                }
                SimulationEvent::ExitReached { id } => {
                    match objective {
                        ReplayObjective::Door(target) if target == id => completed = true,
                        ReplayObjective::Door(_) | ReplayObjective::Pickup(_) => {
                            unexpected_door = Some(id.clone());
                        }
                    }
                    SemanticEvent::Exit
                }
            };
            semantic_events.push(SemanticEventAt {
                tick: completion_ticks,
                event: semantic,
            });
        }

        let cell = traversal_cell(&simulation, grid, room_width, room_height);
        push_traversal_span(&mut traversal_spans, cell);
        visited.insert(cell);
        let reset_this_tick = semantic_events.last().is_some_and(|event| {
            event.tick == completion_ticks && event.event == SemanticEvent::Reset
        });
        previous = if reset_this_tick {
            SemanticAction::default()
        } else {
            action
        };
        if !completed && unexpected_door.is_some() {
            break;
        }
    }

    if !completed {
        if let Some(actual_door_id) = unexpected_door {
            return Err(PickupDetourAnalysisError::UnexpectedDoorBeforeTarget {
                objective: objective.label(),
                actual_door_id,
            });
        }
        return Err(PickupDetourAnalysisError::TargetNotReached {
            objective: objective.label(),
        });
    }

    Ok(ObservedReplayPrefix {
        completion_ticks,
        target_collection_tick,
        traversal: TraversalTrace {
            grid,
            sample_count: completion_ticks + 1,
            spans: traversal_spans.into_boxed_slice(),
            visited_cells: visited.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        },
        actions: SemanticActionTrace {
            total_ticks: completion_ticks,
            spans: action_spans.into_boxed_slice(),
            events: semantic_events.into_boxed_slice(),
            jump_presses,
            dash_presses,
            restart_presses,
            successful_jumps,
            successful_wall_jumps,
            successful_dashes,
            deaths,
            pickups_collected,
        },
        collection_events,
        retained_pickups,
    })
}

impl ReplayObjective<'_> {
    fn label(self) -> SearchTarget {
        match self {
            Self::Pickup(id) => SearchTarget::pickup(id),
            Self::Door(id) => SearchTarget::door(id),
        }
    }
}

fn traversal_cell(
    simulation: &Simulation,
    grid: TraversalGrid,
    room_width: i32,
    room_height: i32,
) -> TraversalCell {
    let bounds = simulation.player().bounds();
    let center_x = (bounds.x + bounds.width / 2).clamp(0, room_width - 1);
    let center_y = (bounds.y + bounds.height / 2).clamp(0, room_height - 1);
    TraversalCell {
        x: ((i64::from(center_x) * i64::from(grid.columns())) / i64::from(room_width)) as u16,
        y: ((i64::from(center_y) * i64::from(grid.rows())) / i64::from(room_height)) as u16,
    }
}

fn push_traversal_span(spans: &mut Vec<TraversalSpan>, cell: TraversalCell) {
    if let Some(last) = spans.last_mut()
        && last.cell == cell
    {
        last.samples += 1;
    } else {
        spans.push(TraversalSpan { cell, samples: 1 });
    }
}

fn push_action_span(spans: &mut Vec<ActionSpan>, action: SemanticAction) {
    if let Some(last) = spans.last_mut()
        && last.action == action
    {
        last.ticks += 1;
    } else {
        spans.push(ActionSpan { action, ticks: 1 });
    }
}

fn positive_challenge(
    loadout: EvaluationLoadout,
    positive: &ReplayCertifiedTargetEvidence,
    observation: &ObservedReplayPrefix,
) -> PickupPositiveChallenge {
    let target_collection_tick = observation
        .target_collection_tick
        .expect("a completed pickup observation records target contact");
    PickupPositiveChallenge {
        witness_fingerprint: positive.witness_fingerprint(),
        completion: PickupCompletionDemand {
            completion_ticks: observation.completion_ticks,
            target_collection_tick,
            deaths_before_completion: observation.actions.deaths,
            resets_before_completion: observation
                .actions
                .events
                .iter()
                .filter(|event| event.event == SemanticEvent::Reset)
                .count(),
            pickup_collection_events: observation.collection_events.clone(),
        },
        control: control_demand(&observation.actions),
        traversal: traversal_demand(&observation.traversal),
        ability: ability_demand(loadout, &observation.actions),
        action_trace: observation.actions.clone(),
        traversal_trace: observation.traversal.clone(),
    }
}

fn control_demand(actions: &SemanticActionTrace) -> PickupControlDemand {
    let horizontal_directions = actions
        .spans
        .iter()
        .filter_map(|span| (span.action.move_x != 0).then_some(span.action.move_x))
        .collect::<BTreeSet<_>>();
    let uses_vertical_input = actions.spans.iter().any(|span| span.action.move_y != 0);
    let uses_jump_input = actions.spans.iter().any(|span| span.action.jump_held);
    let uses_dash_input = actions.spans.iter().any(|span| span.action.dash_held);
    let uses_restart_input = actions.spans.iter().any(|span| span.action.restart);
    let horizontal_reversals = direction_reversals(&actions.spans, |action| action.move_x);
    let vertical_input_changes = input_direction_changes(&actions.spans, |action| action.move_y);
    let dash_direction_changes = accepted_dash_direction_changes(actions);
    let vertical_decisions = actions
        .jump_presses
        .saturating_add(vertical_input_changes)
        .saturating_add(dash_direction_changes);
    let run_only = horizontal_reversals == 0
        && horizontal_directions.len() <= 1
        && !uses_vertical_input
        && !uses_jump_input
        && !uses_dash_input
        && !uses_restart_input
        && actions.successful_jumps == 0
        && actions.successful_dashes == 0;
    let monotone_simple = horizontal_reversals == 0
        && horizontal_directions.len() <= 1
        && !uses_vertical_input
        && !uses_dash_input
        && !uses_restart_input
        && actions.successful_dashes == 0;
    PickupControlDemand {
        run_only,
        monotone_simple,
        jump_press_edges: actions.jump_presses,
        dash_press_edges: actions.dash_presses,
        horizontal_reversals,
        vertical_input_changes,
        dash_direction_changes,
        vertical_decisions,
        semantic_spans: actions.spans.len(),
        semantic_transitions: actions.spans.len().saturating_sub(usize::from(
            actions
                .spans
                .first()
                .is_some_and(|span| span.action == SemanticAction::default()),
        )),
        duration_ticks: actions.total_ticks,
    }
}

fn direction_reversals(spans: &[ActionSpan], direction: impl Fn(SemanticAction) -> i8) -> usize {
    let mut previous_nonzero = 0;
    let mut reversals = 0;
    for span in spans {
        let current = direction(span.action);
        if current == 0 {
            continue;
        }
        if previous_nonzero != 0 && current != previous_nonzero {
            reversals += 1;
        }
        previous_nonzero = current;
    }
    reversals
}

fn input_direction_changes(
    spans: &[ActionSpan],
    direction: impl Fn(SemanticAction) -> i8,
) -> usize {
    let Some(first) = spans.first() else {
        return 0;
    };
    let mut previous = direction(first.action);
    spans[1..]
        .iter()
        .filter(|span| {
            let current = direction(span.action);
            let changed = current != previous && (current != 0 || previous != 0);
            previous = current;
            changed
        })
        .count()
}

fn accepted_dash_direction_changes(actions: &SemanticActionTrace) -> usize {
    let mut previous = None::<DashDirection>;
    let mut changes = 0;
    for direction in actions.events.iter().filter_map(|event| match event.event {
        SemanticEvent::Dash(direction) => Some(direction),
        _ => None,
    }) {
        if previous.is_some_and(|previous| previous != direction) {
            changes += 1;
        }
        previous = Some(direction);
    }
    changes
}

fn traversal_demand(trace: &TraversalTrace) -> PickupTraversalDemand {
    let (min_x, max_x, min_y, max_y) = trace.visited_cells.iter().fold(
        (u16::MAX, 0_u16, u16::MAX, 0_u16),
        |(min_x, max_x, min_y, max_y), cell| {
            (
                min_x.min(cell.x),
                max_x.max(cell.x),
                min_y.min(cell.y),
                max_y.max(cell.y),
            )
        },
    );
    let coarse_path_steps = trace
        .spans
        .windows(2)
        .map(|pair| {
            usize::from(pair[0].cell.x.abs_diff(pair[1].cell.x))
                + usize::from(pair[0].cell.y.abs_diff(pair[1].cell.y))
        })
        .sum();
    PickupTraversalDemand {
        sample_count: trace.sample_count,
        traversal_spans: trace.spans.len(),
        visited_cell_count: trace.visited_cells.len(),
        horizontal_cell_span: if trace.visited_cells.is_empty() {
            0
        } else {
            max_x - min_x
        },
        vertical_cell_span: if trace.visited_cells.is_empty() {
            0
        } else {
            max_y - min_y
        },
        coarse_path_steps,
    }
}

fn ability_demand(
    loadout: EvaluationLoadout,
    actions: &SemanticActionTrace,
) -> PickupAbilityDemand {
    let ordinary_jump_events = actions
        .successful_jumps
        .saturating_sub(actions.successful_wall_jumps);
    PickupAbilityDemand {
        loadout,
        ordinary_jump_events,
        wall_jump_events: actions.successful_wall_jumps,
        dash_events: actions.successful_dashes,
        used_wall_jump: actions.successful_wall_jumps > 0,
        used_dash: actions.successful_dashes > 0,
        enabled_wall_jump_unused_by_witness: loadout.abilities().wall_jump
            && actions.successful_wall_jumps == 0,
        enabled_dash_unused_by_witness: loadout.abilities().dash && actions.successful_dashes == 0,
    }
}

struct ObservedDoorRow<'a> {
    row: &'a downwards_validation::DoorRouteEvidence,
    observed: Option<ObservedReplayPrefix>,
}

fn door_context(
    pickup_id: &str,
    pickup: Option<&ObservedReplayPrefix>,
    door_rows: &[ObservedDoorRow<'_>],
) -> CanonicalDoorPickupContext {
    let mut routes = Vec::with_capacity(door_rows.len());
    let mut opportunistic = Vec::new();
    let mut positive_noncollecting = 0;
    let mut bounded = 0;
    for door in door_rows {
        match (&door.row.evidence, &door.observed) {
            (BoundedTargetEvidence::Positive(_), Some(observed)) => {
                let retained = observed.retained_pickups.contains(pickup_id);
                if retained {
                    opportunistic.push(door.row.target_door_id.clone());
                } else {
                    positive_noncollecting += 1;
                }
                let collection_events = observed
                    .collection_events
                    .iter()
                    .filter(|event| event.pickup_id == pickup_id)
                    .cloned()
                    .collect();
                routes.push(CanonicalDoorPickupRoute::Positive {
                    target_door_id: door.row.target_door_id.clone(),
                    pickup_retained_at_door: retained,
                    collection_events,
                    difference_from_pickup_witness: pickup
                        .map(|pickup| route_difference(pickup, observed)),
                });
            }
            (BoundedTargetEvidence::Inconclusive(inconclusive), None) => {
                bounded += 1;
                routes.push(CanonicalDoorPickupRoute::BoundedInconclusive {
                    target_door_id: door.row.target_door_id.clone(),
                    reason: inconclusive.reason,
                });
            }
            _ => unreachable!("door observation presence follows the evidence variant"),
        }
    }
    let relation = if !opportunistic.is_empty() {
        FiniteCanonicalPickupRelation::ObservedOpportunistically {
            target_door_ids: opportunistic,
            positive_noncollecting_door_witnesses: positive_noncollecting,
            bounded_door_cells: bounded,
        }
    } else if positive_noncollecting > 0 {
        FiniteCanonicalPickupRelation::TargetDirectedOnlyAmongObservedWitnesses {
            positive_door_witnesses: positive_noncollecting,
            bounded_door_cells: bounded,
        }
    } else {
        FiniteCanonicalPickupRelation::NoPositiveDoorWitnessContext {
            bounded_door_cells: bounded,
        }
    };
    CanonicalDoorPickupContext { relation, routes }
}

fn route_difference(
    pickup: &ObservedReplayPrefix,
    door: &ObservedReplayPrefix,
) -> PickupDoorRouteDifference {
    let pickup_cells = pickup
        .traversal
        .visited_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let door_cells = door
        .traversal
        .visited_cells
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let shared = pickup_cells.intersection(&door_cells).count();
    let pickup_only = pickup_cells.difference(&door_cells).count();
    let door_only = door_cells.difference(&pickup_cells).count();
    let pickup_span_cells = pickup
        .traversal
        .spans
        .iter()
        .map(|span| span.cell)
        .collect::<Vec<_>>();
    let door_span_cells = door
        .traversal
        .spans
        .iter()
        .map(|span| span.cell)
        .collect::<Vec<_>>();
    let pickup_span_actions = pickup
        .actions
        .spans
        .iter()
        .map(|span| span.action)
        .collect::<Vec<_>>();
    let door_span_actions = door
        .actions
        .spans
        .iter()
        .map(|span| span.action)
        .collect::<Vec<_>>();
    let shared_prefix_ticks = expanded_actions(&pickup.actions)
        .zip(expanded_actions(&door.actions))
        .take_while(|(left, right)| left == right)
        .count();
    PickupDoorRouteDifference {
        spatial: PickupDoorSpatialDifference {
            shared_visited_cells: shared,
            pickup_only_visited_cells: pickup_only,
            door_only_visited_cells: door_only,
            union_visited_cells: shared + pickup_only + door_only,
            traversal_span_edit_distance: edit_distance(&pickup_span_cells, &door_span_cells),
        },
        action: PickupDoorActionDifference {
            shared_prefix_ticks,
            pickup_duration_ticks: pickup.completion_ticks,
            door_duration_ticks: door.completion_ticks,
            absolute_duration_difference: pickup.completion_ticks.abs_diff(door.completion_ticks),
            semantic_span_edit_distance: edit_distance(&pickup_span_actions, &door_span_actions),
        },
    }
}

fn expanded_actions(trace: &SemanticActionTrace) -> impl Iterator<Item = SemanticAction> + '_ {
    trace
        .spans
        .iter()
        .flat_map(|span| std::iter::repeat_n(span.action, span.ticks))
}

fn edit_distance<T: Eq>(left: &[T], right: &[T]) -> usize {
    if left.len() < right.len() {
        return edit_distance(right, left);
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_value) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_value) in right.iter().enumerate() {
            current[right_index + 1] = if left_value == right_value {
                previous[right_index]
            } else {
                previous[right_index]
                    .min(previous[right_index + 1])
                    .min(current[right_index])
                    + 1
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn cross_loadout_evidence(
    door_ids: &[String],
    pickup_ids: &[String],
    cells: &[PickupChallengeCell],
) -> Vec<PickupCrossLoadoutEvidence> {
    let mut result = Vec::with_capacity(door_ids.len() * pickup_ids.len());
    for source_door_id in door_ids {
        for pickup_id in pickup_ids {
            let matching = cells.iter().filter(|cell| {
                cell.source_door_id == *source_door_id && cell.pickup_id == *pickup_id
            });
            let mut positive_loadouts = Vec::new();
            let mut bounded_loadouts = Vec::new();
            let mut witnesses_using_wall_jump = Vec::new();
            let mut witnesses_using_dash = Vec::new();
            for cell in matching {
                match &cell.evidence {
                    PickupCellEvidence::Positive(positive) => {
                        positive_loadouts.push(cell.loadout);
                        if positive.ability.used_wall_jump {
                            witnesses_using_wall_jump.push(cell.loadout);
                        }
                        if positive.ability.used_dash {
                            witnesses_using_dash.push(cell.loadout);
                        }
                    }
                    PickupCellEvidence::BoundedInconclusive { reason } => {
                        bounded_loadouts.push(PickupBoundedLoadout {
                            loadout: cell.loadout,
                            reason: *reason,
                        });
                    }
                }
            }
            result.push(PickupCrossLoadoutEvidence {
                source_door_id: source_door_id.clone(),
                pickup_id: pickup_id.clone(),
                positive_without_wall_jump: positive_loadouts
                    .iter()
                    .any(|loadout| !loadout.abilities().wall_jump),
                positive_without_dash: positive_loadouts
                    .iter()
                    .any(|loadout| !loadout.abilities().dash),
                positive_loadouts,
                bounded_loadouts,
                witnesses_using_wall_jump,
                witnesses_using_dash,
            });
        }
    }
    result
}

fn structural_placements(
    room: &downwards_core::Room,
    plan: &RoutePlan,
    pickup_ids: &[String],
) -> Result<Vec<PickupStructuralPlacement>, PickupDetourAnalysisError> {
    let graph = AuthoredGraph::new(plan)?;
    let spine = graph.shortest_port_spine();
    let distances_to_spine = graph.distances_from_set(&spine);
    pickup_ids
        .iter()
        .map(|pickup_id| {
            let pickup = room
                .pickups()
                .iter()
                .find(|pickup| pickup.id() == pickup_id)
                .expect("pickup IDs were taken from this exact room");
            let matching = plan
                .nodes
                .iter()
                .filter(|node| {
                    node.role == NodeRole::Pickup
                        && pickup_is_above_support(
                            pickup.bounds(),
                            node.support.start_x,
                            node.support.end_x,
                            node.support.row,
                            room.tile_size(),
                        )
                })
                .map(|node| node.id)
                .collect::<Vec<_>>();
            let mapping = match matching.as_slice() {
                [] => PickupRoutePlanMapping::NoMatchingAuthoredPickupNode,
                [node_id] => {
                    let index = graph.index_by_id[node_id];
                    let incident = plan
                        .edges
                        .iter()
                        .filter(|edge| edge.from == *node_id || edge.to == *node_id)
                        .collect::<Vec<_>>();
                    PickupRoutePlanMapping::Unique(PickupAuthoredNodePlacement {
                        node_id: *node_id,
                        undirected_degree: graph.adjacency[index].len(),
                        authored_leaf: graph.adjacency[index].len() == 1,
                        on_shortest_port_spine: spine.contains(&index),
                        distance_to_shortest_port_spine: distances_to_spine[index],
                        incident_critical_edges: incident
                            .iter()
                            .filter(|edge| edge.critical)
                            .count(),
                        incident_wall_climb_edges: incident
                            .iter()
                            .filter(|edge| edge.verb == RouteVerb::WallClimb)
                            .count(),
                        incident_dash_edges: incident
                            .iter()
                            .filter(|edge| {
                                matches!(edge.verb, RouteVerb::DashAcross | RouteVerb::DashUp)
                            })
                            .count(),
                    })
                }
                _ => PickupRoutePlanMapping::AmbiguousMatchingAuthoredPickupNodes {
                    node_ids: matching,
                },
            };
            Ok(PickupStructuralPlacement {
                pickup_id: pickup_id.clone(),
                mapping,
            })
        })
        .collect()
}

fn pickup_is_above_support(
    pickup: downwards_core::Rect,
    start_x: u16,
    end_x: u16,
    row: u16,
    tile_size: i32,
) -> bool {
    let support_left = i32::from(start_x) * tile_size;
    let support_right = i32::from(end_x) * tile_size;
    let support_top = i32::from(row) * tile_size;
    pickup.x >= support_left
        && pickup.right() <= support_right
        && support_top - pickup.bottom() == downwards_core::PLAYER_HEIGHT
}

struct AuthoredGraph {
    index_by_id: BTreeMap<u16, usize>,
    adjacency: Vec<Vec<usize>>,
    ports: Vec<usize>,
}

impl AuthoredGraph {
    fn new(plan: &RoutePlan) -> Result<Self, PickupDetourAnalysisError> {
        let mut index_by_id = BTreeMap::new();
        for (index, node) in plan.nodes.iter().enumerate() {
            if index_by_id.insert(node.id, index).is_some() {
                return Err(PickupDetourAnalysisError::DuplicateRouteNodeId { node_id: node.id });
            }
        }
        let mut adjacency = vec![Vec::new(); plan.nodes.len()];
        for edge in &plan.edges {
            let Some(&from) = index_by_id.get(&edge.from) else {
                return Err(PickupDetourAnalysisError::MissingRouteEdgeNode { node_id: edge.from });
            };
            let Some(&to) = index_by_id.get(&edge.to) else {
                return Err(PickupDetourAnalysisError::MissingRouteEdgeNode { node_id: edge.to });
            };
            if !adjacency[from].contains(&to) {
                adjacency[from].push(to);
            }
            if !adjacency[to].contains(&from) {
                adjacency[to].push(from);
            }
        }
        for neighbors in &mut adjacency {
            neighbors.sort_unstable();
        }
        let ports = plan
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.role == NodeRole::Port).then_some(index))
            .collect();
        Ok(Self {
            index_by_id,
            adjacency,
            ports,
        })
    }

    fn distances_from(&self, source: usize) -> Vec<Option<usize>> {
        let mut distances = vec![None; self.adjacency.len()];
        distances[source] = Some(0);
        let mut queue = VecDeque::from([source]);
        while let Some(node) = queue.pop_front() {
            let distance = distances[node].expect("queued nodes have a distance");
            for &neighbor in &self.adjacency[node] {
                if distances[neighbor].is_none() {
                    distances[neighbor] = Some(distance + 1);
                    queue.push_back(neighbor);
                }
            }
        }
        distances
    }

    fn shortest_port_spine(&self) -> BTreeSet<usize> {
        let distances = self
            .ports
            .iter()
            .map(|&port| self.distances_from(port))
            .collect::<Vec<_>>();
        let mut spine = BTreeSet::new();
        for left in 0..self.ports.len() {
            for right in left + 1..self.ports.len() {
                let Some(total) = distances[left][self.ports[right]] else {
                    continue;
                };
                for (node, to_node) in distances[left].iter().copied().enumerate() {
                    if let (Some(to_node), Some(from_node)) = (to_node, distances[right][node])
                        && to_node + from_node == total
                    {
                        spine.insert(node);
                    }
                }
            }
        }
        spine
    }

    fn distances_from_set(&self, sources: &BTreeSet<usize>) -> Vec<Option<usize>> {
        let mut distances = vec![None; self.adjacency.len()];
        let mut queue = VecDeque::new();
        for &source in sources {
            distances[source] = Some(0);
            queue.push_back(source);
        }
        while let Some(node) = queue.pop_front() {
            let distance = distances[node].expect("queued nodes have a distance");
            for &neighbor in &self.adjacency[node] {
                if distances[neighbor].is_none() {
                    distances[neighbor] = Some(distance + 1);
                    queue.push_back(neighbor);
                }
            }
        }
        distances
    }
}

#[derive(Debug)]
pub enum PickupDetourAnalysisError {
    CorpusV2Identity {
        source: Box<CorpusMetricInputV2Error>,
    },
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    CandidateMismatch {
        room_id: RoomId,
    },
    DuplicateDoorId {
        room_id: RoomId,
        door_id: String,
    },
    DuplicatePickupId {
        room_id: RoomId,
        pickup_id: String,
    },
    MissingLoadoutMatrix {
        room_id: RoomId,
        loadout: EvaluationLoadout,
    },
    DuplicateLoadoutMatrix {
        room_id: RoomId,
        loadout: EvaluationLoadout,
    },
    MatrixLoadoutMismatch {
        room_id: RoomId,
        loadout: EvaluationLoadout,
    },
    DoorRowCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected: usize,
        actual: usize,
    },
    DoorRowOrder {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        row_index: usize,
        expected: Box<(String, String)>,
        actual: Box<(String, String)>,
    },
    PickupRowCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected: usize,
        actual: usize,
    },
    PickupRowOrder {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        row_index: usize,
        expected: Box<(String, String)>,
        actual: Box<(String, String)>,
    },
    SourceEffortCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected: usize,
        actual: usize,
    },
    SourceEffortOrder {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source_index: usize,
        expected_source_door_id: String,
        actual_source_door_id: String,
    },
    DoorEntry {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source_door_id: String,
        source: DoorEntryError,
    },
    MalformedPickupPositive {
        expected_pickup_id: String,
        target: SearchTarget,
        reached: ReachedTarget,
    },
    MalformedDoorPositive {
        expected_door_id: String,
        target: SearchTarget,
        reached: ReachedTarget,
    },
    ReplayDiverged {
        objective: SearchTarget,
        source: ReplayDivergence,
    },
    UnexpectedDoorBeforeTarget {
        objective: SearchTarget,
        actual_door_id: String,
    },
    TargetNotReached {
        objective: SearchTarget,
    },
    DuplicateRouteNodeId {
        node_id: u16,
    },
    MissingRouteEdgeNode {
        node_id: u16,
    },
}

impl fmt::Display for PickupDetourAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorpusV2Identity { source } => source.fmt(formatter),
            Self::MissingCanonicalVariant { room_id } => {
                write!(formatter, "room {} has no canonical variant", room_id.0)
            }
            Self::CandidateMismatch { room_id } => write!(
                formatter,
                "explicit pickup-analysis candidate does not match room {} canonical variant",
                room_id.0
            ),
            Self::DuplicateDoorId { room_id, door_id } => {
                write!(
                    formatter,
                    "room {} has duplicate door ID {door_id:?}",
                    room_id.0
                )
            }
            Self::DuplicatePickupId { room_id, pickup_id } => write!(
                formatter,
                "room {} has duplicate pickup ID {pickup_id:?}",
                room_id.0
            ),
            Self::MissingLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "room {} is missing its {} pickup matrix",
                room_id.0,
                loadout.slug()
            ),
            Self::DuplicateLoadoutMatrix { room_id, loadout } => write!(
                formatter,
                "room {} has duplicate {} pickup matrices",
                room_id.0,
                loadout.slug()
            ),
            Self::MatrixLoadoutMismatch { room_id, loadout } => write!(
                formatter,
                "room {} {} matrix carries different exact abilities",
                room_id.0,
                loadout.slug()
            ),
            Self::DoorRowCardinality {
                room_id,
                loadout,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} {} matrix has {actual}/{expected} door rows",
                room_id.0,
                loadout.slug()
            ),
            Self::DoorRowOrder {
                room_id,
                loadout,
                row_index,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} {} door row {row_index} is {actual:?}, expected {expected:?}",
                room_id.0,
                loadout.slug()
            ),
            Self::PickupRowCardinality {
                room_id,
                loadout,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} {} matrix has {actual}/{expected} pickup rows",
                room_id.0,
                loadout.slug()
            ),
            Self::PickupRowOrder {
                room_id,
                loadout,
                row_index,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} {} pickup row {row_index} is {actual:?}, expected {expected:?}",
                room_id.0,
                loadout.slug()
            ),
            Self::SourceEffortCardinality {
                room_id,
                loadout,
                expected,
                actual,
            } => write!(
                formatter,
                "room {} {} matrix has {actual}/{expected} source-effort rows",
                room_id.0,
                loadout.slug()
            ),
            Self::SourceEffortOrder {
                room_id,
                loadout,
                source_index,
                expected_source_door_id,
                actual_source_door_id,
            } => write!(
                formatter,
                "room {} {} source-effort row {source_index} is {actual_source_door_id:?}, expected {expected_source_door_id:?}",
                room_id.0,
                loadout.slug()
            ),
            Self::DoorEntry {
                room_id,
                loadout,
                source_door_id,
                source,
            } => write!(
                formatter,
                "cannot enter room {} through {source_door_id:?} under {}: {source}",
                room_id.0,
                loadout.slug()
            ),
            Self::MalformedPickupPositive {
                expected_pickup_id,
                target,
                reached,
            } => write!(
                formatter,
                "pickup positive for {expected_pickup_id:?} carries target {target:?} and reached {reached:?}"
            ),
            Self::MalformedDoorPositive {
                expected_door_id,
                target,
                reached,
            } => write!(
                formatter,
                "door positive for {expected_door_id:?} carries target {target:?} and reached {reached:?}"
            ),
            Self::ReplayDiverged { objective, source } => {
                write!(
                    formatter,
                    "exact replay for {objective:?} diverged: {source}"
                )
            }
            Self::UnexpectedDoorBeforeTarget {
                objective,
                actual_door_id,
            } => write!(
                formatter,
                "replay for {objective:?} reached unexpected door {actual_door_id:?} first"
            ),
            Self::TargetNotReached { objective } => {
                write!(formatter, "replay exhausted without reaching {objective:?}")
            }
            Self::DuplicateRouteNodeId { node_id } => {
                write!(
                    formatter,
                    "authored route plan has duplicate node ID {node_id}"
                )
            }
            Self::MissingRouteEdgeNode { node_id } => write!(
                formatter,
                "authored route edge refers to missing node ID {node_id}"
            ),
        }
    }
}

impl Error for PickupDetourAnalysisError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CorpusV2Identity { source } => Some(source.as_ref()),
            Self::DoorEntry { source, .. } => Some(source),
            Self::ReplayDiverged { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_ai::{Replay, ReplayFrame};
    use downwards_core::{AbilitySet, StateDigest};
    use downwards_validation::{BoundedInconclusiveEvidence, ValidationConfig};

    use super::*;
    use crate::corpus::{CorpusBuildConfigV1, evaluate_route_matrices_with, generate_seed_block};

    fn trace(cells: &[(u16, u16)], actions: &[(SemanticAction, usize)]) -> ObservedReplayPrefix {
        let traversal_cells = cells
            .iter()
            .map(|&(x, y)| TraversalCell { x, y })
            .collect::<Vec<_>>();
        let visited = traversal_cells
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let spans = traversal_cells
            .iter()
            .copied()
            .map(|cell| TraversalSpan { cell, samples: 1 })
            .collect::<Vec<_>>();
        let action_spans = actions
            .iter()
            .map(|&(action, ticks)| ActionSpan { action, ticks })
            .collect::<Vec<_>>();
        let ticks = action_spans.iter().map(|span| span.ticks).sum();
        ObservedReplayPrefix {
            completion_ticks: ticks,
            target_collection_tick: Some(ticks),
            traversal: TraversalTrace {
                grid: TraversalGrid::default(),
                sample_count: spans.len(),
                spans: spans.into_boxed_slice(),
                visited_cells: visited.into_boxed_slice(),
            },
            actions: SemanticActionTrace {
                total_ticks: ticks,
                spans: action_spans.into_boxed_slice(),
                events: Box::new([]),
                jump_presses: 0,
                dash_presses: 0,
                restart_presses: 0,
                successful_jumps: 0,
                successful_wall_jumps: 0,
                successful_dashes: 0,
                deaths: 0,
                pickups_collected: 1,
            },
            collection_events: Vec::new(),
            retained_pickups: BTreeSet::new(),
        }
    }

    #[test]
    fn on_path_and_real_detour_signatures_remain_distinct() {
        let right = SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        };
        let left = SemanticAction {
            move_x: -1,
            ..SemanticAction::default()
        };
        let pickup_on_path = trace(&[(0, 0), (1, 0), (2, 0)], &[(right, 12)]);
        let door_on_path = trace(&[(0, 0), (1, 0), (2, 0), (3, 0)], &[(right, 16)]);
        let on_path = route_difference(&pickup_on_path, &door_on_path);
        assert_eq!(on_path.spatial.pickup_only_visited_cells, 0);
        assert_eq!(on_path.action.semantic_span_edit_distance, 0);

        let pickup_detour = trace(
            &[(0, 0), (1, 0), (1, 1), (1, 2), (1, 1), (1, 0)],
            &[(right, 6), (left, 6)],
        );
        let detour = route_difference(&pickup_detour, &door_on_path);
        assert!(detour.spatial.pickup_only_visited_cells >= 2);
        assert!(
            detour.spatial.traversal_span_edit_distance
                > on_path.spatial.traversal_span_edit_distance
        );
        assert!(
            detour.action.semantic_span_edit_distance > on_path.action.semantic_span_edit_distance
        );
    }

    #[test]
    fn bounded_cell_keeps_reason_and_cost_separate() {
        let bounded = BoundedInconclusiveEvidence {
            reason: InconclusiveReason::ExpandedNodeBudget,
            search_effort: SearchStats {
                expanded_nodes: 17,
                generated_nodes: 23,
                simulated_ticks: 91,
                deepest_path_ticks: 7,
            },
        };
        let evidence = PickupCellEvidence::BoundedInconclusive {
            reason: bounded.reason,
        };
        let operational = PickupCellOperationalCost {
            shared_source_search: bounded.search_effort,
            pickup_target_snapshot: bounded.search_effort,
        };
        assert!(!evidence.is_positive());
        assert_eq!(
            evidence,
            PickupCellEvidence::BoundedInconclusive {
                reason: InconclusiveReason::ExpandedNodeBudget
            }
        );
        assert_eq!(operational.pickup_target_snapshot.expanded_nodes, 17);
    }

    #[test]
    fn target_identity_validation_rejects_wrong_kind_and_wrong_id() {
        let dummy = Replay {
            initial_digest: StateDigest(0),
            frames: Vec::<ReplayFrame>::new(),
        };
        let wrong_kind = TargetSolution {
            target: SearchTarget::door("coin"),
            reached: ReachedTarget::Door("coin".to_owned()),
            replay: dummy.clone(),
            stats: SearchStats::default(),
        };
        assert!(matches!(
            validate_pickup_solution(&wrong_kind, "coin"),
            Err(PickupDetourAnalysisError::MalformedPickupPositive { .. })
        ));
        let wrong_id = TargetSolution {
            target: SearchTarget::pickup("other"),
            reached: ReachedTarget::Pickup("other".to_owned()),
            replay: dummy,
            stats: SearchStats::default(),
        };
        assert!(matches!(
            validate_pickup_solution(&wrong_id, "coin"),
            Err(PickupDetourAnalysisError::MalformedPickupPositive { .. })
        ));
    }

    #[test]
    fn real_evaluated_room_is_exact_loadout_bound_and_deterministic() {
        let mut generated =
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        generated.rooms.truncate(1);
        let evaluated = evaluate_route_matrices_with(generated, |loadout| {
            let mut config = ValidationConfig::for_loadout(loadout.abilities());
            config.solver.max_expanded_nodes = 120_000;
            config
        })
        .unwrap();
        let room = &evaluated.rooms[0];
        let first = analyze_pickup_detours(room, TraversalGrid::default()).unwrap();
        let second = analyze_pickup_detours(room, TraversalGrid::default()).unwrap();
        assert_eq!(first, second);

        let door_count = room.generated.variants[0].generated.room.doors().len();
        let pickup_count = room.generated.variants[0].generated.room.pickups().len();
        assert_eq!(
            first.cells.len(),
            EvaluationLoadout::ALL.len() * door_count * pickup_count
        );
        for cell in &first.cells {
            if let PickupCellEvidence::Positive(positive) = &cell.evidence {
                assert_eq!(positive.ability.loadout, cell.loadout);
                assert_eq!(
                    positive.completion.target_collection_tick,
                    positive.completion.completion_ticks
                );
                assert!(
                    positive
                        .completion
                        .pickup_collection_events
                        .iter()
                        .any(|event| event.pickup_id == cell.pickup_id
                            && event.replay_tick == positive.completion.completion_ticks)
                );
            }
        }
        assert!(first.operational_cost.source_searches.iter().all(|source| {
            EvaluationLoadout::ALL.contains(&source.loadout)
                && source.source_index < door_count
                && room.generated.variants[0]
                    .generated
                    .room
                    .doors()
                    .iter()
                    .any(|door| door.id == source.source_door_id)
        }));

        // The exact replay digest makes relabelling a matrix cell as another
        // loadout invalid even when that loadout is a strict superset.
        let positive = room
            .matrices
            .iter()
            .flat_map(|matrix| matrix.evidence.pickup_routes())
            .find_map(|row| row.evidence.positive().map(|positive| (row, positive)))
            .unwrap();
        let declared_matrix = room
            .matrices
            .iter()
            .find(|matrix| {
                matrix
                    .evidence
                    .pickup_routes()
                    .iter()
                    .any(|row| std::ptr::eq(row, positive.0))
            })
            .unwrap();
        let other_loadout = EvaluationLoadout::ALL
            .into_iter()
            .find(|loadout| *loadout != declared_matrix.loadout)
            .unwrap();
        let initial = Simulation::enter_via_door(
            room.generated.variants[0].generated.room.clone(),
            other_loadout.abilities(),
            &positive.0.source_door_id,
        )
        .unwrap();
        assert!(positive.1.solution().replay.verify(&initial).is_err());
    }

    #[test]
    fn finite_relation_never_turns_bounded_context_into_negative_proof() {
        let positive = trace(&[(0, 0), (1, 0)], &[(SemanticAction::default(), 2)]);
        let door = downwards_validation::DoorRouteEvidence {
            source_door_id: "source".to_owned(),
            target_door_id: "target".to_owned(),
            evidence: BoundedTargetEvidence::Inconclusive(BoundedInconclusiveEvidence {
                reason: InconclusiveReason::PathHorizon,
                search_effort: SearchStats::default(),
            }),
        };
        let context = door_context(
            "coin",
            Some(&positive),
            &[ObservedDoorRow {
                row: &door,
                observed: None,
            }],
        );
        assert_eq!(
            context.relation,
            FiniteCanonicalPickupRelation::NoPositiveDoorWitnessContext {
                bounded_door_cells: 1
            }
        );
    }

    #[test]
    fn authored_shortest_spine_distinguishes_attached_pickup_branch() {
        use downwards_gen::experimental::{RouteEdge, RouteNode, SupportKind, SupportSpec};

        let support = |row| SupportSpec {
            start_x: 1,
            end_x: 3,
            row,
            kind: SupportKind::Solid,
        };
        let plan = RoutePlan {
            nodes: vec![
                RouteNode {
                    id: 10,
                    role: NodeRole::Port,
                    support: support(16),
                },
                RouteNode {
                    id: 20,
                    role: NodeRole::Junction,
                    support: support(12),
                },
                RouteNode {
                    id: 30,
                    role: NodeRole::Port,
                    support: support(8),
                },
                RouteNode {
                    id: 40,
                    role: NodeRole::Pickup,
                    support: support(14),
                },
            ],
            edges: vec![
                RouteEdge {
                    from: 10,
                    to: 20,
                    verb: RouteVerb::Jump,
                    critical: true,
                },
                RouteEdge {
                    from: 20,
                    to: 30,
                    verb: RouteVerb::Jump,
                    critical: true,
                },
                RouteEdge {
                    from: 20,
                    to: 40,
                    verb: RouteVerb::Drop,
                    critical: false,
                },
            ],
        };
        let graph = AuthoredGraph::new(&plan).unwrap();
        let spine = graph.shortest_port_spine();
        assert!(spine.contains(&graph.index_by_id[&20]));
        assert!(!spine.contains(&graph.index_by_id[&40]));
        assert_eq!(
            graph.distances_from_set(&spine)[graph.index_by_id[&40]],
            Some(1)
        );
    }

    #[test]
    fn edit_distance_is_deterministic_and_symmetric() {
        let left = [1, 2, 3, 4];
        let right = [1, 3, 5];
        assert_eq!(edit_distance(&left, &right), 2);
        assert_eq!(edit_distance(&left, &right), edit_distance(&right, &left));
    }

    #[test]
    fn exact_loadout_enum_still_maps_to_four_distinct_ability_sets() {
        let abilities = EvaluationLoadout::ALL
            .into_iter()
            .map(EvaluationLoadout::abilities)
            .collect::<HashSet<AbilitySet>>();
        assert_eq!(abilities.len(), 4);
    }
}
