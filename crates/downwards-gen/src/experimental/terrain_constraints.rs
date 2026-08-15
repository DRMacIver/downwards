//! Opt-in terrain-only transforms for testing route constraints.
//!
//! The v6 generators were authored with static floor hazards as the main
//! reason to leave the boundary floor.  Removing those hazards for the first
//! terrain-only corpus exposed a universal floor bypass.  This experiment
//! instead turns existing authored structure into a physical floor
//! constraint. Ordered-arc generators ground a route landing as a narrow
//! pier. Reachability growth is deliberately outside this transform: trials
//! showed that grounding a random junction can sever pickup branches, while
//! low hazard-derived hurdles did not improve easiest-known controllers.

use downwards_core::{PLAYER_HEIGHT, PLAYER_WIDTH, Rect, Room};

use super::{
    CandidateParts, ChallengeIntent, ExperimentalCandidate, ExperimentalGenerationError,
    GenerationStrategy, NodeRole, common::FLOOR_ROW,
};

/// Version of the independent terrain-constraint experiment mapping.
pub const TERRAIN_CONSTRAINT_EXPERIMENT_VERSION: u32 = 1;

/// Frozen opt-in terrain transforms. New experiments must add a variant
/// rather than silently changing an existing seed-to-room mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TerrainConstraintExperiment {
    /// Ground one central, two-sided route landing as a narrow pier.
    GroundedRoutePierV1,
}

impl TerrainConstraintExperiment {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::GroundedRoutePierV1 => "grounded-route-pier-v1",
        }
    }
}

/// Exact constructive facts produced by one terrain transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainConstraintSummary {
    pub experiment: TerrainConstraintExperiment,
    pub grounded_node_id: u16,
    pub grounded_row: u16,
    pub pier_start_x: u16,
    pub pier_end_x: u16,
    pub removed_static_hazard_tiles: u16,
    pub removed_timed_hazards: u16,
}

/// A transformed candidate and the exact local rewrite applied to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainConstrainedCandidate {
    pub candidate: ExperimentalCandidate,
    pub summary: TerrainConstraintSummary,
}

pub(super) fn apply(
    parts: &mut CandidateParts,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
    experiment: TerrainConstraintExperiment,
) -> Result<TerrainConstraintSummary, ExperimentalGenerationError> {
    match strategy {
        GenerationStrategy::CyclicGraph | GenerationStrategy::RhythmWeave => {
            apply_grounded_route_pier(parts, strategy, intent, experiment)
        }
        GenerationStrategy::ReachabilityGrowth => {
            Err(ExperimentalGenerationError::GrammarExhausted {
                strategy,
                detail: format!(
                    "{} does not apply to the {} strategy",
                    experiment.slug(),
                    strategy.slug()
                ),
            })
        }
    }
}

fn apply_grounded_route_pier(
    parts: &mut CandidateParts,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
    experiment: TerrainConstraintExperiment,
) -> Result<TerrainConstraintSummary, ExperimentalGenerationError> {
    let (removed_static_hazard_tiles, removed_timed_hazards) = parts.draft.clear_hazards();
    let preferred_row = match intent {
        ChallengeIntent::Gentle => 13,
        ChallengeIntent::Standard => 11,
        ChallengeIntent::Technical => 9,
    };

    let selected = parts
        .route_plan
        .nodes
        .iter()
        .filter(|node| {
            !matches!(
                node.role,
                NodeRole::Port | NodeRole::Pickup | NodeRole::Recovery
            )
        })
        .filter(|node| (4..=15).contains(&node.support.row))
        .filter(|node| (6..=26).contains(&node.support.center_x()))
        .filter_map(|node| {
            let support = node.support;
            // One tile is enough to break the universal floor line. Keeping
            // the pier narrow minimizes terrain that the resulting positive
            // controllers may never approach.
            let pier_width = 1;
            let pier_start_x = support
                .center_x()
                .saturating_sub(pier_width / 2)
                .max(support.start_x);
            let pier_end_x = pier_start_x + pier_width;
            let has_left_neighbor = incident_neighbors(parts, node.id)
                .any(|neighbor| neighbor.support.center_x() < pier_start_x);
            let has_right_neighbor = incident_neighbors(parts, node.id)
                .any(|neighbor| neighbor.support.center_x() >= pier_end_x);
            let neighbor_count = incident_neighbors(parts, node.id).count();
            if neighbor_count < 2 {
                return None;
            }
            let pier = Rect::new(
                i32::from(pier_start_x) * crate::TILE_SIZE,
                i32::from(support.row) * crate::TILE_SIZE,
                i32::from(pier_end_x - pier_start_x) * crate::TILE_SIZE,
                i32::from(FLOOR_ROW - support.row) * crate::TILE_SIZE,
            );
            let blocks_arrival = parts.boundary_ports.iter().any(|port| {
                Rect::new(
                    port.door.arrival.x,
                    port.door.arrival.y,
                    PLAYER_WIDTH,
                    PLAYER_HEIGHT,
                )
                .intersects(pier)
            });
            let blocks_floor_socket = parts.boundary_ports.iter().any(|port| {
                port.door.side == downwards_core::BoundarySide::Floor
                    && port.door.trigger_bounds.intersects(pier)
            });
            (!blocks_arrival && !blocks_floor_socket).then_some((
                node.id,
                support,
                pier_start_x,
                pier_end_x,
                has_left_neighbor && has_right_neighbor,
            ))
        })
        .min_by_key(|(node_id, support, _, _, two_sided)| {
            (
                !*two_sided,
                support.row.abs_diff(preferred_row),
                support.center_x().abs_diff(crate::ROOM_WIDTH / 2),
                *node_id,
            )
        })
        .ok_or_else(|| ExperimentalGenerationError::EmbeddingExhausted {
            strategy,
            detail: format!(
                "{} found no connected central route landing for a grounded pier",
                experiment.slug()
            ),
        })?;

    let (node_id, support, pier_start_x, pier_end_x, _) = selected;
    parts
        .draft
        .ground_support_pier(support, pier_start_x, pier_end_x);

    Ok(TerrainConstraintSummary {
        experiment,
        grounded_node_id: node_id,
        grounded_row: support.row,
        pier_start_x,
        pier_end_x,
        removed_static_hazard_tiles,
        removed_timed_hazards,
    })
}
fn incident_neighbors<'a>(
    parts: &'a CandidateParts,
    node_id: u16,
) -> impl Iterator<Item = &'a super::RouteNode> + 'a {
    parts.route_plan.edges.iter().filter_map(move |edge| {
        let neighbor = if edge.from == node_id {
            edge.to
        } else if edge.to == node_id {
            edge.from
        } else {
            return None;
        };
        parts.route_plan.nodes.get(usize::from(neighbor))
    })
}

