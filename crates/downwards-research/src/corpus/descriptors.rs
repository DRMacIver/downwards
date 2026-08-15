//! Integer-quantized room descriptors used by deterministic corpus selection.

use downwards_core::{BoundarySide, Tile};
use downwards_gen::{
    GeneratedLevel, StagedCompositionalCandidate,
    experimental::{RoutePlan, RoutePlanSummary},
};
use downwards_lab::CollisionTopologyDescriptor;

use crate::structural::{StructuralDescriptorError, describe_terrain_utility};

pub const ROOM_EMBEDDING_PREFIX_VERSION: u32 = 1;
pub const MORPHOLOGY_DIMENSIONS: usize = 12;
pub const TOPOLOGY_DIMENSIONS: usize = 8;
const QUANTIZED_MAX: u32 = u16::MAX as u32;

/// The selection-independent prefix of the planned 60-coordinate embedding.
///
/// Difficulty, behavior, and terrain-use evidence are appended only after
/// complete route matrices exist. Keeping this prefix separately versioned
/// prevents a generation-only pilot from being mistaken for a fully measured
/// corpus room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomEmbeddingPrefix {
    pub version: u32,
    pub morphology: [u16; MORPHOLOGY_DIMENSIONS],
    pub topology: [u16; TOPOLOGY_DIMENSIONS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RoomEmbeddingPrefixDistance {
    /// Mean absolute morphology-coordinate distance, scaled to `0..=65535`.
    pub morphology: u16,
    /// Mean absolute topology-coordinate distance, scaled to `0..=65535`.
    pub topology: u16,
    /// Equal mean of the two group distances. No group wins by having more
    /// coordinates.
    pub combined: u16,
}

pub fn room_embedding_prefix(
    candidate: &StagedCompositionalCandidate,
) -> Result<RoomEmbeddingPrefix, StructuralDescriptorError> {
    room_embedding_prefix_from_parts(
        &candidate.generated,
        &candidate.route_plan,
        &candidate.route_summary,
    )
}

/// Derive the selection-independent embedding prefix from the common native
/// generator surface.
///
/// The historical adapter above remains source-compatible, while final-path
/// generator families can supply their own exact generated room and route
/// evidence without being flattened into a staged-v6 candidate.
pub fn room_embedding_prefix_from_parts(
    generated: &GeneratedLevel,
    route_plan: &RoutePlan,
    summary: &RoutePlanSummary,
) -> Result<RoomEmbeddingPrefix, StructuralDescriptorError> {
    let room = &generated.room;
    let tile_count = room.tiles().len();
    let solid_tiles = room
        .tiles()
        .iter()
        .filter(|&&tile| tile == Tile::Solid)
        .count();
    let one_way_tiles = room
        .tiles()
        .iter()
        .filter(|&&tile| tile == Tile::OneWay)
        .count();
    let static_hazard_tiles = room
        .tiles()
        .iter()
        .filter(|&&tile| tile == Tile::Hazard)
        .count();
    let timed_hazard_area = room
        .timed_hazards()
        .iter()
        .map(|hazard| {
            let bounds = hazard.bounds();
            usize::try_from(bounds.width.saturating_mul(bounds.height)).unwrap_or(usize::MAX)
        })
        .sum::<usize>();
    let room_pixel_area = usize::from(room.width())
        .saturating_mul(usize::from(room.height()))
        .saturating_mul(usize::try_from(room.tile_size()).unwrap_or_default().pow(2));
    let collision = CollisionTopologyDescriptor::from_room(room);
    let exposed_face_length = collision
        .faces
        .iter()
        .map(|face| usize::from(face.length_cells()))
        .sum::<usize>();
    let maximum_face_length = tile_count.saturating_mul(4);
    let side_mask = room.doors().iter().fold(0_u8, |mask, door| {
        mask | match door.side {
            BoundarySide::Left => 1,
            BoundarySide::Right => 2,
            BoundarySide::Ceiling => 4,
            BoundarySide::Floor => 8,
        }
    });
    let terrain = describe_terrain_utility(room, route_plan, &[])?;

    Ok(RoomEmbeddingPrefix {
        version: ROOM_EMBEDDING_PREFIX_VERSION,
        morphology: [
            quantized_ratio(solid_tiles, tile_count),
            quantized_ratio(one_way_tiles, tile_count),
            quantized_ratio(static_hazard_tiles, tile_count),
            quantized_ratio(timed_hazard_area, room_pixel_area),
            quantized_capped(room.doors().len(), 4),
            quantized_capped(room.pickups().len(), 8),
            quantized_capped(collision.regions.len(), 8),
            quantized_ratio(exposed_face_length, maximum_face_length),
            quantized_bool(side_mask & 1 != 0),
            quantized_bool(side_mask & 2 != 0),
            quantized_bool(side_mask & 4 != 0),
            quantized_bool(side_mask & 8 != 0),
        ],
        topology: [
            quantized_capped(usize::from(summary.node_count), 32),
            quantized_capped(usize::from(summary.edge_count), 48),
            quantized_capped(usize::from(summary.cycle_rank), 4),
            quantized_capped(usize::from(summary.branch_nodes), 8),
            quantized_capped(usize::from(summary.vertical_span_rows), 17),
            quantized_ratio(
                usize::from(summary.wall_edges),
                usize::from(summary.edge_count),
            ),
            quantized_ratio(
                usize::from(summary.dash_edges),
                usize::from(summary.edge_count),
            ),
            quantized_capped(terrain.components.len(), 16),
        ],
    })
}

#[must_use]
pub fn room_embedding_prefix_distance(
    left: &RoomEmbeddingPrefix,
    right: &RoomEmbeddingPrefix,
) -> Option<RoomEmbeddingPrefixDistance> {
    if left.version != right.version {
        return None;
    }
    let morphology = mean_absolute_distance(&left.morphology, &right.morphology);
    let topology = mean_absolute_distance(&left.topology, &right.topology);
    let combined = (u32::from(morphology) + u32::from(topology)).div_ceil(2) as u16;
    Some(RoomEmbeddingPrefixDistance {
        morphology,
        topology,
        combined,
    })
}

fn mean_absolute_distance<const N: usize>(left: &[u16; N], right: &[u16; N]) -> u16 {
    let sum = left
        .iter()
        .zip(right)
        .map(|(left, right)| u32::from(left.abs_diff(*right)))
        .sum::<u32>();
    ((sum + u32::try_from(N / 2).unwrap_or_default()) / u32::try_from(N).unwrap_or(1)) as u16
}

fn quantized_bool(value: bool) -> u16 {
    if value { u16::MAX } else { 0 }
}

fn quantized_ratio(numerator: usize, denominator: usize) -> u16 {
    let numerator = numerator as u128;
    let denominator = denominator as u128;
    if denominator == 0 {
        return 0;
    }
    let scaled = numerator
        .min(denominator)
        .saturating_mul(u128::from(QUANTIZED_MAX));
    ((scaled + denominator / 2) / denominator) as u16
}

fn quantized_capped(value: usize, cap: u16) -> u16 {
    quantized_ratio(value, usize::from(cap))
}

#[cfg(test)]
mod tests {
    use downwards_core::AbilitySet;
    use downwards_gen::{
        CompositionalFeatureSet, CompositionalKey, CompositionalProfile, StagedCompositionalKey,
        experimental::{ChallengeIntent, GenerationStrategy},
        generate_staged_compositional,
    };

    use super::*;

    fn candidate(
        strategy: GenerationStrategy,
        features: CompositionalFeatureSet,
    ) -> StagedCompositionalCandidate {
        generate_staged_compositional(StagedCompositionalKey::new(
            CompositionalKey::new(
                9,
                CompositionalProfile::new(AbilitySet::ALL, strategy, ChallengeIntent::Technical),
            ),
            features,
        ))
        .unwrap()
    }

    #[test]
    fn prefix_is_bounded_repeatable_and_identity_distance_is_zero() {
        let candidate = candidate(
            GenerationStrategy::ReachabilityGrowth,
            CompositionalFeatureSet::TerrainOnly,
        );
        let first = room_embedding_prefix(&candidate).unwrap();
        let second = room_embedding_prefix(&candidate).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            room_embedding_prefix_distance(&first, &second),
            Some(RoomEmbeddingPrefixDistance::default())
        );
        assert_eq!(first.morphology.len(), MORPHOLOGY_DIMENSIONS);
        assert_eq!(first.topology.len(), TOPOLOGY_DIMENSIONS);
    }

    #[test]
    fn strategy_and_feature_changes_are_visible_in_separate_groups() {
        let terrain = candidate(
            GenerationStrategy::ReachabilityGrowth,
            CompositionalFeatureSet::TerrainOnly,
        );
        let hazards = candidate(
            GenerationStrategy::ReachabilityGrowth,
            CompositionalFeatureSet::TimedHazards,
        );
        let cyclic = candidate(
            GenerationStrategy::CyclicGraph,
            CompositionalFeatureSet::TerrainOnly,
        );
        let terrain = room_embedding_prefix(&terrain).unwrap();
        let hazards = room_embedding_prefix(&hazards).unwrap();
        let cyclic = room_embedding_prefix(&cyclic).unwrap();

        let feature_distance = room_embedding_prefix_distance(&terrain, &hazards).unwrap();
        assert!(feature_distance.morphology > 0);
        assert_eq!(feature_distance.topology, 0);
        let strategy_distance = room_embedding_prefix_distance(&terrain, &cyclic).unwrap();
        assert!(strategy_distance.combined > 0);
    }
}
