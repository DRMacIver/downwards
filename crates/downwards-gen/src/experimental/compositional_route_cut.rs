//! Coordinate-free mission derivation for a compositional route-cut experiment.
//!
//! This module deliberately stops before geometry.  A mission begins as one
//! source-to-sink edge and is transformed by explicit edge subdivision,
//! route-cut, fork/rejoin, port-attachment, and pickup rewrites.  Embedding is
//! a later constrained phase, so neither a room template nor coordinate
//! jitter can influence the topology recorded here.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use downwards_core::{
    AbilitySet, BoundarySide, Door, DoorError, DoorSocket, PLAYER_HEIGHT, PLAYER_WIDTH, Point,
    Rect, Room, RoomError, Tile,
};

use super::{
    BoundaryPort, ChallengeIntent, NodeRole, RoutePlan, RoutePlanSummary, RouteVerb, SupportKind,
    SupportSpec,
    ability_edge_rewrite::{
        AbilityGateGeometryViolation, AbilityGateRealization, AbilityGateTileCell,
        AbilityRewrittenMission, COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
        COMPOSITIONAL_ABILITY_GENERATION_VERSION, CompositionalAbilityCandidate,
        CompositionalAbilityEmbeddingPhase, CompositionalAbilityEmbeddingSummary,
        CompositionalAbilityGenerationFailure, CompositionalAbilityGenerationKey,
        DirectedAbilityGate, DirectedTraversalRequirement, GateAbility,
    },
    common::{
        FLOOR_ROW, RoomDraft, StableRng, add_route_edge, add_route_node,
        conservative_baseline_transition, reversible_baseline_transition,
    },
};
use crate::{
    AbilityTier, GeneratedLevel, GeneratedMetadata, LayoutFamily, ROOM_HEIGHT, ROOM_WIDTH,
    TILE_SIZE,
};

mod wall_chimney_v4_mapping;

/// Version of the seed-to-mission derivation implemented by this experiment.
pub const COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION: u32 = 1;

/// Version of the exact mission-to-room embedding mapping.
pub const COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION: u32 = 2;

/// Version of the deliberately opposite-mate-closed aperture inventory.
pub const COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION: u32 = 1;

/// Highest explicit embedding-attempt identity accepted by the v2 key.
///
/// The attempt does not affect mission derivation.  A future embedding phase
/// may try these exact identities in order and must persist the accepted one.
pub const COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT: u8 = 15;

const DERIVATION_RNG_STREAM: u64 = 0x4352_4355_545f_4431;
const EMBEDDING_RNG_STREAM: u64 = 0x4352_4355_545f_4532;
const SPINE_CONSTRAINT_SEARCH_LIMIT: u32 = 500_000;
const FORK_CONSTRAINT_SEARCH_LIMIT: u32 = 100_000;
const SIDE_DOOR_DEPTH: i32 = 12;
const CEILING_DOOR_DEPTH: i32 = 12;
const DOOR_SPAN: i32 = 20;
const HORIZONTAL_SOCKET_COLUMNS: [u16; 5] = [6, 10, 14, 18, 22];

/// Frozen coordinate-free grammar.  New mappings add variants rather than
/// changing an existing seed's mission graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompositionalRouteCutGrammar {
    /// Repeatedly rewrite one initial edge into a route network containing
    /// zero or more physical-cut obligations and optional retained-path
    /// fork/rejoin cycles.
    RecursiveMissionCutsV1,
}

impl CompositionalRouteCutGrammar {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::RecursiveMissionCutsV1 => "recursive-mission-cuts-v1",
        }
    }
}

/// Exact, stable request identity for one future room embedding.
///
/// `embedding_attempt` is explicit rather than a hidden retry counter.  The
/// coordinate-free mission is invariant across attempts; only the later
/// constrained embedding may vary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalRouteCutKey {
    pub source_seed: u64,
    pub construction_abilities: AbilitySet,
    pub intent: ChallengeIntent,
    pub grammar: CompositionalRouteCutGrammar,
    pub embedding_attempt: u8,
}

impl CompositionalRouteCutKey {
    #[must_use]
    pub const fn new(
        source_seed: u64,
        construction_abilities: AbilitySet,
        intent: ChallengeIntent,
    ) -> Self {
        Self {
            source_seed,
            construction_abilities,
            intent,
            grammar: CompositionalRouteCutGrammar::RecursiveMissionCutsV1,
            embedding_attempt: 0,
        }
    }

    /// Select an exact grammar and future embedding retry identity.
    #[must_use]
    pub const fn with_embedding(
        mut self,
        grammar: CompositionalRouteCutGrammar,
        embedding_attempt: u8,
    ) -> Self {
        self.grammar = grammar;
        self.embedding_attempt = embedding_attempt;
        self
    }

    /// Derive the coordinate-free mission associated with this exact key.
    pub fn derive_mission(self) -> Result<DerivedMission, MissionDerivationError> {
        derive_compositional_route_cut_mission(self)
    }

    /// Regenerate precisely this future room identity without retrying a
    /// different attempt or selecting a fallback grammar.
    pub fn regenerate(
        self,
    ) -> Result<CompositionalRouteCutCandidate, CompositionalRouteCutGenerationError> {
        generate_compositional_route_cut(self)
    }
}

/// Coordinate-free semantic role of a mission node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MissionNodeKind {
    Port,
    Transit,
    Junction,
    Cut,
    ForkBranch,
    Pickup,
}

/// One abstract mission node.  IDs are stable derivation identities, while
/// canonical topology hashing relabels nodes independently of these IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MissionNode {
    pub id: u16,
    pub kind: MissionNodeKind,
}

/// Coordinate-free origin of a mission edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MissionEdgeKind {
    Spine,
    ForkBranch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MissionEdge {
    pub from: u16,
    pub to: u16,
    pub kind: MissionEdgeKind,
    /// The original spine remains the authored critical route when a
    /// retained-path fork is added beside it.
    pub critical: bool,
}

/// Side to which a future horizontal collision cut must be anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CutAnchor {
    West,
    East,
}

impl CutAnchor {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

/// One ordered cut obligation on the mission spine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MissionCut {
    pub order: u16,
    pub node_id: u16,
    pub spine_index: u16,
    pub anchor: CutAnchor,
}

/// One retained-spine fork and its ordered alternative branch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MissionFork {
    pub from: u16,
    pub to: u16,
    pub branch_nodes: Vec<u16>,
}

/// One dungeon-facing port attachment.  Socket offsets belong to embedding;
/// this phase fixes only graph ownership and boundary orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MissionPort {
    pub ordinal: u16,
    pub node_id: u16,
    pub side: BoundarySide,
}

/// Exact rewrite log.  This records how the graph was built, not merely the
/// graph that happened to result.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MissionRewrite {
    SubdivideEdge {
        removed_from: u16,
        removed_to: u16,
        inserted_node: u16,
    },
    InsertRouteCut {
        node_id: u16,
        spine_index: u16,
        order: u16,
        anchor: CutAnchor,
    },
    InsertForkRejoin {
        from: u16,
        to: u16,
        branch_nodes: Vec<u16>,
    },
    AttachPort {
        node_id: u16,
        ordinal: u16,
        side: BoundarySide,
    },
    MarkPickup {
        node_id: u16,
    },
}

/// Complete graph produced before any support coordinate is selected.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MissionPlan {
    pub nodes: Vec<MissionNode>,
    pub edges: Vec<MissionEdge>,
    /// Source-to-sink critical route in traversal order.
    pub spine: Vec<u16>,
    pub cuts: Vec<MissionCut>,
    pub forks: Vec<MissionFork>,
    pub ports: Vec<MissionPort>,
    pub pickup_node_id: u16,
}

/// Auditable counts and signatures for the derivation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MissionProvenance {
    pub derivation_version: u32,
    pub initial_node_count: u16,
    pub initial_edge_count: u16,
    pub subdivision_rewrites: u16,
    pub cut_rewrites: u16,
    pub fork_rewrites: u16,
    pub port_rewrites: u16,
    pub pickup_rewrites: u16,
    /// Canonical final graph signature.  It excludes node derivation IDs,
    /// seed, loadout, intent, embedding attempt, and all coordinates.
    pub topology_signature: u64,
    /// Exact ordered rewrite-history signature.  It also excludes geometry.
    pub derivation_signature: u64,
    pub rewrites: Vec<MissionRewrite>,
}

/// A coordinate-free mission plus its exact request and derivation evidence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DerivedMission {
    pub key: CompositionalRouteCutKey,
    pub plan: MissionPlan,
    pub provenance: MissionProvenance,
}

impl DerivedMission {
    #[must_use]
    pub const fn topology_signature(&self) -> u64 {
        self.provenance.topology_signature
    }

    #[must_use]
    pub const fn derivation_signature(&self) -> u64 {
        self.provenance.derivation_signature
    }
}

/// Exact one-to-one ownership link between coordinate-free and embedded
/// route nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MissionRouteNodeMapping {
    pub mission_node_id: u16,
    pub route_node_id: u16,
}

/// Physical evidence for one route-cut rewrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RouteCutRealization {
    pub mission_node_id: u16,
    pub route_node_id: u16,
    pub order: u16,
    pub anchor: CutAnchor,
    pub row: u16,
    pub shelf_start_x: u16,
    pub shelf_end_x: u16,
    pub opening_start_x: u16,
    pub opening_end_x: u16,
    pub predecessor_route_node_id: u16,
    pub successor_route_node_id: u16,
}

/// Constructive facts about the exact embedding.  These are not solver
/// difficulty or ability-requirement claims.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalRouteCutEmbeddingSummary {
    pub generation_version: u32,
    pub embedding_attempt: u8,
    pub ascent_edges: u16,
    pub descent_edges: u16,
    pub level_edges: u16,
    pub horizontal_direction_reversals: u16,
    pub cut_realizations: Vec<RouteCutRealization>,
    pub socket_columns: Vec<u16>,
    /// This v2 embedding deliberately authors only baseline-capable edges.
    pub claims_wall_jump_requirement: bool,
    /// This v2 embedding deliberately authors only baseline-capable edges.
    pub claims_dash_requirement: bool,
}

/// One structurally valid terrain-only candidate.  Gameplay reachability is
/// intentionally not implied; authoritative solver evidence is a later gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalRouteCutCandidate {
    pub key: CompositionalRouteCutKey,
    pub mission: DerivedMission,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
    pub mission_route_nodes: Vec<MissionRouteNodeMapping>,
    pub embedding: CompositionalRouteCutEmbeddingSummary,
}

/// Bounded placement phase which exhausted its deterministic candidate set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SupportConstraintPhase {
    Spine,
    Fork,
    FinalRouteContract,
    RasterizedRouteContract,
}

impl SupportConstraintPhase {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Spine => "spine",
            Self::Fork => "fork",
            Self::FinalRouteContract => "final-route-contract",
            Self::RasterizedRouteContract => "rasterized-route-contract",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositionalRouteCutGenerationFailure {
    Mission(MissionDerivationFailure),
    RhythmExhausted,
    MissingMissionNode {
        node_id: u16,
    },
    SupportConstraintExhausted {
        phase: SupportConstraintPhase,
        explored: u32,
    },
    ForkEmbeddingExhausted {
        from: u16,
        to: u16,
    },
    PortContract(String),
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for CompositionalRouteCutGenerationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mission(error) => write!(formatter, "mission derivation failed: {error}"),
            Self::RhythmExhausted => write!(
                formatter,
                "no reversible rise/fall rhythm satisfied the exact mission constraints"
            ),
            Self::MissingMissionNode { node_id } => {
                write!(formatter, "mission node {node_id} has no embedded support")
            }
            Self::SupportConstraintExhausted { phase, explored } => write!(
                formatter,
                "{} support constraint search exhausted after {explored} candidates",
                phase.slug()
            ),
            Self::ForkEmbeddingExhausted { from, to } => write!(
                formatter,
                "fork {from}->{to} has no conservative collision-distinct embedding"
            ),
            Self::PortContract(detail) => write!(formatter, "port contract failed: {detail}"),
            Self::Room(error) => write!(formatter, "generated room was invalid: {error}"),
            Self::Door(error) => write!(formatter, "generated room doors were invalid: {error}"),
        }
    }
}

impl Error for CompositionalRouteCutGenerationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Mission(error) => Some(error),
            Self::Room(error) => Some(error),
            Self::Door(error) => Some(error),
            Self::RhythmExhausted
            | Self::MissingMissionNode { .. }
            | Self::SupportConstraintExhausted { .. }
            | Self::ForkEmbeddingExhausted { .. }
            | Self::PortContract(_) => None,
        }
    }
}

/// A generation failure bound to the exact key; callers may explicitly try a
/// different `embedding_attempt`, but this function never does so itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalRouteCutGenerationError {
    pub key: CompositionalRouteCutKey,
    pub cause: CompositionalRouteCutGenerationFailure,
}

impl fmt::Display for CompositionalRouteCutGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compositional route-cut room v{} {} {} attempt {} seed {:016x} failed: {}",
            COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            self.key.grammar.slug(),
            self.key.intent.slug(),
            self.key.embedding_attempt,
            self.key.source_seed,
            self.cause,
        )
    }
}

impl Error for CompositionalRouteCutGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionDerivationFailure {
    UnsupportedEmbeddingAttempt { maximum: u8, actual: u8 },
    CutPlacementExhausted { requested: u16, eligible: u16 },
    ForkPlacementExhausted { requested: u16, eligible: u16 },
    NoPickupNode,
    NoPortAttachmentNode,
}

impl fmt::Display for MissionDerivationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEmbeddingAttempt { maximum, actual } => write!(
                formatter,
                "embedding attempt {actual} exceeds the frozen maximum {maximum}"
            ),
            Self::CutPlacementExhausted {
                requested,
                eligible,
            } => write!(
                formatter,
                "requested {requested} separated cuts but only {eligible} placements exist"
            ),
            Self::ForkPlacementExhausted {
                requested,
                eligible,
            } => write!(
                formatter,
                "requested {requested} distinct fork spans but only {eligible} exist"
            ),
            Self::NoPickupNode => write!(formatter, "no non-port, non-cut pickup node remained"),
            Self::NoPortAttachmentNode => {
                write!(
                    formatter,
                    "no non-cut spine node remained for an extra port"
                )
            }
        }
    }
}

impl Error for MissionDerivationFailure {}

/// A derivation failure bound to the exact requested key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionDerivationError {
    pub key: CompositionalRouteCutKey,
    pub cause: MissionDerivationFailure,
}

impl fmt::Display for MissionDerivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compositional route-cut mission v{} {} {} attempt {} seed {:016x} failed: {}",
            COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            self.key.grammar.slug(),
            self.key.intent.slug(),
            self.key.embedding_attempt,
            self.key.source_seed,
            self.cause,
        )
    }
}

impl Error for MissionDerivationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Derive one mission without choosing any room coordinate or tile.
pub fn derive_compositional_route_cut_mission(
    key: CompositionalRouteCutKey,
) -> Result<DerivedMission, MissionDerivationError> {
    derive_mission(key).map_err(|cause| MissionDerivationError { key, cause })
}

/// Embed one exact key without changing its attempt or selecting a fallback.
pub fn generate_compositional_route_cut(
    key: CompositionalRouteCutKey,
) -> Result<CompositionalRouteCutCandidate, CompositionalRouteCutGenerationError> {
    embed_mission(key).map_err(|cause| CompositionalRouteCutGenerationError { key, cause })
}

