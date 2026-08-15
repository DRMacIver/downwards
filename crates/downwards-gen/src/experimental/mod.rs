//! Experimental, non-template single-room generators.
//!
//! These strategies deliberately remain separate from the production
//! [`crate::generate_for_abilities`] mapping while their expressive ranges and
//! solver acceptance rates are measured. Each strategy produces the same
//! strategy-neutral route graph and room artifact, allowing the lab tooling to
//! compare structure and play traces rather than strategy-specific counters.

mod ability_edge_rewrite;
mod common;
mod compositional_route_cut;
mod cyclic_graph;
mod partition_route;
mod reachability_growth;
mod rhythm_weave;
mod switchback_cut;
mod terrain_constraints;
mod wall_chimney_v4;

use std::{collections::HashSet, error::Error, fmt};

use downwards_core::{AbilitySet, DoorError, RoomError};

pub use ability_edge_rewrite::{
    AbilityEdgeRewriteProvenance, AbilityGateEmbeddingContract, AbilityGateEmbeddingPendingReason,
    AbilityGateEmbeddingState, AbilityGateGeometry, AbilityGateGeometryViolation,
    AbilityGateRealization, AbilityGateTileCell, AbilityRewrittenMission,
    AbilityRewrittenMissionPlan, COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
    COMPOSITIONAL_ABILITY_EDGE_SEARCH_LIMIT, COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
    COMPOSITIONAL_ABILITY_GENERATION_VERSION, CompositionalAbilityCandidate,
    CompositionalAbilityEdgeRewriteError, CompositionalAbilityEdgeRewriteFailure,
    CompositionalAbilityEdgeRewriteKey, CompositionalAbilityEmbeddingPhase,
    CompositionalAbilityEmbeddingSeam, CompositionalAbilityEmbeddingSummary,
    CompositionalAbilityGateProfile, CompositionalAbilityGenerationError,
    CompositionalAbilityGenerationFailure, CompositionalAbilityGenerationKey, DirectedAbilityGate,
    DirectedMissionEdge, DirectedTraversalRequirement, GateAbility, GateCertificationCheck,
    GateUnavoidabilityCertificate, MissionPlanValidationFailure,
    generate_compositional_ability_candidate, rewrite_compositional_mission_ability_edges,
};
pub use common::{
    BoundaryPort, CandidateParts, NodeRole, RouteEdge, RouteNode, RoutePlan, RoutePlanSummary,
    RouteVerb, SupportKind, SupportSpec,
};
pub use compositional_route_cut::{
    COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION, COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
    COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT,
    COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION, CompositionalRouteCutCandidate,
    CompositionalRouteCutEmbeddingSummary, CompositionalRouteCutGenerationError,
    CompositionalRouteCutGenerationFailure, CompositionalRouteCutGrammar, CompositionalRouteCutKey,
    CutAnchor, DerivedMission, MissionCut, MissionDerivationError, MissionDerivationFailure,
    MissionEdge, MissionEdgeKind, MissionFork, MissionNode, MissionNodeKind, MissionPlan,
    MissionPort, MissionProvenance, MissionRewrite, MissionRouteNodeMapping, RouteCutRealization,
    SupportConstraintPhase, compositional_route_cut_socket_in_inventory,
    derive_compositional_route_cut_mission, generate_compositional_route_cut,
};
pub use partition_route::{
    PARTITION_ROUTE_GENERATION_VERSION, PARTITION_ROUTE_MAX_EMBEDDING_ATTEMPT, PartitionDerivation,
    PartitionDerivationBeat, PartitionDerivationSplit, PartitionRouteCandidate,
    PartitionRouteFailure, PartitionRouteGenerationError, PartitionRouteKey, PartitionRouteProfile,
    PartitionRouteSummary, PartitionSplitAxis, generate_partition_route,
};
pub use switchback_cut::{
    SWITCHBACK_CUT_GENERATION_VERSION, SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT,
    SwitchbackCutCandidate, SwitchbackCutFailure, SwitchbackCutGenerationError,
    SwitchbackCutGrammar, SwitchbackCutKey, SwitchbackCutSummary, generate_switchback_cut,
};
pub use terrain_constraints::{
    TERRAIN_CONSTRAINT_EXPERIMENT_VERSION, TerrainConstrainedCandidate,
    TerrainConstraintExperiment, TerrainConstraintSummary,
};
pub use wall_chimney_v4::{
    COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION, COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
    COMPOSITIONAL_WALL_CHIMNEY_V4_MAX_ATTEMPT, CompositionalWallChimneyV4Candidate,
    CompositionalWallChimneyV4GenerationKey, WallChimneyV4Contract, WallChimneyV4EmbeddingSummary,
    WallChimneyV4ExitSide, WallChimneyV4GenerationError, WallChimneyV4GenerationFailure,
    WallChimneyV4Realization, generate_compositional_wall_chimney_v4_candidate,
};

