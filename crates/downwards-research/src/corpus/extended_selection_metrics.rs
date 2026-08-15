//! Selection-facing aggregates for route-choice and pickup-detour evidence.
//!
//! These metrics deliberately remain outside the scalar Pareto-quality vector.
//! They describe independent quality-diversity projections.  Counts of
//! positives are evidence coverage, not a claim that more pickups or more
//! controller witnesses make a room better.

use std::{error::Error, fmt};

use super::{
    CanonicalDoorPickupRoute, EvaluationLoadout, FiniteCanonicalPickupRelation,
    IntegerCoordinateDistribution, NamedSelectionCoordinate, PickupCellEvidence,
    PickupRoutePlanMapping, QuantizedSelectionEvidence, RoomId, RoomPickupDetourAnalysis,
    RoomRouteChoiceDiversity, RouteAlternativeDistanceReport, RouteChoiceAggregate,
    RouteChoiceCellEvidence,
};

/// Version of the route-choice selection aggregate and projection coordinates.
pub const ROUTE_CHOICE_SELECTION_METRICS_VERSION: u32 = 1;
/// Version of the pickup-detour selection aggregate and projection coordinates.
pub const PICKUP_DETOUR_SELECTION_METRICS_VERSION: u32 = 1;

/// Interpretation boundary for the route-choice projection.
pub const ROUTE_CHOICE_SELECTION_DISCLAIMER: &str = "route-choice coordinates compare materially distinct positive witnesses only within an exact source-door, target-door, and loadout cell; finite-vocabulary completion, bounded non-success, and missing assessments remain separate; observed alternatives are not an exhaustive route count, and solver work is excluded";

/// Interpretation boundary for the pickup-detour projection.
pub const PICKUP_DETOUR_SELECTION_DISCLAIMER: &str = "pickup coordinates describe exact retained positive witnesses, finite canonical-door comparisons, authored placement, and cross-loadout positive or bounded facts; target-directed-only is finite witness context rather than proof that collection is mandatory, bounded absence is not unreachability, the retained witness is not proven easiest, and neither pickup count nor solver work is a quality objective";

const ROUTE_CLASS_COUNT_CAP: usize = 16;
const PICKUP_COMPLETION_TICK_CAP: usize = 720;
const PICKUP_CONTROL_COUNT_CAP: usize = 32;
const PICKUP_PATH_STEP_CAP: usize = 96;
const PICKUP_ONLY_CELL_CAP: usize = 32;
const PICKUP_EDIT_DISTANCE_CAP: usize = 64;
const PICKUP_OFF_SPINE_DISTANCE_CAP: usize = 12;

/// Route-choice evidence transformed into stable room/loadout aggregates.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteChoiceSelectionMetricSummary {
    pub version: u32,
    pub source_route_choice_version: u32,
    pub room_id: RoomId,
    pub by_loadout: Vec<RouteChoiceSelectionLoadoutMetric>,
    pub aggregate: RouteChoiceSelectionAggregate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteChoiceSelectionLoadoutMetric {
    pub loadout: EvaluationLoadout,
    pub aggregate: RouteChoiceSelectionAggregate,
}

/// Transparent route-choice aggregate.  `source` retains every audit-state
/// count and all pooled within-cell distances from the source analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteChoiceSelectionAggregate {
    pub source: RouteChoiceAggregate,
    pub cells_with_spatially_distinct_alternatives: usize,
    pub cells_with_action_distinct_alternatives: usize,
    pub cells_with_event_distinct_alternatives: usize,
    pub cells_with_gate_style_distinct_alternatives: usize,
    pub alternative_count: Option<IntegerCoordinateDistribution>,
    pub spatial_path_classes: Option<IntegerCoordinateDistribution>,
    pub semantic_action_classes: Option<IntegerCoordinateDistribution>,
    pub accepted_event_classes: Option<IntegerCoordinateDistribution>,
    pub gate_path_style_classes: Option<IntegerCoordinateDistribution>,
}

/// Pickup evidence transformed into stable structural, per-loadout, room, and
/// cross-loadout aggregates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupDetourSelectionMetricSummary {
    pub version: u32,
    pub source_pickup_detour_version: u32,
    pub room_id: RoomId,
    pub structural: PickupStructuralSelectionAggregate,
    pub by_loadout: Vec<PickupDetourSelectionLoadoutMetric>,
    pub aggregate: PickupDetourSelectionAggregate,
    pub cross_loadout: PickupCrossLoadoutSelectionAggregate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickupDetourSelectionLoadoutMetric {
    pub loadout: EvaluationLoadout,
    pub aggregate: PickupDetourSelectionAggregate,
}

/// Authored pickup placement.  Unmatched and ambiguous mappings are retained
/// explicitly rather than silently treated as on-spine placement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupStructuralSelectionAggregate {
    pub expected_pickups: usize,
    pub unique_mappings: usize,
    pub unique_on_shortest_port_spine: usize,
    pub unique_off_shortest_port_spine: usize,
    pub authored_leaf_mappings: usize,
    pub no_matching_authored_node: usize,
    pub ambiguous_authored_node: usize,
    pub off_spine_distance: Option<IntegerCoordinateDistribution>,
}

/// One loadout or room-wide pickup aggregate.  Route differences are reduced
/// per pickup cell to the independently smallest observed difference on each
/// axis across retained positive door witnesses before being pooled.  This
/// prevents a deliberately circuitous door witness from manufacturing a large
/// detour coordinate and does not hide a scalar or lexicographic tradeoff.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupDetourSelectionAggregate {
    pub expected_cells: usize,
    pub positive_cells: usize,
    pub bounded_inconclusive_cells: usize,
    pub opportunistic_context_cells: usize,
    pub target_directed_only_context_cells: usize,
    pub no_positive_door_witness_context_cells: usize,
    pub contexts_with_bounded_door_cells: usize,
    pub positive_cells_with_door_comparison: usize,
    pub target_directed_positive_cells_with_door_comparison: usize,
    pub retained_positive_challenge: PickupChallengeCoordinateDistributions,
    pub minimum_known_detour: PickupDetourCoordinateDistributions,
    pub target_directed_minimum_known_detour: PickupDetourCoordinateDistributions,
}