fn derive_mission(
    key: CompositionalRouteCutKey,
) -> Result<DerivedMission, MissionDerivationFailure> {
    if key.embedding_attempt > COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT {
        return Err(MissionDerivationFailure::UnsupportedEmbeddingAttempt {
            maximum: COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT,
            actual: key.embedding_attempt,
        });
    }

    let loadout_salt = u64::from(key.construction_abilities.wall_jump)
        | (u64::from(key.construction_abilities.dash) << 1);
    let intent_salt = match key.intent {
        ChallengeIntent::Gentle => 0_u64,
        ChallengeIntent::Standard => 1,
        ChallengeIntent::Technical => 2,
    };
    let mut rng = StableRng::new(
        key.source_seed
            ^ loadout_salt.wrapping_mul(0xa076_1d64_78bd_642f)
            ^ intent_salt.wrapping_mul(0xe703_7ed1_a0b4_28db),
        DERIVATION_RNG_STREAM,
    );

    let subdivision_count = match key.intent {
        ChallengeIntent::Gentle => 9 + rng.below(3),
        ChallengeIntent::Standard => 10 + rng.below(3),
        ChallengeIntent::Technical => 11 + rng.below(3),
    };
    let requested_cuts = match key.intent {
        ChallengeIntent::Gentle => rng.below(3),
        ChallengeIntent::Standard | ChallengeIntent::Technical => rng.below(4),
    };
    let requested_forks = rng.below(3);
    let requested_ports = 2 + rng.below(3);

    let mut nodes = vec![
        MissionNode {
            id: 0,
            kind: MissionNodeKind::Port,
        },
        MissionNode {
            id: 1,
            kind: MissionNodeKind::Port,
        },
    ];
    let mut spine = vec![0_u16, 1_u16];
    let mut edges = vec![MissionEdge {
        from: 0,
        to: 1,
        kind: MissionEdgeKind::Spine,
        critical: true,
    }];
    let mut rewrites = Vec::new();

    // Repeated edge subdivision is the primary construction operation.  A
    // random live edge is replaced each time; the final order is not a
    // prewritten sequence of room slots.
    for _ in 0..subdivision_count {
        let edge_index = usize::from(
            rng.below(
                (spine.len() - 1)
                    .try_into()
                    .expect("the small mission spine fits u16"),
            ),
        );
        let removed_from = spine[edge_index];
        let removed_to = spine[edge_index + 1];
        let inserted_node = nodes.len().try_into().expect("mission graph fits u16");
        nodes.push(MissionNode {
            id: inserted_node,
            kind: MissionNodeKind::Transit,
        });
        spine.insert(edge_index + 1, inserted_node);
        let existing = edges
            .iter()
            .position(|edge| {
                edge.kind == MissionEdgeKind::Spine
                    && edge.from == removed_from
                    && edge.to == removed_to
            })
            .expect("every live spine edge is present");
        edges.remove(existing);
        edges.push(MissionEdge {
            from: removed_from,
            to: inserted_node,
            kind: MissionEdgeKind::Spine,
            critical: true,
        });
        edges.push(MissionEdge {
            from: inserted_node,
            to: removed_to,
            kind: MissionEdgeKind::Spine,
            critical: true,
        });
        rewrites.push(MissionRewrite::SubdivideEdge {
            removed_from,
            removed_to,
            inserted_node,
        });
    }

    let cut_placements = separated_cut_placements(spine.len(), requested_cuts);
    if cut_placements.is_empty() {
        return Err(MissionDerivationFailure::CutPlacementExhausted {
            requested: requested_cuts,
            eligible: 0,
        });
    }
    let selected_cut_indices = cut_placements[usize::from(
        rng.below(
            cut_placements
                .len()
                .try_into()
                .expect("the cut placement set fits u16"),
        ),
    )]
    .clone();
    let mut anchor = if rng.coin() {
        CutAnchor::West
    } else {
        CutAnchor::East
    };
    let mut cuts = Vec::with_capacity(selected_cut_indices.len());
    for (order, spine_index) in selected_cut_indices.into_iter().enumerate() {
        let node_id = spine[spine_index];
        nodes[usize::from(node_id)].kind = MissionNodeKind::Cut;
        let cut = MissionCut {
            order: order.try_into().expect("at most three cuts fit u16"),
            node_id,
            spine_index: spine_index
                .try_into()
                .expect("the small mission spine fits u16"),
            anchor,
        };
        rewrites.push(MissionRewrite::InsertRouteCut {
            node_id: cut.node_id,
            spine_index: cut.spine_index,
            order: cut.order,
            anchor: cut.anchor,
        });
        cuts.push(cut);
        anchor = anchor.opposite();
    }

    let mut fork_candidates = fork_spans(&spine, &cuts);
    let requested_forks_usize = usize::from(requested_forks);
    if fork_candidates.len() < requested_forks_usize {
        return Err(MissionDerivationFailure::ForkPlacementExhausted {
            requested: requested_forks,
            eligible: fork_candidates.len().try_into().unwrap_or(u16::MAX),
        });
    }
    shuffle(&mut fork_candidates, &mut rng);
    let mut forks = Vec::with_capacity(requested_forks_usize);
    for (from_index, to_index) in fork_candidates.into_iter().take(requested_forks_usize) {
        let from = spine[from_index];
        let to = spine[to_index];
        // The alternative has at least one landing per retained spine edge.
        // A conservative embedding can therefore realize the same net rise
        // without inventing an ability verb merely to bridge a long fork.
        let branch_count = u16::try_from(to_index - from_index)
            .expect("the short mission span fits u16")
            + rng.below(2);
        let mut branch_nodes = Vec::with_capacity(usize::from(branch_count));
        let mut previous = from;
        for _ in 0..branch_count {
            let node_id = nodes.len().try_into().expect("mission graph fits u16");
            nodes.push(MissionNode {
                id: node_id,
                kind: MissionNodeKind::ForkBranch,
            });
            edges.push(MissionEdge {
                from: previous,
                to: node_id,
                kind: MissionEdgeKind::ForkBranch,
                critical: false,
            });
            branch_nodes.push(node_id);
            previous = node_id;
        }
        edges.push(MissionEdge {
            from: previous,
            to,
            kind: MissionEdgeKind::ForkBranch,
            critical: false,
        });
        mark_junction(&mut nodes, from);
        mark_junction(&mut nodes, to);
        rewrites.push(MissionRewrite::InsertForkRejoin {
            from,
            to,
            branch_nodes: branch_nodes.clone(),
        });
        forks.push(MissionFork {
            from,
            to,
            branch_nodes,
        });
    }

    let starting_side = plan_starting_side(&cuts, &mut rng);
    let mut ports = vec![
        MissionPort {
            ordinal: 0,
            node_id: spine[0],
            side: starting_side,
        },
        MissionPort {
            ordinal: 1,
            node_id: *spine.last().expect("mission spine has a sink"),
            side: BoundarySide::Ceiling,
        },
    ];
    for port in &ports {
        rewrites.push(MissionRewrite::AttachPort {
            node_id: port.node_id,
            ordinal: port.ordinal,
            side: port.side,
        });
    }
    let mut extra_port_candidates = spine[3..spine.len() - 1]
        .iter()
        .copied()
        .filter(|&node_id| nodes[usize::from(node_id)].kind != MissionNodeKind::Cut)
        .collect::<Vec<_>>();
    shuffle(&mut extra_port_candidates, &mut rng);
    for ordinal in 2..requested_ports {
        // The floor connector owns the first interior spine node.  Its later
        // shaft can therefore terminate above every authored cut instead of
        // punching an unmodelled shortcut through the route network.
        let selected = if ordinal == 2 {
            Some(spine[1])
        } else {
            extra_port_candidates.pop()
        };
        let Some(node_id) = selected else {
            return Err(MissionDerivationFailure::NoPortAttachmentNode);
        };
        nodes[usize::from(node_id)].kind = MissionNodeKind::Port;
        let side = if ordinal == 2 {
            BoundarySide::Floor
        } else {
            let spine_index = spine
                .iter()
                .position(|&candidate| candidate == node_id)
                .expect("extra ports are selected from the spine");
            let default = match starting_side {
                BoundarySide::Right => CutAnchor::East,
                BoundarySide::Left | BoundarySide::Ceiling | BoundarySide::Floor => CutAnchor::West,
            };
            match logical_side_from_cuts(&cuts, spine_index, default) {
                CutAnchor::West => BoundarySide::Left,
                CutAnchor::East => BoundarySide::Right,
            }
        };
        let port = MissionPort {
            ordinal,
            node_id,
            side,
        };
        rewrites.push(MissionRewrite::AttachPort {
            node_id,
            ordinal,
            side,
        });
        ports.push(port);
    }

    let mut pickup_candidates = nodes
        .iter()
        .filter(|node| {
            matches!(
                node.kind,
                MissionNodeKind::Transit | MissionNodeKind::Junction | MissionNodeKind::ForkBranch
            )
        })
        .map(|node| node.id)
        .collect::<Vec<_>>();
    if pickup_candidates.is_empty() {
        return Err(MissionDerivationFailure::NoPickupNode);
    }
    shuffle(&mut pickup_candidates, &mut rng);
    let pickup_node_id = pickup_candidates[0];
    nodes[usize::from(pickup_node_id)].kind = MissionNodeKind::Pickup;
    rewrites.push(MissionRewrite::MarkPickup {
        node_id: pickup_node_id,
    });

    edges.sort_unstable_by_key(|edge| (edge.from, edge.to, edge.kind, edge.critical));
    let plan = MissionPlan {
        nodes,
        edges,
        spine,
        cuts,
        forks,
        ports,
        pickup_node_id,
    };
    let topology_signature = canonical_topology_signature(&plan);
    let derivation_signature = rewrite_signature(&rewrites);
    let provenance = MissionProvenance {
        derivation_version: COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
        initial_node_count: 2,
        initial_edge_count: 1,
        subdivision_rewrites: subdivision_count,
        cut_rewrites: requested_cuts,
        fork_rewrites: requested_forks,
        port_rewrites: requested_ports,
        pickup_rewrites: 1,
        topology_signature,
        derivation_signature,
        rewrites,
    };
    Ok(DerivedMission {
        key,
        plan,
        provenance,
    })
}

fn embed_mission(
    key: CompositionalRouteCutKey,
) -> Result<CompositionalRouteCutCandidate, CompositionalRouteCutGenerationFailure> {
    let mission = derive_mission(key).map_err(CompositionalRouteCutGenerationFailure::Mission)?;
    let attempt_salt = u64::from(key.embedding_attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut rng = StableRng::new(key.source_seed ^ attempt_salt, EMBEDDING_RNG_STREAM);
    let spine_rows = embed_spine_rows(&mission.plan, &mut rng)?;
    let (mut supports, cut_realizations) =
        embed_spine_supports(&mission.plan, &spine_rows, &mut rng)?;
    embed_fork_supports(&mission.plan, &mut supports, &mut rng)?;
    validate_support_transition_contract(&mission.plan, &supports)?;

    let mut route_plan = RoutePlan::default();
    let mut mission_route_nodes = Vec::with_capacity(mission.plan.nodes.len());
    let mut route_by_mission = vec![u16::MAX; mission.plan.nodes.len()];
    for mission_node in &mission.plan.nodes {
        let support = supports
            .get(usize::from(mission_node.id))
            .and_then(|support| *support)
            .ok_or(CompositionalRouteCutGenerationFailure::MissingMissionNode {
                node_id: mission_node.id,
            })?;
        let route_node_id = add_route_node(&mut route_plan, route_role(mission_node.kind), support);
        route_by_mission[usize::from(mission_node.id)] = route_node_id;
        mission_route_nodes.push(MissionRouteNodeMapping {
            mission_node_id: mission_node.id,
            route_node_id,
        });
    }
    for edge in &mission.plan.edges {
        let from = route_by_mission[usize::from(edge.from)];
        let to = route_by_mission[usize::from(edge.to)];
        let from_support = route_plan.nodes[usize::from(from)].support;
        let to_support = route_plan.nodes[usize::from(to)].support;
        let verb = conservative_baseline_transition(from_support, to_support).ok_or(
            CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                phase: SupportConstraintPhase::FinalRouteContract,
                explored: 0,
            },
        )?;
        add_route_edge(&mut route_plan, from, to, verb, edge.critical);
    }

    let route_summary = route_plan.summary();
    let socket_columns = socket_columns(&mission.plan, &supports);
    let boundary_ports =
        build_boundary_ports(&mission.plan, &route_by_mission, &supports, &socket_columns)?;
    validate_embedded_port_contract(&route_plan, &boundary_ports)?;

    let mut draft = RoomDraft::new();
    for node in &route_plan.nodes {
        if node.support.row != FLOOR_ROW {
            draft.platform(node.support);
        }
    }
    let pickup_route_node = route_by_mission[usize::from(mission.plan.pickup_node_id)];
    draft.pickup_above(
        "route-cut-cache",
        route_plan.nodes[usize::from(pickup_route_node)].support,
    );
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-compositional-route-cut-v{}-{}-{}-a{:02}-{:016x}",
        COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
        key.grammar.slug(),
        key.intent.slug(),
        key.embedding_attempt,
        key.source_seed,
    );
    let name = format!(
        "Experimental compositional route cut v{} {} {} attempt {} {:016x}",
        COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
        key.grammar.slug(),
        key.intent.slug(),
        key.embedding_attempt,
        key.source_seed,
    );
    let doors = boundary_ports
        .iter()
        .map(|port| port.door.clone())
        .collect();
    let spawn = safe_ground_spawn(&supports, &boundary_ports).ok_or_else(|| {
        CompositionalRouteCutGenerationFailure::PortContract(
            "no collision-free ground spawn remained".to_owned(),
        )
    })?;
    let room = draft
        .finish_without_exits(id, name, spawn)
        .map_err(CompositionalRouteCutGenerationFailure::Room)?
        .with_doors(doors)
        .map_err(CompositionalRouteCutGenerationFailure::Door)?;
    validate_rasterized_route_contract(&room, &route_plan)?;
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: 30_000 + COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            seed: key.source_seed,
            layout_family: LayoutFamily::TerracedAscent,
            ability_tier: AbilityTier::from_abilities(key.construction_abilities),
            intended_abilities: key.construction_abilities,
            stats,
        },
    };

    let (ascent_edges, descent_edges, level_edges) = rhythm_counts(&spine_rows);
    let horizontal_direction_reversals = horizontal_reversals(&mission.plan, &supports)
        .try_into()
        .unwrap_or(u16::MAX);
    let cut_realizations = cut_realizations
        .into_iter()
        .map(|mut realization| {
            realization.route_node_id = route_by_mission[usize::from(realization.mission_node_id)];
            realization.predecessor_route_node_id =
                route_by_mission[usize::from(realization.predecessor_route_node_id)];
            realization.successor_route_node_id =
                route_by_mission[usize::from(realization.successor_route_node_id)];
            realization
        })
        .collect();
    let embedding = CompositionalRouteCutEmbeddingSummary {
        generation_version: COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
        embedding_attempt: key.embedding_attempt,
        ascent_edges,
        descent_edges,
        level_edges,
        horizontal_direction_reversals,
        cut_realizations,
        socket_columns: mission
            .plan
            .ports
            .iter()
            .filter_map(|port| socket_columns.get(&port.ordinal).copied())
            .collect(),
        claims_wall_jump_requirement: false,
        claims_dash_requirement: false,
    };
    Ok(CompositionalRouteCutCandidate {
        key,
        mission,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
        mission_route_nodes,
        embedding,
    })
}

