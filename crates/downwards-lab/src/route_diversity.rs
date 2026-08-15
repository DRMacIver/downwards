use std::collections::BTreeSet;

use crate::{
    SemanticActionTrace, SuccessfulWitnessObservation, TraversalTrace, semantic_action_distance,
    traversal_distance,
};

/// Version of the route-set aggregation contract.
pub const ROUTE_DIVERSITY_VERSION: u32 = 1;

/// Distribution of normalized pairwise distances in `[0, 1]`.
///
/// `None` values mean fewer than two routes were supplied. Nearest-neighbour
/// distance is computed once per route and then averaged, so a large corpus
/// cannot hide a cluster of near-duplicates behind a few distant outliers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairwiseDistanceSummary {
    pub comparisons: usize,
    pub minimum: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub maximum: Option<f64>,
    pub mean_nearest_neighbor: Option<f64>,
}

/// Timing-insensitive route classes plus continuous behavior distances.
///
/// The class counts deliberately discard span durations and event ticks.
/// Two solutions do not become distinct play styles merely because the same
/// actions happened a few frames apart. The continuous traversal and semantic
/// distances still report pacing/duration differences for callers studying
/// timing variation.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteDiversityReport {
    pub version: u32,
    pub route_count: usize,
    pub reached_target_count: usize,
    pub spatial_path_classes: usize,
    pub semantic_controller_classes: usize,
    pub joint_play_style_classes: usize,
    pub traversal_distance: PairwiseDistanceSummary,
    pub semantic_action_distance: PairwiseDistanceSummary,
    /// Unweighted mean of traversal and semantic-action distance per pair.
    pub combined_behavior_distance: PairwiseDistanceSummary,
}

/// Aggregate a set of verified successful route observations.
///
/// This function does not claim that distinct behavior is fun or that a
/// small distance makes a route redundant. It supplies deterministic evidence
/// for a quality-diversity archive and later human calibration.
#[must_use]
pub fn route_diversity(observations: &[SuccessfulWitnessObservation]) -> RouteDiversityReport {
    let route_count = observations.len();
    let reached_target_count = observations
        .iter()
        .map(|observation| observation.reached_exit_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let spatial_path_classes = timing_insensitive_class_count(observations, |left, right| {
        same_spatial_path(&left.traversal, &right.traversal)
    });
    let semantic_controller_classes =
        timing_insensitive_class_count(observations, |left, right| {
            same_semantic_controller(&left.actions, &right.actions)
        });
    let joint_play_style_classes = timing_insensitive_class_count(observations, |left, right| {
        left.reached_exit_id == right.reached_exit_id
            && same_spatial_path(&left.traversal, &right.traversal)
            && same_semantic_controller(&left.actions, &right.actions)
    });

    let mut traversal = Vec::new();
    let mut semantic = Vec::new();
    let mut combined = Vec::new();
    for left in 0..route_count {
        for right in left + 1..route_count {
            let traversal_distance = traversal_distance(
                &observations[left].traversal,
                &observations[right].traversal,
            )
            .combined;
            let semantic_distance =
                semantic_action_distance(&observations[left].actions, &observations[right].actions)
                    .combined;
            traversal.push((left, right, traversal_distance));
            semantic.push((left, right, semantic_distance));
            combined.push((left, right, (traversal_distance + semantic_distance) / 2.0));
        }
    }

    RouteDiversityReport {
        version: ROUTE_DIVERSITY_VERSION,
        route_count,
        reached_target_count,
        spatial_path_classes,
        semantic_controller_classes,
        joint_play_style_classes,
        traversal_distance: summarize_distances(route_count, &traversal),
        semantic_action_distance: summarize_distances(route_count, &semantic),
        combined_behavior_distance: summarize_distances(route_count, &combined),
    }
}

fn timing_insensitive_class_count(
    observations: &[SuccessfulWitnessObservation],
    equivalent: impl Fn(&SuccessfulWitnessObservation, &SuccessfulWitnessObservation) -> bool,
) -> usize {
    let mut representatives = Vec::<usize>::new();
    for (index, observation) in observations.iter().enumerate() {
        if representatives
            .iter()
            .all(|&representative| !equivalent(observation, &observations[representative]))
        {
            representatives.push(index);
        }
    }
    representatives.len()
}

fn same_spatial_path(left: &TraversalTrace, right: &TraversalTrace) -> bool {
    left.grid == right.grid
        && left.visited_cells == right.visited_cells
        && left
            .spans
            .iter()
            .map(|span| span.cell)
            .eq(right.spans.iter().map(|span| span.cell))
}

fn same_semantic_controller(left: &SemanticActionTrace, right: &SemanticActionTrace) -> bool {
    left.spans
        .iter()
        .map(|span| span.action)
        .eq(right.spans.iter().map(|span| span.action))
        && left
            .events
            .iter()
            .map(|event| event.event)
            .eq(right.events.iter().map(|event| event.event))
}

fn summarize_distances(
    route_count: usize,
    indexed_distances: &[(usize, usize, f64)],
) -> PairwiseDistanceSummary {
    if indexed_distances.is_empty() {
        return PairwiseDistanceSummary {
            comparisons: 0,
            minimum: None,
            mean: None,
            median: None,
            maximum: None,
            mean_nearest_neighbor: None,
        };
    }

    let mut values = indexed_distances
        .iter()
        .map(|(_, _, distance)| *distance)
        .collect::<Vec<_>>();
    values.sort_unstable_by(f64::total_cmp);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let median = if values.len() % 2 == 0 {
        let upper = values.len() / 2;
        (values[upper - 1] + values[upper]) / 2.0
    } else {
        values[values.len() / 2]
    };
    let mut nearest = vec![f64::INFINITY; route_count];
    for &(left, right, distance) in indexed_distances {
        nearest[left] = nearest[left].min(distance);
        nearest[right] = nearest[right].min(distance);
    }
    debug_assert!(nearest.iter().all(|distance| distance.is_finite()));

    PairwiseDistanceSummary {
        comparisons: values.len(),
        minimum: values.first().copied(),
        mean: Some(mean),
        median: Some(median),
        maximum: values.last().copied(),
        mean_nearest_neighbor: Some(nearest.iter().sum::<f64>() / route_count as f64),
    }
}
