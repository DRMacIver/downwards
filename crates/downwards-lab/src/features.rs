use downwards_core::{AbilitySet, Simulation};

use crate::{
    CollisionTopologyDescriptor, SemanticAction, StaticVisualDescriptor,
    SuccessfulWitnessObservation, TraversalCell, VisualTile,
};

pub const OBSERVATION_FEATURE_VERSION: u32 = 1;
pub const OBSERVATION_FEATURE_COUNT: usize = 29;

/// Stable coordinate names for [`ObservationFeatureVector`]. All coordinates
/// are observations in `[0, 1]`, not estimates of human difficulty or quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum ObservationFeature {
    SolidCellFraction = 0,
    OneWayCellFraction = 1,
    StaticHazardCellFraction = 2,
    TimedHazardAreaFraction = 3,
    ExitCount = 4,
    PickupCount = 5,
    SpawnX = 6,
    SpawnY = 7,
    PassableRegionCount = 8,
    CollisionFaceDensity = 9,
    TraversalCoverage = 10,
    HorizontalSpan = 11,
    VerticalSpan = 12,
    HorizontalTravel = 13,
    VerticalTravel = 14,
    PathDirectness = 15,
    CompletionDuration = 16,
    InputTransitionRate = 17,
    ActiveInputFraction = 18,
    JumpPressRate = 19,
    SuccessfulJumpRate = 20,
    WallJumpShare = 21,
    DashPressRate = 22,
    SuccessfulDashRate = 23,
    HorizontalReversalRate = 24,
    DeathCount = 25,
    PickupCollectionCount = 26,
    WallJumpAbility = 27,
    DashAbility = 28,
}

impl ObservationFeature {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SolidCellFraction => "solid_cell_fraction",
            Self::OneWayCellFraction => "one_way_cell_fraction",
            Self::StaticHazardCellFraction => "static_hazard_cell_fraction",
            Self::TimedHazardAreaFraction => "timed_hazard_area_fraction",
            Self::ExitCount => "exit_count_saturated_at_8",
            Self::PickupCount => "pickup_count_saturated_at_8",
            Self::SpawnX => "spawn_x",
            Self::SpawnY => "spawn_y",
            Self::PassableRegionCount => "passable_region_count_saturated_at_8",
            Self::CollisionFaceDensity => "collision_face_density",
            Self::TraversalCoverage => "traversal_coverage",
            Self::HorizontalSpan => "horizontal_span",
            Self::VerticalSpan => "vertical_span",
            Self::HorizontalTravel => "horizontal_travel_saturated_at_4_screens",
            Self::VerticalTravel => "vertical_travel_saturated_at_4_screens",
            Self::PathDirectness => "path_directness",
            Self::CompletionDuration => "completion_duration_saturated_at_1200_ticks",
            Self::InputTransitionRate => "input_transition_rate_saturated_at_1_per_10_ticks",
            Self::ActiveInputFraction => "active_input_fraction",
            Self::JumpPressRate => "jump_press_rate_saturated_at_1_per_30_ticks",
            Self::SuccessfulJumpRate => "successful_jump_rate_saturated_at_1_per_30_ticks",
            Self::WallJumpShare => "wall_jump_share",
            Self::DashPressRate => "dash_press_rate_saturated_at_1_per_60_ticks",
            Self::SuccessfulDashRate => "successful_dash_rate_saturated_at_1_per_60_ticks",
            Self::HorizontalReversalRate => "horizontal_reversal_rate_saturated_at_1_per_60_ticks",
            Self::DeathCount => "death_count_saturated_at_4",
            Self::PickupCollectionCount => "pickup_collection_count_saturated_at_8",
            Self::WallJumpAbility => "wall_jump_ability",
            Self::DashAbility => "dash_ability",
        }
    }
}