/// Separate ability-aware embedding entry point.  The baseline v2 mapping
/// above does not call this function and retains its frozen behavior.
pub(super) fn embed_ability_rewritten_mission(
    key: CompositionalAbilityGenerationKey,
    rewritten_mission: AbilityRewrittenMission,
) -> Result<CompositionalAbilityCandidate, CompositionalAbilityGenerationFailure> {
    let base_key = key.rewrite_key.base_key;
    let attempt_salt = u64::from(base_key.embedding_attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut rng = StableRng::new(
        base_key.source_seed ^ attempt_salt,
        EMBEDDING_RNG_STREAM ^ 0x4142_494c_4954_5931,
    );
    let spine_rows = embed_ability_spine_rows(&rewritten_mission, &mut rng)?;
    let (mut supports, cut_realizations) =
        embed_ability_spine_supports(&rewritten_mission, &spine_rows, &mut rng)?;
    embed_ability_fork_supports(&rewritten_mission, &mut supports, &mut rng)?;
    let mut gate_realizations = ability_gate_realizations(&rewritten_mission, &supports)?;
    validate_ability_support_contract(&rewritten_mission, &supports, &gate_realizations)?;

    let mission = &rewritten_mission.base_mission;
    let mut route_plan = RoutePlan::default();
    let mut mission_route_nodes = Vec::with_capacity(mission.plan.nodes.len());
    let mut route_by_mission = vec![u16::MAX; mission.plan.nodes.len()];
    for mission_node in &mission.plan.nodes {
        let support = supports
            .get(usize::from(mission_node.id))
            .and_then(|support| *support)
            .ok_or({
                CompositionalAbilityGenerationFailure::BaselineEmbedding(
                    CompositionalRouteCutGenerationFailure::MissingMissionNode {
                        node_id: mission_node.id,
                    },
                )
            })?;
        let route_node_id = add_route_node(&mut route_plan, route_role(mission_node.kind), support);
        route_by_mission[usize::from(mission_node.id)] = route_node_id;
        mission_route_nodes.push(MissionRouteNodeMapping {
            mission_node_id: mission_node.id,
            route_node_id,
        });
    }
    for directed in &rewritten_mission.plan.edges {
        let edge = directed.edge;
        let from = route_by_mission[usize::from(edge.from)];
        let to = route_by_mission[usize::from(edge.to)];
        let from_support = route_plan.nodes[usize::from(from)].support;
        let to_support = route_plan.nodes[usize::from(to)].support;
        let verb = match directed.forward_requirement {
            DirectedTraversalRequirement::Baseline => {
                conservative_baseline_transition(from_support, to_support).ok_or(
                    CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                        phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                        explored: 0,
                    },
                )?
            }
            DirectedTraversalRequirement::Ability(GateAbility::WallJump) => RouteVerb::WallClimb,
            DirectedTraversalRequirement::Ability(GateAbility::Dash) => RouteVerb::DashUp,
        };
        add_route_edge(&mut route_plan, from, to, verb, edge.critical);
    }

    let route_summary = route_plan.summary();
    let socket_columns = socket_columns(&mission.plan, &supports);
    let boundary_ports =
        build_boundary_ports(&mission.plan, &route_by_mission, &supports, &socket_columns)
            .map_err(CompositionalAbilityGenerationFailure::BaselineEmbedding)?;
    validate_embedded_port_contract(&route_plan, &boundary_ports)
        .map_err(CompositionalAbilityGenerationFailure::BaselineEmbedding)?;
    validate_gate_boundary_reservations(&gate_realizations, &boundary_ports)?;

    let mut draft = RoomDraft::new();
    for node in &route_plan.nodes {
        if node.support.row != FLOOR_ROW {
            draft.platform(node.support);
        }
    }
    rasterize_gate_reservations(&mut draft, &gate_realizations);
    let pickup_route_node = route_by_mission[usize::from(mission.plan.pickup_node_id)];
    draft.pickup_above(
        "ability-route-cut-cache",
        route_plan.nodes[usize::from(pickup_route_node)].support,
    );
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-compositional-ability-v{}-{}-{}-e{:02}-r{:03}-{:016x}",
        COMPOSITIONAL_ABILITY_GENERATION_VERSION,
        key.rewrite_key.profile.slug(),
        base_key.intent.slug(),
        base_key.embedding_attempt,
        key.rewrite_key.rewrite_attempt,
        base_key.source_seed,
    );
    let name = format!(
        "Experimental compositional ability v{} {} {} embedding {} rewrite {} {:016x}",
        COMPOSITIONAL_ABILITY_GENERATION_VERSION,
        key.rewrite_key.profile.slug(),
        base_key.intent.slug(),
        base_key.embedding_attempt,
        key.rewrite_key.rewrite_attempt,
        base_key.source_seed,
    );
    let doors = boundary_ports
        .iter()
        .map(|port| port.door.clone())
        .collect();
    let spawn =
        safe_ground_spawn_avoiding_gate_tiles(&supports, &boundary_ports, &gate_realizations)
            .ok_or_else(|| {
                CompositionalAbilityGenerationFailure::BaselineEmbedding(
                    CompositionalRouteCutGenerationFailure::PortContract(
                        "no collision-free ground spawn outside gate reservations remained"
                            .to_owned(),
                    ),
                )
            })?;
    let room = draft
        .finish_without_exits(id, name, spawn)
        .map_err(|error| {
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::Room(error),
            )
        })?
        .with_doors(doors)
        .map_err(|error| {
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::Door(error),
            )
        })?;
    validate_rasterized_ability_contract(&room, &route_plan, &gate_realizations)?;
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: 31_000 + COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            seed: base_key.source_seed,
            layout_family: LayoutFamily::TerracedAscent,
            ability_tier: AbilityTier::from_abilities(key.rewrite_key.profile.abilities()),
            intended_abilities: key.rewrite_key.profile.abilities(),
            stats,
        },
    };

    for realization in &mut gate_realizations {
        realization.from_route_node_id =
            route_by_mission[usize::from(realization.gate.ascent_from)];
        realization.to_route_node_id = route_by_mission[usize::from(realization.gate.ascent_to)];
    }
    let (ascent_edges, descent_edges, level_edges) = rhythm_counts(&spine_rows);
    let horizontal_direction_reversals = horizontal_reversals(&mission.plan, &supports)
        .try_into()
        .unwrap_or(u16::MAX);
    let cut_realizations = cut_realizations
        .into_iter()
        .map(|mut realization| {
            realization.route_node_id = route_by_mission[usize::from(realization.mission_node_id)];
            realization.predecessor_route_node_id =
                route_by_mission[usize::from(realization.predecessor_route_node_id)];
            realization.successor_route_node_id =
                route_by_mission[usize::from(realization.successor_route_node_id)];
            realization
        })
        .collect();
    let embedding = CompositionalAbilityEmbeddingSummary {
        generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
        graph_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
        embedding_attempt: base_key.embedding_attempt,
        rewrite_attempt: key.rewrite_key.rewrite_attempt,
        ascent_edges,
        descent_edges,
        level_edges,
        horizontal_direction_reversals,
        cut_realizations,
        socket_columns: mission
            .plan
            .ports
            .iter()
            .filter_map(|port| socket_columns.get(&port.ordinal).copied())
            .collect(),
        gate_realizations,
    };
    Ok(CompositionalAbilityCandidate {
        key,
        rewritten_mission,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
        mission_route_nodes,
        embedding,
        evidence_state:
            super::ability_edge_rewrite::AbilityGateEmbeddingState::geometry_embedded_replay_pending(
            ),
    })
}

/// Isolated v4 entry point. It never falls back to the frozen physical-v2
/// mapper and remains outside every public candidate-selection path.
pub(super) fn embed_wall_chimney_v4(
    key: super::wall_chimney_v4::CompositionalWallChimneyV4GenerationKey,
    rewritten_mission: AbilityRewrittenMission,
) -> Result<
    super::wall_chimney_v4::CompositionalWallChimneyV4Candidate,
    CompositionalAbilityGenerationFailure,
> {
    wall_chimney_v4_mapping::embed(key, rewritten_mission)
}

fn embed_ability_spine_rows(
    rewritten: &AbilityRewrittenMission,
    rng: &mut StableRng,
) -> Result<Vec<u16>, CompositionalAbilityGenerationFailure> {
    let plan = &rewritten.base_mission.plan;
    let transition_count = plan.spine.len() - 1;
    let constrained_edges = plan
        .cuts
        .iter()
        .flat_map(|cut| {
            let index = usize::from(cut.spine_index);
            [index - 1, index]
        })
        .collect::<Vec<_>>();
    let mut rows = vec![FLOOR_ROW, FLOOR_ROW];
    let mut exhausted = HashSet::new();
    let mut explored = 0_u32;
    if complete_ability_rhythm(
        rewritten,
        1,
        transition_count,
        FLOOR_ROW,
        &constrained_edges,
        rng,
        &mut exhausted,
        &mut explored,
        &mut rows,
    ) {
        return Ok(rows);
    }
    Err(
        CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
            phase: CompositionalAbilityEmbeddingPhase::Rhythm,
            explored,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn complete_ability_rhythm(
    rewritten: &AbilityRewrittenMission,
    edge_index: usize,
    transition_count: usize,
    current_row: u16,
    constrained_edges: &[usize],
    rng: &mut StableRng,
    exhausted: &mut HashSet<(usize, u16)>,
    explored: &mut u32,
    rows: &mut Vec<u16>,
) -> bool {
    if edge_index == transition_count {
        return current_row == 3 && ability_spine_rows_respect_gate_isolation(rewritten, rows);
    }
    if exhausted.contains(&(edge_index, current_row)) {
        return false;
    }
    let mut steps = if let Some(gate) = gate_for_spine_edge(rewritten, edge_index) {
        vec![
            i16::try_from(physical_gate_rise_rows(
                rewritten.key.profile,
                gate.required_ability,
            ))
            .expect("physical gate rise fits i16"),
        ]
    } else if edge_index == 1
        || edge_index + 1 == transition_count
        || constrained_edges.contains(&edge_index)
    {
        vec![1_i16, 2]
    } else {
        vec![2_i16, 1, 0, -1]
    };
    shuffle(&mut steps, rng);
    for step in steps {
        *explored = explored.saturating_add(1);
        let next_row_i32 = i32::from(current_row) - i32::from(step);
        let Ok(next_row) = u16::try_from(next_row_i32) else {
            continue;
        };
        if !(3..FLOOR_ROW).contains(&next_row) {
            continue;
        }
        rows.push(next_row);
        if complete_ability_rhythm(
            rewritten,
            edge_index + 1,
            transition_count,
            next_row,
            constrained_edges,
            rng,
            exhausted,
            explored,
            rows,
        ) {
            return true;
        }
        rows.pop();
    }
    exhausted.insert((edge_index, current_row));
    false
}

fn ability_spine_rows_respect_gate_isolation(
    rewritten: &AbilityRewrittenMission,
    rows: &[u16],
) -> bool {
    let plan = &rewritten.base_mission.plan;
    rewritten.plan.gates.iter().all(|gate| {
        let lower_index = usize::from(gate.spine_edge_index);
        let upper_index = lower_index + 1;
        let lower_row = rows[lower_index];
        let upper_row = rows[upper_index];
        rows.iter().enumerate().all(|(index, &row)| {
            index == lower_index || index == upper_index || !(upper_row < row && row < lower_row)
        }) && plan.spine[lower_index] == gate.ascent_from
            && plan.spine[upper_index] == gate.ascent_to
    })
}

fn physical_gate_rise_rows(
    profile: super::ability_edge_rewrite::CompositionalAbilityGateProfile,
    ability: GateAbility,
) -> u16 {
    match (profile, ability) {
        (
            super::ability_edge_rewrite::CompositionalAbilityGateProfile::Both,
            GateAbility::WallJump,
        ) => 8,
        (_, GateAbility::WallJump) => 6,
        (_, GateAbility::Dash) => 5,
    }
}

fn gate_for_spine_edge(
    rewritten: &AbilityRewrittenMission,
    spine_edge_index: usize,
) -> Option<&DirectedAbilityGate> {
    rewritten
        .plan
        .gates
        .iter()
        .find(|gate| usize::from(gate.spine_edge_index) == spine_edge_index)
}

fn gate_for_mission_edge(
    rewritten: &AbilityRewrittenMission,
    mission_edge_index: usize,
) -> Option<&DirectedAbilityGate> {
    rewritten
        .plan
        .gates
        .iter()
        .find(|gate| usize::from(gate.mission_edge_index) == mission_edge_index)
}

fn embed_ability_spine_supports(
    rewritten: &AbilityRewrittenMission,
    rows: &[u16],
    rng: &mut StableRng,
) -> Result<
    (Vec<Option<SupportSpec>>, Vec<RouteCutRealization>),
    CompositionalAbilityGenerationFailure,
> {
    let plan = &rewritten.base_mission.plan;
    let mut supports = vec![None; plan.nodes.len()];
    let domains = spine_support_domains(plan, rows, rng);
    let mut explored = 0;
    if !complete_ability_spine_supports(
        rewritten,
        &domains,
        0,
        0,
        false,
        &mut supports,
        &mut explored,
    ) {
        return Err(
            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                phase: CompositionalAbilityEmbeddingPhase::Spine,
                explored,
            },
        );
    }
    debug_assert!(cut_obligations_hold(plan, &supports));
    let cuts = realized_cuts(plan, &supports)
        .map_err(CompositionalAbilityGenerationFailure::BaselineEmbedding)?;
    Ok((supports, cuts))
}

#[allow(clippy::too_many_arguments)]
fn complete_ability_spine_supports(
    rewritten: &AbilityRewrittenMission,
    domains: &[Vec<SupportSpec>],
    index: usize,
    previous_direction: i8,
    has_reversal: bool,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
) -> bool {
    let plan = &rewritten.base_mission.plan;
    if index == plan.spine.len() {
        return has_reversal
            && ability_gate_realizations(rewritten, supports).is_ok()
            && assigned_ability_route_edges_are_clear(rewritten, supports);
    }
    let node_id = plan.spine[index];
    for &candidate in &domains[index] {
        if *explored >= SPINE_CONSTRAINT_SEARCH_LIMIT {
            return false;
        }
        *explored += 1;
        let (next_direction, next_has_reversal) = if index > 0 {
            let Some(previous) = supports[usize::from(plan.spine[index - 1])] else {
                continue;
            };
            let transition_ok = gate_for_spine_edge(rewritten, index - 1).map_or_else(
                || reversible_baseline_transition(previous, candidate),
                |gate| {
                    gate_realization_for_supports(rewritten, gate, previous, candidate, supports)
                        .is_ok()
                },
            );
            if !transition_ok {
                continue;
            }
            let direction = match previous.center_x().cmp(&candidate.center_x()) {
                std::cmp::Ordering::Less => 1,
                std::cmp::Ordering::Greater => -1,
                std::cmp::Ordering::Equal => 0,
            };
            (
                if direction == 0 {
                    previous_direction
                } else {
                    direction
                },
                has_reversal
                    || (direction != 0
                        && previous_direction != 0
                        && direction != previous_direction),
            )
        } else {
            (previous_direction, has_reversal)
        };
        if floor_socket_overlaps_assigned_support(plan, node_id, candidate, supports) {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_ability_route_edges_are_clear(rewritten, supports)
            && assigned_gate_reservations_are_clear(rewritten, supports)
            && partial_cut_obligations_hold(plan, index, supports)
            && future_cut_obligations_remain_feasible(plan, domains, index, supports)
            && complete_ability_spine_supports(
                rewritten,
                domains,
                index + 1,
                next_direction,
                next_has_reversal,
                supports,
                explored,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    false
}

fn ability_gate_realizations(
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> Result<Vec<AbilityGateRealization>, CompositionalAbilityGenerationFailure> {
    let realizations = rewritten
        .plan
        .gates
        .iter()
        .map(|gate| {
            let lower = supports
                .get(usize::from(gate.ascent_from))
                .and_then(|support| *support)
                .ok_or_else(|| {
                    gate_contract_failure(gate, AbilityGateGeometryViolation::MissingGateEdge)
                })?;
            let upper = supports
                .get(usize::from(gate.ascent_to))
                .and_then(|support| *support)
                .ok_or_else(|| {
                    gate_contract_failure(gate, AbilityGateGeometryViolation::MissingGateEdge)
                })?;
            gate_realization_for_supports(rewritten, gate, lower, upper, supports)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, first) in realizations.iter().enumerate() {
        if realizations[index + 1..]
            .iter()
            .any(|second| gate_reservations_overlap(first, second))
        {
            return Err(gate_contract_failure(
                &first.gate,
                AbilityGateGeometryViolation::ReservedVolumeBlocked,
            ));
        }
    }
    Ok(realizations)
}

fn gate_realization_for_supports(
    rewritten: &AbilityRewrittenMission,
    gate: &DirectedAbilityGate,
    lower: SupportSpec,
    upper: SupportSpec,
    supports: &[Option<SupportSpec>],
) -> Result<AbilityGateRealization, CompositionalAbilityGenerationFailure> {
    if lower.row <= upper.row
        || lower.row - upper.row
            != physical_gate_rise_rows(rewritten.key.profile, gate.required_ability)
    {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::AscentEnvelope,
        ));
    }
    if lower.kind != SupportKind::OneWay || upper.kind != SupportKind::OneWay {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::EndpointMaterial,
        ));
    }
    if conservative_baseline_transition(upper, lower).is_none() {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::ReverseBaselineContract,
        ));
    }
    if gate.required_ability == GateAbility::WallJump
        && rewritten.base_mission.plan.cuts.iter().any(|cut| {
            supports[usize::from(cut.node_id)]
                .is_some_and(|support| (upper.row + 1..lower.row).contains(&support.row))
        })
    {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::CutRowIntersected,
        ));
    }
    let realization = match gate.required_ability {
        GateAbility::WallJump => wall_gate_realization(gate, lower, upper)?,
        GateAbility::Dash => dash_gate_realization(gate, lower, upper)?,
    };
    if !gate_isolation_avoids_supports(&realization, supports) {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::IntermediateSupportSurface,
        ));
    }
    if !gate_reservation_avoids_supports(&realization, supports) {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::ReservedVolumeBlocked,
        ));
    }
    Ok(realization)
}

fn gate_isolation_avoids_supports(
    realization: &AbilityGateRealization,
    supports: &[Option<SupportSpec>],
) -> bool {
    supports.iter().enumerate().all(|(node_id, support)| {
        if node_id == usize::from(realization.gate.ascent_from)
            || node_id == usize::from(realization.gate.ascent_to)
        {
            return true;
        }
        support.is_none_or(|support| {
            !(realization.upper_support.row < support.row
                && support.row < realization.lower_support.row)
        })
    })
}

fn dash_gate_realization(
    gate: &DirectedAbilityGate,
    lower: SupportSpec,
    upper: SupportSpec,
) -> Result<AbilityGateRealization, CompositionalAbilityGenerationFailure> {
    let overlap_start = lower.start_x.max(upper.start_x);
    let overlap_end = lower.end_x.min(upper.end_x);
    let width = gate.embedding_contract.minimum_clear_interior_width_tiles;
    if overlap_end.saturating_sub(overlap_start) < width {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::AscentEnvelope,
        ));
    }
    let corridor_start = overlap_start + (overlap_end - overlap_start - width) / 2;
    let corridor_end = corridor_start + width;
    let required_empty_tiles = ((upper.row + 1)..lower.row)
        .flat_map(|row| (corridor_start..corridor_end).map(move |x| AbilityGateTileCell { x, row }))
        .collect::<Vec<_>>();
    let lower_standing_bounds = standing_bounds_at_tile(lower, corridor_start + width / 2)
        .ok_or_else(|| gate_contract_failure(gate, AbilityGateGeometryViolation::AscentEnvelope))?;
    let upper_standing_bounds = standing_bounds_at_tile(upper, corridor_start + width / 2)
        .ok_or_else(|| gate_contract_failure(gate, AbilityGateGeometryViolation::AscentEnvelope))?;
    Ok(AbilityGateRealization {
        gate: gate.clone(),
        from_route_node_id: gate.ascent_from,
        to_route_node_id: gate.ascent_to,
        lower_support: lower,
        upper_support: upper,
        ascent_bounds: Rect::new(
            i32::from(corridor_start) * TILE_SIZE,
            i32::from(upper.row) * TILE_SIZE - PLAYER_HEIGHT,
            i32::from(width) * TILE_SIZE,
            i32::from(lower.row - upper.row) * TILE_SIZE + PLAYER_HEIGHT,
        ),
        lower_standing_bounds,
        upper_standing_bounds,
        required_solid_tiles: Vec::new(),
        required_empty_tiles,
    })
}

