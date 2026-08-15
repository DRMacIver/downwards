use std::cmp::Ordering;

use crate::{
    CollisionTopologyDescriptor, PassableRegion, SemanticAction, SemanticActionTrace,
    StaticVisualDescriptor, TraversalCell, TraversalTrace,
};

const RESAMPLED_TRACE_POINTS: usize = 64;

/// Normalized static-preview distance. Every component and `combined` is in
/// `[0, 1]`; zero means exact descriptor equality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticVisualDistance {
    pub dimensions: f64,
    pub tiles: f64,
    pub spawn: f64,
    pub exits: f64,
    pub doors: f64,
    pub pickups: f64,
    pub timed_hazards: f64,
    /// Unweighted mean of the seven documented components.
    pub combined: f64,
}

#[must_use]
pub fn static_visual_distance(
    left: &StaticVisualDescriptor,
    right: &StaticVisualDescriptor,
) -> StaticVisualDistance {
    let dimensions = if left.version == right.version
        && left.width == right.width
        && left.height == right.height
        && left.tile_size == right.tile_size
    {
        0.0
    } else {
        1.0
    };
    let tiles = positional_mismatch(&left.tiles, &right.tiles);
    let width = (i64::from(left.width) * i64::from(left.tile_size))
        .abs()
        .max((i64::from(right.width) * i64::from(right.tile_size)).abs());
    let height = (i64::from(left.height) * i64::from(left.tile_size))
        .abs()
        .max((i64::from(right.height) * i64::from(right.tile_size)).abs());
    let spawn_denominator = (width + height).max(1) as f64;
    let spawn = ((i64::from(left.spawn.x).abs_diff(i64::from(right.spawn.x))
        + i64::from(left.spawn.y).abs_diff(i64::from(right.spawn.y))) as f64
        / spawn_denominator)
        .min(1.0);
    let exits = multiset_jaccard_distance(&left.exits, &right.exits);
    let doors = multiset_jaccard_distance(&left.doors, &right.doors);
    let pickups = multiset_jaccard_distance(&left.pickups, &right.pickups);
    let timed_hazards = multiset_jaccard_distance(&left.timed_hazards, &right.timed_hazards);
    let combined = (dimensions + tiles + spawn + exits + doors + pickups + timed_hazards) / 7.0;
    StaticVisualDistance {
        dimensions,
        tiles,
        spawn,
        exits,
        doors,
        pickups,
        timed_hazards,
        combined,
    }
}

/// Normalized collision comparison. The region component compares region
/// areas, extents, and touched boundaries while ignoring their canonical IDs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionTopologyDistance {
    pub dimensions: f64,
    pub collision_field: f64,
    pub exposed_faces: f64,
    pub passable_regions: f64,
    /// Unweighted mean of the four documented components.
    pub combined: f64,
}