pub const OBSERVATION_FEATURES: [ObservationFeature; OBSERVATION_FEATURE_COUNT] = [
    ObservationFeature::SolidCellFraction,
    ObservationFeature::OneWayCellFraction,
    ObservationFeature::StaticHazardCellFraction,
    ObservationFeature::TimedHazardAreaFraction,
    ObservationFeature::ExitCount,
    ObservationFeature::PickupCount,
    ObservationFeature::SpawnX,
    ObservationFeature::SpawnY,
    ObservationFeature::PassableRegionCount,
    ObservationFeature::CollisionFaceDensity,
    ObservationFeature::TraversalCoverage,
    ObservationFeature::HorizontalSpan,
    ObservationFeature::VerticalSpan,
    ObservationFeature::HorizontalTravel,
    ObservationFeature::VerticalTravel,
    ObservationFeature::PathDirectness,
    ObservationFeature::CompletionDuration,
    ObservationFeature::InputTransitionRate,
    ObservationFeature::ActiveInputFraction,
    ObservationFeature::JumpPressRate,
    ObservationFeature::SuccessfulJumpRate,
    ObservationFeature::WallJumpShare,
    ObservationFeature::DashPressRate,
    ObservationFeature::SuccessfulDashRate,
    ObservationFeature::HorizontalReversalRate,
    ObservationFeature::DeathCount,
    ObservationFeature::PickupCollectionCount,
    ObservationFeature::WallJumpAbility,
    ObservationFeature::DashAbility,
];

/// Fixed, versioned behavior characterization suitable for experimental
/// quality-diversity archives. Selection policy is deliberately left to the
/// caller; in particular, no coordinate is designated as fitness.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservationFeatureVector {
    pub version: u32,
    pub values: [f64; OBSERVATION_FEATURE_COUNT],
}

impl ObservationFeatureVector {
    #[must_use]
    pub fn get(&self, feature: ObservationFeature) -> f64 {
        self.values[feature as usize]
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (ObservationFeature, f64)> + '_ {
        OBSERVATION_FEATURES
            .into_iter()
            .zip(self.values.iter().copied())
    }

    /// Mean absolute coordinate difference, normalized to `[0, 1]`.
    #[must_use]
    pub fn normalized_l1_distance(&self, other: &Self) -> f64 {
        if self.version != other.version {
            return 1.0;
        }
        self.values
            .iter()
            .zip(&other.values)
            .map(|(left, right)| (left - right).abs())
            .sum::<f64>()
            / OBSERVATION_FEATURE_COUNT as f64
    }

    /// Root-mean-square coordinate difference, normalized to `[0, 1]`.
    #[must_use]
    pub fn normalized_l2_distance(&self, other: &Self) -> f64 {
        if self.version != other.version {
            return 1.0;
        }
        (self
            .values
            .iter()
            .zip(&other.values)
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f64>()
            / OBSERVATION_FEATURE_COUNT as f64)
            .sqrt()
    }
}