use crate::{GeneratedLevel, GeneratedMetadata, LayoutFamily};

/// Version of the experimental seed-to-candidate mapping.
///
/// This is intentionally independent from [`crate::GENERATION_VERSION`]. A
/// strategy is promoted only after the experiment report and curated catalogue
/// are reproducible; promotion then bumps the production version.
pub const EXPERIMENTAL_GENERATION_VERSION: u32 = 2;

/// Independent constructive approaches under comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GenerationStrategy {
    /// Rewrite an abstract start/exit cycle, then embed its route graph.
    CyclicGraph,
    /// Grow a movement-reachable platform graph and retain fork/rejoin cycles.
    ReachabilityGrowth,
    /// Generate movement rhythms first and weave their landing beats together.
    RhythmWeave,
}

impl GenerationStrategy {
    pub const ALL: [Self; 3] = [
        Self::CyclicGraph,
        Self::ReachabilityGrowth,
        Self::RhythmWeave,
    ];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::CyclicGraph => "cyclic-graph",
            Self::ReachabilityGrowth => "reachability-growth",
            Self::RhythmWeave => "rhythm-weave",
        }
    }

    const fn presentation_family(self, abilities: AbilitySet) -> LayoutFamily {
        match self {
            Self::CyclicGraph => LayoutFamily::TerracedAscent,
            Self::ReachabilityGrowth => LayoutFamily::HazardRun,
            Self::RhythmWeave if abilities.wall_jump && !abilities.dash => LayoutFamily::Chimney,
            Self::RhythmWeave => LayoutFamily::DashGallery,
        }
    }
}

/// Requested challenge character supplied before geometry exists.
///
/// This is an input to generation, not a claim about human difficulty. The
/// gameplay AI independently measures the realized candidate afterward.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChallengeIntent {
    Gentle,
    Standard,
    Technical,
}

impl ChallengeIntent {
    pub const ALL: [Self; 3] = [Self::Gentle, Self::Standard, Self::Technical];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Standard => "standard",
            Self::Technical => "technical",
        }
    }
}

/// A generated room plus the abstract structure that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentalCandidate {
    pub generated: GeneratedLevel,
    pub strategy: GenerationStrategy,
    pub intent: ChallengeIntent,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExperimentalGenerationError {
    GrammarExhausted {
        strategy: GenerationStrategy,
        detail: String,
    },
    EmbeddingExhausted {
        strategy: GenerationStrategy,
        detail: String,
    },
    PortContract {
        strategy: GenerationStrategy,
        detail: String,
    },
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for ExperimentalGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GrammarExhausted { strategy, detail } => {
                write!(formatter, "{} grammar exhausted: {detail}", strategy.slug())
            }
            Self::EmbeddingExhausted { strategy, detail } => {
                write!(
                    formatter,
                    "{} embedding exhausted: {detail}",
                    strategy.slug()
                )
            }
            Self::PortContract { strategy, detail } => {
                write!(
                    formatter,
                    "{} port contract failed: {detail}",
                    strategy.slug()
                )
            }
            Self::Room(error) => write!(formatter, "generated room was invalid: {error}"),
            Self::Door(error) => write!(formatter, "generated room doors were invalid: {error}"),
        }
    }
}