/// Demand of the one replay-certified retained positive pickup witness per
/// cell.  These are observations, not an easiest-controller certificate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupChallengeCoordinateDistributions {
    pub completion_ticks: Option<IntegerCoordinateDistribution>,
    pub horizontal_reversals: Option<IntegerCoordinateDistribution>,
    pub vertical_decisions: Option<IntegerCoordinateDistribution>,
    pub semantic_transitions: Option<IntegerCoordinateDistribution>,
    pub coarse_path_steps: Option<IntegerCoordinateDistribution>,
}

/// Minimum same-cell difference from any retained positive canonical door
/// witness, reported separately in space and actions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupDetourCoordinateDistributions {
    pub pickup_only_visited_cells: Option<IntegerCoordinateDistribution>,
    pub traversal_span_edit_distance: Option<IntegerCoordinateDistribution>,
    pub semantic_span_edit_distance: Option<IntegerCoordinateDistribution>,
    pub absolute_duration_difference: Option<IntegerCoordinateDistribution>,
}

/// Cross-loadout positive and unresolved coverage.  No zero here establishes
/// an ability requirement: the bounded counterpart remains visible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickupCrossLoadoutSelectionAggregate {
    pub expected_source_pickup_cells: usize,
    pub cells_with_any_positive_loadout: usize,
    pub cells_with_any_bounded_loadout: usize,
    pub cells_positive_at_baseline: usize,
    pub cells_bounded_at_baseline: usize,
    pub cells_positive_without_wall_jump: usize,
    pub cells_with_bounded_non_wall_jump_loadout: usize,
    pub cells_positive_without_dash: usize,
    pub cells_with_bounded_non_dash_loadout: usize,
    pub cells_with_wall_jump_use_in_positive_witness: usize,
    pub cells_with_dash_use_in_positive_witness: usize,
}

/// Invalid linkage between selection-facing and source evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtendedSelectionMetricError {
    UnsupportedRouteChoiceVersion { actual: u32 },
    UnsupportedPickupDetourVersion { actual: u32 },
    DuplicateRouteChoiceLoadout { loadout: EvaluationLoadout },
    MissingRouteChoiceLoadout { loadout: EvaluationLoadout },
}

impl fmt::Display for ExtendedSelectionMetricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRouteChoiceVersion { actual } => write!(
                formatter,
                "route-choice selection metrics require source version {}, got {actual}",
                super::ROUTE_CHOICE_DIVERSITY_VERSION
            ),
            Self::UnsupportedPickupDetourVersion { actual } => write!(
                formatter,
                "pickup-detour selection metrics require source version {}, got {actual}",
                super::PICKUP_DETOUR_ANALYSIS_VERSION
            ),
            Self::DuplicateRouteChoiceLoadout { loadout } => write!(
                formatter,
                "route-choice report contains duplicate {} loadout summaries",
                loadout.slug()
            ),
            Self::MissingRouteChoiceLoadout { loadout } => write!(
                formatter,
                "route-choice report is missing its {} loadout summary",
                loadout.slug()
            ),
        }
    }
}

impl Error for ExtendedSelectionMetricError {}

/// Summarize observed route choices without comparing routes across endpoints
/// or loadouts.
pub fn summarize_route_choice_selection_metrics(
    report: &RoomRouteChoiceDiversity,
) -> Result<RouteChoiceSelectionMetricSummary, ExtendedSelectionMetricError> {
    if report.version != super::ROUTE_CHOICE_DIVERSITY_VERSION {
        return Err(
            ExtendedSelectionMetricError::UnsupportedRouteChoiceVersion {
                actual: report.version,
            },
        );
    }
    let mut seen = Vec::new();
    let mut by_loadout = Vec::with_capacity(EvaluationLoadout::ALL.len());
    for loadout in EvaluationLoadout::ALL {
        let matching = report
            .by_loadout
            .iter()
            .filter(|summary| summary.loadout == loadout)
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(ExtendedSelectionMetricError::DuplicateRouteChoiceLoadout { loadout });
        }
        let source = matching
            .first()
            .ok_or(ExtendedSelectionMetricError::MissingRouteChoiceLoadout { loadout })?
            .aggregate
            .clone();
        let cells = report
            .cells
            .iter()
            .filter(|cell| cell.loadout == loadout)
            .collect::<Vec<_>>();
        seen.extend(cells.iter().copied());
        by_loadout.push(RouteChoiceSelectionLoadoutMetric {
            loadout,
            aggregate: route_choice_aggregate(source, &cells),
        });
    }
    Ok(RouteChoiceSelectionMetricSummary {
        version: ROUTE_CHOICE_SELECTION_METRICS_VERSION,
        source_route_choice_version: report.version,
        room_id: report.room_id.clone(),
        by_loadout,
        aggregate: route_choice_aggregate(report.room.clone(), &seen),
    })
}

fn route_choice_aggregate(
    source: RouteChoiceAggregate,
    cells: &[&super::DirectedRouteChoiceCell],
) -> RouteChoiceSelectionAggregate {
    let positives = cells
        .iter()
        .filter_map(|cell| match &cell.evidence {
            RouteChoiceCellEvidence::Positive(set) => Some(set.as_ref()),
            RouteChoiceCellEvidence::NoPositiveInCompleteFiniteVocabulary
            | RouteChoiceCellEvidence::BoundedInconclusiveWithoutPositive { .. }
            | RouteChoiceCellEvidence::MissingDirectedRouteAssessment
            | RouteChoiceCellEvidence::MissingLoadoutAudit => None,
        })
        .collect::<Vec<_>>();
    RouteChoiceSelectionAggregate {
        cells_with_spatially_distinct_alternatives: positives
            .iter()
            .filter(|set| set.spatial_path_classes > 1)
            .count(),
        cells_with_action_distinct_alternatives: positives
            .iter()
            .filter(|set| set.semantic_action_classes > 1)
            .count(),
        cells_with_event_distinct_alternatives: positives
            .iter()
            .filter(|set| set.accepted_event_sequence_classes > 1)
            .count(),
        cells_with_gate_style_distinct_alternatives: positives
            .iter()
            .filter(|set| set.gate_path_style_classes > 1)
            .count(),
        alternative_count: integer_distribution(
            positives.iter().map(|set| set.positive_alternative_count),
        ),
        spatial_path_classes: integer_distribution(
            positives.iter().map(|set| set.spatial_path_classes),
        ),
        semantic_action_classes: integer_distribution(
            positives.iter().map(|set| set.semantic_action_classes),
        ),
        accepted_event_classes: integer_distribution(
            positives
                .iter()
                .map(|set| set.accepted_event_sequence_classes),
        ),
        gate_path_style_classes: integer_distribution(
            positives.iter().map(|set| set.gate_path_style_classes),
        ),
        source,
    }
}

