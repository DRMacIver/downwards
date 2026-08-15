//! Isolated, unpromoted ceiling-anchored WallJump chimney experiment.
//!
//! This mapping deliberately has its own exact key and result types.  It does
//! not replace or fall back to the frozen physical-v2 ability generator.

use std::{error::Error, fmt};

use downwards_core::AbilitySet;

use super::{
    AbilityGateEmbeddingState, AbilityGateRealization, AbilityGateTileCell,
    AbilityRewrittenMission, BoundaryPort, COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
    ChallengeIntent, CompositionalAbilityEdgeRewriteFailure, CompositionalAbilityEdgeRewriteKey,
    CompositionalAbilityGenerationFailure, CompositionalRouteCutKey, MissionDerivationFailure,
    MissionRouteNodeMapping, RouteCutRealization, RoutePlan, RoutePlanSummary,
};
use crate::GeneratedLevel;

/// Version of the v4-only graph/geometry contract retained by exact keys.
pub const COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION: u32 = 4;

/// Version of the isolated ceiling-anchored chimney mapping.
pub const COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION: u32 = 4;

/// Highest explicit chimney-domain ordering attempt accepted by v4.
pub const COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT: u8 = 15;

/// The physical v4 contract, independent of the frozen graph-rewrite v1
/// reservation hint retained in [`AbilityRewrittenMission`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WallChimneyV4Contract {
    pub version: u32,
    pub ascent_rows: u16,
    pub minimum_clear_width_tiles: u16,
    pub maximum_clear_width_tiles: u16,
    pub side_exit_height_tiles: u16,
    pub ceiling_anchored: bool,
    pub forbid_intermediate_supports: bool,
    pub require_baseline_reverse_descent: bool,
}

impl WallChimneyV4Contract {
    pub const CURRENT: Self = Self {
        version: COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION,
        ascent_rows: 10,
        minimum_clear_width_tiles: 4,
        maximum_clear_width_tiles: 6,
        side_exit_height_tiles: 2,
        ceiling_anchored: true,
        forbid_intermediate_supports: true,
        require_baseline_reverse_descent: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WallChimneyV4ExitSide {
    Left,
    Right,
}

/// One cap-free local chimney reservation.  `physical` owns every required
/// solid/empty tile; the extra fields make the two wall segments and aperture
/// independently auditable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallChimneyV4Realization {
    pub physical: AbilityGateRealization,
    pub contract: WallChimneyV4Contract,
    pub exit_side: WallChimneyV4ExitSide,
    pub continuous_wall_column: u16,
    pub exit_wall_column: u16,
    pub side_exit_aperture_tiles: Vec<AbilityGateTileCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallChimneyV4EmbeddingSummary {
    pub mapping_version: u32,
    pub contract_version: u32,
    pub graph_rewrite_version: u32,
    pub embedding_attempt: u8,
    pub rewrite_attempt: u16,
    pub chimney_attempt: u8,
    pub ascent_edges: u16,
    pub descent_edges: u16,
    pub level_edges: u16,
    pub horizontal_direction_reversals: u16,
    pub cut_realizations: Vec<RouteCutRealization>,
    pub socket_columns: Vec<u16>,
    pub chimney_realizations: Vec<WallChimneyV4Realization>,
}

/// Exact v4 request.  Version fields are data, not implicit process state, so
/// stale persisted keys fail rather than silently selecting current code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalWallChimneyV4GenerationKey {
    pub base_key: CompositionalRouteCutKey,
    pub graph_rewrite_version: u32,
    pub contract_version: u32,
    pub chimney_mapping_version: u32,
    pub rewrite_attempt: u16,
    pub chimney_attempt: u8,
}

impl CompositionalWallChimneyV4GenerationKey {
    #[must_use]
    pub const fn new(source_seed: u64, intent: ChallengeIntent) -> Self {
        Self {
            base_key: CompositionalRouteCutKey::new(source_seed, AbilitySet::NONE, intent),
            graph_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            contract_version: COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION,
            chimney_mapping_version: COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
            rewrite_attempt: 0,
            chimney_attempt: 0,
        }
    }

    #[must_use]
    pub const fn with_attempts(
        mut self,
        embedding_attempt: u8,
        rewrite_attempt: u16,
        chimney_attempt: u8,
    ) -> Self {
        self.base_key.embedding_attempt = embedding_attempt;
        self.rewrite_attempt = rewrite_attempt;
        self.chimney_attempt = chimney_attempt;
        self
    }