impl Error for ExperimentalGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Room(error) => Some(error),
            Self::Door(error) => Some(error),
            Self::GrammarExhausted { .. }
            | Self::EmbeddingExhausted { .. }
            | Self::PortContract { .. } => None,
        }
    }
}

impl From<RoomError> for ExperimentalGenerationError {
    fn from(value: RoomError) -> Self {
        Self::Room(value)
    }
}

impl From<DoorError> for ExperimentalGenerationError {
    fn from(value: DoorError) -> Self {
        Self::Door(value)
    }
}

/// Construct one deterministic experimental candidate.
///
/// Success establishes core room invariants only. Exit and pickup reachability
/// remain positive claims made by the authoritative solver and certificate
/// layer during offline evaluation.
pub fn generate_candidate(
    seed: u64,
    abilities: AbilitySet,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
) -> Result<ExperimentalCandidate, ExperimentalGenerationError> {
    let parts = match strategy {
        GenerationStrategy::CyclicGraph => cyclic_graph::generate(seed, abilities, intent)?,
        GenerationStrategy::ReachabilityGrowth => {
            reachability_growth::generate(seed, abilities, intent)?
        }
        GenerationStrategy::RhythmWeave => rhythm_weave::generate(seed, abilities, intent)?,
    };
    finish_candidate(seed, abilities, strategy, intent, parts)
}

/// Construct one opt-in terrain-only constraint experiment.
///
/// This deliberately does not change [`generate_candidate`] or the v6
/// compositional mapping.  The experiment starts from the same strategy
/// grammar, removes both hazard layers, and applies the selected versioned
/// terrain transform before rebuilding the room under a distinct identity.
/// Solver validation remains mandatory: the transform establishes room and
/// door construction safety, not reachability. The v1 grounded-pier transform
/// is defined only for the ordered-arc cyclic and rhythm strategies;
/// reachability growth is rejected explicitly because its random graph has no
/// safe generic cut landing.
pub fn generate_terrain_constrained_candidate(
    seed: u64,
    abilities: AbilitySet,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
    experiment: TerrainConstraintExperiment,
) -> Result<TerrainConstrainedCandidate, ExperimentalGenerationError> {
    let mut parts = match strategy {
        GenerationStrategy::CyclicGraph => cyclic_graph::generate(seed, abilities, intent)?,
        GenerationStrategy::ReachabilityGrowth => {
            reachability_growth::generate(seed, abilities, intent)?
        }
        GenerationStrategy::RhythmWeave => rhythm_weave::generate(seed, abilities, intent)?,
    };
    let summary = terrain_constraints::apply(&mut parts, strategy, intent, experiment)?;
    let candidate = finish_candidate(seed, abilities, strategy, intent, parts)?;
    let candidate = terrain_constraints::reidentify(candidate, experiment)?;
    Ok(TerrainConstrainedCandidate { candidate, summary })
}

fn finish_candidate(
    seed: u64,
    abilities: AbilitySet,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
    parts: CandidateParts,
) -> Result<ExperimentalCandidate, ExperimentalGenerationError> {
    let CandidateParts {
        draft,
        spawn,
        boundary_ports,
        route_plan,
    } = parts;
    validate_port_contract(strategy, &route_plan, &boundary_ports)?;
    let route_summary = route_plan.summary();
    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-v{EXPERIMENTAL_GENERATION_VERSION}-{}-{}-{seed:016x}",
        strategy.slug(),
        intent.slug()
    );
    let name = format!(
        "Experimental v{EXPERIMENTAL_GENERATION_VERSION} {} {} {seed:016x}",
        strategy.slug(),
        intent.slug()
    );
    let doors = boundary_ports
        .iter()
        .map(|port| port.door.clone())
        .collect();
    let room = draft
        .finish_without_exits(id, name, spawn)?
        .with_doors(doors)?;
    let layout_family = strategy.presentation_family(abilities);
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: 6_000 + EXPERIMENTAL_GENERATION_VERSION,
            seed,
            layout_family,
            ability_tier: crate::AbilityTier::from_abilities(abilities),
            intended_abilities: abilities,
            stats,
        },
    };
    Ok(ExperimentalCandidate {
        generated,
        strategy,
        intent,
        route_plan,
        route_summary,
        boundary_ports,
    })
}