/// Build the feature vector for a witness observed from `initial`.
///
/// Callers should pass the same initial simulation used to produce the
/// witness. The descriptor arguments make batch pipelines able to cache exact
/// room measurements instead of recomputing them for every witness.
#[must_use]
pub fn observation_feature_vector(
    initial: &Simulation,
    visual: &StaticVisualDescriptor,
    collision: &CollisionTopologyDescriptor,
    witness: &SuccessfulWitnessObservation,
) -> ObservationFeatureVector {
    let mut values = [0.0; OBSERVATION_FEATURE_COUNT];
    let tile_count = visual.tiles.len().max(1) as f64;
    set(
        &mut values,
        ObservationFeature::SolidCellFraction,
        visual
            .tiles
            .iter()
            .filter(|&&tile| tile == VisualTile::Solid)
            .count() as f64
            / tile_count,
    );
    set(
        &mut values,
        ObservationFeature::OneWayCellFraction,
        visual
            .tiles
            .iter()
            .filter(|&&tile| tile == VisualTile::OneWay)
            .count() as f64
            / tile_count,
    );
    set(
        &mut values,
        ObservationFeature::StaticHazardCellFraction,
        visual
            .tiles
            .iter()
            .filter(|&&tile| tile == VisualTile::Hazard)
            .count() as f64
            / tile_count,
    );

    let pixel_width = f64::from(visual.width) * f64::from(visual.tile_size);
    let pixel_height = f64::from(visual.height) * f64::from(visual.tile_size);
    let screen_area = (pixel_width * pixel_height).max(1.0);
    let timed_area = visual
        .timed_hazards
        .iter()
        .map(|bounds| f64::from(bounds.width.max(0)) * f64::from(bounds.height.max(0)))
        .sum::<f64>();
    set(
        &mut values,
        ObservationFeature::TimedHazardAreaFraction,
        (timed_area / screen_area).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::ExitCount,
        saturated_count(visual.exits.len() + visual.doors.len(), 8),
    );
    set(
        &mut values,
        ObservationFeature::PickupCount,
        saturated_count(visual.pickups.len(), 8),
    );
    set(
        &mut values,
        ObservationFeature::SpawnX,
        (f64::from(visual.spawn.x) / pixel_width.max(1.0)).clamp(0.0, 1.0),
    );
    set(
        &mut values,
        ObservationFeature::SpawnY,
        (f64::from(visual.spawn.y) / pixel_height.max(1.0)).clamp(0.0, 1.0),
    );
    set(
        &mut values,
        ObservationFeature::PassableRegionCount,
        saturated_count(collision.regions.len(), 8),
    );
    let face_length = collision
        .faces
        .iter()
        .map(|face| usize::from(face.length_cells()))
        .sum::<usize>();
    let maximum_faces = usize::from(collision.width) * usize::from(collision.height) * 4;
    set(
        &mut values,
        ObservationFeature::CollisionFaceDensity,
        face_length as f64 / maximum_faces.max(1) as f64,
    );

    let traversal = &witness.traversal;
    let grid_cells = usize::from(traversal.grid.columns()) * usize::from(traversal.grid.rows());
    set(
        &mut values,
        ObservationFeature::TraversalCoverage,
        traversal.visited_cells.len() as f64 / grid_cells.max(1) as f64,
    );
    let (horizontal_span, vertical_span) =
        traversal_span(traversal.visited_cells.as_ref(), traversal.grid);
    set(
        &mut values,
        ObservationFeature::HorizontalSpan,
        horizontal_span,
    );
    set(&mut values, ObservationFeature::VerticalSpan, vertical_span);
    let (horizontal_travel, vertical_travel, directness) = traversal_motion(traversal);
    set(
        &mut values,
        ObservationFeature::HorizontalTravel,
        horizontal_travel,
    );
    set(
        &mut values,
        ObservationFeature::VerticalTravel,
        vertical_travel,
    );
    set(&mut values, ObservationFeature::PathDirectness, directness);

    let actions = &witness.actions;
    let ticks = actions.total_ticks.max(1) as f64;
    set(
        &mut values,
        ObservationFeature::CompletionDuration,
        (actions.total_ticks as f64 / 1_200.0).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::InputTransitionRate,
        ((actions.spans.len().saturating_sub(1) as f64 * 10.0) / ticks).min(1.0),
    );
    let active_ticks = actions
        .spans
        .iter()
        .filter(|span| span.action != SemanticAction::default())
        .map(|span| span.ticks)
        .sum::<usize>();
    set(
        &mut values,
        ObservationFeature::ActiveInputFraction,
        active_ticks as f64 / ticks,
    );
    set(
        &mut values,
        ObservationFeature::JumpPressRate,
        (actions.jump_presses as f64 * 30.0 / ticks).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::SuccessfulJumpRate,
        (actions.successful_jumps as f64 * 30.0 / ticks).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::WallJumpShare,
        if actions.successful_jumps == 0 {
            0.0
        } else {
            actions.successful_wall_jumps as f64 / actions.successful_jumps as f64
        },
    );
    set(
        &mut values,
        ObservationFeature::DashPressRate,
        (actions.dash_presses as f64 * 60.0 / ticks).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::SuccessfulDashRate,
        (actions.successful_dashes as f64 * 60.0 / ticks).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::HorizontalReversalRate,
        (horizontal_reversals(actions) as f64 * 60.0 / ticks).min(1.0),
    );
    set(
        &mut values,
        ObservationFeature::DeathCount,
        saturated_count(actions.deaths, 4),
    );
    set(
        &mut values,
        ObservationFeature::PickupCollectionCount,
        saturated_count(actions.pickups_collected, 8),
    );
    let AbilitySet { wall_jump, dash } = initial.abilities();
    set(
        &mut values,
        ObservationFeature::WallJumpAbility,
        f64::from(wall_jump),
    );
    set(
        &mut values,
        ObservationFeature::DashAbility,
        f64::from(dash),
    );

    debug_assert!(values.iter().all(|value| (0.0..=1.0).contains(value)));
    ObservationFeatureVector {
        version: OBSERVATION_FEATURE_VERSION,
        values,
    }
}