/// Summarize pickup evidence without interpreting non-success as a negative
/// feasibility or ability claim.
pub fn summarize_pickup_detour_selection_metrics(
    report: &RoomPickupDetourAnalysis,
) -> Result<PickupDetourSelectionMetricSummary, ExtendedSelectionMetricError> {
    if report.version != super::PICKUP_DETOUR_ANALYSIS_VERSION {
        return Err(
            ExtendedSelectionMetricError::UnsupportedPickupDetourVersion {
                actual: report.version,
            },
        );
    }
    let by_loadout = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| PickupDetourSelectionLoadoutMetric {
            loadout,
            aggregate: pickup_aggregate(report.cells.iter().filter(|cell| cell.loadout == loadout)),
        })
        .collect();
    Ok(PickupDetourSelectionMetricSummary {
        version: PICKUP_DETOUR_SELECTION_METRICS_VERSION,
        source_pickup_detour_version: report.version,
        room_id: report.room_id.clone(),
        structural: pickup_structural_aggregate(report),
        by_loadout,
        aggregate: pickup_aggregate(report.cells.iter()),
        cross_loadout: pickup_cross_loadout_aggregate(report),
    })
}

fn pickup_structural_aggregate(
    report: &RoomPickupDetourAnalysis,
) -> PickupStructuralSelectionAggregate {
    let mut result = PickupStructuralSelectionAggregate {
        expected_pickups: report.structural_placements.len(),
        ..PickupStructuralSelectionAggregate::default()
    };
    let mut off_spine_distances = Vec::new();
    for placement in &report.structural_placements {
        match &placement.mapping {
            PickupRoutePlanMapping::Unique(mapping) => {
                result.unique_mappings += 1;
                result.authored_leaf_mappings += usize::from(mapping.authored_leaf);
                if mapping.on_shortest_port_spine {
                    result.unique_on_shortest_port_spine += 1;
                } else {
                    result.unique_off_shortest_port_spine += 1;
                    if let Some(distance) = mapping.distance_to_shortest_port_spine {
                        off_spine_distances.push(distance);
                    }
                }
            }
            PickupRoutePlanMapping::NoMatchingAuthoredPickupNode => {
                result.no_matching_authored_node += 1;
            }
            PickupRoutePlanMapping::AmbiguousMatchingAuthoredPickupNodes { .. } => {
                result.ambiguous_authored_node += 1;
            }
        }
    }
    result.off_spine_distance = integer_distribution(off_spine_distances);
    result
}

fn pickup_aggregate<'a>(
    cells: impl Iterator<Item = &'a super::PickupChallengeCell>,
) -> PickupDetourSelectionAggregate {
    let cells = cells.collect::<Vec<_>>();
    let mut result = PickupDetourSelectionAggregate {
        expected_cells: cells.len(),
        ..PickupDetourSelectionAggregate::default()
    };
    let mut completion_ticks = Vec::new();
    let mut horizontal_reversals = Vec::new();
    let mut vertical_decisions = Vec::new();
    let mut semantic_transitions = Vec::new();
    let mut coarse_path_steps = Vec::new();
    let mut detours = Vec::new();
    let mut target_directed_detours = Vec::new();

    for cell in cells {
        match &cell.evidence {
            PickupCellEvidence::Positive(positive) => {
                result.positive_cells += 1;
                completion_ticks.push(positive.completion.completion_ticks);
                horizontal_reversals.push(positive.control.horizontal_reversals);
                vertical_decisions.push(positive.control.vertical_decisions);
                semantic_transitions.push(positive.control.semantic_transitions);
                coarse_path_steps.push(positive.traversal.coarse_path_steps);
            }
            PickupCellEvidence::BoundedInconclusive { .. } => {
                result.bounded_inconclusive_cells += 1;
            }
        }

        let target_directed = match &cell.canonical_door_context.relation {
            FiniteCanonicalPickupRelation::ObservedOpportunistically {
                bounded_door_cells, ..
            } => {
                result.opportunistic_context_cells += 1;
                result.contexts_with_bounded_door_cells += usize::from(*bounded_door_cells > 0);
                false
            }
            FiniteCanonicalPickupRelation::TargetDirectedOnlyAmongObservedWitnesses {
                bounded_door_cells,
                ..
            } => {
                result.target_directed_only_context_cells += 1;
                result.contexts_with_bounded_door_cells += usize::from(*bounded_door_cells > 0);
                true
            }
            FiniteCanonicalPickupRelation::NoPositiveDoorWitnessContext { bounded_door_cells } => {
                result.no_positive_door_witness_context_cells += 1;
                result.contexts_with_bounded_door_cells += usize::from(*bounded_door_cells > 0);
                false
            }
        };

        if !matches!(cell.evidence, PickupCellEvidence::Positive(_)) {
            continue;
        }
        let minimum = minimum_cell_detour(&cell.canonical_door_context.routes);
        if let Some(minimum) = minimum {
            result.positive_cells_with_door_comparison += 1;
            if target_directed {
                result.target_directed_positive_cells_with_door_comparison += 1;
                target_directed_detours.push(minimum);
            }
            detours.push(minimum);
        }
    }

    result.retained_positive_challenge = PickupChallengeCoordinateDistributions {
        completion_ticks: integer_distribution(completion_ticks),
        horizontal_reversals: integer_distribution(horizontal_reversals),
        vertical_decisions: integer_distribution(vertical_decisions),
        semantic_transitions: integer_distribution(semantic_transitions),
        coarse_path_steps: integer_distribution(coarse_path_steps),
    };
    result.minimum_known_detour = pickup_detour_distributions(&detours);
    result.target_directed_minimum_known_detour =
        pickup_detour_distributions(&target_directed_detours);
    result
}