    pub fn generate(
        self,
    ) -> Result<CompositionalWallChimneyV4Candidate, WallChimneyV4GenerationError> {
        generate_compositional_wall_chimney_v4_candidate(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalWallChimneyV4Candidate {
    pub key: CompositionalWallChimneyV4GenerationKey,
    pub rewritten_mission: AbilityRewrittenMission,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
    pub mission_route_nodes: Vec<MissionRouteNodeMapping>,
    pub embedding: WallChimneyV4EmbeddingSummary,
    pub evidence_state: AbilityGateEmbeddingState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WallChimneyV4GenerationFailure {
    VersionMismatch {
        graph_rewrite_version: u32,
        contract_version: u32,
        chimney_mapping_version: u32,
    },
    ChimneyAttemptOutOfRange {
        requested: u8,
        maximum: u8,
    },
    Mission(MissionDerivationFailure),
    Rewrite(CompositionalAbilityEdgeRewriteFailure),
    Physical(CompositionalAbilityGenerationFailure),
}

impl fmt::Display for WallChimneyV4GenerationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionMismatch {
                graph_rewrite_version,
                contract_version,
                chimney_mapping_version,
            } => write!(
                formatter,
                "stale v4 versions graph={graph_rewrite_version} contract={contract_version} mapping={chimney_mapping_version}"
            ),
            Self::ChimneyAttemptOutOfRange { requested, maximum } => write!(
                formatter,
                "chimney attempt {requested} exceeds the finite maximum {maximum}"
            ),
            Self::Mission(error) => write!(formatter, "base mission derivation failed: {error}"),
            Self::Rewrite(error) => write!(formatter, "WallJump graph rewrite failed: {error}"),
            Self::Physical(error) => write!(formatter, "v4 chimney embedding failed: {error}"),
        }
    }
}

impl Error for WallChimneyV4GenerationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Mission(error) => Some(error),
            Self::Rewrite(error) => Some(error),
            Self::Physical(error) => Some(error),
            Self::VersionMismatch { .. } | Self::ChimneyAttemptOutOfRange { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallChimneyV4GenerationError {
    pub key: CompositionalWallChimneyV4GenerationKey,
    pub cause: WallChimneyV4GenerationFailure,
}

impl fmt::Display for WallChimneyV4GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unpromoted wall-chimney-v4 embedding-attempt {} rewrite-attempt {} chimney-attempt {} seed {:016x} failed: {}",
            self.key.base_key.embedding_attempt,
            self.key.rewrite_attempt,
            self.key.chimney_attempt,
            self.key.base_key.source_seed,
            self.cause,
        )
    }
}

impl Error for WallChimneyV4GenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

