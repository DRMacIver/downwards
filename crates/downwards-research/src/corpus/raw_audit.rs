//! Selection-independent diversity audit for a generated seed block.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use downwards_core::BoundarySide;
use downwards_lab::{
    CollisionTopologyDescriptor, StaticVisualDescriptor, collision_topology_distance,
    static_visual_distance,
};
use serde::Serialize;

use super::GeneratedCorpusBatch;

pub const RAW_CORPUS_DIVERSITY_AUDIT_VERSION: u32 = 1;

/// A deterministic distribution of normalized pairwise distances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DistanceDistribution {
    pub comparisons: usize,
    pub minimum: Option<f64>,
    pub p10: Option<f64>,
    pub median: Option<f64>,
    pub p90: Option<f64>,
    pub maximum: Option<f64>,
    pub mean: Option<f64>,
    /// One nearest-neighbour value per item, averaged after all pairs have
    /// been inspected. This exposes clusters that a global mean can hide.
    pub mean_nearest_neighbor: Option<f64>,
    pub nearest_p10: Option<f64>,
    pub nearest_median: Option<f64>,
}

/// Cheap expressive-range evidence computed before any solver result is used.
///
/// Profile aliases, same-preview simulation variants, and accidental
/// cross-seed duplicates are deliberately separate. They have very different
/// implications for a generator and must not be collapsed into one count.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawCorpusDiversityAudit {
    pub version: u32,
    pub attempted_profiles: usize,
    pub constructed_profiles: usize,
    pub rejected_profiles: usize,
    pub exact_simulation_rooms: usize,
    pub exact_static_rooms: usize,
    pub exact_collision_topologies: usize,
    pub exact_route_signatures: usize,
    pub profile_alias_candidates: usize,
    pub same_static_different_simulation_rooms: usize,
    pub cross_seed_static_duplicate_groups: usize,
    pub rooms_in_cross_seed_static_duplicate_groups: usize,
    pub largest_static_group: usize,
    pub variable_tile_cells: usize,
    pub static_visual_distance: DistanceDistribution,
    pub collision_topology_distance: DistanceDistribution,
    pub port_count_distribution: BTreeMap<usize, usize>,
    pub boundary_side_mask_distribution: BTreeMap<u8, usize>,
    pub canonical_strategy_distribution: BTreeMap<String, usize>,
    pub canonical_intent_distribution: BTreeMap<String, usize>,
}

#[must_use]
pub fn audit_raw_corpus_diversity(batch: &GeneratedCorpusBatch) -> RawCorpusDiversityAudit {
    let mut static_groups = HashMap::<StaticVisualDescriptor, StaticGroup>::new();
    let mut collision_topologies = HashSet::<CollisionTopologyDescriptor>::new();
    let mut route_signatures = HashSet::<u64>::new();
    let mut tile_state_masks = Vec::<u8>::new();
    let mut port_count_distribution = BTreeMap::new();
    let mut boundary_side_mask_distribution = BTreeMap::new();
    let mut canonical_strategy_distribution = BTreeMap::new();
    let mut canonical_intent_distribution = BTreeMap::new();

    for room in &batch.rooms {
        let group = static_groups.entry(room.static_visual.clone()).or_default();
        group.simulation_rooms += 1;
        for variant in &room.variants {
            group.seeds.insert(variant.key.source.seed);
            route_signatures.insert(variant.route_summary.signature);
        }

        let canonical = &room.variants[0];
        let core_room = &canonical.generated.room;
        collision_topologies.insert(CollisionTopologyDescriptor::from_room(core_room));
        *port_count_distribution
            .entry(core_room.doors().len())
            .or_default() += 1;
        let side_mask = core_room.doors().iter().fold(0_u8, |mask, door| {
            mask | match door.side {
                BoundarySide::Left => 1,
                BoundarySide::Right => 2,
                BoundarySide::Ceiling => 4,
                BoundarySide::Floor => 8,
            }
        });
        *boundary_side_mask_distribution
            .entry(side_mask)
            .or_default() += 1;
        *canonical_strategy_distribution
            .entry(canonical.key.source.profile.strategy.slug().to_owned())
            .or_default() += 1;
        *canonical_intent_distribution
            .entry(canonical.key.source.profile.intent.slug().to_owned())
            .or_default() += 1;

        if tile_state_masks.is_empty() {
            tile_state_masks.resize(room.static_visual.tiles.len(), 0);
        }
        for (&tile, state_mask) in room.static_visual.tiles.iter().zip(&mut tile_state_masks) {
            *state_mask |= 1_u8 << (tile as u8);
        }
    }

    let static_descriptors = static_groups.keys().cloned().collect::<Vec<_>>();
    let collision_descriptors = collision_topologies.into_iter().collect::<Vec<_>>();
    let cross_seed_groups = static_groups
        .values()
        .filter(|group| group.seeds.len() > 1)
        .collect::<Vec<_>>();
    let exact_static_rooms = static_descriptors.len();

    RawCorpusDiversityAudit {
        version: RAW_CORPUS_DIVERSITY_AUDIT_VERSION,
        attempted_profiles: batch.summary.attempted,
        constructed_profiles: batch.summary.constructed,
        rejected_profiles: batch.summary.rejected,
        exact_simulation_rooms: batch.rooms.len(),
        exact_static_rooms,
        exact_collision_topologies: collision_descriptors.len(),
        exact_route_signatures: route_signatures.len(),
        profile_alias_candidates: batch.summary.constructed.saturating_sub(batch.rooms.len()),
        same_static_different_simulation_rooms: batch
            .rooms
            .len()
            .saturating_sub(exact_static_rooms),
        cross_seed_static_duplicate_groups: cross_seed_groups.len(),
        rooms_in_cross_seed_static_duplicate_groups: cross_seed_groups
            .iter()
            .map(|group| group.simulation_rooms)
            .sum(),
        largest_static_group: static_groups
            .values()
            .map(|group| group.simulation_rooms)
            .max()
            .unwrap_or_default(),
        variable_tile_cells: tile_state_masks
            .iter()
            .filter(|mask| mask.count_ones() > 1)
            .count(),
        static_visual_distance: summarize_distances(&static_descriptors, |left, right| {
            static_visual_distance(left, right).combined
        }),
        collision_topology_distance: summarize_distances(&collision_descriptors, |left, right| {
            collision_topology_distance(left, right).combined
        }),
        port_count_distribution,
        boundary_side_mask_distribution,
        canonical_strategy_distribution,
        canonical_intent_distribution,
    }
}