fn validate_port_contract(
    strategy: GenerationStrategy,
    route_plan: &RoutePlan,
    boundary_ports: &[BoundaryPort],
) -> Result<(), ExperimentalGenerationError> {
    if !(2..=4).contains(&boundary_ports.len()) {
        return Err(ExperimentalGenerationError::PortContract {
            strategy,
            detail: format!(
                "expected two to four boundary ports, found {}",
                boundary_ports.len()
            ),
        });
    }

    let referenced = boundary_ports
        .iter()
        .map(|port| port.node_id)
        .collect::<HashSet<_>>();
    if referenced.len() != boundary_ports.len() {
        return Err(ExperimentalGenerationError::PortContract {
            strategy,
            detail: "multiple doors reference the same route node".to_owned(),
        });
    }
    let declared = route_plan
        .nodes
        .iter()
        .filter(|node| node.role == NodeRole::Port)
        .map(|node| node.id)
        .collect::<HashSet<_>>();
    if declared != referenced {
        return Err(ExperimentalGenerationError::PortContract {
            strategy,
            detail: "route-plan port nodes do not exactly match physical doors".to_owned(),
        });
    }

    let first = boundary_ports[0].node_id;
    let mut reachable = HashSet::from([first]);
    let mut frontier = vec![first];
    while let Some(node) = frontier.pop() {
        for edge in &route_plan.edges {
            let adjacent = if edge.from == node {
                Some(edge.to)
            } else if edge.to == node {
                Some(edge.from)
            } else {
                None
            };
            if let Some(adjacent) = adjacent
                && reachable.insert(adjacent)
            {
                frontier.push(adjacent);
            }
        }
    }
    if !referenced.is_subset(&reachable) {
        return Err(ExperimentalGenerationError::PortContract {
            strategy,
            detail: "route-plan ports are not in one connected component".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_strategy_is_deterministic_for_success_and_failure() {
        for strategy in GenerationStrategy::ALL {
            for intent in ChallengeIntent::ALL {
                for seed in 0..32 {
                    let first = generate_candidate(seed, AbilitySet::ALL, strategy, intent);
                    let second = generate_candidate(seed, AbilitySet::ALL, strategy, intent);
                    assert_eq!(first, second, "{strategy:?} {intent:?} seed {seed}");
                }
            }
        }
    }

    #[test]
    fn every_strategy_uses_the_shared_boundary_socket_grid() {
        for strategy in GenerationStrategy::ALL {
            let mut successes = 0;
            for abilities in [AbilitySet::NONE, AbilitySet::ALL] {
                for intent in ChallengeIntent::ALL {
                    for seed in 0..128 {
                        let Ok(candidate) = generate_candidate(seed, abilities, strategy, intent)
                        else {
                            continue;
                        };
                        successes += 1;
                        for port in candidate.boundary_ports {
                            let socket = port.door.socket();
                            assert_eq!(socket.span, 2 * crate::TILE_SIZE);
                            assert_eq!(socket.offset.rem_euclid(crate::TILE_SIZE), 0);
                        }
                    }
                }
            }
            assert!(successes > 0, "{strategy:?} produced no socket fixtures");
        }
    }

    #[test]
    fn generated_socket_inventory_has_an_opposite_mate_for_every_aperture() {
        let mut sockets = Vec::new();
        for strategy in GenerationStrategy::ALL {
            for intent in ChallengeIntent::ALL {
                for seed in 0..256 {
                    let Ok(candidate) = generate_candidate(seed, AbilitySet::ALL, strategy, intent)
                    else {
                        continue;
                    };
                    sockets.extend(
                        candidate
                            .boundary_ports
                            .iter()
                            .map(|port| port.door.socket()),
                    );
                }
            }
        }

        assert!(!sockets.is_empty());
        for &socket in &sockets {
            assert!(
                sockets.iter().any(|&candidate| socket.matches(candidate)),
                "no generated mate for boundary socket {socket:?}"
            );
        }
    }
}