#[derive(Clone, Copy)]
struct CellDetourMinimum {
    pickup_only_visited_cells: usize,
    traversal_span_edit_distance: usize,
    semantic_span_edit_distance: usize,
    absolute_duration_difference: usize,
}

fn minimum_cell_detour(routes: &[CanonicalDoorPickupRoute]) -> Option<CellDetourMinimum> {
    routes
        .iter()
        .filter_map(|route| match route {
            CanonicalDoorPickupRoute::Positive {
                difference_from_pickup_witness: Some(difference),
                ..
            } => Some(CellDetourMinimum {
                pickup_only_visited_cells: difference.spatial.pickup_only_visited_cells,
                traversal_span_edit_distance: difference.spatial.traversal_span_edit_distance,
                semantic_span_edit_distance: difference.action.semantic_span_edit_distance,
                absolute_duration_difference: difference.action.absolute_duration_difference,
            }),
            CanonicalDoorPickupRoute::Positive {
                difference_from_pickup_witness: None,
                ..
            }
            | CanonicalDoorPickupRoute::BoundedInconclusive { .. } => None,
        })
        .reduce(|left, right| CellDetourMinimum {
            pickup_only_visited_cells: left
                .pickup_only_visited_cells
                .min(right.pickup_only_visited_cells),
            traversal_span_edit_distance: left
                .traversal_span_edit_distance
                .min(right.traversal_span_edit_distance),
            semantic_span_edit_distance: left
                .semantic_span_edit_distance
                .min(right.semantic_span_edit_distance),
            absolute_duration_difference: left
                .absolute_duration_difference
                .min(right.absolute_duration_difference),
        })
}

fn pickup_detour_distributions(
    detours: &[CellDetourMinimum],
) -> PickupDetourCoordinateDistributions {
    PickupDetourCoordinateDistributions {
        pickup_only_visited_cells: integer_distribution(
            detours
                .iter()
                .map(|difference| difference.pickup_only_visited_cells),
        ),
        traversal_span_edit_distance: integer_distribution(
            detours
                .iter()
                .map(|difference| difference.traversal_span_edit_distance),
        ),
        semantic_span_edit_distance: integer_distribution(
            detours
                .iter()
                .map(|difference| difference.semantic_span_edit_distance),
        ),
        absolute_duration_difference: integer_distribution(
            detours
                .iter()
                .map(|difference| difference.absolute_duration_difference),
        ),
    }
}

fn pickup_cross_loadout_aggregate(
    report: &RoomPickupDetourAnalysis,
) -> PickupCrossLoadoutSelectionAggregate {
    let mut result = PickupCrossLoadoutSelectionAggregate {
        expected_source_pickup_cells: report.cross_loadout.len(),
        ..PickupCrossLoadoutSelectionAggregate::default()
    };
    for cell in &report.cross_loadout {
        result.cells_with_any_positive_loadout += usize::from(!cell.positive_loadouts.is_empty());
        result.cells_with_any_bounded_loadout += usize::from(!cell.bounded_loadouts.is_empty());
        result.cells_positive_at_baseline += usize::from(
            cell.positive_loadouts
                .contains(&EvaluationLoadout::Baseline),
        );
        result.cells_bounded_at_baseline += usize::from(
            cell.bounded_loadouts
                .iter()
                .any(|bounded| bounded.loadout == EvaluationLoadout::Baseline),
        );
        result.cells_positive_without_wall_jump += usize::from(cell.positive_without_wall_jump);
        result.cells_with_bounded_non_wall_jump_loadout += usize::from(
            cell.bounded_loadouts
                .iter()
                .any(|bounded| !bounded.loadout.abilities().wall_jump),
        );
        result.cells_positive_without_dash += usize::from(cell.positive_without_dash);
        result.cells_with_bounded_non_dash_loadout += usize::from(
            cell.bounded_loadouts
                .iter()
                .any(|bounded| !bounded.loadout.abilities().dash),
        );
        result.cells_with_wall_jump_use_in_positive_witness +=
            usize::from(!cell.witnesses_using_wall_jump.is_empty());
        result.cells_with_dash_use_in_positive_witness +=
            usize::from(!cell.witnesses_using_dash.is_empty());
    }
    result
}

fn integer_distribution(
    values: impl IntoIterator<Item = usize>,
) -> Option<IntegerCoordinateDistribution> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let upper = values.len() / 2;
    let lower = (values.len() - 1) / 2;
    let minimum = values[0];
    let maximum = *values.last().expect("non-empty checked above");
    Some(IntegerCoordinateDistribution {
        sample_count: values.len(),
        minimum,
        median_lower: values[lower],
        median_upper: values[upper],
        maximum,
        spread: maximum.saturating_sub(minimum),
    })
}

/// Projection coordinates without an archive projection ID.  The selection
/// adapter supplies the independently versioned ID when wiring this draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedSelectionProjectionDraft {
    pub cell: Vec<NamedSelectionCoordinate>,
    pub detail: Vec<NamedSelectionCoordinate>,
}