fn wall_gate_realization(
    gate: &DirectedAbilityGate,
    lower: SupportSpec,
    upper: SupportSpec,
) -> Result<AbilityGateRealization, CompositionalAbilityGenerationFailure> {
    let minimum_width = gate.embedding_contract.minimum_clear_interior_width_tiles;
    let maximum_width = minimum_width.saturating_add(2);
    let prefers_right = upper.center_x() >= lower.center_x();
    for lip_on_right in [prefers_right, !prefers_right] {
        for width in minimum_width..=maximum_width {
            if lower.width() < width {
                continue;
            }
            for shaft_start in lower.start_x..=lower.end_x - width {
                let shaft_end = shaft_start + width;
                let Some(left_wall_x) = shaft_start.checked_sub(1) else {
                    continue;
                };
                let right_wall_x = shaft_end;
                if left_wall_x == 0 || right_wall_x >= ROOM_WIDTH - 1 {
                    continue;
                }
                let lip_fits = if lip_on_right {
                    upper.start_x <= right_wall_x && upper.end_x >= right_wall_x.saturating_add(2)
                } else {
                    upper.start_x.saturating_add(2) <= left_wall_x && upper.end_x > left_wall_x
                };
                if !lip_fits {
                    continue;
                }
                let required_solid_tiles = [left_wall_x, right_wall_x]
                    .into_iter()
                    .flat_map(|x| {
                        ((upper.row + 1)..lower.row).map(move |row| AbilityGateTileCell { x, row })
                    })
                    .collect::<Vec<_>>();
                let required_empty_tiles = ((upper.row + 1)..lower.row)
                    .flat_map(|row| {
                        (shaft_start..shaft_end).map(move |x| AbilityGateTileCell { x, row })
                    })
                    .collect::<Vec<_>>();
                let lower_standing_bounds = standing_bounds_at_tile(lower, shaft_start + width / 2)
                    .ok_or_else(|| {
                        gate_contract_failure(gate, AbilityGateGeometryViolation::AscentEnvelope)
                    })?;
                let upper_standing_tile = if lip_on_right {
                    right_wall_x + 1
                } else {
                    left_wall_x - 1
                };
                let Some(upper_standing_bounds) =
                    standing_bounds_at_tile(upper, upper_standing_tile)
                else {
                    continue;
                };
                return Ok(AbilityGateRealization {
                    gate: gate.clone(),
                    from_route_node_id: gate.ascent_from,
                    to_route_node_id: gate.ascent_to,
                    lower_support: lower,
                    upper_support: upper,
                    ascent_bounds: Rect::new(
                        i32::from(shaft_start) * TILE_SIZE,
                        i32::from(upper.row) * TILE_SIZE - PLAYER_HEIGHT,
                        i32::from(width) * TILE_SIZE,
                        i32::from(lower.row - upper.row) * TILE_SIZE + PLAYER_HEIGHT,
                    ),
                    lower_standing_bounds,
                    upper_standing_bounds,
                    required_solid_tiles,
                    required_empty_tiles,
                });
            }
        }
    }
    Err(gate_contract_failure(
        gate,
        AbilityGateGeometryViolation::AscentEnvelope,
    ))
}

fn standing_bounds_at_tile(support: SupportSpec, tile_x: u16) -> Option<Rect> {
    let preferred_x = i32::from(tile_x) * TILE_SIZE - PLAYER_WIDTH / 2;
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    (minimum_x <= maximum_x).then(|| {
        Rect::new(
            preferred_x.clamp(minimum_x, maximum_x),
            i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT,
            PLAYER_WIDTH,
            PLAYER_HEIGHT,
        )
    })
}

fn gate_reservation_avoids_supports(
    realization: &AbilityGateRealization,
    supports: &[Option<SupportSpec>],
) -> bool {
    supports.iter().enumerate().all(|(node_id, support)| {
        if node_id == usize::from(realization.gate.ascent_from)
            || node_id == usize::from(realization.gate.ascent_to)
        {
            return true;
        }
        support.is_none_or(|support| {
            realization
                .required_solid_tiles
                .iter()
                .chain(&realization.required_empty_tiles)
                .all(|cell| {
                    cell.row != support.row || !(support.start_x..support.end_x).contains(&cell.x)
                })
        })
    })
}

fn assigned_gate_reservations_are_clear(
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> bool {
    let mut realizations = Vec::new();
    for gate in &rewritten.plan.gates {
        let Some(lower) = supports[usize::from(gate.ascent_from)] else {
            continue;
        };
        let Some(upper) = supports[usize::from(gate.ascent_to)] else {
            continue;
        };
        let Ok(realization) =
            gate_realization_for_supports(rewritten, gate, lower, upper, supports)
        else {
            return false;
        };
        realizations.push(realization);
    }
    gate_reservations_are_pairwise_disjoint(&realizations)
}

fn gate_reservations_are_pairwise_disjoint(realizations: &[AbilityGateRealization]) -> bool {
    realizations.iter().enumerate().all(|(index, first)| {
        realizations[index + 1..]
            .iter()
            .all(|second| !gate_reservations_overlap(first, second))
    })
}

fn gate_reservations_overlap(
    first: &AbilityGateRealization,
    second: &AbilityGateRealization,
) -> bool {
    first
        .required_solid_tiles
        .iter()
        .chain(&first.required_empty_tiles)
        .any(|cell| {
            second
                .required_solid_tiles
                .iter()
                .chain(&second.required_empty_tiles)
                .any(|other| other == cell)
        })
}

fn gate_contract_failure(
    gate: &DirectedAbilityGate,
    violation: AbilityGateGeometryViolation,
) -> CompositionalAbilityGenerationFailure {
    CompositionalAbilityGenerationFailure::GateContract {
        gate_ordinal: gate.ordinal,
        violation,
    }
}

fn assigned_ability_route_edges_are_clear(
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> bool {
    let assigned = supports.iter().flatten().copied().collect::<Vec<_>>();
    let realizations = rewritten
        .plan
        .gates
        .iter()
        .filter_map(|gate| {
            let lower = supports[usize::from(gate.ascent_from)]?;
            let upper = supports[usize::from(gate.ascent_to)]?;
            gate_realization_for_supports(rewritten, gate, lower, upper, supports).ok()
        })
        .collect::<Vec<_>>();
    rewritten
        .plan
        .edges
        .iter()
        .enumerate()
        .all(|(edge_index, directed)| {
            let edge = directed.edge;
            let Some(first) = supports[usize::from(edge.from)] else {
                return true;
            };
            let Some(second) = supports[usize::from(edge.to)] else {
                return true;
            };
            if let Some(gate) = gate_for_mission_edge(rewritten, edge_index) {
                return gate_realization_for_supports(rewritten, gate, first, second, supports)
                    .is_ok();
            }
            if !reversible_baseline_transition(first, second)
                || !support_transfer_headroom_is_clear(first, second, &assigned)
            {
                return false;
            }
            let Some(corridor) = ability_transfer_corridor(first, second, &assigned) else {
                return false;
            };
            realizations.iter().all(|realization| {
                realization.required_solid_tiles.iter().all(|cell| {
                    !corridor.intersects(Rect::new(
                        i32::from(cell.x) * TILE_SIZE,
                        i32::from(cell.row) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    ))
                })
            })
        })
}

fn ability_transfer_corridor(
    first: SupportSpec,
    second: SupportSpec,
    supports: &[SupportSpec],
) -> Option<Rect> {
    let first_positions = support_clear_standing_positions(first, supports);
    let second_positions = support_clear_standing_positions(second, supports);
    let (first_x, second_x) = first_positions
        .iter()
        .flat_map(|&first_x| {
            second_positions
                .iter()
                .map(move |&second_x| (first_x, second_x))
        })
        .min_by_key(|(first_x, second_x)| first_x.abs_diff(*second_x))?;
    let first_standing_y = i32::from(first.row) * TILE_SIZE - PLAYER_HEIGHT;
    let second_standing_y = i32::from(second.row) * TILE_SIZE - PLAYER_HEIGHT;
    let left = first_x.min(second_x);
    let right = (first_x + PLAYER_WIDTH).max(second_x + PLAYER_WIDTH);
    let top = first_standing_y.min(second_standing_y);
    let bottom = (i32::from(first.row) * TILE_SIZE).max(i32::from(second.row) * TILE_SIZE);
    Some(Rect::new(left, top, right - left, bottom - top))
}

fn embed_ability_fork_supports(
    rewritten: &AbilityRewrittenMission,
    supports: &mut [Option<SupportSpec>],
    rng: &mut StableRng,
) -> Result<(), CompositionalAbilityGenerationFailure> {
    let plan = &rewritten.base_mission.plan;
    let spine_supports = plan
        .spine
        .iter()
        .filter_map(|&node_id| supports[usize::from(node_id)])
        .collect::<Vec<_>>();
    for fork in &plan.forks {
        let from = supports[usize::from(fork.from)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.from },
            )
        })?;
        let to = supports[usize::from(fork.to)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.to },
            )
        })?;
        let domains = fork_support_domains(fork, from, to, rng);
        let mut explored = 0;
        if !complete_ability_fork_supports(
            rewritten,
            fork,
            &domains,
            &spine_supports,
            0,
            supports,
            &mut explored,
        ) {
            for &node_id in &fork.branch_nodes {
                supports[usize::from(node_id)] = None;
            }
            return Err(
                CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                    phase: CompositionalAbilityEmbeddingPhase::Fork,
                    explored,
                },
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn complete_ability_fork_supports(
    rewritten: &AbilityRewrittenMission,
    fork: &MissionFork,
    domains: &[Vec<SupportSpec>],
    spine_supports: &[SupportSpec],
    index: usize,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
) -> bool {
    let plan = &rewritten.base_mission.plan;
    let previous = if index == 0 {
        supports[usize::from(fork.from)].expect("fork source support exists")
    } else {
        supports[usize::from(fork.branch_nodes[index - 1])]
            .expect("previous branch support was assigned")
    };
    if index == fork.branch_nodes.len() {
        let target = supports[usize::from(fork.to)].expect("fork target support exists");
        return reversible_baseline_transition(previous, target)
            && floor_arrivals_remain_clear(plan, supports)
            && assigned_gate_reservations_are_clear(rewritten, supports)
            && assigned_ability_route_edges_are_clear(rewritten, supports);
    }
    let node_id = fork.branch_nodes[index];
    let remaining_edges = fork.branch_nodes.len() - index;
    let target = supports[usize::from(fork.to)].expect("fork target support exists");
    for &candidate in &domains[index] {
        if *explored >= FORK_CONSTRAINT_SEARCH_LIMIT {
            return false;
        }
        *explored += 1;
        if !reversible_baseline_transition(previous, candidate)
            || candidate.row.abs_diff(target.row)
                > u16::try_from(remaining_edges * 2).unwrap_or(u16::MAX)
            || !has_unique_tile(candidate, spine_supports)
        {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_gate_reservations_are_clear(rewritten, supports)
            && assigned_ability_route_edges_are_clear(rewritten, supports)
            && complete_ability_fork_supports(
                rewritten,
                fork,
                domains,
                spine_supports,
                index + 1,
                supports,
                explored,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    false
}

fn validate_ability_support_contract(
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
    realizations: &[AbilityGateRealization],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    if realizations.len() != rewritten.plan.gates.len()
        || !cut_obligations_hold(&rewritten.base_mission.plan, supports)
    {
        return Err(
            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                explored: 0,
            },
        );
    }
    for (edge_index, directed) in rewritten.plan.edges.iter().enumerate() {
        let edge = directed.edge;
        let from = supports[usize::from(edge.from)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.from },
            )
        })?;
        let to = supports[usize::from(edge.to)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.to },
            )
        })?;
        match directed.forward_requirement {
            DirectedTraversalRequirement::Baseline => {
                if !reversible_baseline_transition(from, to) {
                    return Err(
                        CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                            phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                            explored: u32::try_from(edge_index + 1).unwrap_or(u32::MAX),
                        },
                    );
                }
            }
            DirectedTraversalRequirement::Ability(_) => {
                let gate = gate_for_mission_edge(rewritten, edge_index).ok_or({
                    CompositionalAbilityGenerationFailure::GateContract {
                        gate_ordinal: 0,
                        violation: AbilityGateGeometryViolation::MissingGateEdge,
                    }
                })?;
                if conservative_baseline_transition(from, to).is_some()
                    || conservative_baseline_transition(to, from).is_none()
                    || !realizations
                        .iter()
                        .any(|realization| realization.gate.ordinal == gate.ordinal)
                {
                    return Err(gate_contract_failure(
                        gate,
                        AbilityGateGeometryViolation::ReverseBaselineContract,
                    ));
                }
            }
        }
    }
    if !assigned_ability_route_edges_are_clear(rewritten, supports) {
        return Err(
            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                explored: rewritten.plan.edges.len().try_into().unwrap_or(u32::MAX),
            },
        );
    }
    Ok(())
}

fn validate_gate_boundary_reservations(
    realizations: &[AbilityGateRealization],
    ports: &[BoundaryPort],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    for realization in realizations {
        let solid_hits_boundary = realization
            .required_solid_tiles
            .iter()
            .map(|cell| {
                Rect::new(
                    i32::from(cell.x) * TILE_SIZE,
                    i32::from(cell.row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                )
            })
            .any(|solid| {
                ports.iter().any(|port| {
                    solid.intersects(port.door.trigger_bounds)
                        || solid.intersects(Rect::new(
                            port.door.arrival.x,
                            port.door.arrival.y,
                            PLAYER_WIDTH,
                            PLAYER_HEIGHT,
                        ))
                })
            });
        let arrival_hits_ascent = ports.iter().any(|port| {
            realization.ascent_bounds.intersects(Rect::new(
                port.door.arrival.x,
                port.door.arrival.y,
                PLAYER_WIDTH,
                PLAYER_HEIGHT,
            ))
        });
        if solid_hits_boundary || arrival_hits_ascent {
            return Err(gate_contract_failure(
                &realization.gate,
                AbilityGateGeometryViolation::BoundaryArrivalBlocked,
            ));
        }
    }
    Ok(())
}

fn rasterize_gate_reservations(draft: &mut RoomDraft, realizations: &[AbilityGateRealization]) {
    for realization in realizations {
        for cell in &realization.required_solid_tiles {
            draft.solid_column(cell.x, cell.row, cell.row + 1);
        }
    }
}

fn safe_ground_spawn_avoiding_gate_tiles(
    supports: &[Option<SupportSpec>],
    ports: &[BoundaryPort],
    realizations: &[AbilityGateRealization],
) -> Option<Point> {
    let y = i32::from(FLOOR_ROW) * TILE_SIZE - PLAYER_HEIGHT;
    let minimum_x = TILE_SIZE + 2;
    let maximum_x = i32::from(ROOM_WIDTH - 1) * TILE_SIZE - PLAYER_WIDTH - 2;
    let preferred_x = i32::from(ROOM_WIDTH / 2) * TILE_SIZE - PLAYER_WIDTH / 2;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let player = Rect::new(x, y, PLAYER_WIDTH, PLAYER_HEIGHT);
            let clear_of_supports = supports.iter().flatten().all(|support| {
                support.row == FLOOR_ROW
                    || !player.intersects(Rect::new(
                        i32::from(support.start_x) * TILE_SIZE,
                        i32::from(support.row) * TILE_SIZE,
                        i32::from(support.width()) * TILE_SIZE,
                        TILE_SIZE,
                    ))
            });
            let over_solid_floor = ports.iter().all(|port| {
                port.door.side != BoundarySide::Floor
                    || x + PLAYER_WIDTH <= port.door.trigger_bounds.x
                    || x >= port.door.trigger_bounds.right()
            });
            let clear_of_gates = realizations.iter().all(|realization| {
                realization.required_solid_tiles.iter().all(|cell| {
                    !player.intersects(Rect::new(
                        i32::from(cell.x) * TILE_SIZE,
                        i32::from(cell.row) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    ))
                })
            });
            clear_of_supports && over_solid_floor && clear_of_gates
        })
        .min_by_key(|&x| x.abs_diff(preferred_x))
        .map(|x| Point::new(x, y))
}