fn set(values: &mut [f64; OBSERVATION_FEATURE_COUNT], feature: ObservationFeature, value: f64) {
    values[feature as usize] = value.clamp(0.0, 1.0);
}

fn saturated_count(value: usize, saturation: usize) -> f64 {
    value.min(saturation) as f64 / saturation as f64
}

fn traversal_span(cells: &[TraversalCell], grid: crate::TraversalGrid) -> (f64, f64) {
    let Some(first) = cells.first().copied() else {
        return (0.0, 0.0);
    };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
    for cell in &cells[1..] {
        min_x = min_x.min(cell.x);
        max_x = max_x.max(cell.x);
        min_y = min_y.min(cell.y);
        max_y = max_y.max(cell.y);
    }
    (
        f64::from(max_x - min_x) / f64::from(grid.columns().saturating_sub(1).max(1)),
        f64::from(max_y - min_y) / f64::from(grid.rows().saturating_sub(1).max(1)),
    )
}

fn traversal_motion(trace: &crate::TraversalTrace) -> (f64, f64, f64) {
    let Some(first) = trace.spans.first().map(|span| span.cell) else {
        return (0.0, 0.0, 0.0);
    };
    let mut previous = first;
    let mut horizontal = 0_u32;
    let mut vertical = 0_u32;
    for span in &trace.spans[1..] {
        horizontal += u32::from(previous.x.abs_diff(span.cell.x));
        vertical += u32::from(previous.y.abs_diff(span.cell.y));
        previous = span.cell;
    }
    let total = horizontal + vertical;
    let endpoint =
        u32::from(first.x.abs_diff(previous.x)) + u32::from(first.y.abs_diff(previous.y));
    let directness = if total == 0 {
        0.0
    } else {
        f64::from(endpoint) / f64::from(total)
    };
    let horizontal_normalizer = f64::from(trace.grid.columns().saturating_sub(1).max(1)) * 4.0;
    let vertical_normalizer = f64::from(trace.grid.rows().saturating_sub(1).max(1)) * 4.0;
    (
        (f64::from(horizontal) / horizontal_normalizer).min(1.0),
        (f64::from(vertical) / vertical_normalizer).min(1.0),
        directness,
    )
}

fn horizontal_reversals(actions: &crate::SemanticActionTrace) -> usize {
    let mut previous_direction = 0;
    let mut reversals = 0;
    for span in &actions.spans {
        if span.action.move_x == 0 {
            continue;
        }
        if previous_direction != 0 && previous_direction != span.action.move_x {
            reversals += 1;
        }
        previous_direction = span.action.move_x;
    }
    reversals
}