pub(crate) fn route_choice_selection_projection(
    metrics: Option<&RouteChoiceSelectionMetricSummary>,
) -> ExtendedSelectionProjectionDraft {
    let mut detail = Vec::new();
    for loadout in EvaluationLoadout::ALL {
        let prefix = loadout.slug();
        match metrics.and_then(|metrics| {
            metrics
                .by_loadout
                .iter()
                .find(|summary| summary.loadout == loadout)
        }) {
            Some(summary) => {
                append_route_choice_projection(&mut detail, prefix, &summary.aggregate)
            }
            None => append_missing_route_choice_projection(&mut detail, prefix),
        }
    }
    match metrics {
        Some(metrics) => {
            append_route_choice_projection(&mut detail, "all-loadouts", &metrics.aggregate);
        }
        None => append_missing_route_choice_projection(&mut detail, "all-loadouts"),
    }
    let cell_names = [
        "both-observed-multiple-alternative-cell-fraction",
        "both-spatially-distinct-cell-fraction",
        "both-action-distinct-cell-fraction",
        "both-event-distinct-cell-fraction",
        "both-gate-style-distinct-cell-fraction",
        "both-bounded-inconclusive-without-positive-fraction",
        "all-loadouts-observed-multiple-alternative-cell-fraction",
        "all-loadouts-spatially-distinct-cell-fraction",
        "all-loadouts-action-distinct-cell-fraction",
        "all-loadouts-event-distinct-cell-fraction",
        "all-loadouts-gate-style-distinct-cell-fraction",
        "all-loadouts-median-nearest-spatial-distance",
        "all-loadouts-median-nearest-action-distance",
        "all-loadouts-median-nearest-event-distance",
        "all-loadouts-median-nearest-gate-style-distance",
    ];
    ExtendedSelectionProjectionDraft {
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn append_route_choice_projection(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    aggregate: &RouteChoiceSelectionAggregate,
) {
    let source = &aggregate.source;
    detail.extend([
        named_fraction(
            format!("{prefix}-known-positive-cell-fraction"),
            source.positive_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-positive-complete-audit-fraction"),
            source.positive_complete_audit_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-positive-bounded-audit-fraction"),
            source.positive_bounded_audit_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-positive-missing-audit-fraction"),
            source.positive_missing_route_or_audit_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-complete-finite-vocabulary-without-positive-fraction"),
            source.complete_without_positive_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-bounded-inconclusive-without-positive-fraction"),
            source.bounded_inconclusive_without_positive_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-missing-route-assessment-fraction"),
            source.missing_route_cells,
            source.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-missing-loadout-audit-fraction"),
            source.missing_audit_cells,
            source.expected_cells,
        ),
        named_positive_fraction(
            format!("{prefix}-observed-multiple-alternative-cell-fraction"),
            source.cells_with_multiple_alternatives,
            aggregate,
        ),
        named_positive_fraction(
            format!("{prefix}-spatially-distinct-cell-fraction"),
            aggregate.cells_with_spatially_distinct_alternatives,
            aggregate,
        ),
        named_positive_fraction(
            format!("{prefix}-action-distinct-cell-fraction"),
            aggregate.cells_with_action_distinct_alternatives,
            aggregate,
        ),
        named_positive_fraction(
            format!("{prefix}-event-distinct-cell-fraction"),
            aggregate.cells_with_event_distinct_alternatives,
            aggregate,
        ),
        named_positive_fraction(
            format!("{prefix}-gate-style-distinct-cell-fraction"),
            aggregate.cells_with_gate_style_distinct_alternatives,
            aggregate,
        ),
    ]);
    for (name, distribution) in [
        ("alternative-count", aggregate.alternative_count),
        ("spatial-path-classes", aggregate.spatial_path_classes),
        ("semantic-action-classes", aggregate.semantic_action_classes),
        ("accepted-event-classes", aggregate.accepted_event_classes),
        ("gate-path-style-classes", aggregate.gate_path_style_classes),
    ] {
        detail.push(named_distribution(
            format!("{prefix}-median-{name}"),
            distribution,
            ROUTE_CLASS_COUNT_CAP,
            route_choice_empty_state(aggregate),
        ));
    }
    append_route_distances(detail, prefix, aggregate);
}

fn append_route_distances(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    aggregate: &RouteChoiceSelectionAggregate,
) {
    let RouteAlternativeDistanceReport {
        spatial_trajectory,
        semantic_actions,
        accepted_event_sequence,
        gate_path_style,
    } = &aggregate.source.distances;
    for (name, value) in [
        (
            "spatial-distance",
            spatial_trajectory.nearest_neighbor.median,
        ),
        ("action-distance", semantic_actions.nearest_neighbor.median),
        (
            "event-distance",
            accepted_event_sequence.nearest_neighbor.median,
        ),
        (
            "gate-style-distance",
            gate_path_style.nearest_neighbor.median,
        ),
    ] {
        detail.push(NamedSelectionCoordinate {
            name: format!("{prefix}-median-nearest-{name}"),
            evidence: value.map_or_else(
                || selection_state_evidence(route_choice_empty_state(aggregate)),
                quantized_unit_interval,
            ),
        });
    }
}

fn append_missing_route_choice_projection(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
) {
    for suffix in route_choice_coordinate_suffixes() {
        detail.push(named_missing(format!("{prefix}-{suffix}")));
    }
}

fn route_choice_coordinate_suffixes() -> [&'static str; 22] {
    [
        "known-positive-cell-fraction",
        "positive-complete-audit-fraction",
        "positive-bounded-audit-fraction",
        "positive-missing-audit-fraction",
        "complete-finite-vocabulary-without-positive-fraction",
        "bounded-inconclusive-without-positive-fraction",
        "missing-route-assessment-fraction",
        "missing-loadout-audit-fraction",
        "observed-multiple-alternative-cell-fraction",
        "spatially-distinct-cell-fraction",
        "action-distinct-cell-fraction",
        "event-distinct-cell-fraction",
        "gate-style-distinct-cell-fraction",
        "median-alternative-count",
        "median-spatial-path-classes",
        "median-semantic-action-classes",
        "median-accepted-event-classes",
        "median-gate-path-style-classes",
        "median-nearest-spatial-distance",
        "median-nearest-action-distance",
        "median-nearest-event-distance",
        "median-nearest-gate-style-distance",
    ]
}

#[derive(Clone, Copy)]
enum EmptySelectionState {
    Missing,
    NotApplicable,
    BoundedInconclusive,
}

fn route_choice_empty_state(aggregate: &RouteChoiceSelectionAggregate) -> EmptySelectionState {
    if aggregate.source.bounded_inconclusive_without_positive_cells > 0 {
        EmptySelectionState::BoundedInconclusive
    } else if aggregate.source.missing_route_cells > 0 || aggregate.source.missing_audit_cells > 0 {
        EmptySelectionState::Missing
    } else {
        EmptySelectionState::NotApplicable
    }
}

fn named_positive_fraction(
    name: impl Into<String>,
    numerator: usize,
    aggregate: &RouteChoiceSelectionAggregate,
) -> NamedSelectionCoordinate {
    if aggregate.source.positive_cells == 0 {
        NamedSelectionCoordinate {
            name: name.into(),
            evidence: selection_state_evidence(route_choice_empty_state(aggregate)),
        }
    } else {
        named_fraction(name, numerator, aggregate.source.positive_cells)
    }
}

pub(crate) fn pickup_detour_selection_projection(
    metrics: Option<&PickupDetourSelectionMetricSummary>,
) -> ExtendedSelectionProjectionDraft {
    let mut detail = Vec::new();
    match metrics {
        Some(metrics) => append_pickup_structural(&mut detail, &metrics.structural),
        None => append_missing_pickup_structural(&mut detail),
    }
    for loadout in EvaluationLoadout::ALL {
        let prefix = loadout.slug();
        match metrics.and_then(|metrics| {
            metrics
                .by_loadout
                .iter()
                .find(|summary| summary.loadout == loadout)
        }) {
            Some(summary) => append_pickup_aggregate(&mut detail, prefix, &summary.aggregate),
            None => append_missing_pickup_aggregate(&mut detail, prefix),
        }
    }
    match metrics {
        Some(metrics) => {
            append_pickup_aggregate(&mut detail, "all-loadouts", &metrics.aggregate);
            append_pickup_cross_loadout(&mut detail, &metrics.cross_loadout);
        }
        None => {
            append_missing_pickup_aggregate(&mut detail, "all-loadouts");
            append_missing_pickup_cross_loadout(&mut detail);
        }
    }
    let cell_names = [
        "authored-unique-off-shortest-port-spine-fraction",
        "authored-median-off-spine-distance",
        "both-opportunistic-context-fraction",
        "both-target-directed-only-finite-context-fraction",
        "both-median-retained-positive-completion-ticks",
        "both-median-retained-positive-vertical-decisions",
        "both-median-minimum-known-pickup-only-cells",
        "both-median-minimum-known-semantic-action-edit-distance",
        "all-loadouts-median-minimum-known-pickup-only-cells",
        "all-loadouts-median-target-directed-minimum-known-pickup-only-cells",
        "known-positive-baseline-source-pickup-fraction",
        "bounded-baseline-source-pickup-fraction",
        "known-positive-without-wall-jump-fraction",
        "bounded-non-wall-jump-fraction",
        "known-positive-without-dash-fraction",
        "bounded-non-dash-fraction",
    ];
    ExtendedSelectionProjectionDraft {
        cell: select_named(&detail, &cell_names),
        detail,
    }
}

fn append_pickup_structural(
    detail: &mut Vec<NamedSelectionCoordinate>,
    aggregate: &PickupStructuralSelectionAggregate,
) {
    detail.extend([
        named_fraction(
            "authored-unique-mapping-fraction",
            aggregate.unique_mappings,
            aggregate.expected_pickups,
        ),
        named_fraction(
            "authored-unique-on-shortest-port-spine-fraction",
            aggregate.unique_on_shortest_port_spine,
            aggregate.expected_pickups,
        ),
        named_fraction(
            "authored-unique-off-shortest-port-spine-fraction",
            aggregate.unique_off_shortest_port_spine,
            aggregate.expected_pickups,
        ),
        named_fraction(
            "authored-leaf-mapping-fraction",
            aggregate.authored_leaf_mappings,
            aggregate.expected_pickups,
        ),
        named_fraction(
            "authored-no-matching-node-fraction",
            aggregate.no_matching_authored_node,
            aggregate.expected_pickups,
        ),
        named_fraction(
            "authored-ambiguous-node-fraction",
            aggregate.ambiguous_authored_node,
            aggregate.expected_pickups,
        ),
        named_distribution(
            "authored-median-off-spine-distance",
            aggregate.off_spine_distance,
            PICKUP_OFF_SPINE_DISTANCE_CAP,
            EmptySelectionState::NotApplicable,
        ),
    ]);
}

fn append_missing_pickup_structural(detail: &mut Vec<NamedSelectionCoordinate>) {
    for name in [
        "authored-unique-mapping-fraction",
        "authored-unique-on-shortest-port-spine-fraction",
        "authored-unique-off-shortest-port-spine-fraction",
        "authored-leaf-mapping-fraction",
        "authored-no-matching-node-fraction",
        "authored-ambiguous-node-fraction",
        "authored-median-off-spine-distance",
    ] {
        detail.push(named_missing(name));
    }
}

fn append_pickup_aggregate(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    aggregate: &PickupDetourSelectionAggregate,
) {
    detail.extend([
        named_fraction(
            format!("{prefix}-known-positive-pickup-cell-fraction"),
            aggregate.positive_cells,
            aggregate.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-bounded-inconclusive-pickup-cell-fraction"),
            aggregate.bounded_inconclusive_cells,
            aggregate.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-opportunistic-context-fraction"),
            aggregate.opportunistic_context_cells,
            aggregate.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-target-directed-only-finite-context-fraction"),
            aggregate.target_directed_only_context_cells,
            aggregate.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-no-positive-door-witness-context-fraction"),
            aggregate.no_positive_door_witness_context_cells,
            aggregate.expected_cells,
        ),
        named_fraction(
            format!("{prefix}-context-with-bounded-door-cell-fraction"),
            aggregate.contexts_with_bounded_door_cells,
            aggregate.expected_cells,
        ),
        named_positive_pickup_fraction(
            format!("{prefix}-positive-cell-with-door-comparison-fraction"),
            aggregate.positive_cells_with_door_comparison,
            aggregate,
        ),
        named_positive_pickup_fraction(
            format!("{prefix}-target-directed-positive-cell-with-door-comparison-fraction"),
            aggregate.target_directed_positive_cells_with_door_comparison,
            aggregate,
        ),
    ]);
    append_pickup_challenge(detail, prefix, aggregate);
    append_pickup_detour_coordinates(
        detail,
        prefix,
        "minimum-known",
        &aggregate.minimum_known_detour,
        pickup_empty_state(aggregate),
    );
    append_pickup_detour_coordinates(
        detail,
        prefix,
        "target-directed-minimum-known",
        &aggregate.target_directed_minimum_known_detour,
        pickup_empty_state(aggregate),
    );
}

fn append_pickup_challenge(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    aggregate: &PickupDetourSelectionAggregate,
) {
    let challenge = &aggregate.retained_positive_challenge;
    let empty = pickup_empty_state(aggregate);
    for (name, distribution, cap) in [
        (
            "completion-ticks",
            challenge.completion_ticks,
            PICKUP_COMPLETION_TICK_CAP,
        ),
        (
            "horizontal-reversals",
            challenge.horizontal_reversals,
            PICKUP_CONTROL_COUNT_CAP,
        ),
        (
            "vertical-decisions",
            challenge.vertical_decisions,
            PICKUP_CONTROL_COUNT_CAP,
        ),
        (
            "semantic-transitions",
            challenge.semantic_transitions,
            PICKUP_CONTROL_COUNT_CAP,
        ),
        (
            "coarse-path-steps",
            challenge.coarse_path_steps,
            PICKUP_PATH_STEP_CAP,
        ),
    ] {
        detail.push(named_distribution(
            format!("{prefix}-median-retained-positive-{name}"),
            distribution,
            cap,
            empty,
        ));
    }
}

fn append_pickup_detour_coordinates(
    detail: &mut Vec<NamedSelectionCoordinate>,
    prefix: &str,
    kind: &str,
    distributions: &PickupDetourCoordinateDistributions,
    empty: EmptySelectionState,
) {
    for (name, distribution, cap) in [
        (
            "pickup-only-cells",
            distributions.pickup_only_visited_cells,
            PICKUP_ONLY_CELL_CAP,
        ),
        (
            "traversal-edit-distance",
            distributions.traversal_span_edit_distance,
            PICKUP_EDIT_DISTANCE_CAP,
        ),
        (
            "semantic-action-edit-distance",
            distributions.semantic_span_edit_distance,
            PICKUP_EDIT_DISTANCE_CAP,
        ),
        (
            "duration-difference",
            distributions.absolute_duration_difference,
            PICKUP_COMPLETION_TICK_CAP,
        ),
    ] {
        detail.push(named_distribution(
            format!("{prefix}-median-{kind}-{name}"),
            distribution,
            cap,
            empty,
        ));
    }
}

fn append_missing_pickup_aggregate(detail: &mut Vec<NamedSelectionCoordinate>, prefix: &str) {
    for suffix in pickup_coordinate_suffixes() {
        detail.push(named_missing(format!("{prefix}-{suffix}")));
    }
}

fn pickup_coordinate_suffixes() -> [&'static str; 21] {
    [
        "known-positive-pickup-cell-fraction",
        "bounded-inconclusive-pickup-cell-fraction",
        "opportunistic-context-fraction",
        "target-directed-only-finite-context-fraction",
        "no-positive-door-witness-context-fraction",
        "context-with-bounded-door-cell-fraction",
        "positive-cell-with-door-comparison-fraction",
        "target-directed-positive-cell-with-door-comparison-fraction",
        "median-retained-positive-completion-ticks",
        "median-retained-positive-horizontal-reversals",
        "median-retained-positive-vertical-decisions",
        "median-retained-positive-semantic-transitions",
        "median-retained-positive-coarse-path-steps",
        "median-minimum-known-pickup-only-cells",
        "median-minimum-known-traversal-edit-distance",
        "median-minimum-known-semantic-action-edit-distance",
        "median-minimum-known-duration-difference",
        "median-target-directed-minimum-known-pickup-only-cells",
        "median-target-directed-minimum-known-traversal-edit-distance",
        "median-target-directed-minimum-known-semantic-action-edit-distance",
        "median-target-directed-minimum-known-duration-difference",
    ]
}

fn pickup_empty_state(aggregate: &PickupDetourSelectionAggregate) -> EmptySelectionState {
    if aggregate.positive_cells > 0 {
        EmptySelectionState::NotApplicable
    } else if aggregate.bounded_inconclusive_cells > 0 {
        EmptySelectionState::BoundedInconclusive
    } else {
        EmptySelectionState::NotApplicable
    }
}

fn named_positive_pickup_fraction(
    name: impl Into<String>,
    numerator: usize,
    aggregate: &PickupDetourSelectionAggregate,
) -> NamedSelectionCoordinate {
    if aggregate.positive_cells == 0 {
        NamedSelectionCoordinate {
            name: name.into(),
            evidence: selection_state_evidence(pickup_empty_state(aggregate)),
        }
    } else {
        named_fraction(name, numerator, aggregate.positive_cells)
    }
}

fn append_pickup_cross_loadout(
    detail: &mut Vec<NamedSelectionCoordinate>,
    aggregate: &PickupCrossLoadoutSelectionAggregate,
) {
    let denominator = aggregate.expected_source_pickup_cells;
    for (name, numerator) in [
        (
            "known-positive-any-loadout-fraction",
            aggregate.cells_with_any_positive_loadout,
        ),
        (
            "bounded-any-loadout-fraction",
            aggregate.cells_with_any_bounded_loadout,
        ),
        (
            "known-positive-baseline-source-pickup-fraction",
            aggregate.cells_positive_at_baseline,
        ),
        (
            "bounded-baseline-source-pickup-fraction",
            aggregate.cells_bounded_at_baseline,
        ),
        (
            "known-positive-without-wall-jump-fraction",
            aggregate.cells_positive_without_wall_jump,
        ),
        (
            "bounded-non-wall-jump-fraction",
            aggregate.cells_with_bounded_non_wall_jump_loadout,
        ),
        (
            "known-positive-without-dash-fraction",
            aggregate.cells_positive_without_dash,
        ),
        (
            "bounded-non-dash-fraction",
            aggregate.cells_with_bounded_non_dash_loadout,
        ),
        (
            "positive-witness-used-wall-jump-fraction",
            aggregate.cells_with_wall_jump_use_in_positive_witness,
        ),
        (
            "positive-witness-used-dash-fraction",
            aggregate.cells_with_dash_use_in_positive_witness,
        ),
    ] {
        detail.push(named_fraction(name, numerator, denominator));
    }
}

fn append_missing_pickup_cross_loadout(detail: &mut Vec<NamedSelectionCoordinate>) {
    for name in [
        "known-positive-any-loadout-fraction",
        "bounded-any-loadout-fraction",
        "known-positive-baseline-source-pickup-fraction",
        "bounded-baseline-source-pickup-fraction",
        "known-positive-without-wall-jump-fraction",
        "bounded-non-wall-jump-fraction",
        "known-positive-without-dash-fraction",
        "bounded-non-dash-fraction",
        "positive-witness-used-wall-jump-fraction",
        "positive-witness-used-dash-fraction",
    ] {
        detail.push(named_missing(name));
    }
}

fn named_fraction(
    name: impl Into<String>,
    numerator: usize,
    denominator: usize,
) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate {
        name: name.into(),
        evidence: if denominator == 0 {
            QuantizedSelectionEvidence::not_applicable()
        } else {
            QuantizedSelectionEvidence::observed(quantized_ratio(numerator, denominator))
        },
    }
}

fn named_distribution(
    name: impl Into<String>,
    distribution: Option<IntegerCoordinateDistribution>,
    cap: usize,
    empty: EmptySelectionState,
) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate {
        name: name.into(),
        evidence: distribution.map_or_else(
            || selection_state_evidence(empty),
            |distribution| {
                let median_sum = distribution
                    .median_lower
                    .saturating_add(distribution.median_upper);
                QuantizedSelectionEvidence::observed(quantized_capped(
                    median_sum,
                    cap.saturating_mul(2),
                ))
            },
        ),
    }
}