#[must_use]
pub fn collision_topology_distance(
    left: &CollisionTopologyDescriptor,
    right: &CollisionTopologyDescriptor,
) -> CollisionTopologyDistance {
    let dimensions = if left.version == right.version
        && left.width == right.width
        && left.height == right.height
        && left.tile_size == right.tile_size
    {
        0.0
    } else {
        1.0
    };
    let collision_field = positional_mismatch(&left.cells, &right.cells);
    let exposed_faces = multiset_jaccard_distance(&left.faces, &right.faces);
    let left_regions = region_signatures(&left.regions);
    let right_regions = region_signatures(&right.regions);
    let passable_regions = multiset_jaccard_distance(&left_regions, &right_regions);
    let combined = (dimensions + collision_field + exposed_faces + passable_regions) / 4.0;
    CollisionTopologyDistance {
        dimensions,
        collision_field,
        exposed_faces,
        passable_regions,
        combined,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RegionSignature {
    area_cells: u32,
    width_cells: u16,
    height_cells: u16,
    boundaries: u8,
}

fn region_signatures(regions: &[PassableRegion]) -> Vec<RegionSignature> {
    let mut result = regions
        .iter()
        .map(|region| RegionSignature {
            area_cells: region.area_cells,
            width_cells: region.max_x - region.min_x + 1,
            height_cells: region.max_y - region.min_y + 1,
            boundaries: region.boundaries.0,
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

/// Comparison of both spatial coverage and the order in which space was used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalDistance {
    pub grid: f64,
    pub visited_cells: f64,
    pub ordered_path: f64,
    /// Unweighted mean of grid compatibility, visited-set Jaccard distance,
    /// and a 64-point time-normalized path distance.
    pub combined: f64,
}

#[must_use]
pub fn traversal_distance(left: &TraversalTrace, right: &TraversalTrace) -> TraversalDistance {
    let same_grid = left.grid == right.grid;
    let grid = if same_grid { 0.0 } else { 1.0 };
    let visited_cells = if same_grid {
        multiset_jaccard_distance(&left.visited_cells, &right.visited_cells)
    } else {
        1.0
    };
    let ordered_path = sampled_path_distance(left, right);
    let combined = (grid + visited_cells + ordered_path) / 3.0;
    TraversalDistance {
        grid,
        visited_cells,
        ordered_path,
        combined,
    }
}

fn sampled_path_distance(left: &TraversalTrace, right: &TraversalTrace) -> f64 {
    (0..RESAMPLED_TRACE_POINTS)
        .map(|index| {
            let left_cell = resampled_cell(left, index);
            let right_cell = resampled_cell(right, index);
            normalized_cell_distance(left_cell, left.grid, right_cell, right.grid)
        })
        .sum::<f64>()
        / RESAMPLED_TRACE_POINTS as f64
}

fn resampled_cell(trace: &TraversalTrace, index: usize) -> TraversalCell {
    let target = index * trace.sample_count.saturating_sub(1) / (RESAMPLED_TRACE_POINTS - 1);
    let mut elapsed = 0;
    for span in &trace.spans {
        elapsed += span.samples;
        if target < elapsed {
            return span.cell;
        }
    }
    trace
        .spans
        .last()
        .expect("a traversal trace always contains its initial position")
        .cell
}

fn normalized_cell_distance(
    left: TraversalCell,
    left_grid: crate::TraversalGrid,
    right: TraversalCell,
    right_grid: crate::TraversalGrid,
) -> f64 {
    let left_x = (f64::from(left.x) + 0.5) / f64::from(left_grid.columns());
    let left_y = (f64::from(left.y) + 0.5) / f64::from(left_grid.rows());
    let right_x = (f64::from(right.x) + 0.5) / f64::from(right_grid.columns());
    let right_y = (f64::from(right.y) + 0.5) / f64::from(right_grid.rows());
    ((left_x - right_x).abs() + (left_y - right_y).abs()) / 2.0
}

/// Input comparison separates time-aligned held input, transition vocabulary,
/// authoritative event order, and duration. This detects different solutions
/// even when the underlying room tiles are identical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SemanticActionDistance {
    pub sampled_inputs: f64,
    pub transitions: f64,
    pub events: f64,
    pub duration: f64,
    /// Unweighted mean of the four documented components.
    pub combined: f64,
}

#[must_use]
pub fn semantic_action_distance(
    left: &SemanticActionTrace,
    right: &SemanticActionTrace,
) -> SemanticActionDistance {
    let sampled_inputs = (0..RESAMPLED_TRACE_POINTS)
        .map(|index| {
            action_field_distance(
                resampled_action(left, index),
                resampled_action(right, index),
            )
        })
        .sum::<f64>()
        / RESAMPLED_TRACE_POINTS as f64;
    let left_transitions = left
        .spans
        .iter()
        .map(|span| span.action)
        .collect::<Vec<_>>();
    let right_transitions = right
        .spans
        .iter()
        .map(|span| span.action)
        .collect::<Vec<_>>();
    let transitions = normalized_edit_distance(&left_transitions, &right_transitions);
    let left_events = left
        .events
        .iter()
        .map(|event| event.event)
        .collect::<Vec<_>>();
    let right_events = right
        .events
        .iter()
        .map(|event| event.event)
        .collect::<Vec<_>>();
    let events = normalized_edit_distance(&left_events, &right_events);
    let duration = normalized_scalar_difference(left.total_ticks, right.total_ticks);
    let combined = (sampled_inputs + transitions + events + duration) / 4.0;
    SemanticActionDistance {
        sampled_inputs,
        transitions,
        events,
        duration,
        combined,
    }
}

fn resampled_action(trace: &SemanticActionTrace, index: usize) -> SemanticAction {
    if trace.total_ticks == 0 {
        return SemanticAction::default();
    }
    let target = index * trace.total_ticks.saturating_sub(1) / (RESAMPLED_TRACE_POINTS - 1);
    let mut elapsed = 0;
    for span in &trace.spans {
        elapsed += span.ticks;
        if target < elapsed {
            return span.action;
        }
    }
    trace
        .spans
        .last()
        .map_or(SemanticAction::default(), |span| span.action)
}

fn action_field_distance(left: SemanticAction, right: SemanticAction) -> f64 {
    let mismatches = usize::from(left.move_x != right.move_x)
        + usize::from(left.move_y != right.move_y)
        + usize::from(left.jump_held != right.jump_held)
        + usize::from(left.dash_held != right.dash_held)
        + usize::from(left.restart != right.restart);
    mismatches as f64 / 5.0
}

fn positional_mismatch<T: Eq>(left: &[T], right: &[T]) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 0.0;
    }
    let common = left.iter().zip(right).filter(|(a, b)| a == b).count();
    let denominator = left.len().max(right.len());
    (denominator - common) as f64 / denominator as f64
}

fn multiset_jaccard_distance<T: Ord>(left: &[T], right: &[T]) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 0.0;
    }
    let mut left_index = 0;
    let mut right_index = 0;
    let mut intersection = 0;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            Ordering::Less => left_index += 1,
            Ordering::Greater => right_index += 1,
            Ordering::Equal => {
                intersection += 1;
                left_index += 1;
                right_index += 1;
            }
        }
    }
    let union = left.len() + right.len() - intersection;
    1.0 - intersection as f64 / union as f64
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
            let substitution = previous[right_index] + usize::from(left_item != right_item);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            current[right_index + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()] as f64 / denominator as f64
}

fn normalized_scalar_difference(left: usize, right: usize) -> f64 {
    let denominator = left.max(right);
    if denominator == 0 {
        0.0
    } else {
        left.abs_diff(right) as f64 / denominator as f64
    }
}