fn validate_rasterized_ability_contract(
    room: &Room,
    plan: &RoutePlan,
    realizations: &[AbilityGateRealization],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    for (node_index, node) in plan.nodes.iter().enumerate() {
        let support_preserved = (node.support.start_x..node.support.end_x)
            .all(|x| room.tile(x, node.support.row) == Some(node.support.kind.tile()));
        if !support_preserved || clear_standing_positions(room, node.support).is_empty() {
            return Err(
                CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                    phase: CompositionalAbilityEmbeddingPhase::RasterizedGateContract,
                    explored: u32::try_from(node_index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    for realization in realizations {
        let solids_match = realization
            .required_solid_tiles
            .iter()
            .all(|cell| room.tile(cell.x, cell.row) == Some(Tile::Solid));
        let empty_matches = realization
            .required_empty_tiles
            .iter()
            .all(|cell| room.tile(cell.x, cell.row) == Some(Tile::Empty));
        if !solids_match
            || !empty_matches
            || !room_rect_is_clear(room, realization.lower_standing_bounds)
            || !room_rect_is_clear(room, realization.upper_standing_bounds)
        {
            return Err(gate_contract_failure(
                &realization.gate,
                AbilityGateGeometryViolation::RasterMismatch,
            ));
        }
    }
    for (edge_index, edge) in plan.edges.iter().enumerate() {
        if matches!(
            edge.verb,
            RouteVerb::WallClimb | RouteVerb::DashAcross | RouteVerb::DashUp
        ) {
            continue;
        }
        let first = plan.nodes[usize::from(edge.from)].support;
        let second = plan.nodes[usize::from(edge.to)].support;
        if !rasterized_transfer_headroom_is_clear(room, first, second) {
            return Err(
                CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                    phase: CompositionalAbilityEmbeddingPhase::RasterizedGateContract,
                    explored: u32::try_from(plan.nodes.len() + edge_index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    Ok(())
}

fn room_rect_is_clear(room: &Room, rect: Rect) -> bool {
    room.tiles().iter().enumerate().all(|(index, &tile)| {
        if tile != Tile::Solid && !tile.is_hazard() {
            return true;
        }
        let width = usize::from(room.width());
        let tile_x = u16::try_from(index % width).expect("room width fits u16");
        let tile_y = u16::try_from(index / width).expect("room height fits u16");
        !rect.intersects(room.tile_bounds(tile_x, tile_y))
    })
}

fn embed_spine_rows(
    plan: &MissionPlan,
    rng: &mut StableRng,
) -> Result<Vec<u16>, CompositionalRouteCutGenerationFailure> {
    let transition_count = plan.spine.len() - 1;
    let constrained_edges = plan
        .cuts
        .iter()
        .flat_map(|cut| {
            let index = usize::from(cut.spine_index);
            [index - 1, index]
        })
        .collect::<Vec<_>>();
    // The first rewrite remains on the boundary floor.  This gives the
    // optional floor socket a genuine route-owned landing without creating a
    // low ceiling over an arrival.  The next edge must ascend, after which a
    // bounded fall is required somewhere in the interior rhythm.
    let mut rows = vec![FLOOR_ROW, FLOOR_ROW];
    let mut memo = HashMap::new();
    if complete_rhythm(
        1,
        transition_count,
        FLOOR_ROW,
        FLOOR_ROW - 2,
        false,
        &constrained_edges,
        rng,
        &mut memo,
        &mut rows,
    ) {
        return Ok(rows);
    }
    Err(CompositionalRouteCutGenerationFailure::RhythmExhausted)
}

#[allow(clippy::too_many_arguments)]
fn complete_rhythm(
    edge_index: usize,
    transition_count: usize,
    current_row: u16,
    maximum_row: u16,
    has_descent: bool,
    constrained_edges: &[usize],
    rng: &mut StableRng,
    memo: &mut HashMap<(usize, u16, bool), bool>,
    rows: &mut Vec<u16>,
) -> bool {
    if edge_index == transition_count {
        return current_row == 3 && has_descent;
    }
    let mut steps = if edge_index == 1
        || edge_index + 1 == transition_count
        || constrained_edges.contains(&edge_index)
    {
        vec![1_i16, 2]
    } else {
        vec![2_i16, 1, 0, -1]
    };
    shuffle(&mut steps, rng);
    for step in steps {
        let next_row_i32 = i32::from(current_row) - i32::from(step);
        let Ok(next_row) = u16::try_from(next_row_i32) else {
            continue;
        };
        if !(3..=maximum_row).contains(&next_row) {
            continue;
        }
        let next_has_descent = has_descent || step < 0;
        let next_maximum_row = if edge_index == 1 {
            next_row.saturating_sub(1)
        } else {
            maximum_row
        };
        let remaining = transition_count - edge_index - 1;
        let minimum_final = i32::from(next_row) - i32::try_from(remaining * 2).unwrap_or(i32::MAX);
        let maximum_final = i32::from(next_row) + i32::try_from(remaining).unwrap_or(i32::MAX);
        if minimum_final > 3 || maximum_final < 3 {
            continue;
        }
        let state = (edge_index + 1, next_row, next_has_descent);
        let feasible = memo.get(&state).copied().unwrap_or_else(|| {
            let value = rhythm_is_feasible(
                edge_index + 1,
                transition_count,
                next_row,
                next_maximum_row,
                next_has_descent,
                constrained_edges,
                memo,
            );
            memo.insert(state, value);
            value
        });
        if feasible {
            rows.push(next_row);
            if complete_rhythm(
                edge_index + 1,
                transition_count,
                next_row,
                next_maximum_row,
                next_has_descent,
                constrained_edges,
                rng,
                memo,
                rows,
            ) {
                return true;
            }
            rows.pop();
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn rhythm_is_feasible(
    edge_index: usize,
    transition_count: usize,
    current_row: u16,
    maximum_row: u16,
    has_descent: bool,
    constrained_edges: &[usize],
    memo: &mut HashMap<(usize, u16, bool), bool>,
) -> bool {
    if edge_index == transition_count {
        return current_row == 3 && has_descent;
    }
    let steps: &[i16] = if edge_index == 1
        || edge_index + 1 == transition_count
        || constrained_edges.contains(&edge_index)
    {
        &[1, 2]
    } else {
        &[2, 1, 0, -1]
    };
    steps.iter().copied().any(|step| {
        let next_row_i32 = i32::from(current_row) - i32::from(step);
        let Ok(next_row) = u16::try_from(next_row_i32) else {
            return false;
        };
        if !(3..=maximum_row).contains(&next_row) {
            return false;
        }
        let next_has_descent = has_descent || step < 0;
        let state = (edge_index + 1, next_row, next_has_descent);
        if let Some(value) = memo.get(&state) {
            return *value;
        }
        let value = rhythm_is_feasible(
            edge_index + 1,
            transition_count,
            next_row,
            maximum_row,
            next_has_descent,
            constrained_edges,
            memo,
        );
        memo.insert(state, value);
        value
    })
}

fn embed_spine_supports(
    plan: &MissionPlan,
    rows: &[u16],
    rng: &mut StableRng,
) -> Result<
    (Vec<Option<SupportSpec>>, Vec<RouteCutRealization>),
    CompositionalRouteCutGenerationFailure,
> {
    let mut supports = vec![None; plan.nodes.len()];
    let domains = spine_support_domains(plan, rows, rng);
    let mut explored = 0;
    let mut exhausted_states = HashSet::new();
    if !complete_spine_supports(
        plan,
        &domains,
        0,
        0,
        false,
        &mut supports,
        &mut explored,
        &mut exhausted_states,
    ) {
        return Err(
            CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                phase: SupportConstraintPhase::Spine,
                explored,
            },
        );
    }
    debug_assert!(cut_obligations_hold(plan, &supports));
    let cuts = realized_cuts(plan, &supports)?;
    Ok((supports, cuts))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SpineConstraintState {
    index: usize,
    previous: SupportSpec,
    previous_direction: i8,
    has_reversal: bool,
    relevant_geometry: Vec<SupportSpec>,
}

fn spine_support_domains(
    plan: &MissionPlan,
    rows: &[u16],
    rng: &mut StableRng,
) -> Vec<Vec<SupportSpec>> {
    let starting_side = starting_anchor(plan);
    let ceiling_side = plan
        .cuts
        .last()
        .map_or(starting_side.opposite(), |cut| cut.anchor);
    let zero_cut_centers = zero_cut_centers(plan.spine.len(), starting_side, rng);
    plan.spine
        .iter()
        .zip(rows)
        .enumerate()
        .map(|(index, (&mission_node_id, &row))| {
            let mut candidates = if index == 0 {
                vec![side_anchored_support(
                    starting_side,
                    row,
                    6,
                    SupportKind::Solid,
                )]
            } else if index + 1 == plan.spine.len() {
                let mut columns = socket_columns_for_side(ceiling_side).to_vec();
                shuffle(&mut columns, rng);
                columns
                    .into_iter()
                    .map(|column| support_around(column + 1, row, 5, SupportKind::OneWay))
                    .collect()
            } else if let Some(cut) = plan
                .cuts
                .iter()
                .find(|cut| usize::from(cut.spine_index) == index)
            {
                let mut widths = (9_u16..=17).collect::<Vec<_>>();
                shuffle(&mut widths, rng);
                widths
                    .into_iter()
                    .map(|width| side_anchored_support(cut.anchor, row, width, SupportKind::Solid))
                    .collect()
            } else if plan
                .ports
                .iter()
                .any(|port| port.ordinal == 2 && port.node_id == mission_node_id)
            {
                let side = logical_side(plan, index, starting_side);
                let mut columns = socket_columns_for_side(side).to_vec();
                shuffle(&mut columns, rng);
                columns
                    .into_iter()
                    .map(|column| floor_support(column, side, row))
                    .collect()
            } else {
                let preferred_center = if plan.cuts.is_empty() {
                    zero_cut_centers[index]
                } else {
                    side_center(logical_side(plan, index, starting_side), rng)
                };
                ordinary_support_domain(preferred_center, row, rng)
            };
            if let Some(port) = plan
                .ports
                .iter()
                .find(|port| port.ordinal == 3 && port.node_id == mission_node_id)
            {
                candidates = candidates
                    .into_iter()
                    .map(|support| extend_lateral_port_support(support, port.side))
                    .collect();
            }
            candidates.dedup();
            candidates
        })
        .collect()
}

fn socket_columns_for_side(side: CutAnchor) -> &'static [u16] {
    match side {
        // The central socket is valid for either route side. In particular,
        // it lets a floor-port landing meet an immediately following cut
        // without exceeding the three-tile support-edge contract.
        // V2's emitted west domain deliberately starts at column 10: column
        // 6 remains part of the shared mate-closed inventory, but emitting it
        // for ceiling ports produced no valid floor-side occurrence in the
        // fixed raw key block. The observed-pool closure regression below
        // freezes this as a source-domain constraint, not a hidden fallback.
        CutAnchor::West => &HORIZONTAL_SOCKET_COLUMNS[1..3],
        CutAnchor::East => &HORIZONTAL_SOCKET_COLUMNS[2..],
    }
}

fn ordinary_support_domain(
    preferred_center: u16,
    row: u16,
    rng: &mut StableRng,
) -> Vec<SupportSpec> {
    let mut centers = (3_u16..=29).collect::<Vec<_>>();
    shuffle(&mut centers, rng);
    centers.sort_by_key(|center| center.abs_diff(preferred_center));
    let mut widths = (4_u16..=7).collect::<Vec<_>>();
    shuffle(&mut widths, rng);
    let mut candidates = Vec::with_capacity(centers.len() * widths.len());
    for center in centers {
        for &width in &widths {
            let support = support_around(center, row, width, SupportKind::OneWay);
            if !candidates.contains(&support) {
                candidates.push(support);
            }
        }
    }
    candidates
}

fn extend_lateral_port_support(support: SupportSpec, side: BoundarySide) -> SupportSpec {
    match side {
        BoundarySide::Left => SupportSpec {
            start_x: 1,
            end_x: support.end_x.max(7),
            ..support
        },
        BoundarySide::Right => SupportSpec {
            start_x: support.start_x.min(ROOM_WIDTH - 7),
            end_x: ROOM_WIDTH - 1,
            ..support
        },
        BoundarySide::Ceiling | BoundarySide::Floor => support,
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_spine_supports(
    plan: &MissionPlan,
    domains: &[Vec<SupportSpec>],
    index: usize,
    previous_direction: i8,
    has_reversal: bool,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
    exhausted_states: &mut HashSet<SpineConstraintState>,
) -> bool {
    if index == plan.spine.len() {
        return has_reversal;
    }
    let state = (index > 0).then(|| {
        let future_solid_rows = plan
            .cuts
            .iter()
            .filter(|cut| usize::from(cut.spine_index) >= index)
            .filter_map(|cut| domains[usize::from(cut.spine_index)].first())
            .map(|support| support.row)
            .collect::<Vec<_>>();
        let relevant_geometry = plan.spine[..index]
            .iter()
            .map(|&node_id| {
                supports[usize::from(node_id)]
                    .expect("every support in the assigned spine prefix exists")
            })
            .filter(|support| {
                support.kind == SupportKind::Solid
                    || future_solid_rows
                        .iter()
                        .any(|&solid_row| support.row >= solid_row && support.row - solid_row <= 2)
            })
            .collect();
        SpineConstraintState {
            index,
            previous: supports[usize::from(plan.spine[index - 1])]
                .expect("the previous spine support was assigned"),
            previous_direction,
            has_reversal,
            relevant_geometry,
        }
    });
    if let Some(state) = &state
        && exhausted_states.contains(state)
    {
        return false;
    }
    let node_id = plan.spine[index];
    for &candidate in &domains[index] {
        if *explored >= SPINE_CONSTRAINT_SEARCH_LIMIT {
            return false;
        }
        *explored += 1;
        let (next_direction, next_has_reversal) = if index > 0 {
            let Some(previous) = supports[usize::from(plan.spine[index - 1])] else {
                continue;
            };
            if !reversible_baseline_transition(previous, candidate) {
                continue;
            }
            let direction = match previous.center_x().cmp(&candidate.center_x()) {
                std::cmp::Ordering::Less => 1,
                std::cmp::Ordering::Greater => -1,
                std::cmp::Ordering::Equal => 0,
            };
            (
                if direction == 0 {
                    previous_direction
                } else {
                    direction
                },
                has_reversal
                    || (direction != 0
                        && previous_direction != 0
                        && direction != previous_direction),
            )
        } else {
            (previous_direction, has_reversal)
        };
        if floor_socket_overlaps_assigned_support(plan, node_id, candidate, supports) {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_route_edges_are_clear(plan, supports)
            && partial_cut_obligations_hold(plan, index, supports)
            && future_cut_obligations_remain_feasible(plan, domains, index, supports)
            && complete_spine_supports(
                plan,
                domains,
                index + 1,
                next_direction,
                next_has_reversal,
                supports,
                explored,
                exhausted_states,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    if let Some(state) = state {
        exhausted_states.insert(state);
    }
    false
}

fn future_cut_obligations_remain_feasible(
    plan: &MissionPlan,
    domains: &[Vec<SupportSpec>],
    assigned_through_index: usize,
    supports: &[Option<SupportSpec>],
) -> bool {
    plan.cuts
        .iter()
        .filter(|cut| usize::from(cut.spine_index) > assigned_through_index)
        .all(|cut| {
            let cut_index = usize::from(cut.spine_index);
            domains[cut_index].iter().copied().any(|shelf| {
                let predecessor = supports[usize::from(plan.spine[cut_index - 1])];
                if predecessor.is_some_and(|predecessor| {
                    let (opening_start, opening_end) = cut_opening(cut.anchor, shelf);
                    predecessor.start_x < opening_start
                        || predecessor.end_x > opening_end
                        || !reversible_baseline_transition(predecessor, shelf)
                }) {
                    return false;
                }
                let mut with_shelf = supports.to_vec();
                with_shelf[usize::from(cut.node_id)] = Some(shelf);
                assigned_support_geometry_is_valid(&with_shelf)
                    && assigned_route_edges_are_clear(plan, &with_shelf)
            })
        })
}

fn assigned_support_geometry_is_valid(supports: &[Option<SupportSpec>]) -> bool {
    let assigned = supports.iter().flatten().copied().collect::<Vec<_>>();
    for (index, first) in assigned.iter().copied().enumerate() {
        if assigned[index + 1..].iter().copied().any(|second| {
            first.row == second.row
                && first.kind != second.kind
                && first.start_x < second.end_x
                && second.start_x < first.end_x
        }) {
            return false;
        }
        if !support_has_clear_standing_position(first, &assigned) {
            return false;
        }
    }
    true
}

fn support_has_clear_standing_position(support: SupportSpec, supports: &[SupportSpec]) -> bool {
    !support_clear_standing_positions(support, supports).is_empty()
}

fn support_clear_standing_positions(support: SupportSpec, supports: &[SupportSpec]) -> Vec<i32> {
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let standing = Rect::new(x, standing_y, PLAYER_WIDTH, PLAYER_HEIGHT);
            supports.iter().copied().all(|blocker| {
                blocker.kind != SupportKind::Solid
                    || !standing.intersects(support_tile_bounds(blocker))
            })
        })
        .collect()
}

fn assigned_route_edges_are_clear(plan: &MissionPlan, supports: &[Option<SupportSpec>]) -> bool {
    let assigned = supports.iter().flatten().copied().collect::<Vec<_>>();
    plan.edges.iter().all(|edge| {
        let Some(first) = supports[usize::from(edge.from)] else {
            return true;
        };
        let Some(second) = supports[usize::from(edge.to)] else {
            return true;
        };
        support_transfer_headroom_is_clear(first, second, &assigned)
    })
}

fn support_transfer_headroom_is_clear(
    first: SupportSpec,
    second: SupportSpec,
    supports: &[SupportSpec],
) -> bool {
    let first_positions = support_clear_standing_positions(first, supports);
    let second_positions = support_clear_standing_positions(second, supports);
    let Some((first_x, second_x)) = first_positions
        .iter()
        .flat_map(|&first_x| {
            second_positions
                .iter()
                .map(move |&second_x| (first_x, second_x))
        })
        .min_by_key(|(first_x, second_x)| first_x.abs_diff(*second_x))
    else {
        return false;
    };
    let first_standing_y = i32::from(first.row) * TILE_SIZE - PLAYER_HEIGHT;
    let second_standing_y = i32::from(second.row) * TILE_SIZE - PLAYER_HEIGHT;
    let left = first_x.min(second_x);
    let right = (first_x + PLAYER_WIDTH).max(second_x + PLAYER_WIDTH);
    let top = first_standing_y.min(second_standing_y);
    let bottom = (i32::from(first.row) * TILE_SIZE).max(i32::from(second.row) * TILE_SIZE);
    let corridor = Rect::new(left, top, right - left, bottom - top);
    supports.iter().copied().all(|blocker| {
        blocker.kind != SupportKind::Solid
            || blocker == first
            || blocker == second
            || !corridor.intersects(support_tile_bounds(blocker))
    })
}

fn support_tile_bounds(support: SupportSpec) -> Rect {
    Rect::new(
        i32::from(support.start_x) * TILE_SIZE,
        i32::from(support.row) * TILE_SIZE,
        i32::from(support.width()) * TILE_SIZE,
        TILE_SIZE,
    )
}

fn floor_socket_overlaps_assigned_support(
    plan: &MissionPlan,
    node_id: u16,
    candidate: SupportSpec,
    supports: &[Option<SupportSpec>],
) -> bool {
    let Some(port) = plan
        .ports
        .iter()
        .find(|port| port.node_id == node_id && port.side == BoundarySide::Floor)
    else {
        return false;
    };
    let column = if candidate.center_x() < ROOM_WIDTH / 2 {
        candidate.end_x
    } else {
        candidate.start_x.saturating_sub(2)
    };
    let aperture = column..column + 2;
    supports.iter().enumerate().any(|(other_id, support)| {
        other_id != usize::from(port.node_id)
            && support.is_some_and(|support| {
                support.row == FLOOR_ROW
                    && support.start_x < aperture.end
                    && aperture.start < support.end_x
            })
    })
}

fn partial_cut_obligations_hold(
    plan: &MissionPlan,
    index: usize,
    supports: &[Option<SupportSpec>],
) -> bool {
    if let Some(cut) = plan
        .cuts
        .iter()
        .find(|cut| usize::from(cut.spine_index) == index)
    {
        let shelf = supports[usize::from(cut.node_id)].expect("current cut support was assigned");
        let predecessor = supports[usize::from(plan.spine[index - 1])]
            .expect("a cut predecessor was assigned first");
        let (opening_start, opening_end) = cut_opening(cut.anchor, shelf);
        if predecessor.start_x < opening_start || predecessor.end_x > opening_end {
            return false;
        }
    }
    if index > 0
        && let Some(cut) = plan
            .cuts
            .iter()
            .find(|cut| usize::from(cut.spine_index) + 1 == index)
    {
        let shelf = supports[usize::from(cut.node_id)].expect("previous cut support was assigned");
        let successor =
            supports[usize::from(plan.spine[index])].expect("current cut successor was assigned");
        if !(shelf.start_x..shelf.end_x).contains(&successor.center_x()) {
            return false;
        }
    }
    true
}

fn cut_opening(anchor: CutAnchor, shelf: SupportSpec) -> (u16, u16) {
    match anchor {
        CutAnchor::West => (shelf.end_x, ROOM_WIDTH - 1),
        CutAnchor::East => (1, shelf.start_x),
    }
}

fn cut_obligations_hold(plan: &MissionPlan, supports: &[Option<SupportSpec>]) -> bool {
    plan.cuts.iter().all(|cut| {
        let index = usize::from(cut.spine_index);
        let Some(shelf) = supports[usize::from(cut.node_id)] else {
            return false;
        };
        let Some(predecessor) = supports[usize::from(plan.spine[index - 1])] else {
            return false;
        };
        let Some(successor) = supports[usize::from(plan.spine[index + 1])] else {
            return false;
        };
        let (opening_start, opening_end) = cut_opening(cut.anchor, shelf);
        predecessor.start_x >= opening_start
            && predecessor.end_x <= opening_end
            && (shelf.start_x..shelf.end_x).contains(&successor.center_x())
    })
}

fn realized_cuts(
    plan: &MissionPlan,
    supports: &[Option<SupportSpec>],
) -> Result<Vec<RouteCutRealization>, CompositionalRouteCutGenerationFailure> {
    plan.cuts
        .iter()
        .map(|cut| {
            let index = usize::from(cut.spine_index);
            let shelf = supports[usize::from(cut.node_id)].ok_or(
                CompositionalRouteCutGenerationFailure::MissingMissionNode {
                    node_id: cut.node_id,
                },
            )?;
            let (opening_start_x, opening_end_x) = cut_opening(cut.anchor, shelf);
            Ok(RouteCutRealization {
                mission_node_id: cut.node_id,
                route_node_id: cut.node_id,
                order: cut.order,
                anchor: cut.anchor,
                row: shelf.row,
                shelf_start_x: shelf.start_x,
                shelf_end_x: shelf.end_x,
                opening_start_x,
                opening_end_x,
                predecessor_route_node_id: plan.spine[index - 1],
                successor_route_node_id: plan.spine[index + 1],
            })
        })
        .collect()
}

fn embed_fork_supports(
    plan: &MissionPlan,
    supports: &mut [Option<SupportSpec>],
    rng: &mut StableRng,
) -> Result<(), CompositionalRouteCutGenerationFailure> {
    let spine_supports = plan
        .spine
        .iter()
        .filter_map(|&node_id| supports[usize::from(node_id)])
        .collect::<Vec<_>>();
    for fork in &plan.forks {
        let from = supports[usize::from(fork.from)].ok_or(
            CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.from },
        )?;
        let to = supports[usize::from(fork.to)].ok_or(
            CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.to },
        )?;
        let domains = fork_support_domains(fork, from, to, rng);
        let mut explored = 0;
        if !complete_fork_supports(
            plan,
            fork,
            &domains,
            &spine_supports,
            0,
            supports,
            &mut explored,
        ) {
            for &node_id in &fork.branch_nodes {
                supports[usize::from(node_id)] = None;
            }
            return Err(
                CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                    phase: SupportConstraintPhase::Fork,
                    explored,
                },
            );
        }
    }
    Ok(())
}

fn fork_support_domains(
    fork: &MissionFork,
    from: SupportSpec,
    to: SupportSpec,
    rng: &mut StableRng,
) -> Vec<Vec<SupportSpec>> {
    let denominator = fork.branch_nodes.len() + 1;
    let base_direction = if (from.center_x() + to.center_x()) / 2 < ROOM_WIDTH / 2 {
        1_i32
    } else {
        -1_i32
    };
    fork.branch_nodes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let step = index + 1;
            let interpolated_row = interpolate_u16(from.row, to.row, step, denominator);
            let base_center = interpolate_u16(from.center_x(), to.center_x(), step, denominator);
            let preferred_center =
                (i32::from(base_center) + base_direction * 5).clamp(3, 29) as u16;
            let mut rows = (3_u16..FLOOR_ROW).collect::<Vec<_>>();
            shuffle(&mut rows, rng);
            rows.sort_by_key(|row| row.abs_diff(interpolated_row));
            let mut centers = (3_u16..=29).collect::<Vec<_>>();
            shuffle(&mut centers, rng);
            centers.sort_by_key(|center| center.abs_diff(preferred_center));
            let mut widths = (4_u16..=6).collect::<Vec<_>>();
            shuffle(&mut widths, rng);
            let mut candidates = Vec::new();
            for row in rows {
                for &center in &centers {
                    for &width in &widths {
                        let candidate = support_around(center, row, width, SupportKind::OneWay);
                        if !candidates.contains(&candidate) {
                            candidates.push(candidate);
                        }
                    }
                }
            }
            candidates
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn complete_fork_supports(
    plan: &MissionPlan,
    fork: &MissionFork,
    domains: &[Vec<SupportSpec>],
    spine_supports: &[SupportSpec],
    index: usize,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
) -> bool {
    let previous = if index == 0 {
        supports[usize::from(fork.from)].expect("fork source support exists")
    } else {
        supports[usize::from(fork.branch_nodes[index - 1])]
            .expect("previous branch support was assigned")
    };
    if index == fork.branch_nodes.len() {
        let target = supports[usize::from(fork.to)].expect("fork target support exists");
        return reversible_baseline_transition(previous, target)
            && floor_arrivals_remain_clear(plan, supports);
    }
    let node_id = fork.branch_nodes[index];
    let remaining_edges = fork.branch_nodes.len() - index;
    let target = supports[usize::from(fork.to)].expect("fork target support exists");
    for &candidate in &domains[index] {
        if *explored >= FORK_CONSTRAINT_SEARCH_LIMIT {
            return false;
        }
        *explored += 1;
        if !reversible_baseline_transition(previous, candidate)
            || candidate.row.abs_diff(target.row)
                > u16::try_from(remaining_edges * 2).unwrap_or(u16::MAX)
            || !has_unique_tile(candidate, spine_supports)
        {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_route_edges_are_clear(plan, supports)
            && complete_fork_supports(
                plan,
                fork,
                domains,
                spine_supports,
                index + 1,
                supports,
                explored,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    false
}

fn floor_arrivals_remain_clear(plan: &MissionPlan, supports: &[Option<SupportSpec>]) -> bool {
    plan.ports
        .iter()
        .filter(|port| port.side == BoundarySide::Floor)
        .all(|port| {
            supports[usize::from(port.node_id)]
                .is_none_or(|support| clear_arrival_x(port.node_id, support, supports).is_some())
        })
}

fn validate_support_transition_contract(
    plan: &MissionPlan,
    supports: &[Option<SupportSpec>],
) -> Result<(), CompositionalRouteCutGenerationFailure> {
    for (edge_index, edge) in plan.edges.iter().enumerate() {
        let from = supports[usize::from(edge.from)].ok_or(
            CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.from },
        )?;
        let to = supports[usize::from(edge.to)].ok_or(
            CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.to },
        )?;
        if !reversible_baseline_transition(from, to) {
            return Err(
                CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                    phase: SupportConstraintPhase::FinalRouteContract,
                    explored: u32::try_from(edge_index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    Ok(())
}

fn validate_rasterized_route_contract(
    room: &Room,
    plan: &RoutePlan,
) -> Result<(), CompositionalRouteCutGenerationFailure> {
    for (node_index, node) in plan.nodes.iter().enumerate() {
        let support_preserved = (node.support.start_x..node.support.end_x)
            .all(|x| room.tile(x, node.support.row) == Some(node.support.kind.tile()));
        if !support_preserved || clear_standing_positions(room, node.support).is_empty() {
            return Err(
                CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                    phase: SupportConstraintPhase::RasterizedRouteContract,
                    explored: u32::try_from(node_index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    for (edge_index, edge) in plan.edges.iter().enumerate() {
        let first = plan.nodes[usize::from(edge.from)].support;
        let second = plan.nodes[usize::from(edge.to)].support;
        if !rasterized_transfer_headroom_is_clear(room, first, second) {
            return Err(
                CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                    phase: SupportConstraintPhase::RasterizedRouteContract,
                    explored: u32::try_from(plan.nodes.len() + edge_index + 1).unwrap_or(u32::MAX),
                },
            );
        }
    }
    Ok(())
}

fn clear_standing_positions(room: &Room, support: SupportSpec) -> Vec<i32> {
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let standing = Rect::new(x, standing_y, PLAYER_WIDTH, PLAYER_HEIGHT);
            room.tiles().iter().enumerate().all(|(index, &tile)| {
                if tile != Tile::Solid && !tile.is_hazard() {
                    return true;
                }
                let width = usize::from(room.width());
                let tile_x = u16::try_from(index % width).expect("room width fits u16");
                let tile_y = u16::try_from(index / width).expect("room height fits u16");
                !standing.intersects(room.tile_bounds(tile_x, tile_y))
            })
        })
        .collect()
}

fn rasterized_transfer_headroom_is_clear(
    room: &Room,
    first: SupportSpec,
    second: SupportSpec,
) -> bool {
    let first_positions = clear_standing_positions(room, first);
    let second_positions = clear_standing_positions(room, second);
    let Some((first_x, second_x)) = first_positions
        .iter()
        .flat_map(|&first_x| {
            second_positions
                .iter()
                .map(move |&second_x| (first_x, second_x))
        })
        .min_by_key(|(first_x, second_x)| first_x.abs_diff(*second_x))
    else {
        return false;
    };
    let first_standing_y = i32::from(first.row) * TILE_SIZE - PLAYER_HEIGHT;
    let second_standing_y = i32::from(second.row) * TILE_SIZE - PLAYER_HEIGHT;
    let left = first_x.min(second_x);
    let right = (first_x + PLAYER_WIDTH).max(second_x + PLAYER_WIDTH);
    let top = first_standing_y.min(second_standing_y);
    let bottom = (i32::from(first.row) * TILE_SIZE).max(i32::from(second.row) * TILE_SIZE);
    let corridor = Rect::new(left, top, right - left, bottom - top);
    room.tiles().iter().enumerate().all(|(index, &tile)| {
        if tile != Tile::Solid {
            return true;
        }
        let width = usize::from(room.width());
        let tile_x = u16::try_from(index % width).expect("room width fits u16");
        let tile_y = u16::try_from(index / width).expect("room height fits u16");
        let belongs_to_endpoint = (tile_y == first.row
            && (first.start_x..first.end_x).contains(&tile_x))
            || (tile_y == second.row && (second.start_x..second.end_x).contains(&tile_x));
        belongs_to_endpoint || !corridor.intersects(room.tile_bounds(tile_x, tile_y))
    })
}

fn build_boundary_ports(
    plan: &MissionPlan,
    route_by_mission: &[u16],
    supports: &[Option<SupportSpec>],
    socket_columns: &HashMap<u16, u16>,
) -> Result<Vec<BoundaryPort>, CompositionalRouteCutGenerationFailure> {
    let mut result = Vec::with_capacity(plan.ports.len());
    for port in &plan.ports {
        let support = supports[usize::from(port.node_id)].ok_or(
            CompositionalRouteCutGenerationFailure::MissingMissionNode {
                node_id: port.node_id,
            },
        )?;
        let route_node_id = route_by_mission[usize::from(port.node_id)];
        let door = match port.side {
            BoundarySide::Left | BoundarySide::Right => {
                wall_port(&format!("port-{}", port.ordinal), support, port.side)
            }
            BoundarySide::Ceiling => ceiling_port(
                &format!("port-{}", port.ordinal),
                support,
                *socket_columns.get(&port.ordinal).ok_or_else(|| {
                    CompositionalRouteCutGenerationFailure::PortContract(format!(
                        "ceiling port {} has no socket column",
                        port.ordinal
                    ))
                })?,
            ),
            BoundarySide::Floor => floor_port(
                &format!("port-{}", port.ordinal),
                support,
                *socket_columns.get(&port.ordinal).ok_or_else(|| {
                    CompositionalRouteCutGenerationFailure::PortContract(format!(
                        "floor port {} has no socket column",
                        port.ordinal
                    ))
                })?,
                clear_arrival_x(port.node_id, support, supports).ok_or_else(|| {
                    CompositionalRouteCutGenerationFailure::PortContract(format!(
                        "floor port {} has no collision-free standing arrival",
                        port.ordinal
                    ))
                })?,
            ),
        };
        result.push(BoundaryPort {
            node_id: route_node_id,
            door,
        });
    }
    Ok(result)
}

fn validate_embedded_port_contract(
    route_plan: &RoutePlan,
    ports: &[BoundaryPort],
) -> Result<(), CompositionalRouteCutGenerationFailure> {
    if !(2..=4).contains(&ports.len()) {
        return Err(CompositionalRouteCutGenerationFailure::PortContract(
            format!("expected two to four ports, found {}", ports.len()),
        ));
    }
    let mut referenced = ports.iter().map(|port| port.node_id).collect::<Vec<_>>();
    referenced.sort_unstable();
    referenced.dedup();
    if referenced.len() != ports.len() {
        return Err(CompositionalRouteCutGenerationFailure::PortContract(
            "multiple ports own the same route node".to_owned(),
        ));
    }
    let declared = route_plan
        .nodes
        .iter()
        .filter(|node| node.role == NodeRole::Port)
        .map(|node| node.id)
        .collect::<Vec<_>>();
    if !declared.iter().all(|node| referenced.contains(node)) || declared.len() != referenced.len()
    {
        return Err(CompositionalRouteCutGenerationFailure::PortContract(
            "route-plan port roles do not exactly match boundary ports".to_owned(),
        ));
    }
    Ok(())
}

fn route_role(kind: MissionNodeKind) -> NodeRole {
    match kind {
        MissionNodeKind::Port => NodeRole::Port,
        MissionNodeKind::Transit | MissionNodeKind::Cut => NodeRole::Landing,
        MissionNodeKind::Junction => NodeRole::Junction,
        MissionNodeKind::ForkBranch => NodeRole::Recovery,
        MissionNodeKind::Pickup => NodeRole::Pickup,
    }
}

fn starting_anchor(plan: &MissionPlan) -> CutAnchor {
    plan.ports
        .iter()
        .find(|port| port.ordinal == 0)
        .map_or(CutAnchor::West, |port| match port.side {
            BoundarySide::Right => CutAnchor::East,
            BoundarySide::Left | BoundarySide::Ceiling | BoundarySide::Floor => CutAnchor::West,
        })
}

fn logical_side(plan: &MissionPlan, spine_index: usize, default: CutAnchor) -> CutAnchor {
    logical_side_from_cuts(&plan.cuts, spine_index, default)
}

fn logical_side_from_cuts(
    cuts: &[MissionCut],
    spine_index: usize,
    default: CutAnchor,
) -> CutAnchor {
    let Some(first) = cuts.first() else {
        return default;
    };
    if spine_index < usize::from(first.spine_index) {
        return first.anchor.opposite();
    }
    cuts.iter()
        .rev()
        .find(|cut| usize::from(cut.spine_index) < spine_index)
        .map_or(first.anchor.opposite(), |cut| cut.anchor)
}

fn zero_cut_centers(length: usize, starting_side: CutAnchor, rng: &mut StableRng) -> Vec<u16> {
    let west_to_east = [6_u16, 11, 16, 21, 26, 21, 16, 11];
    let phase = usize::from(rng.below(west_to_east.len().try_into().unwrap_or(u16::MAX)));
    let mut centers = (0..length)
        .map(|index| {
            let center = west_to_east[(index + phase) % west_to_east.len()];
            if starting_side == CutAnchor::West {
                center
            } else {
                ROOM_WIDTH - center
            }
        })
        .collect::<Vec<_>>();
    if let Some(first_interior) = centers.get_mut(2) {
        *first_interior = match starting_side {
            CutAnchor::West => 26,
            CutAnchor::East => 6,
        };
    }
    centers
}

fn side_center(side: CutAnchor, rng: &mut StableRng) -> u16 {
    match side {
        // Keep the two-tile lateral arrival pockets clear at low rows.  Cut
        // shelves themselves anchor to walls only on the opposite side of
        // the initial port.
        CutAnchor::West => 7 + rng.below(3),
        CutAnchor::East => 23 + rng.below(3),
    }
}

fn side_anchored_support(side: CutAnchor, row: u16, width: u16, kind: SupportKind) -> SupportSpec {
    match side {
        CutAnchor::West => SupportSpec {
            start_x: 1,
            end_x: 1 + width,
            row,
            kind,
        },
        CutAnchor::East => SupportSpec {
            start_x: ROOM_WIDTH - 1 - width,
            end_x: ROOM_WIDTH - 1,
            row,
            kind,
        },
    }
}

fn support_around(center_x: u16, row: u16, width: u16, kind: SupportKind) -> SupportSpec {
    let mut start_x = center_x.saturating_sub(width / 2).max(1);
    let mut end_x = start_x + width;
    if end_x >= ROOM_WIDTH {
        end_x = ROOM_WIDTH - 1;
        start_x = end_x - width;
    }
    SupportSpec {
        start_x,
        end_x,
        row,
        // The boundary floor is already rasterized as solid by RoomDraft.
        // Treat every route-owned footprint on that row as the same material
        // so provenance and collision geometry cannot disagree.
        kind: if row == FLOOR_ROW {
            SupportKind::Solid
        } else {
            kind
        },
    }
}

fn floor_support(column: u16, side: CutAnchor, row: u16) -> SupportSpec {
    match side {
        CutAnchor::West => SupportSpec {
            // Leave the two-tile lateral arrival pocket clear even when the
            // source wall and floor connector occupy the same side.
            start_x: 3,
            end_x: column,
            row,
            kind: SupportKind::Solid,
        },
        CutAnchor::East => SupportSpec {
            start_x: column + 2,
            end_x: ROOM_WIDTH - 3,
            row,
            kind: SupportKind::Solid,
        },
    }
}

fn socket_columns(plan: &MissionPlan, supports: &[Option<SupportSpec>]) -> HashMap<u16, u16> {
    let mut result = HashMap::new();
    for port in &plan.ports {
        match port.side {
            BoundarySide::Ceiling => {
                let support = supports[usize::from(port.node_id)]
                    .expect("all port supports exist before socket construction");
                result.insert(port.ordinal, support.center_x().saturating_sub(1));
            }
            BoundarySide::Floor => {
                let support = supports[usize::from(port.node_id)]
                    .expect("all port supports exist before socket construction");
                let column = if support.center_x() < ROOM_WIDTH / 2 {
                    support.end_x
                } else {
                    support.start_x.saturating_sub(2)
                };
                result.insert(port.ordinal, column);
            }
            BoundarySide::Left | BoundarySide::Right => {}
        }
    }
    result
}

fn wall_port(id: &str, support: SupportSpec, side: BoundarySide) -> Door {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let trigger_y =
        (i32::from(support.row) * TILE_SIZE - DOOR_SPAN).clamp(0, room_height - DOOR_SPAN);
    let (trigger_x, arrival_x) = match side {
        BoundarySide::Left => (0, TILE_SIZE + 2),
        BoundarySide::Right => (
            room_width - SIDE_DOOR_DEPTH,
            room_width - TILE_SIZE - 2 - PLAYER_WIDTH,
        ),
        BoundarySide::Ceiling | BoundarySide::Floor => {
            unreachable!("wall_port is only called for lateral boundaries")
        }
    };
    Door {
        id: id.to_owned(),
        side,
        trigger_bounds: Rect::new(trigger_x, trigger_y, SIDE_DOOR_DEPTH, DOOR_SPAN),
        arrival: Point::new(arrival_x, standing_y),
        destination_room: None,
        destination_door: None,
    }
}

fn ceiling_port(id: &str, support: SupportSpec, column: u16) -> Door {
    Door {
        id: id.to_owned(),
        side: BoundarySide::Ceiling,
        trigger_bounds: Rect::new(
            i32::from(column) * TILE_SIZE,
            0,
            DOOR_SPAN,
            CEILING_DOOR_DEPTH,
        ),
        arrival: Point::new(
            i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2,
            TILE_SIZE + 2,
        ),
        destination_room: None,
        destination_door: None,
    }
}

fn floor_port(id: &str, support: SupportSpec, column: u16, arrival_x: i32) -> Door {
    let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
    let shaft_top = i32::from(support.row) * TILE_SIZE;
    Door {
        id: id.to_owned(),
        side: BoundarySide::Floor,
        trigger_bounds: Rect::new(
            i32::from(column) * TILE_SIZE,
            shaft_top,
            DOOR_SPAN,
            room_height - shaft_top,
        ),
        arrival: Point::new(arrival_x, shaft_top - PLAYER_HEIGHT),
        destination_room: None,
        destination_door: None,
    }
}

/// Whether a socket belongs to the finite v1 inventory.  The predicate is
/// closed under [`DoorSocket::mate`]: lateral offsets admit both left and
/// right, and horizontal offsets admit both ceiling and floor.
#[must_use]
pub fn compositional_route_cut_socket_in_inventory(socket: DoorSocket) -> bool {
    if socket.span != DOOR_SPAN {
        return false;
    }
    match socket.side {
        BoundarySide::Left | BoundarySide::Right => {
            (0..=150).contains(&socket.offset) && socket.offset % TILE_SIZE == 0
        }
        BoundarySide::Ceiling | BoundarySide::Floor => HORIZONTAL_SOCKET_COLUMNS
            .iter()
            .any(|&column| i32::from(column) * TILE_SIZE == socket.offset),
    }
}

fn clear_arrival_x(
    owning_node: u16,
    support: SupportSpec,
    supports: &[Option<SupportSpec>],
) -> Option<i32> {
    let y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    let preferred_x = i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let arrival = Rect::new(x, y, PLAYER_WIDTH, PLAYER_HEIGHT);
            supports.iter().enumerate().all(|(node_id, candidate)| {
                node_id == usize::from(owning_node)
                    || candidate.is_none_or(|candidate| {
                        !arrival.intersects(Rect::new(
                            i32::from(candidate.start_x) * TILE_SIZE,
                            i32::from(candidate.row) * TILE_SIZE,
                            i32::from(candidate.width()) * TILE_SIZE,
                            TILE_SIZE,
                        ))
                    })
            })
        })
        .min_by_key(|&x| x.abs_diff(preferred_x))
}

fn safe_ground_spawn(supports: &[Option<SupportSpec>], ports: &[BoundaryPort]) -> Option<Point> {
    let y = i32::from(FLOOR_ROW) * TILE_SIZE - PLAYER_HEIGHT;
    let minimum_x = TILE_SIZE + 2;
    let maximum_x = i32::from(ROOM_WIDTH - 1) * TILE_SIZE - PLAYER_WIDTH - 2;
    let preferred_x = i32::from(ROOM_WIDTH / 2) * TILE_SIZE - PLAYER_WIDTH / 2;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let player = Rect::new(x, y, PLAYER_WIDTH, PLAYER_HEIGHT);
            let clear_of_supports = supports.iter().flatten().all(|support| {
                support.row == FLOOR_ROW
                    || !player.intersects(Rect::new(
                        i32::from(support.start_x) * TILE_SIZE,
                        i32::from(support.row) * TILE_SIZE,
                        i32::from(support.width()) * TILE_SIZE,
                        TILE_SIZE,
                    ))
            });
            let over_solid_floor = ports.iter().all(|port| {
                port.door.side != BoundarySide::Floor
                    || x + PLAYER_WIDTH <= port.door.trigger_bounds.x
                    || x >= port.door.trigger_bounds.right()
            });
            clear_of_supports && over_solid_floor
        })
        .min_by_key(|&x| x.abs_diff(preferred_x))
        .map(|x| Point::new(x, y))
}

fn interpolate_u16(from: u16, to: u16, step: usize, denominator: usize) -> u16 {
    let from_weight = denominator - step;
    let numerator = usize::from(from) * from_weight + usize::from(to) * step;
    u16::try_from((numerator + denominator / 2) / denominator)
        .expect("interpolated room coordinate fits u16")
}

fn has_unique_tile(support: SupportSpec, others: &[SupportSpec]) -> bool {
    (support.start_x..support.end_x).any(|x| {
        others
            .iter()
            .all(|other| other.row != support.row || !(other.start_x..other.end_x).contains(&x))
    })
}

fn rhythm_counts(rows: &[u16]) -> (u16, u16, u16) {
    let mut ascent = 0_u16;
    let mut descent = 0_u16;
    let mut level = 0_u16;
    for pair in rows.windows(2) {
        match pair[0].cmp(&pair[1]) {
            std::cmp::Ordering::Greater => ascent += 1,
            std::cmp::Ordering::Less => descent += 1,
            std::cmp::Ordering::Equal => level += 1,
        }
    }
    (ascent, descent, level)
}

fn horizontal_reversals(plan: &MissionPlan, supports: &[Option<SupportSpec>]) -> usize {
    let mut previous_direction = 0_i8;
    let mut reversals = 0;
    for pair in plan.spine.windows(2) {
        let from = supports[usize::from(pair[0])]
            .expect("all spine supports exist")
            .center_x();
        let to = supports[usize::from(pair[1])]
            .expect("all spine supports exist")
            .center_x();
        let direction = match from.cmp(&to) {
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal => 0,
        };
        if direction != 0 {
            if previous_direction != 0 && direction != previous_direction {
                reversals += 1;
            }
            previous_direction = direction;
        }
    }
    reversals
}

fn separated_cut_placements(spine_len: usize, requested: u16) -> Vec<Vec<usize>> {
    if requested == 0 {
        return vec![Vec::new()];
    }
    // Leave a predecessor and successor around every cut.  A separation of
    // three leaves each local collision rewrite an independent embedding
    // obligation rather than collapsing adjacent cuts into one prefab shape.
    let eligible = (2..spine_len.saturating_sub(2)).collect::<Vec<_>>();
    let mut results = Vec::new();
    choose_separated(
        &eligible,
        usize::from(requested),
        0,
        &mut Vec::new(),
        &mut results,
    );
    results
}

fn choose_separated(
    eligible: &[usize],
    remaining: usize,
    start: usize,
    current: &mut Vec<usize>,
    results: &mut Vec<Vec<usize>>,
) {
    if remaining == 0 {
        results.push(current.clone());
        return;
    }
    for position in start..eligible.len() {
        let candidate = eligible[position];
        if current
            .last()
            .is_some_and(|previous| candidate - previous < 3)
        {
            continue;
        }
        current.push(candidate);
        choose_separated(eligible, remaining - 1, position + 1, current, results);
        current.pop();
    }
}

fn fork_spans(spine: &[u16], cuts: &[MissionCut]) -> Vec<(usize, usize)> {
    let cut_indices = cuts
        .iter()
        .map(|cut| usize::from(cut.spine_index))
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for from in 1..spine.len().saturating_sub(3) {
        // A one-edge retained fork is still a real cycle: embedding must
        // realize a collision-distinct alternative between the same two
        // junctions.  Including it leaves enough independent fork sites even
        // when three separated cuts partition the spine into short regions.
        for span in 1..=4 {
            let to = from + span;
            if to >= spine.len() - 1 || cut_indices.iter().any(|&cut| (from..=to).contains(&cut)) {
                continue;
            }
            result.push((from, to));
        }
    }
    result
}

fn mark_junction(nodes: &mut [MissionNode], node_id: u16) {
    let node = &mut nodes[usize::from(node_id)];
    if node.kind == MissionNodeKind::Transit {
        node.kind = MissionNodeKind::Junction;
    }
}

fn plan_starting_side(cuts: &[MissionCut], rng: &mut StableRng) -> BoundarySide {
    cuts.first().map_or_else(
        || {
            if rng.coin() {
                BoundarySide::Left
            } else {
                BoundarySide::Right
            }
        },
        |cut| match cut.anchor.opposite() {
            CutAnchor::West => BoundarySide::Left,
            CutAnchor::East => BoundarySide::Right,
        },
    )
}

fn shuffle<T>(values: &mut [T], rng: &mut StableRng) {
    for index in (1..values.len()).rev() {
        let swap_with = usize::from(
            rng.below(
                (index + 1)
                    .try_into()
                    .expect("the short derivation collection fits u16"),
            ),
        );
        values.swap(index, swap_with);
    }
}

fn canonical_topology_signature(plan: &MissionPlan) -> u64 {
    let canonical_nodes = canonical_node_order(plan);
    let mut canonical_id = vec![u16::MAX; plan.nodes.len()];
    for (index, &node_id) in canonical_nodes.iter().enumerate() {
        canonical_id[usize::from(node_id)] = index.try_into().expect("mission graph fits u16");
    }

    let mut signature = CoordinateFreeSignature::new();
    signature.u32(COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION);
    signature.u16(canonical_nodes.len().try_into().unwrap_or(u16::MAX));
    for node_id in canonical_nodes {
        signature.byte(node_kind_tag(plan.nodes[usize::from(node_id)].kind));
    }
    let mut edges = plan
        .edges
        .iter()
        .map(|edge| {
            (
                canonical_id[usize::from(edge.from)],
                canonical_id[usize::from(edge.to)],
                edge.kind,
                edge.critical,
            )
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    signature.u16(edges.len().try_into().unwrap_or(u16::MAX));
    for (from, to, kind, critical) in edges {
        signature.u16(from);
        signature.u16(to);
        signature.byte(edge_kind_tag(kind));
        signature.byte(u8::from(critical));
    }
    signature.u16(plan.cuts.len().try_into().unwrap_or(u16::MAX));
    for cut in &plan.cuts {
        signature.u16(canonical_id[usize::from(cut.node_id)]);
        signature.u16(cut.order);
        signature.byte(cut_anchor_tag(cut.anchor));
    }
    let mut ports = plan
        .ports
        .iter()
        .map(|port| {
            (
                port.ordinal,
                canonical_id[usize::from(port.node_id)],
                boundary_side_tag(port.side),
            )
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    signature.u16(ports.len().try_into().unwrap_or(u16::MAX));
    for (ordinal, node_id, side) in ports {
        signature.u16(ordinal);
        signature.u16(node_id);
        signature.byte(side);
    }
    signature.u16(canonical_id[usize::from(plan.pickup_node_id)]);
    signature.finish()
}

fn canonical_node_order(plan: &MissionPlan) -> Vec<u16> {
    let mut result = plan.spine.clone();
    let spine_positions = plan
        .spine
        .iter()
        .enumerate()
        .map(|(index, &node_id)| (node_id, index))
        .collect::<Vec<_>>();
    let mut forks = plan.forks.iter().collect::<Vec<_>>();
    forks.sort_unstable_by_key(|fork| {
        let from = spine_positions
            .iter()
            .find(|(node_id, _)| *node_id == fork.from)
            .map_or(usize::MAX, |(_, index)| *index);
        let to = spine_positions
            .iter()
            .find(|(node_id, _)| *node_id == fork.to)
            .map_or(usize::MAX, |(_, index)| *index);
        (from, to, fork.branch_nodes.len())
    });
    for fork in forks {
        result.extend(&fork.branch_nodes);
    }
    debug_assert_eq!(result.len(), plan.nodes.len());
    result
}

fn rewrite_signature(rewrites: &[MissionRewrite]) -> u64 {
    let mut signature = CoordinateFreeSignature::new();
    signature.u32(COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION);
    signature.u16(rewrites.len().try_into().unwrap_or(u16::MAX));
    for rewrite in rewrites {
        match rewrite {
            MissionRewrite::SubdivideEdge {
                removed_from,
                removed_to,
                inserted_node,
            } => {
                signature.byte(0);
                signature.u16(*removed_from);
                signature.u16(*removed_to);
                signature.u16(*inserted_node);
            }
            MissionRewrite::InsertRouteCut {
                node_id,
                spine_index,
                order,
                anchor,
            } => {
                signature.byte(1);
                signature.u16(*node_id);
                signature.u16(*spine_index);
                signature.u16(*order);
                signature.byte(cut_anchor_tag(*anchor));
            }
            MissionRewrite::InsertForkRejoin {
                from,
                to,
                branch_nodes,
            } => {
                signature.byte(2);
                signature.u16(*from);
                signature.u16(*to);
                signature.u16(branch_nodes.len().try_into().unwrap_or(u16::MAX));
                for node in branch_nodes {
                    signature.u16(*node);
                }
            }
            MissionRewrite::AttachPort {
                node_id,
                ordinal,
                side,
            } => {
                signature.byte(3);
                signature.u16(*node_id);
                signature.u16(*ordinal);
                signature.byte(boundary_side_tag(*side));
            }
            MissionRewrite::MarkPickup { node_id } => {
                signature.byte(4);
                signature.u16(*node_id);
            }
        }
    }
    signature.finish()
}

const fn node_kind_tag(kind: MissionNodeKind) -> u8 {
    match kind {
        MissionNodeKind::Port => 0,
        MissionNodeKind::Transit => 1,
        MissionNodeKind::Junction => 2,
        MissionNodeKind::Cut => 3,
        MissionNodeKind::ForkBranch => 4,
        MissionNodeKind::Pickup => 5,
    }
}

const fn edge_kind_tag(kind: MissionEdgeKind) -> u8 {
    match kind {
        MissionEdgeKind::Spine => 0,
        MissionEdgeKind::ForkBranch => 1,
    }
}

const fn cut_anchor_tag(anchor: CutAnchor) -> u8 {
    match anchor {
        CutAnchor::West => 0,
        CutAnchor::East => 1,
    }
}

const fn boundary_side_tag(side: BoundarySide) -> u8 {
    match side {
        BoundarySide::Left => 0,
        BoundarySide::Right => 1,
        BoundarySide::Ceiling => 2,
        BoundarySide::Floor => 3,
    }
}

struct CoordinateFreeSignature(u64);

impl CoordinateFreeSignature {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use downwards_core::Tile;

    use super::super::common::horizontal_support_gap;
    use super::*;

    #[test]
    fn exact_embedding_constructs_and_maps_every_mission_node() {
        let mut constructed = 0;
        for seed in 0..128 {
            let key =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard);
            let first = key.regenerate();
            assert_eq!(first, key.regenerate());
            let Ok(first) = first else {
                continue;
            };
            constructed += 1;
            assert_eq!(first.key, key);
            assert_eq!(first.mission.key, key);
            assert_eq!(
                first.mission_route_nodes.len(),
                first.mission.plan.nodes.len()
            );
            assert_eq!(first.route_plan.nodes.len(), first.mission.plan.nodes.len());
            assert_eq!(first.route_plan.edges.len(), first.mission.plan.edges.len());
            assert_eq!(first.boundary_ports.len(), first.mission.plan.ports.len());
            assert_eq!(first.generated.room.pickups().len(), 1);
            assert!(first.generated.room.timed_hazards().is_empty());
            assert!(
                first
                    .generated
                    .room
                    .tiles()
                    .iter()
                    .all(|&tile| !tile.is_hazard())
            );
            assert!(first.embedding.descent_edges >= 1);
            assert!(first.embedding.horizontal_direction_reversals >= 1);
            assert!(!first.embedding.claims_wall_jump_requirement);
            assert!(!first.embedding.claims_dash_requirement);
            validate_rasterized_route_contract(&first.generated.room, &first.route_plan).unwrap();

            let mission_ids = first
                .mission_route_nodes
                .iter()
                .map(|mapping| mapping.mission_node_id)
                .collect::<HashSet<_>>();
            let route_ids = first
                .mission_route_nodes
                .iter()
                .map(|mapping| mapping.route_node_id)
                .collect::<HashSet<_>>();
            assert_eq!(mission_ids.len(), first.mission.plan.nodes.len());
            assert_eq!(route_ids.len(), first.route_plan.nodes.len());
            for edge in &first.route_plan.edges {
                let from = first.route_plan.nodes[usize::from(edge.from)].support;
                let to = first.route_plan.nodes[usize::from(edge.to)].support;
                assert!(reversible_baseline_transition(from, to));
                assert!(conservative_baseline_transition(from, to).is_some());
                assert!(conservative_baseline_transition(to, from).is_some());
                assert!(horizontal_support_gap(from, to) <= 3);
                assert!(!matches!(
                    edge.verb,
                    super::super::RouteVerb::WallClimb
                        | super::super::RouteVerb::DashAcross
                        | super::super::RouteVerb::DashUp
                ));
            }
            if constructed == 64 {
                break;
            }
        }
        assert_eq!(constructed, 64);
    }

    #[test]
    fn realized_cuts_are_single_opening_boundary_separators() {
        let mut observed = 0;
        let mut constructed = 0;
        for seed in 0..128 {
            let Ok(candidate) =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .regenerate()
            else {
                continue;
            };
            constructed += 1;
            for cut in &candidate.embedding.cut_realizations {
                observed += 1;
                assert!(cut.opening_start_x < cut.opening_end_x);
                match cut.anchor {
                    CutAnchor::West => {
                        assert_eq!(cut.shelf_start_x, 1);
                        assert_eq!(cut.opening_start_x, cut.shelf_end_x);
                        assert_eq!(cut.opening_end_x, ROOM_WIDTH - 1);
                    }
                    CutAnchor::East => {
                        assert_eq!(cut.shelf_end_x, ROOM_WIDTH - 1);
                        assert_eq!(cut.opening_start_x, 1);
                        assert_eq!(cut.opening_end_x, cut.shelf_start_x);
                    }
                }
                for x in cut.shelf_start_x..cut.shelf_end_x {
                    assert_eq!(candidate.generated.room.tile(x, cut.row), Some(Tile::Solid));
                }
                let predecessor =
                    candidate.route_plan.nodes[usize::from(cut.predecessor_route_node_id)].support;
                let successor =
                    candidate.route_plan.nodes[usize::from(cut.successor_route_node_id)].support;
                assert!(predecessor.start_x >= cut.opening_start_x);
                assert!(predecessor.end_x <= cut.opening_end_x);
                assert!((cut.shelf_start_x..cut.shelf_end_x).contains(&successor.center_x()));
            }
        }
        assert!(constructed >= 120);
        assert!(observed > 100);
    }

    #[test]
    fn fixed_256_key_pool_has_inventory_and_observed_mate_closure() {
        let mut constructed = 0;
        let mut failures = Vec::new();
        let mut occurrences = Vec::new();
        let mut static_geometry = HashSet::new();
        let mut route_geometry = HashSet::new();
        let mut topology = HashSet::new();
        for seed in 0..256 {
            let candidate = match CompositionalRouteCutKey::new(
                seed,
                AbilitySet::ALL,
                ChallengeIntent::Standard,
            )
            .regenerate()
            {
                Ok(candidate) => candidate,
                Err(error) => {
                    failures.push((seed, error.cause));
                    continue;
                }
            };
            constructed += 1;
            static_geometry.insert(static_digest(&candidate));
            route_geometry.insert(candidate.route_summary.signature);
            topology.insert(candidate.mission.topology_signature());
            for door in candidate.generated.room.doors() {
                let socket = door.socket();
                assert!(compositional_route_cut_socket_in_inventory(socket));
                assert!(compositional_route_cut_socket_in_inventory(socket.mate()));
                occurrences.push((seed, socket));
            }
        }
        assert_eq!(constructed, 253, "unexpected failures: {failures:?}");
        assert_eq!(
            failures,
            vec![
                (
                    9,
                    CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                        phase: SupportConstraintPhase::Spine,
                        explored: 117_890,
                    },
                ),
                (
                    93,
                    CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                        phase: SupportConstraintPhase::Spine,
                        explored: SPINE_CONSTRAINT_SEARCH_LIMIT,
                    },
                ),
                (
                    211,
                    CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
                        phase: SupportConstraintPhase::Spine,
                        explored: SPINE_CONSTRAINT_SEARCH_LIMIT,
                    },
                ),
            ]
        );
        assert_eq!(static_geometry.len(), 253);
        assert_eq!(route_geometry.len(), 253);
        assert_eq!(topology.len(), 252);
        let unmatched = occurrences
            .iter()
            .copied()
            .filter(|&(seed, socket)| {
                !occurrences
                    .iter()
                    .any(|&(mate_seed, mate)| mate_seed != seed && socket.matches(mate))
            })
            .collect::<Vec<_>>();
        assert!(
            unmatched.is_empty(),
            "emitted sockets without a mate on a different candidate: {unmatched:?}"
        );

        for offset in (0..=150).step_by(usize::try_from(TILE_SIZE).unwrap()) {
            for side in [BoundarySide::Left, BoundarySide::Right] {
                let socket = DoorSocket {
                    side,
                    offset,
                    span: DOOR_SPAN,
                };
                assert!(compositional_route_cut_socket_in_inventory(socket));
                assert!(compositional_route_cut_socket_in_inventory(socket.mate()));
            }
        }
        for column in HORIZONTAL_SOCKET_COLUMNS {
            for side in [BoundarySide::Ceiling, BoundarySide::Floor] {
                let socket = DoorSocket {
                    side,
                    offset: i32::from(column) * TILE_SIZE,
                    span: DOOR_SPAN,
                };
                assert!(compositional_route_cut_socket_in_inventory(socket));
                assert!(compositional_route_cut_socket_in_inventory(socket.mate()));
            }
        }
    }

    #[test]
    fn fork_rewrites_have_collision_distinct_route_supports() {
        let mut observed = 0;
        let mut constructed = 0;
        for seed in 0..64 {
            let Ok(candidate) =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .regenerate()
            else {
                continue;
            };
            constructed += 1;
            let spine = candidate
                .mission
                .plan
                .spine
                .iter()
                .map(|&node_id| candidate.route_plan.nodes[usize::from(node_id)].support)
                .collect::<Vec<_>>();
            for fork in &candidate.mission.plan.forks {
                observed += 1;
                assert!(fork.branch_nodes.iter().any(|&node_id| {
                    has_unique_tile(
                        candidate.route_plan.nodes[usize::from(node_id)].support,
                        &spine,
                    )
                }));
            }
        }
        assert!(constructed >= 62);
        assert!(observed > 30);
    }

    #[test]
    fn sixty_four_exact_embeddings_have_a_static_expressivity_floor() {
        let mut static_geometry = HashSet::new();
        let mut route_geometry = HashSet::new();
        let mut topology = HashSet::new();
        let mut constructed = 0;
        for seed in 0..128 {
            let Ok(candidate) =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .regenerate()
            else {
                continue;
            };
            constructed += 1;
            static_geometry.insert(static_digest(&candidate));
            route_geometry.insert(candidate.route_summary.signature);
            topology.insert(candidate.mission.topology_signature());
            if constructed == 64 {
                break;
            }
        }
        assert_eq!(constructed, 64);
        assert!(static_geometry.len() >= 58);
        assert!(route_geometry.len() >= 58);
        assert!(topology.len() >= 58);
    }

    fn static_digest(candidate: &CompositionalRouteCutCandidate) -> u64 {
        let mut digest = CoordinateFreeSignature::new();
        for tile in candidate.generated.room.tiles() {
            digest.byte(match tile {
                Tile::Empty => 0,
                Tile::Solid => 1,
                Tile::HazardUp => 2,
                Tile::OneWay => 3,
                Tile::HazardDown => 4,
                Tile::HazardLeft => 5,
                Tile::HazardRight => 6,
            });
        }
        for door in candidate.generated.room.doors() {
            digest.byte(boundary_side_tag(door.side));
            for byte in door.socket().offset.to_le_bytes() {
                digest.byte(byte);
            }
            for byte in door.socket().span.to_le_bytes() {
                digest.byte(byte);
            }
        }
        digest.finish()
    }

    #[test]
    fn exact_derivation_is_deterministic_and_attempt_invariant() {
        for seed in 0..128 {
            let first =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Technical)
                    .derive_mission()
                    .unwrap();
            let second =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Technical)
                    .derive_mission()
                    .unwrap();
            assert_eq!(first, second);

            let alternate_attempt =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Technical)
                    .with_embedding(
                        CompositionalRouteCutGrammar::RecursiveMissionCutsV1,
                        COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT,
                    )
                    .derive_mission()
                    .unwrap();
            assert_eq!(first.plan, alternate_attempt.plan);
            assert_eq!(first.provenance, alternate_attempt.provenance);
        }
    }

    #[test]
    fn derivation_uses_real_rewrites_and_connected_fork_cycles() {
        for seed in 0..256 {
            let mission =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .derive_mission()
                    .unwrap();
            assert_eq!(mission.provenance.initial_node_count, 2);
            assert_eq!(mission.provenance.initial_edge_count, 1);
            assert!(mission.provenance.subdivision_rewrites >= 10);
            assert_eq!(
                mission.plan.ports.len(),
                usize::from(mission.provenance.port_rewrites)
            );
            assert!((2..=4).contains(&mission.plan.ports.len()));
            assert_eq!(
                mission.plan.cuts.len(),
                usize::from(mission.provenance.cut_rewrites)
            );
            assert_eq!(
                mission.plan.forks.len(),
                usize::from(mission.provenance.fork_rewrites)
            );
            assert_eq!(connected_nodes(&mission.plan), mission.plan.nodes.len());
            let cycle_rank = mission
                .plan
                .edges
                .len()
                .saturating_add(1)
                .saturating_sub(mission.plan.nodes.len());
            assert_eq!(cycle_rank, mission.plan.forks.len());
        }
    }

    #[test]
    fn raw_derivations_cover_zero_one_and_multiple_cuts_and_all_port_counts() {
        let mut cut_counts = BTreeSet::new();
        let mut port_counts = BTreeSet::new();
        let mut fork_counts = BTreeSet::new();
        let mut first_anchors = BTreeSet::new();
        for seed in 0..512 {
            let mission =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .derive_mission()
                    .unwrap();
            cut_counts.insert(mission.plan.cuts.len());
            port_counts.insert(mission.plan.ports.len());
            fork_counts.insert(mission.plan.forks.len());
            if let Some(first) = mission.plan.cuts.first() {
                first_anchors.insert(first.anchor);
                for pair in mission.plan.cuts.windows(2) {
                    assert_eq!(pair[0].anchor.opposite(), pair[1].anchor);
                    assert!(pair[1].spine_index - pair[0].spine_index >= 3);
                }
            }
        }
        assert_eq!(cut_counts, BTreeSet::from([0, 1, 2, 3]));
        assert_eq!(port_counts, BTreeSet::from([2, 3, 4]));
        assert_eq!(fork_counts, BTreeSet::from([0, 1, 2]));
        assert_eq!(
            first_anchors,
            BTreeSet::from([CutAnchor::West, CutAnchor::East])
        );
    }

    #[test]
    fn coordinate_free_topology_is_not_seed_or_id_jitter() {
        let mut topology = HashSet::new();
        let mut derivations = HashSet::new();
        for seed in 0..128 {
            let mission =
                CompositionalRouteCutKey::new(seed, AbilitySet::ALL, ChallengeIntent::Standard)
                    .derive_mission()
                    .unwrap();
            topology.insert(mission.topology_signature());
            derivations.insert(mission.derivation_signature());
        }
        assert!(
            topology.len() >= 116,
            "expected at least 90% unique coordinate-free topologies, got {}",
            topology.len()
        );
        assert!(
            derivations.len() >= 116,
            "expected at least 90% unique rewrite histories, got {}",
            derivations.len()
        );
    }

    #[test]
    fn invalid_attempt_is_typed_and_never_silently_retried() {
        let key = CompositionalRouteCutKey::new(0, AbilitySet::NONE, ChallengeIntent::Gentle)
            .with_embedding(
                CompositionalRouteCutGrammar::RecursiveMissionCutsV1,
                COMPOSITIONAL_ROUTE_CUT_MAX_EMBEDDING_ATTEMPT + 1,
            );
        assert!(matches!(
            key.derive_mission(),
            Err(MissionDerivationError {
                cause: MissionDerivationFailure::UnsupportedEmbeddingAttempt { .. },
                ..
            })
        ));
    }

    fn connected_nodes(plan: &MissionPlan) -> usize {
        let mut reached = HashSet::from([plan.spine[0]]);
        let mut frontier = vec![plan.spine[0]];
        while let Some(node) = frontier.pop() {
            for edge in &plan.edges {
                let adjacent = if edge.from == node {
                    Some(edge.to)
                } else if edge.to == node {
                    Some(edge.from)
                } else {
                    None
                };
                if let Some(adjacent) = adjacent
                    && reached.insert(adjacent)
                {
                    frontier.push(adjacent);
                }
            }
        }
        reached.len()
    }
}