pub fn generate_compositional_wall_chimney_v4_candidate(
    key: CompositionalWallChimneyV4GenerationKey,
) -> Result<CompositionalWallChimneyV4Candidate, WallChimneyV4GenerationError> {
    if key.graph_rewrite_version != COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION
        || key.contract_version != COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION
        || key.chimney_mapping_version != COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION
    {
        return Err(WallChimneyV4GenerationError {
            key,
            cause: WallChimneyV4GenerationFailure::VersionMismatch {
                graph_rewrite_version: key.graph_rewrite_version,
                contract_version: key.contract_version,
                chimney_mapping_version: key.chimney_mapping_version,
            },
        });
    }
    if key.chimney_attempt > COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT {
        return Err(WallChimneyV4GenerationError {
            key,
            cause: WallChimneyV4GenerationFailure::ChimneyAttemptOutOfRange {
                requested: key.chimney_attempt,
                maximum: COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT,
            },
        });
    }
    let mission = key
        .base_key
        .derive_mission()
        .map_err(|error| WallChimneyV4GenerationError {
            key,
            cause: WallChimneyV4GenerationFailure::Mission(error.cause),
        })?;
    let rewrite_key = CompositionalAbilityEdgeRewriteKey::new(
        key.base_key,
        super::CompositionalAbilityGateProfile::WallJump,
    )
    .with_rewrite_attempt(key.rewrite_attempt);
    let rewritten_mission =
        rewrite_key
            .rewrite(&mission)
            .map_err(|error| WallChimneyV4GenerationError {
                key,
                cause: WallChimneyV4GenerationFailure::Rewrite(error.cause),
            })?;
    super::compositional_route_cut::embed_wall_chimney_v4(key, rewritten_mission).map_err(|cause| {
        WallChimneyV4GenerationError {
            key,
            cause: WallChimneyV4GenerationFailure::Physical(cause),
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use downwards_core::Tile;

    use super::*;
    use crate::experimental::{
        AbilityGateGeometryViolation, CompositionalAbilityEmbeddingPhase,
        CompositionalRouteCutGenerationFailure,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FrozenConstructionOutcome {
        Positive,
        Search {
            phase: CompositionalAbilityEmbeddingPhase,
            explored: u32,
        },
        DoorArrivalBlocked,
        Gate {
            gate_ordinal: u16,
            violation: AbilityGateGeometryViolation,
        },
    }

    #[test]
    fn exact_key_rejects_stale_versions_and_out_of_range_attempts() {
        let current = CompositionalWallChimneyV4GenerationKey::new(
            0,
            crate::experimental::ChallengeIntent::Gentle,
        );
        for stale in [
            CompositionalWallChimneyV4GenerationKey {
                graph_rewrite_version: current.graph_rewrite_version + 1,
                ..current
            },
            CompositionalWallChimneyV4GenerationKey {
                contract_version: current.contract_version + 1,
                ..current
            },
            CompositionalWallChimneyV4GenerationKey {
                chimney_mapping_version: current.chimney_mapping_version + 1,
                ..current
            },
        ] {
            assert!(matches!(
                stale
                    .generate()
                    .expect_err("stale v4 version must fail")
                    .cause,
                WallChimneyV4GenerationFailure::VersionMismatch { .. },
            ));
        }
        let out_of_range = CompositionalWallChimneyV4GenerationKey {
            chimney_attempt: COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT + 1,
            ..current
        };
        assert_eq!(
            out_of_range
                .generate()
                .expect_err("out-of-range v4 attempt must fail")
                .cause,
            WallChimneyV4GenerationFailure::ChimneyAttemptOutOfRange {
                requested: COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT + 1,
                maximum: COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT,
            },
        );
    }

    #[test]
    fn exact_attempt_zero_block_freezes_rejected_construction_evidence() {
        let expected = [
            FrozenConstructionOutcome::Positive,
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Fork,
                explored: 100_000,
            },
            FrozenConstructionOutcome::DoorArrivalBlocked,
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Rhythm,
                explored: 4_944,
            },
            FrozenConstructionOutcome::Positive,
            FrozenConstructionOutcome::Gate {
                gate_ordinal: 0,
                violation: AbilityGateGeometryViolation::BoundaryArrivalBlocked,
            },
            FrozenConstructionOutcome::DoorArrivalBlocked,
            FrozenConstructionOutcome::Positive,
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Rhythm,
                explored: 764,
            },
            FrozenConstructionOutcome::Positive,
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Rhythm,
                explored: 136,
            },
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Rhythm,
                explored: 112,
            },
            FrozenConstructionOutcome::DoorArrivalBlocked,
            FrozenConstructionOutcome::Positive,
            FrozenConstructionOutcome::Search {
                phase: CompositionalAbilityEmbeddingPhase::Spine,
                explored: 1,
            },
        ];
        let mut successes = 0_usize;
        let mut geometries = BTreeSet::new();
        let mut outcome_index = 0_usize;
        for intent in crate::experimental::ChallengeIntent::ALL {
            for seed in 0_u64..=4_u64 {
                let key = CompositionalWallChimneyV4GenerationKey::new(seed, intent);
                let actual = match key.generate() {
                    Ok(candidate) => {
                        assert_eq!(candidate.key, key);
                        assert_eq!(candidate, key.generate().expect("exact v4 regeneration"));
                        assert_eq!(
                            candidate.embedding.mapping_version,
                            COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
                        );
                        assert_eq!(
                            candidate.embedding.contract_version,
                            COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION,
                        );
                        assert!(!candidate.embedding.chimney_realizations.is_empty());
                        for chimney in &candidate.embedding.chimney_realizations {
                            assert_eq!(chimney.contract, WallChimneyV4Contract::CURRENT);
                            assert_eq!(
                                chimney.physical.lower_support.row
                                    - chimney.physical.upper_support.row,
                                WallChimneyV4Contract::CURRENT.ascent_rows,
                            );
                            let upper_row = chimney.physical.upper_support.row;
                            let lower_row = chimney.physical.lower_support.row;
                            let aperture_start =
                                upper_row - WallChimneyV4Contract::CURRENT.side_exit_height_tiles;
                            let shaft_start =
                                u16::try_from(chimney.physical.ascent_bounds.x / crate::TILE_SIZE)
                                    .expect("v4 shaft starts on a non-negative tile");
                            let shaft_width = u16::try_from(
                                chimney.physical.ascent_bounds.width / crate::TILE_SIZE,
                            )
                            .expect("v4 shaft width fits tiles");
                            let shaft_end = shaft_start + shaft_width;
                            assert_eq!(
                                chimney.side_exit_aperture_tiles,
                                (aperture_start..upper_row)
                                    .map(|row| AbilityGateTileCell {
                                        x: chimney.exit_wall_column,
                                        row,
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            for row in 0..lower_row {
                                assert_eq!(
                                    candidate
                                        .generated
                                        .room
                                        .tile(chimney.continuous_wall_column, row),
                                    Some(Tile::Solid),
                                );
                                let expected_exit_tile =
                                    if (aperture_start..upper_row).contains(&row) {
                                        Tile::Empty
                                    } else {
                                        Tile::Solid
                                    };
                                assert_eq!(
                                    candidate.generated.room.tile(chimney.exit_wall_column, row),
                                    Some(expected_exit_tile),
                                );
                            }
                            for row in 1..lower_row {
                                for x in shaft_start..shaft_end {
                                    assert_eq!(
                                        candidate.generated.room.tile(x, row),
                                        Some(Tile::Empty),
                                    );
                                }
                            }
                            assert!(candidate.route_plan.nodes.iter().all(|node| {
                                node.id == chimney.physical.from_route_node_id
                                    || node.id == chimney.physical.to_route_node_id
                                    || !(upper_row < node.support.row
                                        && node.support.row < lower_row)
                            }));
                        }
                        let geometry = candidate
                            .embedding
                            .chimney_realizations
                            .iter()
                            .map(|chimney| {
                                (
                                    chimney.physical.lower_support.row,
                                    chimney.physical.upper_support.row,
                                    chimney.physical.ascent_bounds.x,
                                    chimney.physical.ascent_bounds.y,
                                    chimney.physical.ascent_bounds.width,
                                    chimney.physical.ascent_bounds.height,
                                    chimney.exit_side,
                                    chimney.continuous_wall_column,
                                    chimney.exit_wall_column,
                                    chimney.physical.lower_support,
                                    chimney.physical.upper_support,
                                )
                            })
                            .collect::<Vec<_>>();
                        assert!(geometries.insert(geometry));
                        successes += 1;
                        FrozenConstructionOutcome::Positive
                    }
                    Err(error) => {
                        assert_eq!(
                            error,
                            key.generate().expect_err("exact v4 refusal regeneration"),
                        );
                        match error.cause {
                            WallChimneyV4GenerationFailure::Physical(
                                CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                                    phase,
                                    explored,
                                },
                            ) => FrozenConstructionOutcome::Search { phase, explored },
                            WallChimneyV4GenerationFailure::Physical(
                                CompositionalAbilityGenerationFailure::BaselineEmbedding(
                                    CompositionalRouteCutGenerationFailure::Door(_),
                                ),
                            ) => FrozenConstructionOutcome::DoorArrivalBlocked,
                            WallChimneyV4GenerationFailure::Physical(
                                CompositionalAbilityGenerationFailure::GateContract {
                                    gate_ordinal,
                                    violation,
                                },
                            ) => FrozenConstructionOutcome::Gate {
                                gate_ordinal,
                                violation,
                            },
                            cause => panic!(
                                "unexpected v4 refusal intent={} seed={seed}: {cause:?}",
                                intent.slug(),
                            ),
                        }
                    }
                };
                assert_eq!(actual, expected[outcome_index]);
                outcome_index += 1;
            }
        }
        assert_eq!(outcome_index, expected.len());
        assert_eq!(successes, 5, "rejected v4 evidence is exactly 5/15");
        assert_eq!(geometries.len(), successes);
    }
}