#[derive(Default)]
struct StaticGroup {
    simulation_rooms: usize,
    seeds: BTreeSet<u64>,
}

fn summarize_distances<T>(
    values: &[T],
    mut distance: impl FnMut(&T, &T) -> f64,
) -> DistanceDistribution {
    if values.len() < 2 {
        return DistanceDistribution::default();
    }
    let mut pairwise = Vec::with_capacity(values.len().saturating_mul(values.len() - 1) / 2);
    let mut nearest = vec![f64::INFINITY; values.len()];
    for left in 0..values.len() {
        for right in left + 1..values.len() {
            let value = distance(&values[left], &values[right]);
            pairwise.push(value);
            nearest[left] = nearest[left].min(value);
            nearest[right] = nearest[right].min(value);
        }
    }
    pairwise.sort_unstable_by(f64::total_cmp);
    nearest.sort_unstable_by(f64::total_cmp);
    let mean = pairwise.iter().sum::<f64>() / pairwise.len() as f64;
    let mean_nearest_neighbor = nearest.iter().sum::<f64>() / nearest.len() as f64;
    DistanceDistribution {
        comparisons: pairwise.len(),
        minimum: pairwise.first().copied(),
        p10: percentile(&pairwise, 10),
        median: percentile(&pairwise, 50),
        p90: percentile(&pairwise, 90),
        maximum: pairwise.last().copied(),
        mean: Some(mean),
        mean_nearest_neighbor: Some(mean_nearest_neighbor),
        nearest_p10: percentile(&nearest, 10),
        nearest_median: percentile(&nearest, 50),
    }
}

fn percentile(sorted: &[f64], percentile: usize) -> Option<f64> {
    (!sorted.is_empty()).then(|| sorted[(sorted.len() - 1) * percentile / 100])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{CorpusBuildConfigV1, generate_seed_block};

    #[test]
    fn four_seed_audit_separates_profile_aliases_from_cross_seed_duplicates() {
        let batch = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 4)).unwrap();
        let first = audit_raw_corpus_diversity(&batch);
        let second = audit_raw_corpus_diversity(&batch);

        assert_eq!(first, second);
        assert_eq!(first.attempted_profiles, 144);
        assert_eq!(first.exact_static_rooms, 84);
        assert_eq!(first.exact_simulation_rooms, 84);
        assert_eq!(first.profile_alias_candidates, 60);
        assert_eq!(first.cross_seed_static_duplicate_groups, 0);
        assert_eq!(first.same_static_different_simulation_rooms, 0);
        assert_eq!(first.largest_static_group, 1);
        assert_eq!(first.static_visual_distance.comparisons, 84 * 83 / 2);
        assert!(
            first
                .static_visual_distance
                .median
                .is_some_and(|value| value > 0.0)
        );
        assert!(
            first
                .collision_topology_distance
                .nearest_median
                .is_some_and(|value| value > 0.0)
        );
        assert_eq!(first.port_count_distribution.values().sum::<usize>(), 84);
    }
}