fn named_missing(name: impl Into<String>) -> NamedSelectionCoordinate {
    NamedSelectionCoordinate {
        name: name.into(),
        evidence: QuantizedSelectionEvidence::missing(),
    }
}

fn selection_state_evidence(state: EmptySelectionState) -> QuantizedSelectionEvidence {
    match state {
        EmptySelectionState::Missing => QuantizedSelectionEvidence::missing(),
        EmptySelectionState::NotApplicable => QuantizedSelectionEvidence::not_applicable(),
        EmptySelectionState::BoundedInconclusive => {
            QuantizedSelectionEvidence::bounded_inconclusive()
        }
    }
}

fn quantized_unit_interval(value: f64) -> QuantizedSelectionEvidence {
    let clamped = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    QuantizedSelectionEvidence::observed((clamped * f64::from(u16::MAX)).round() as u16)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_extended_evidence_is_not_numeric_zero() {
        let route = route_choice_selection_projection(None);
        let pickup = pickup_detour_selection_projection(None);
        assert_eq!(route.cell.len(), 15);
        assert_eq!(pickup.cell.len(), 16);
        assert!(
            route
                .detail
                .iter()
                .chain(&pickup.detail)
                .all(|coordinate| coordinate.evidence == QuantizedSelectionEvidence::missing())
        );
    }

    #[test]
    fn integer_distributions_retain_even_median_endpoints() {
        assert_eq!(integer_distribution([]), None);
        assert_eq!(
            integer_distribution([1, 3, 9, 20]),
            Some(IntegerCoordinateDistribution {
                sample_count: 4,
                minimum: 1,
                median_lower: 3,
                median_upper: 9,
                maximum: 20,
                spread: 19,
            })
        );
    }

    #[test]
    fn minimum_known_detour_uses_best_aligned_door_witness() {
        use super::super::{
            PickupDoorActionDifference, PickupDoorRouteDifference, PickupDoorSpatialDifference,
        };

        let route = |target: &str, pickup_only, traversal_edit, action_edit, duration| {
            CanonicalDoorPickupRoute::Positive {
                target_door_id: target.to_owned(),
                pickup_retained_at_door: false,
                collection_events: Vec::new(),
                difference_from_pickup_witness: Some(PickupDoorRouteDifference {
                    spatial: PickupDoorSpatialDifference {
                        shared_visited_cells: 1,
                        pickup_only_visited_cells: pickup_only,
                        door_only_visited_cells: 0,
                        union_visited_cells: pickup_only + 1,
                        traversal_span_edit_distance: traversal_edit,
                    },
                    action: PickupDoorActionDifference {
                        shared_prefix_ticks: 0,
                        pickup_duration_ticks: duration,
                        door_duration_ticks: 0,
                        absolute_duration_difference: duration,
                        semantic_span_edit_distance: action_edit,
                    },
                }),
            }
        };
        let minimum = minimum_cell_detour(&[
            route("circuitous", 9, 2, 1, 5),
            route("best-aligned", 1, 7, 6, 20),
        ])
        .unwrap();
        assert_eq!(minimum.pickup_only_visited_cells, 1);
        assert_eq!(minimum.traversal_span_edit_distance, 2);
        assert_eq!(minimum.semantic_span_edit_distance, 1);
        assert_eq!(minimum.absolute_duration_difference, 5);
    }

    #[test]
    fn route_empty_state_distinguishes_bounded_missing_and_complete_finite() {
        let aggregate = |bounded: bool, missing: bool| RouteChoiceSelectionAggregate {
            source: RouteChoiceAggregate {
                expected_cells: 1,
                complete_without_positive_cells: usize::from(!bounded && !missing),
                bounded_inconclusive_without_positive_cells: usize::from(bounded),
                missing_route_cells: usize::from(missing),
                ..RouteChoiceAggregate::default()
            },
            cells_with_spatially_distinct_alternatives: 0,
            cells_with_action_distinct_alternatives: 0,
            cells_with_event_distinct_alternatives: 0,
            cells_with_gate_style_distinct_alternatives: 0,
            alternative_count: None,
            spatial_path_classes: None,
            semantic_action_classes: None,
            accepted_event_classes: None,
            gate_path_style_classes: None,
        };
        let mut detail = Vec::new();
        append_route_choice_projection(&mut detail, "bounded", &aggregate(true, false));
        append_route_choice_projection(&mut detail, "missing", &aggregate(false, true));
        append_route_choice_projection(&mut detail, "complete", &aggregate(false, false));
        let evidence = |name: &str| {
            detail
                .iter()
                .find(|coordinate| coordinate.name == name)
                .unwrap()
                .evidence
                .clone()
        };
        assert_eq!(
            evidence("bounded-median-alternative-count"),
            QuantizedSelectionEvidence::bounded_inconclusive()
        );
        assert_eq!(
            evidence("missing-median-alternative-count"),
            QuantizedSelectionEvidence::missing()
        );
        assert_eq!(
            evidence("complete-median-alternative-count"),
            QuantizedSelectionEvidence::not_applicable()
        );
    }
}