pub(super) fn reidentify(
    mut candidate: ExperimentalCandidate,
    experiment: TerrainConstraintExperiment,
) -> Result<ExperimentalCandidate, ExperimentalGenerationError> {
    let original = &candidate.generated.room;
    let room = Room::new(
        format!(
            "experimental-terrain-constraint-v{TERRAIN_CONSTRAINT_EXPERIMENT_VERSION}-{}-{}-{}-{:016x}",
            experiment.slug(),
            candidate.strategy.slug(),
            candidate.intent.slug(),
            candidate.generated.metadata.seed,
        ),
        format!(
            "Experimental terrain constraint v{TERRAIN_CONSTRAINT_EXPERIMENT_VERSION} {} {} {} {:016x}",
            experiment.slug(),
            candidate.strategy.slug(),
            candidate.intent.slug(),
            candidate.generated.metadata.seed,
        ),
        original.width(),
        original.height(),
        original.tile_size(),
        original.tiles().to_vec(),
        original.spawn(),
        original.exits().to_vec(),
    )?
    .with_objects(
        original.timed_hazards().to_vec(),
        original.pickups().to_vec(),
    )?
    .with_doors(original.doors().to_vec())?;
    candidate.generated.room = room;
    candidate.generated.metadata.generation_version = 6_100 + TERRAIN_CONSTRAINT_EXPERIMENT_VERSION;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_core::{AbilitySet, Tile};

    use super::*;

    #[test]
    fn grounded_pier_experiment_is_deterministic_and_hazard_free() {
        for strategy in [
            GenerationStrategy::CyclicGraph,
            GenerationStrategy::RhythmWeave,
        ] {
            for intent in ChallengeIntent::ALL {
                let experiment = TerrainConstraintExperiment::GroundedRoutePierV1;
                let first = super::super::generate_terrain_constrained_candidate(
                    0,
                    AbilitySet::NONE,
                    strategy,
                    intent,
                    experiment,
                )
                .unwrap();
                let second = super::super::generate_terrain_constrained_candidate(
                    0,
                    AbilitySet::NONE,
                    strategy,
                    intent,
                    experiment,
                )
                .unwrap();
                assert_eq!(first, second);
                assert!(
                    !first
                        .candidate
                        .generated
                        .room
                        .tiles()
                        .iter()
                        .any(|tile| tile.is_hazard())
                );
                assert!(first.candidate.generated.room.timed_hazards().is_empty());
                assert!(first.summary.grounded_row < FLOOR_ROW);
                assert!(
                    first
                        .candidate
                        .generated
                        .room
                        .id()
                        .contains("terrain-constraint-v1")
                );
            }
        }
    }

    #[test]
    fn broad_batch_constructs_distinct_safe_rooms() {
        let mut digests = HashSet::new();
        for strategy in [
            GenerationStrategy::CyclicGraph,
            GenerationStrategy::RhythmWeave,
        ] {
            for intent in ChallengeIntent::ALL {
                for seed in 0..32 {
                    let experiment = TerrainConstraintExperiment::GroundedRoutePierV1;
                    let transformed = super::super::generate_terrain_constrained_candidate(
                        seed,
                        AbilitySet::ALL,
                        strategy,
                        intent,
                        experiment,
                    )
                    .unwrap();
                    digests.insert(
                        transformed
                            .candidate
                            .generated
                            .room
                            .tiles()
                            .iter()
                            .map(|tile| match tile {
                                Tile::Empty => 0_u8,
                                Tile::Solid => 1,
                                Tile::HazardUp => 2,
                                Tile::OneWay => 3,
                                Tile::HazardDown => 4,
                                Tile::HazardLeft => 5,
                                Tile::HazardRight => 6,
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
        }
        assert!(digests.len() > 160, "only {} unique rooms", digests.len());
    }

    #[test]
    fn grounded_pier_explicitly_rejects_unordered_growth_graphs() {
        let error = super::super::generate_terrain_constrained_candidate(
            0,
            AbilitySet::ALL,
            GenerationStrategy::ReachabilityGrowth,
            ChallengeIntent::Technical,
            TerrainConstraintExperiment::GroundedRoutePierV1,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ExperimentalGenerationError::GrammarExhausted {
                strategy: GenerationStrategy::ReachabilityGrowth,
                ..
            }
        ));
    }
}
