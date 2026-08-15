//! Coordinate-free ability gates and their separate physical mapping.
//!
//! The graph rewrite replaces selected edges in an already-derived
//! [`MissionPlan`] with directional traversal contracts.  Selected edges are
//! graph bridges on the source-to-sink route, so a mission fork cannot bypass
//! them.  A separately versioned generator then reserves the corresponding
//! local geometry during constrained embedding.  Construction never promotes
//! itself to a gameplay claim: authoritative positive replay and the explicit
//! reduced-loadout bypass audit remain separate evidence.

use std::{collections::VecDeque, error::Error, fmt};

use downwards_core::{AbilitySet, Rect};

use super::{
    BoundaryPort, CompositionalRouteCutGenerationFailure, CompositionalRouteCutKey, DerivedMission,
    MissionDerivationFailure, MissionEdge, MissionEdgeKind, MissionNodeKind, MissionPlan,
    MissionRouteNodeMapping, RouteCutRealization, RoutePlan, RoutePlanSummary, SupportSpec,
    common::StableRng,
};
use crate::GeneratedLevel;

/// Version of the deterministic base-mission-to-gated-graph rewrite.
pub const COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION: u32 = 1;

/// Version of the contract between the graph rewrite and a future embedder.
pub const COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION: u32 = 1;

/// Version of the separate graph-rewrite-to-physical-room mapping.
pub const COMPOSITIONAL_ABILITY_GENERATION_VERSION: u32 = 2;

/// Hard bound on the finite spine-edge set examined by one exact rewrite.
pub const COMPOSITIONAL_ABILITY_EDGE_SEARCH_LIMIT: u16 = 64;

const ABILITY_REWRITE_RNG_STREAM: u64 = 0x4142_494c_4954_5931;
const MINIMUM_COMBINED_GATE_EDGE_SEPARATION: usize = 2;

/// An honest construction profile: each variant adds at least one structural
/// ability gate to a baseline-derived mission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompositionalAbilityGateProfile {
    WallJump,
    Dash,
    Both,
}

impl CompositionalAbilityGateProfile {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
            Self::Both => "wall-jump-and-dash",
        }
    }

    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        match self {
            Self::WallJump => AbilitySet::new(true, false),
            Self::Dash => AbilitySet::new(false, true),
            Self::Both => AbilitySet::ALL,
        }
    }

    const fn gate_count(self) -> usize {
        match self {
            Self::WallJump | Self::Dash => 1,
            Self::Both => 2,
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::WallJump => 0,
            Self::Dash => 1,
            Self::Both => 2,
        }
    }
}

/// One primitive capability required by a directed mission edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GateAbility {
    WallJump,
    Dash,
}

impl GateAbility {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
        }
    }

    const fn is_available(self, abilities: AbilitySet) -> bool {
        match self {
            Self::WallJump => abilities.wall_jump,
            Self::Dash => abilities.dash,
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::WallJump => 1,
            Self::Dash => 2,
        }
    }
}

/// Requirement on one direction of a rewritten mission edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DirectedTraversalRequirement {
    Baseline,
    Ability(GateAbility),
}

impl DirectedTraversalRequirement {
    const fn is_available(self, abilities: AbilitySet) -> bool {
        match self {
            Self::Baseline => true,
            Self::Ability(ability) => ability.is_available(abilities),
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::Baseline => 0,
            Self::Ability(ability) => ability.tag(),
        }
    }
}

/// An original mission edge with independent forward and reverse contracts.
///
/// The base mission stores its authored source-to-sink direction in `edge`.
/// All ordinary reverse traversals are baseline here.  A gate replaces only
/// the forward traversal; the future embedder must make that direction a
/// physical ascent and preserve a baseline drop in reverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirectedMissionEdge {
    pub edge: MissionEdge,
    pub forward_requirement: DirectedTraversalRequirement,
    pub reverse_requirement: DirectedTraversalRequirement,
}

/// Local geometry primitive which a constrained embedder must reserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AbilityGateGeometry {
    PairedWallShaft,
    DashRiseTransfer,
}

/// The smallest existing compositional embedding seam able to own a gate.
///
/// The current v2 embedder chooses rows, then independent support domains,
/// then rasterizes.  A gate cannot safely be painted afterward: its reserved
/// volume must participate in row and support backtracking and in every
/// collision/cut/port check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompositionalAbilityEmbeddingSeam {
    SpineRowsAndPairedSupportDomainsBeforeRasterization,
}

/// A structural reservation request.  These numeric envelopes are generation
/// inputs, not physics or reachability proofs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AbilityGateEmbeddingContract {
    pub version: u32,
    pub seam: CompositionalAbilityEmbeddingSeam,
    pub geometry: AbilityGateGeometry,
    pub minimum_ascent_rows: u16,
    pub minimum_clear_interior_width_tiles: u16,
    pub reserve_empty_transfer_volume: bool,
    pub forbid_intermediate_supports: bool,
    pub forbid_wall_contact_in_ascent: bool,
    pub preserve_all_incident_route_edges: bool,
    pub require_baseline_reverse_descent: bool,
}

impl AbilityGateEmbeddingContract {
    const fn for_ability(ability: GateAbility) -> Self {
        match ability {
            GateAbility::WallJump => Self {
                version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
                seam: CompositionalAbilityEmbeddingSeam::
                    SpineRowsAndPairedSupportDomainsBeforeRasterization,
                geometry: AbilityGateGeometry::PairedWallShaft,
                // Four rows is outside the v2 baseline rise envelope and
                // leaves room for more than one wall contact.
                minimum_ascent_rows: 4,
                // The physical player is narrower than one tile; four clear
                // tiles retain substantial non-frame-perfect lateral margin.
                minimum_clear_interior_width_tiles: 4,
                reserve_empty_transfer_volume: true,
                forbid_intermediate_supports: true,
                forbid_wall_contact_in_ascent: false,
                preserve_all_incident_route_edges: true,
                require_baseline_reverse_descent: true,
            },
            GateAbility::Dash => Self {
                version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
                seam: CompositionalAbilityEmbeddingSeam::
                    SpineRowsAndPairedSupportDomainsBeforeRasterization,
                geometry: AbilityGateGeometry::DashRiseTransfer,
                // The earlier isolated dash primitive uses five rows; retain
                // that conservative construction envelope here.
                minimum_ascent_rows: 5,
                minimum_clear_interior_width_tiles: 3,
                reserve_empty_transfer_volume: true,
                forbid_intermediate_supports: true,
                // In a combined profile, nearby walls could turn the dash
                // cut into a wall-jump bypass.  The reservation forbids them.
                forbid_wall_contact_in_ascent: true,
                preserve_all_incident_route_edges: true,
                require_baseline_reverse_descent: true,
            },
        }
    }
}

/// Why this graph-only candidate is not yet a physical ability claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AbilityGateEmbeddingPendingReason {
    ReservedGeometryNotEmbedded,
    AuthoritativePositiveReplayNotObserved,
    ReducedLoadoutBypassAuditNotObserved,
}

/// Explicit evidence state for the not-yet-embedded graph rewrite.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AbilityGateEmbeddingState {
    Pending {
        reasons: Vec<AbilityGateEmbeddingPendingReason>,
    },
}

impl AbilityGateEmbeddingState {
    fn graph_only() -> Self {
        Self::Pending {
            reasons: vec![
                AbilityGateEmbeddingPendingReason::ReservedGeometryNotEmbedded,
                AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
            ],
        }
    }

    pub(super) fn geometry_embedded_replay_pending() -> Self {
        Self::Pending {
            reasons: vec![
                AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
            ],
        }
    }
}

/// One selected bridge in traversal order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirectedAbilityGate {
    pub ordinal: u16,
    pub spine_edge_index: u16,
    pub mission_edge_index: u16,
    pub ascent_from: u16,
    pub ascent_to: u16,
    pub required_ability: GateAbility,
    pub embedding_contract: AbilityGateEmbeddingContract,
}

/// Exhaustive finite-graph evidence for one gate.
///
/// Each reachable set is sorted by mission-node ID.  Absence of the sink (or
/// source for the reverse check) follows from an exhaustive breadth-first
/// traversal of the complete rewritten graph, not from sampled paths.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GateUnavoidabilityCertificate {
    pub gate_ordinal: u16,
    pub source_reachable_after_edge_deletion: Vec<u16>,
    pub source_reachable_without_required_ability: Vec<u16>,
    pub sink_reachable_with_baseline_reverse_traversal: Vec<u16>,
}

/// Graph-level mission after edge replacement.  The original plan remains
/// intact and auditable; `edges` has exactly one entry per original edge.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbilityRewrittenMissionPlan {
    pub base: MissionPlan,
    pub edges: Vec<DirectedMissionEdge>,
    pub gates: Vec<DirectedAbilityGate>,
}

impl AbilityRewrittenMissionPlan {
    #[must_use]
    pub fn source_node_id(&self) -> u16 {
        self.base.spine[0]
    }

    #[must_use]
    pub fn sink_node_id(&self) -> u16 {
        *self
            .base
            .spine
            .last()
            .expect("validated rewritten missions have a sink")
    }

    /// Exact graph reachability under one loadout.  This does not imply
    /// physical room reachability.
    #[must_use]
    pub fn graph_can_reach(&self, from: u16, to: u16, abilities: AbilitySet) -> bool {
        reachable_nodes(&self.edges, from, abilities, None)
            .get(usize::from(to))
            .copied()
            .unwrap_or(false)
    }

    /// Recompute the bridge-deletion claim retained in a certificate.
    #[must_use]
    pub fn graph_can_reach_without_gate_edge(&self, gate_ordinal: u16) -> Option<bool> {
        let gate = self
            .gates
            .iter()
            .find(|gate| gate.ordinal == gate_ordinal)?;
        let reachable = reachable_nodes(
            &self.edges,
            self.source_node_id(),
            AbilitySet::ALL,
            Some(usize::from(gate.mission_edge_index)),
        );
        Some(reachable[usize::from(self.sink_node_id())])
    }
}

/// Exact request identity for one edge-rewrite arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalAbilityEdgeRewriteKey {
    pub base_key: CompositionalRouteCutKey,
    pub profile: CompositionalAbilityGateProfile,
    /// Index into the finite deterministically shuffled arrangement set.
    pub rewrite_attempt: u16,
}

impl CompositionalAbilityEdgeRewriteKey {
    #[must_use]
    pub const fn new(
        base_key: CompositionalRouteCutKey,
        profile: CompositionalAbilityGateProfile,
    ) -> Self {
        Self {
            base_key,
            profile,
            rewrite_attempt: 0,
        }
    }

    #[must_use]
    pub const fn with_rewrite_attempt(mut self, rewrite_attempt: u16) -> Self {
        self.rewrite_attempt = rewrite_attempt;
        self
    }

    pub fn rewrite(
        self,
        mission: &DerivedMission,
    ) -> Result<AbilityRewrittenMission, CompositionalAbilityEdgeRewriteError> {
        rewrite_compositional_mission_ability_edges(self, mission)
    }
}

/// Exact graph-rewrite provenance, independent of future coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbilityEdgeRewriteProvenance {
    pub rewrite_version: u32,
    pub base_topology_signature: u64,
    pub base_derivation_signature: u64,
    pub eligible_bridge_count: u16,
    pub arrangement_count: u16,
    pub rewritten_topology_signature: u64,
    pub rewrite_signature: u64,
}

/// A graph-only ability candidate.  It intentionally contains no room.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbilityRewrittenMission {
    pub key: CompositionalAbilityEdgeRewriteKey,
    pub base_mission: DerivedMission,
    pub plan: AbilityRewrittenMissionPlan,
    pub provenance: AbilityEdgeRewriteProvenance,
    pub certificates: Vec<GateUnavoidabilityCertificate>,
    pub embedding_state: AbilityGateEmbeddingState,
}

impl AbilityRewrittenMission {
    #[must_use]
    pub const fn topology_signature(&self) -> u64 {
        self.provenance.rewritten_topology_signature
    }

    #[must_use]
    pub const fn rewrite_signature(&self) -> u64 {
        self.provenance.rewrite_signature
    }
}

/// Exact request for the separately versioned physical ability mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalAbilityGenerationKey {
    pub rewrite_key: CompositionalAbilityEdgeRewriteKey,
}

impl CompositionalAbilityGenerationKey {
    #[must_use]
    pub const fn new(
        source_seed: u64,
        profile: CompositionalAbilityGateProfile,
        intent: super::ChallengeIntent,
    ) -> Self {
        Self {
            rewrite_key: CompositionalAbilityEdgeRewriteKey::new(
                CompositionalRouteCutKey::new(source_seed, AbilitySet::NONE, intent),
                profile,
            ),
        }
    }

    #[must_use]
    pub const fn with_attempts(mut self, embedding_attempt: u8, rewrite_attempt: u16) -> Self {
        self.rewrite_key.base_key.embedding_attempt = embedding_attempt;
        self.rewrite_key.rewrite_attempt = rewrite_attempt;
        self
    }

    pub fn generate(
        self,
    ) -> Result<CompositionalAbilityCandidate, CompositionalAbilityGenerationError> {
        generate_compositional_ability_candidate(self)
    }
}

/// One exact tile reservation owned by a local gate realization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AbilityGateTileCell {
    pub x: u16,
    pub row: u16,
}

/// Physical realization of one graph-certified directed gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbilityGateRealization {
    pub gate: DirectedAbilityGate,
    pub from_route_node_id: u16,
    pub to_route_node_id: u16,
    pub lower_support: SupportSpec,
    pub upper_support: SupportSpec,
    pub ascent_bounds: Rect,
    pub lower_standing_bounds: Rect,
    pub upper_standing_bounds: Rect,
    pub required_solid_tiles: Vec<AbilityGateTileCell>,
    pub required_empty_tiles: Vec<AbilityGateTileCell>,
}

/// Constructive facts from the separate physical mapping.  They remain
/// construction evidence until authoritative replay is attached externally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalAbilityEmbeddingSummary {
    pub generation_version: u32,
    pub graph_rewrite_version: u32,
    pub embedding_attempt: u8,
    pub rewrite_attempt: u16,
    pub ascent_edges: u16,
    pub descent_edges: u16,
    pub level_edges: u16,
    pub horizontal_direction_reversals: u16,
    pub cut_realizations: Vec<RouteCutRealization>,
    pub socket_columns: Vec<u16>,
    pub gate_realizations: Vec<AbilityGateRealization>,
}

/// A structurally valid physical room from the ability-edge mapping.
/// Gameplay reachability and ability use are intentionally still pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalAbilityCandidate {
    pub key: CompositionalAbilityGenerationKey,
    pub rewritten_mission: AbilityRewrittenMission,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
    pub mission_route_nodes: Vec<MissionRouteNodeMapping>,
    pub embedding: CompositionalAbilityEmbeddingSummary,
    pub evidence_state: AbilityGateEmbeddingState,
}

/// Bounded phase of the separate gate-aware constraint search.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompositionalAbilityEmbeddingPhase {
    Rhythm,
    Spine,
    Fork,
    FinalGateContract,
    RasterizedGateContract,
}

impl CompositionalAbilityEmbeddingPhase {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Rhythm => "rhythm",
            Self::Spine => "spine",
            Self::Fork => "fork",
            Self::FinalGateContract => "final-gate-contract",
            Self::RasterizedGateContract => "rasterized-gate-contract",
        }
    }
}

/// Exact local contract which rejected a proposed gate realization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AbilityGateGeometryViolation {
    MissingGateEdge,
    AscentEnvelope,
    ReverseBaselineContract,
    EndpointMaterial,
    IntermediateSupportSurface,
    ReservedVolumeBlocked,
    CutRowIntersected,
    IncidentRouteBlocked,
    BoundaryArrivalBlocked,
    RasterMismatch,
}

impl fmt::Display for AbilityGateGeometryViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slug = match self {
            Self::MissingGateEdge => "missing-gate-edge",
            Self::AscentEnvelope => "ascent-envelope",
            Self::ReverseBaselineContract => "reverse-baseline-contract",
            Self::EndpointMaterial => "endpoint-material",
            Self::IntermediateSupportSurface => "intermediate-support-surface",
            Self::ReservedVolumeBlocked => "reserved-volume-blocked",
            Self::CutRowIntersected => "cut-row-intersected",
            Self::IncidentRouteBlocked => "incident-route-blocked",
            Self::BoundaryArrivalBlocked => "boundary-arrival-blocked",
            Self::RasterMismatch => "raster-mismatch",
        };
        formatter.write_str(slug)
    }
}

/// Typed failure of one exact physical ability-generation key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositionalAbilityGenerationFailure {
    Mission(MissionDerivationFailure),
    Rewrite(CompositionalAbilityEdgeRewriteFailure),
    BaselineEmbedding(CompositionalRouteCutGenerationFailure),
    ConstraintSearchExhausted {
        phase: CompositionalAbilityEmbeddingPhase,
        explored: u32,
    },
    GateContract {
        gate_ordinal: u16,
        violation: AbilityGateGeometryViolation,
    },
}

impl fmt::Display for CompositionalAbilityGenerationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mission(error) => write!(formatter, "base mission derivation failed: {error}"),
            Self::Rewrite(error) => write!(formatter, "ability graph rewrite failed: {error}"),
            Self::BaselineEmbedding(error) => {
                write!(
                    formatter,
                    "shared compositional embedding operation failed: {error}"
                )
            }
            Self::ConstraintSearchExhausted { phase, explored } => write!(
                formatter,
                "{} constraint search exhausted after {explored} candidates",
                phase.slug()
            ),
            Self::GateContract {
                gate_ordinal,
                violation,
            } => write!(
                formatter,
                "gate {gate_ordinal} failed physical contract {violation}"
            ),
        }
    }
}

impl Error for CompositionalAbilityGenerationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Mission(error) => Some(error),
            Self::Rewrite(error) => Some(error),
            Self::BaselineEmbedding(error) => Some(error),
            Self::ConstraintSearchExhausted { .. } | Self::GateContract { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalAbilityGenerationError {
    pub key: CompositionalAbilityGenerationKey,
    pub cause: CompositionalAbilityGenerationFailure,
}

impl fmt::Display for CompositionalAbilityGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compositional ability room v{} {} embedding-attempt {} rewrite-attempt {} seed {:016x} failed: {}",
            COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            self.key.rewrite_key.profile.slug(),
            self.key.rewrite_key.base_key.embedding_attempt,
            self.key.rewrite_key.rewrite_attempt,
            self.key.rewrite_key.base_key.source_seed,
            self.cause,
        )
    }
}

impl Error for CompositionalAbilityGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Generate one exact physical ability candidate without hidden retries.
pub fn generate_compositional_ability_candidate(
    key: CompositionalAbilityGenerationKey,
) -> Result<CompositionalAbilityCandidate, CompositionalAbilityGenerationError> {
    let mission = key.rewrite_key.base_key.derive_mission().map_err(|error| {
        CompositionalAbilityGenerationError {
            key,
            cause: CompositionalAbilityGenerationFailure::Mission(error.cause),
        }
    })?;
    let rewritten_mission =
        key.rewrite_key
            .rewrite(&mission)
            .map_err(|error| CompositionalAbilityGenerationError {
                key,
                cause: CompositionalAbilityGenerationFailure::Rewrite(error.cause),
            })?;
    super::compositional_route_cut::embed_ability_rewritten_mission(key, rewritten_mission)
        .map_err(|cause| CompositionalAbilityGenerationError { key, cause })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissionPlanValidationFailure {
    EmptySpine,
    NodeCountExceedsIdSpace,
    EdgeCountExceedsIdSpace,
    NonContiguousNodeId { expected: u16, actual: u16 },
    EdgeEndpointMissing { edge_index: u16, node_id: u16 },
    MissingCriticalSpineEdge { spine_edge_index: u16 },
    DuplicateCriticalSpineEdge { spine_edge_index: u16 },
    SourcePortMismatch,
    SinkPortMismatch,
}

impl fmt::Display for MissionPlanValidationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySpine => write!(formatter, "mission spine is empty"),
            Self::NodeCountExceedsIdSpace => write!(formatter, "mission node count exceeds u16"),
            Self::EdgeCountExceedsIdSpace => write!(formatter, "mission edge count exceeds u16"),
            Self::NonContiguousNodeId { expected, actual } => write!(
                formatter,
                "mission node IDs are not contiguous: expected {expected}, found {actual}"
            ),
            Self::EdgeEndpointMissing {
                edge_index,
                node_id,
            } => write!(
                formatter,
                "mission edge {edge_index} references missing node {node_id}"
            ),
            Self::MissingCriticalSpineEdge { spine_edge_index } => write!(
                formatter,
                "spine transition {spine_edge_index} has no critical spine edge"
            ),
            Self::DuplicateCriticalSpineEdge { spine_edge_index } => write!(
                formatter,
                "spine transition {spine_edge_index} has duplicate critical spine edges"
            ),
            Self::SourcePortMismatch => {
                write!(formatter, "port ordinal zero does not own the spine source")
            }
            Self::SinkPortMismatch => {
                write!(formatter, "port ordinal one does not own the spine sink")
            }
        }
    }
}

/// Finite, typed failure of one exact graph rewrite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositionalAbilityEdgeRewriteFailure {
    KeyDoesNotMatchMission,
    BaseMissionIsNotBaseline {
        actual: AbilitySet,
    },
    InvalidMission(MissionPlanValidationFailure),
    CandidateSearchBoundExceeded {
        maximum: u16,
        actual: u16,
    },
    InsufficientUnavoidableEdges {
        required: u16,
        eligible: u16,
    },
    NoSeparatedUnavoidableEdgeArrangement {
        minimum_edge_separation: u16,
        eligible: u16,
    },
    ArrangementAttemptExhausted {
        requested: u16,
        available: u16,
    },
    CertificationFailed {
        gate_ordinal: u16,
        check: GateCertificationCheck,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GateCertificationCheck {
    EdgeDeletionDidNotDisconnect,
    MissingAbilityStillReachedSink,
    IntendedProfileCouldNotReachSink,
    BaselineReverseCouldNotReachSource,
}

impl fmt::Display for GateCertificationCheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EdgeDeletionDidNotDisconnect => {
                write!(
                    formatter,
                    "deleting the selected edge did not disconnect source and sink"
                )
            }
            Self::MissingAbilityStillReachedSink => {
                write!(
                    formatter,
                    "the graph still reached the sink without the required ability"
                )
            }
            Self::IntendedProfileCouldNotReachSink => {
                write!(
                    formatter,
                    "the intended profile could not traverse the rewritten graph"
                )
            }
            Self::BaselineReverseCouldNotReachSource => {
                write!(
                    formatter,
                    "baseline reverse traversal could not return to the source"
                )
            }
        }
    }
}

impl fmt::Display for CompositionalAbilityEdgeRewriteFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyDoesNotMatchMission => write!(formatter, "rewrite key does not match mission"),
            Self::BaseMissionIsNotBaseline { actual } => write!(
                formatter,
                "ability rewrites require a baseline-derived mission, found {actual:?}"
            ),
            Self::InvalidMission(cause) => write!(formatter, "invalid base mission: {cause}"),
            Self::CandidateSearchBoundExceeded { maximum, actual } => write!(
                formatter,
                "ability-edge search bound {maximum} is below the {actual} spine transitions"
            ),
            Self::InsufficientUnavoidableEdges { required, eligible } => write!(
                formatter,
                "ability profile requires {required} unavoidable edges, found {eligible}"
            ),
            Self::NoSeparatedUnavoidableEdgeArrangement {
                minimum_edge_separation,
                eligible,
            } => write!(
                formatter,
                "none of the {eligible} unavoidable edges form a pair separated by at least {minimum_edge_separation} spine edges"
            ),
            Self::ArrangementAttemptExhausted {
                requested,
                available,
            } => write!(
                formatter,
                "rewrite attempt {requested} is outside the {available} exact arrangements"
            ),
            Self::CertificationFailed {
                gate_ordinal,
                check,
            } => write!(
                formatter,
                "gate {gate_ordinal} certification failed: {check}"
            ),
        }
    }
}

impl Error for CompositionalAbilityEdgeRewriteFailure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalAbilityEdgeRewriteError {
    pub key: CompositionalAbilityEdgeRewriteKey,
    pub cause: CompositionalAbilityEdgeRewriteFailure,
}

impl fmt::Display for CompositionalAbilityEdgeRewriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "compositional ability edge rewrite v{} {} attempt {} seed {:016x} failed: {}",
            COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            self.key.profile.slug(),
            self.key.rewrite_attempt,
            self.key.base_key.source_seed,
            self.cause,
        )
    }
}

impl Error for CompositionalAbilityEdgeRewriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Replace exact bridge edges in `mission` without mutating or re-deriving it.
pub fn rewrite_compositional_mission_ability_edges(
    key: CompositionalAbilityEdgeRewriteKey,
    mission: &DerivedMission,
) -> Result<AbilityRewrittenMission, CompositionalAbilityEdgeRewriteError> {
    rewrite_ability_edges(key, mission)
        .map_err(|cause| CompositionalAbilityEdgeRewriteError { key, cause })
}

fn rewrite_ability_edges(
    key: CompositionalAbilityEdgeRewriteKey,
    mission: &DerivedMission,
) -> Result<AbilityRewrittenMission, CompositionalAbilityEdgeRewriteFailure> {
    if key.base_key != mission.key {
        return Err(CompositionalAbilityEdgeRewriteFailure::KeyDoesNotMatchMission);
    }
    if mission.key.construction_abilities != AbilitySet::NONE {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::BaseMissionIsNotBaseline {
                actual: mission.key.construction_abilities,
            },
        );
    }
    validate_mission_plan(&mission.plan)
        .map_err(CompositionalAbilityEdgeRewriteFailure::InvalidMission)?;

    let transition_count = mission.plan.spine.len() - 1;
    let actual = u16::try_from(transition_count).unwrap_or(u16::MAX);
    if actual > COMPOSITIONAL_ABILITY_EDGE_SEARCH_LIMIT {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::CandidateSearchBoundExceeded {
                maximum: COMPOSITIONAL_ABILITY_EDGE_SEARCH_LIMIT,
                actual,
            },
        );
    }

    let eligible = eligible_unavoidable_edges(&mission.plan);
    if eligible.len() < key.profile.gate_count() {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::InsufficientUnavoidableEdges {
                required: key.profile.gate_count().try_into().unwrap_or(u16::MAX),
                eligible: eligible.len().try_into().unwrap_or(u16::MAX),
            },
        );
    }
    let arrangements = gate_arrangements(&eligible, key.profile);
    if arrangements.is_empty() {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::NoSeparatedUnavoidableEdgeArrangement {
                minimum_edge_separation: MINIMUM_COMBINED_GATE_EDGE_SEPARATION
                    .try_into()
                    .expect("small gate separation fits u16"),
                eligible: eligible.len().try_into().unwrap_or(u16::MAX),
            },
        );
    }

    let arrangement_count = arrangements.len().try_into().unwrap_or(u16::MAX);
    let mut arrangement_order = (0..arrangements.len()).collect::<Vec<_>>();
    let mut rng = StableRng::new(
        mission.key.source_seed
            ^ mission.topology_signature().rotate_left(17)
            ^ mission.derivation_signature().rotate_right(11)
            ^ u64::from(key.profile.tag()),
        ABILITY_REWRITE_RNG_STREAM,
    );
    shuffle(&mut arrangement_order, &mut rng);
    let Some(&selected_arrangement_index) = arrangement_order.get(usize::from(key.rewrite_attempt))
    else {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::ArrangementAttemptExhausted {
                requested: key.rewrite_attempt,
                available: arrangement_count,
            },
        );
    };
    let selected = &arrangements[selected_arrangement_index];

    // The selected pair is already sorted in spine order.  Ability assignment
    // is an independent deterministic coin so combined rooms contain both
    // serial orders across seeds rather than always teaching one first.
    let abilities = match key.profile {
        CompositionalAbilityGateProfile::WallJump => vec![GateAbility::WallJump],
        CompositionalAbilityGateProfile::Dash => vec![GateAbility::Dash],
        CompositionalAbilityGateProfile::Both if rng.coin() => {
            vec![GateAbility::WallJump, GateAbility::Dash]
        }
        CompositionalAbilityGateProfile::Both => {
            vec![GateAbility::Dash, GateAbility::WallJump]
        }
    };

    let mut edges = mission
        .plan
        .edges
        .iter()
        .copied()
        .map(|edge| DirectedMissionEdge {
            edge,
            forward_requirement: DirectedTraversalRequirement::Baseline,
            reverse_requirement: DirectedTraversalRequirement::Baseline,
        })
        .collect::<Vec<_>>();
    let mut gates = Vec::with_capacity(selected.len());
    for (ordinal, (candidate, ability)) in selected.iter().zip(abilities).enumerate() {
        edges[candidate.mission_edge_index].forward_requirement =
            DirectedTraversalRequirement::Ability(ability);
        gates.push(DirectedAbilityGate {
            ordinal: ordinal.try_into().expect("at most two gates fit u16"),
            spine_edge_index: candidate
                .spine_edge_index
                .try_into()
                .expect("bounded spine index fits u16"),
            mission_edge_index: candidate
                .mission_edge_index
                .try_into()
                .expect("bounded mission edge index fits u16"),
            ascent_from: candidate.from,
            ascent_to: candidate.to,
            required_ability: ability,
            embedding_contract: AbilityGateEmbeddingContract::for_ability(ability),
        });
    }
    let plan = AbilityRewrittenMissionPlan {
        base: mission.plan.clone(),
        edges,
        gates,
    };
    let certificates = certify_gates(&plan, key.profile)?;
    let rewritten_topology_signature = rewritten_topology_signature(mission, &plan.gates);
    let rewrite_signature = exact_rewrite_signature(key, mission, &plan.gates);
    Ok(AbilityRewrittenMission {
        key,
        base_mission: mission.clone(),
        plan,
        provenance: AbilityEdgeRewriteProvenance {
            rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            base_topology_signature: mission.topology_signature(),
            base_derivation_signature: mission.derivation_signature(),
            eligible_bridge_count: eligible.len().try_into().unwrap_or(u16::MAX),
            arrangement_count,
            rewritten_topology_signature,
            rewrite_signature,
        },
        certificates,
        embedding_state: AbilityGateEmbeddingState::graph_only(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GateCandidate {
    spine_edge_index: usize,
    mission_edge_index: usize,
    from: u16,
    to: u16,
}

fn eligible_unavoidable_edges(plan: &MissionPlan) -> Vec<GateCandidate> {
    let source = plan.spine[0];
    let sink = *plan.spine.last().expect("validated plan has a sink");
    plan.spine
        .windows(2)
        .enumerate()
        // Boundary-owned nodes and cut shelves need their existing port/cut
        // geometry.  Reserve gates only on a genuinely local interior seam.
        .filter(|(index, pair)| {
            *index > 0
                && index + 2 < plan.spine.len()
                && pair.iter().all(|node_id| {
                    plan.ports.iter().all(|port| port.node_id != *node_id)
                        && plan.nodes[usize::from(*node_id)].kind != MissionNodeKind::Cut
                })
        })
        .filter_map(|(spine_edge_index, pair)| {
            let matching = plan
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| {
                    edge.from == pair[0]
                        && edge.to == pair[1]
                        && edge.kind == MissionEdgeKind::Spine
                        && edge.critical
                })
                .collect::<Vec<_>>();
            let [(mission_edge_index, _)] = matching.as_slice() else {
                return None;
            };
            let deleted = reachable_nodes_for_base(plan, source, Some(*mission_edge_index));
            (!deleted[usize::from(sink)]).then_some(GateCandidate {
                spine_edge_index,
                mission_edge_index: *mission_edge_index,
                from: pair[0],
                to: pair[1],
            })
        })
        .collect()
}

fn gate_arrangements(
    eligible: &[GateCandidate],
    profile: CompositionalAbilityGateProfile,
) -> Vec<Vec<GateCandidate>> {
    match profile {
        CompositionalAbilityGateProfile::WallJump | CompositionalAbilityGateProfile::Dash => {
            eligible.iter().copied().map(|edge| vec![edge]).collect()
        }
        CompositionalAbilityGateProfile::Both => eligible
            .iter()
            .enumerate()
            .flat_map(|(first_index, &first)| {
                eligible[first_index + 1..]
                    .iter()
                    .copied()
                    .filter(move |second| {
                        second.spine_edge_index - first.spine_edge_index
                            >= MINIMUM_COMBINED_GATE_EDGE_SEPARATION
                    })
                    .map(move |second| vec![first, second])
            })
            .collect(),
    }
}

fn certify_gates(
    plan: &AbilityRewrittenMissionPlan,
    profile: CompositionalAbilityGateProfile,
) -> Result<Vec<GateUnavoidabilityCertificate>, CompositionalAbilityEdgeRewriteFailure> {
    let source = plan.source_node_id();
    let sink = plan.sink_node_id();
    if !plan.graph_can_reach(source, sink, profile.abilities()) {
        return Err(
            CompositionalAbilityEdgeRewriteFailure::CertificationFailed {
                gate_ordinal: 0,
                check: GateCertificationCheck::IntendedProfileCouldNotReachSink,
            },
        );
    }
    let mut certificates = Vec::with_capacity(plan.gates.len());
    for gate in &plan.gates {
        let deleted = reachable_nodes(
            &plan.edges,
            source,
            AbilitySet::ALL,
            Some(usize::from(gate.mission_edge_index)),
        );
        if deleted[usize::from(sink)] {
            return Err(
                CompositionalAbilityEdgeRewriteFailure::CertificationFailed {
                    gate_ordinal: gate.ordinal,
                    check: GateCertificationCheck::EdgeDeletionDidNotDisconnect,
                },
            );
        }
        let mut missing_ability = profile.abilities();
        match gate.required_ability {
            GateAbility::WallJump => missing_ability.wall_jump = false,
            GateAbility::Dash => missing_ability.dash = false,
        }
        let missing = reachable_nodes(&plan.edges, source, missing_ability, None);
        if missing[usize::from(sink)] {
            return Err(
                CompositionalAbilityEdgeRewriteFailure::CertificationFailed {
                    gate_ordinal: gate.ordinal,
                    check: GateCertificationCheck::MissingAbilityStillReachedSink,
                },
            );
        }
        let reverse = reachable_nodes(&plan.edges, sink, AbilitySet::NONE, None);
        if !reverse[usize::from(source)] {
            return Err(
                CompositionalAbilityEdgeRewriteFailure::CertificationFailed {
                    gate_ordinal: gate.ordinal,
                    check: GateCertificationCheck::BaselineReverseCouldNotReachSource,
                },
            );
        }
        certificates.push(GateUnavoidabilityCertificate {
            gate_ordinal: gate.ordinal,
            source_reachable_after_edge_deletion: node_ids(&deleted),
            source_reachable_without_required_ability: node_ids(&missing),
            sink_reachable_with_baseline_reverse_traversal: node_ids(&reverse),
        });
    }
    Ok(certificates)
}

fn validate_mission_plan(plan: &MissionPlan) -> Result<(), MissionPlanValidationFailure> {
    if plan.spine.is_empty() {
        return Err(MissionPlanValidationFailure::EmptySpine);
    }
    if plan.nodes.len() > usize::from(u16::MAX) {
        return Err(MissionPlanValidationFailure::NodeCountExceedsIdSpace);
    }
    if plan.edges.len() > usize::from(u16::MAX) {
        return Err(MissionPlanValidationFailure::EdgeCountExceedsIdSpace);
    }
    for (expected, node) in plan.nodes.iter().enumerate() {
        let expected = expected
            .try_into()
            .map_err(|_| MissionPlanValidationFailure::NodeCountExceedsIdSpace)?;
        if node.id != expected {
            return Err(MissionPlanValidationFailure::NonContiguousNodeId {
                expected,
                actual: node.id,
            });
        }
    }
    for (edge_index, edge) in plan.edges.iter().enumerate() {
        for node_id in [edge.from, edge.to] {
            if usize::from(node_id) >= plan.nodes.len() {
                return Err(MissionPlanValidationFailure::EdgeEndpointMissing {
                    edge_index: edge_index.try_into().unwrap_or(u16::MAX),
                    node_id,
                });
            }
        }
    }
    for (spine_edge_index, pair) in plan.spine.windows(2).enumerate() {
        let matching = plan
            .edges
            .iter()
            .filter(|edge| {
                edge.from == pair[0]
                    && edge.to == pair[1]
                    && edge.kind == MissionEdgeKind::Spine
                    && edge.critical
            })
            .count();
        let spine_edge_index = spine_edge_index.try_into().unwrap_or(u16::MAX);
        match matching {
            0 => {
                return Err(MissionPlanValidationFailure::MissingCriticalSpineEdge {
                    spine_edge_index,
                });
            }
            1 => {}
            _ => {
                return Err(MissionPlanValidationFailure::DuplicateCriticalSpineEdge {
                    spine_edge_index,
                });
            }
        }
    }
    if !plan
        .ports
        .iter()
        .any(|port| port.ordinal == 0 && port.node_id == plan.spine[0])
    {
        return Err(MissionPlanValidationFailure::SourcePortMismatch);
    }
    if !plan
        .ports
        .iter()
        .any(|port| port.ordinal == 1 && Some(&port.node_id) == plan.spine.last())
    {
        return Err(MissionPlanValidationFailure::SinkPortMismatch);
    }
    Ok(())
}

fn reachable_nodes_for_base(
    plan: &MissionPlan,
    start: u16,
    skipped_edge: Option<usize>,
) -> Vec<bool> {
    let edges = plan
        .edges
        .iter()
        .copied()
        .map(|edge| DirectedMissionEdge {
            edge,
            forward_requirement: DirectedTraversalRequirement::Baseline,
            reverse_requirement: DirectedTraversalRequirement::Baseline,
        })
        .collect::<Vec<_>>();
    reachable_nodes(&edges, start, AbilitySet::ALL, skipped_edge)
}

fn reachable_nodes(
    edges: &[DirectedMissionEdge],
    start: u16,
    abilities: AbilitySet,
    skipped_edge: Option<usize>,
) -> Vec<bool> {
    let node_count = edges
        .iter()
        .map(|edge| edge.edge.from.max(edge.edge.to))
        .max()
        .map_or(usize::from(start) + 1, |maximum| usize::from(maximum) + 1)
        .max(usize::from(start) + 1);
    let mut reached = vec![false; node_count];
    let mut queue = VecDeque::from([start]);
    reached[usize::from(start)] = true;
    while let Some(node) = queue.pop_front() {
        for (edge_index, directed) in edges.iter().enumerate() {
            if skipped_edge == Some(edge_index) {
                continue;
            }
            let next = if directed.edge.from == node
                && directed.forward_requirement.is_available(abilities)
            {
                Some(directed.edge.to)
            } else if directed.edge.to == node
                && directed.reverse_requirement.is_available(abilities)
            {
                Some(directed.edge.from)
            } else {
                None
            };
            if let Some(next) = next
                && !reached[usize::from(next)]
            {
                reached[usize::from(next)] = true;
                queue.push_back(next);
            }
        }
    }
    reached
}

fn node_ids(reached: &[bool]) -> Vec<u16> {
    reached
        .iter()
        .enumerate()
        .filter(|(_, is_reached)| **is_reached)
        .map(|(node_id, _)| node_id.try_into().expect("validated mission IDs fit u16"))
        .collect()
}

fn rewritten_topology_signature(mission: &DerivedMission, gates: &[DirectedAbilityGate]) -> u64 {
    let mut signature = StableSignature::new();
    signature.u32(COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION);
    signature.u64(mission.topology_signature());
    signature.u16(gates.len().try_into().unwrap_or(u16::MAX));
    for gate in gates {
        signature.u16(gate.spine_edge_index);
        signature.byte(gate.required_ability.tag());
        signature.byte(DirectedTraversalRequirement::Baseline.tag());
    }
    signature.finish()
}

fn exact_rewrite_signature(
    key: CompositionalAbilityEdgeRewriteKey,
    mission: &DerivedMission,
    gates: &[DirectedAbilityGate],
) -> u64 {
    let mut signature = StableSignature::new();
    signature.u32(COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION);
    signature.u64(mission.derivation_signature());
    signature.byte(key.profile.tag());
    signature.u16(key.rewrite_attempt);
    signature.u16(gates.len().try_into().unwrap_or(u16::MAX));
    for gate in gates {
        signature.u16(gate.ordinal);
        signature.u16(gate.spine_edge_index);
        signature.u16(gate.mission_edge_index);
        signature.u16(gate.ascent_from);
        signature.u16(gate.ascent_to);
        signature.byte(gate.required_ability.tag());
    }
    signature.finish()
}

fn shuffle<T>(values: &mut [T], rng: &mut StableRng) {
    for index in (1..values.len()).rev() {
        let upper = u16::try_from(index + 1).expect("bounded arrangement set fits u16");
        let swap_with = usize::from(rng.below(upper));
        values.swap(index, swap_with);
    }
}

struct StableSignature {
    state: u64,
}

impl StableSignature {
    const fn new() -> Self {
        Self {
            state: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn byte(&mut self, value: u8) {
        self.state ^= u64::from(value);
        self.state = self.state.wrapping_mul(0x0000_0100_0000_01b3);
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

    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_core::Tile;

    use super::*;
    use crate::experimental::{ChallengeIntent, derive_compositional_route_cut_mission};

    fn base_mission(seed: u64) -> DerivedMission {
        derive_compositional_route_cut_mission(CompositionalRouteCutKey::new(
            seed,
            AbilitySet::NONE,
            ChallengeIntent::Standard,
        ))
        .expect("baseline mission derives")
    }

    fn first_success(
        profile: CompositionalAbilityGateProfile,
    ) -> (DerivedMission, AbilityRewrittenMission) {
        for seed in 0..512 {
            let mission = base_mission(seed);
            let key = CompositionalAbilityEdgeRewriteKey::new(mission.key, profile);
            if let Ok(rewritten) = key.rewrite(&mission) {
                return (mission, rewritten);
            }
        }
        panic!("sample contains no {profile:?} rewrite")
    }

    #[test]
    fn single_gate_profiles_replace_one_unavoidable_directed_edge() {
        for profile in [
            CompositionalAbilityGateProfile::WallJump,
            CompositionalAbilityGateProfile::Dash,
        ] {
            let (mission, rewritten) = first_success(profile);
            assert_eq!(rewritten.base_mission, mission);
            assert_eq!(rewritten.plan.base, mission.plan);
            assert_eq!(rewritten.plan.edges.len(), mission.plan.edges.len());
            assert_eq!(rewritten.plan.gates.len(), 1);
            assert_eq!(rewritten.certificates.len(), 1);
            let gate = &rewritten.plan.gates[0];
            let edge = rewritten.plan.edges[usize::from(gate.mission_edge_index)];
            assert_eq!(edge.edge.from, gate.ascent_from);
            assert_eq!(edge.edge.to, gate.ascent_to);
            assert_eq!(
                edge.forward_requirement,
                DirectedTraversalRequirement::Ability(gate.required_ability)
            );
            assert_eq!(
                edge.reverse_requirement,
                DirectedTraversalRequirement::Baseline
            );
            assert_eq!(
                rewritten
                    .plan
                    .graph_can_reach_without_gate_edge(gate.ordinal),
                Some(false)
            );
            assert!(rewritten.plan.graph_can_reach(
                rewritten.plan.source_node_id(),
                rewritten.plan.sink_node_id(),
                profile.abilities(),
            ));
            assert!(!rewritten.plan.graph_can_reach(
                rewritten.plan.source_node_id(),
                rewritten.plan.sink_node_id(),
                AbilitySet::NONE,
            ));
            assert!(rewritten.plan.graph_can_reach(
                rewritten.plan.sink_node_id(),
                rewritten.plan.source_node_id(),
                AbilitySet::NONE,
            ));
        }
    }

    #[test]
    fn combined_profile_serializes_distinct_separated_gates_in_both_orders() {
        let mut orders = HashSet::new();
        let mut successes = 0;
        for seed in 0..512 {
            let mission = base_mission(seed);
            let key = CompositionalAbilityEdgeRewriteKey::new(
                mission.key,
                CompositionalAbilityGateProfile::Both,
            );
            let Ok(rewritten) = key.rewrite(&mission) else {
                continue;
            };
            successes += 1;
            let [first, second] = rewritten.plan.gates.as_slice() else {
                panic!("combined profile has exactly two gates")
            };
            assert!(first.spine_edge_index < second.spine_edge_index);
            assert!(second.spine_edge_index - first.spine_edge_index >= 2);
            assert_ne!(first.mission_edge_index, second.mission_edge_index);
            assert_ne!(first.required_ability, second.required_ability);
            orders.insert((first.required_ability, second.required_ability));

            for gate in &rewritten.plan.gates {
                let mut missing = AbilitySet::ALL;
                match gate.required_ability {
                    GateAbility::WallJump => missing.wall_jump = false,
                    GateAbility::Dash => missing.dash = false,
                }
                assert!(!rewritten.plan.graph_can_reach(
                    rewritten.plan.source_node_id(),
                    rewritten.plan.sink_node_id(),
                    missing,
                ));
                assert_eq!(
                    rewritten
                        .plan
                        .graph_can_reach_without_gate_edge(gate.ordinal),
                    Some(false)
                );
            }
            assert!(rewritten.plan.graph_can_reach(
                rewritten.plan.sink_node_id(),
                rewritten.plan.source_node_id(),
                AbilitySet::NONE,
            ));
        }
        assert!(
            successes >= 64,
            "too few combined graph rewrites: {successes}"
        );
        assert_eq!(
            orders,
            HashSet::from([
                (GateAbility::WallJump, GateAbility::Dash),
                (GateAbility::Dash, GateAbility::WallJump),
            ])
        );
    }

    #[test]
    fn rewrites_are_deterministic_and_retain_exact_provenance() {
        for profile in [
            CompositionalAbilityGateProfile::WallJump,
            CompositionalAbilityGateProfile::Dash,
            CompositionalAbilityGateProfile::Both,
        ] {
            let (mission, first) = first_success(profile);
            let key = CompositionalAbilityEdgeRewriteKey::new(mission.key, profile);
            let second = key.rewrite(&mission).expect("same exact rewrite succeeds");
            assert_eq!(first, second);
            assert_eq!(
                first.provenance.base_topology_signature,
                mission.topology_signature()
            );
            assert_eq!(
                first.provenance.base_derivation_signature,
                mission.derivation_signature()
            );
            assert_ne!(first.topology_signature(), mission.topology_signature());
            assert_ne!(first.rewrite_signature(), mission.derivation_signature());
        }
    }

    #[test]
    fn a_direct_bypass_produces_typed_bounded_failure() {
        let mut mission = base_mission(0);
        let source = mission.plan.spine[0];
        let sink = *mission.plan.spine.last().unwrap();
        mission.plan.edges.push(MissionEdge {
            from: source,
            to: sink,
            kind: MissionEdgeKind::ForkBranch,
            critical: false,
        });
        let key = CompositionalAbilityEdgeRewriteKey::new(
            mission.key,
            CompositionalAbilityGateProfile::WallJump,
        );
        let error = key
            .rewrite(&mission)
            .expect_err("direct bypass removes every bridge");
        assert_eq!(
            error.cause,
            CompositionalAbilityEdgeRewriteFailure::InsufficientUnavoidableEdges {
                required: 1,
                eligible: 0,
            }
        );
    }

    #[test]
    fn exact_attempts_never_fall_back_or_retry() {
        let (mission, first) = first_success(CompositionalAbilityGateProfile::WallJump);
        let unavailable = first.provenance.arrangement_count;
        let key = CompositionalAbilityEdgeRewriteKey::new(
            mission.key,
            CompositionalAbilityGateProfile::WallJump,
        )
        .with_rewrite_attempt(unavailable);
        let error = key.rewrite(&mission).expect_err("attempt is out of range");
        assert_eq!(
            error.cause,
            CompositionalAbilityEdgeRewriteFailure::ArrangementAttemptExhausted {
                requested: unavailable,
                available: unavailable,
            }
        );
    }

    #[test]
    fn capability_salted_base_missions_are_rejected() {
        let mission = derive_compositional_route_cut_mission(CompositionalRouteCutKey::new(
            7,
            AbilitySet::new(true, false),
            ChallengeIntent::Standard,
        ))
        .unwrap();
        let key = CompositionalAbilityEdgeRewriteKey::new(
            mission.key,
            CompositionalAbilityGateProfile::WallJump,
        );
        let error = key
            .rewrite(&mission)
            .expect_err("base must be honest baseline");
        assert_eq!(
            error.cause,
            CompositionalAbilityEdgeRewriteFailure::BaseMissionIsNotBaseline {
                actual: AbilitySet::new(true, false),
            }
        );
    }

    #[test]
    fn geometry_contract_is_explicitly_pending_authoritative_evidence() {
        let (_, rewritten) = first_success(CompositionalAbilityGateProfile::Both);
        assert_eq!(
            rewritten.embedding_state,
            AbilityGateEmbeddingState::Pending {
                reasons: vec![
                    AbilityGateEmbeddingPendingReason::ReservedGeometryNotEmbedded,
                    AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                    AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
                ],
            }
        );
        for gate in &rewritten.plan.gates {
            let contract = gate.embedding_contract;
            assert_eq!(
                contract.seam,
                CompositionalAbilityEmbeddingSeam::
                    SpineRowsAndPairedSupportDomainsBeforeRasterization
            );
            assert!(contract.reserve_empty_transfer_volume);
            assert!(contract.forbid_intermediate_supports);
            assert!(contract.preserve_all_incident_route_edges);
            assert!(contract.require_baseline_reverse_descent);
            match gate.required_ability {
                GateAbility::WallJump => {
                    assert_eq!(contract.geometry, AbilityGateGeometry::PairedWallShaft);
                    assert!(!contract.forbid_wall_contact_in_ascent);
                }
                GateAbility::Dash => {
                    assert_eq!(contract.geometry, AbilityGateGeometry::DashRiseTransfer);
                    assert!(contract.forbid_wall_contact_in_ascent);
                }
            }
        }
    }

    #[test]
    fn physical_gate_embeddings_are_exact_deterministic_constructions() {
        for (profile, seed, construction_required) in [
            (CompositionalAbilityGateProfile::WallJump, 0_u64, true),
            (CompositionalAbilityGateProfile::Dash, 0_u64, true),
            (CompositionalAbilityGateProfile::Both, 1_u64, false),
        ] {
            let key =
                CompositionalAbilityGenerationKey::new(seed, profile, ChallengeIntent::Standard);
            let candidate = match key.generate() {
                Ok(candidate) => candidate,
                Err(error)
                    if !construction_required
                        && matches!(
                            &error.cause,
                            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                                phase: CompositionalAbilityEmbeddingPhase::Rhythm
                                    | CompositionalAbilityEmbeddingPhase::Spine
                                    | CompositionalAbilityEmbeddingPhase::Fork,
                                ..
                            }
                        ) =>
                {
                    eprintln!(
                        "ability-physical-v{} {profile:?} seed={seed}: retained typed failure: {error}",
                        COMPOSITIONAL_ABILITY_GENERATION_VERSION,
                    );
                    continue;
                }
                Err(error) => panic!("required exact {profile:?} key failed: {error}"),
            };
            let regenerated = candidate
                .key
                .generate()
                .expect("the same exact physical key succeeds again");
            assert_eq!(candidate, regenerated);
            assert_eq!(
                candidate.embedding.generation_version,
                COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            );
            assert_eq!(candidate.embedding.graph_rewrite_version, 1);
            assert_eq!(
                candidate.embedding.gate_realizations.len(),
                profile.gate_count()
            );
            assert_eq!(
                candidate.generated.metadata.intended_abilities,
                profile.abilities()
            );
            assert_eq!(
                candidate.evidence_state,
                AbilityGateEmbeddingState::Pending {
                    reasons: vec![
                        AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                        AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
                    ],
                }
            );

            let expected_wall_edges = candidate
                .rewritten_mission
                .plan
                .gates
                .iter()
                .filter(|gate| gate.required_ability == GateAbility::WallJump)
                .count()
                .try_into()
                .expect("at most two gates fit u16");
            let expected_dash_edges = candidate
                .rewritten_mission
                .plan
                .gates
                .iter()
                .filter(|gate| gate.required_ability == GateAbility::Dash)
                .count()
                .try_into()
                .expect("at most two gates fit u16");
            assert_eq!(candidate.route_summary.wall_edges, expected_wall_edges);
            assert_eq!(candidate.route_summary.dash_edges, expected_dash_edges);

            let cut_rows = candidate
                .embedding
                .cut_realizations
                .iter()
                .map(|cut| cut.row)
                .collect::<HashSet<_>>();
            for realization in &candidate.embedding.gate_realizations {
                assert!(!realization.required_empty_tiles.is_empty());
                let reserved_rows = realization
                    .required_empty_tiles
                    .iter()
                    .map(|cell| cell.row)
                    .collect::<HashSet<_>>();
                assert_eq!(
                    reserved_rows.len(),
                    usize::from(realization.lower_support.row - realization.upper_support.row - 1)
                );
                assert_eq!(
                    reserved_rows.iter().copied().min(),
                    Some(realization.upper_support.row + 1)
                );
                assert_eq!(
                    reserved_rows.iter().copied().max(),
                    Some(realization.lower_support.row - 1)
                );
                assert!(realization.required_solid_tiles.iter().all(|solid| {
                    !realization.required_empty_tiles.contains(solid)
                        && candidate.generated.room.tile(solid.x, solid.row) == Some(Tile::Solid)
                        && !cut_rows.contains(&solid.row)
                }));
                assert!(realization.required_empty_tiles.iter().all(|empty| {
                    candidate.generated.room.tile(empty.x, empty.row) == Some(Tile::Empty)
                }));
                match realization.gate.required_ability {
                    GateAbility::WallJump => {
                        assert!(!realization.required_solid_tiles.is_empty());
                    }
                    GateAbility::Dash => assert!(realization.required_solid_tiles.is_empty()),
                }
                assert_eq!(
                    candidate
                        .rewritten_mission
                        .plan
                        .graph_can_reach_without_gate_edge(realization.gate.ordinal),
                    Some(false)
                );
            }
            assert!(candidate.rewritten_mission.plan.graph_can_reach(
                candidate.rewritten_mission.plan.sink_node_id(),
                candidate.rewritten_mission.plan.source_node_id(),
                AbilitySet::NONE,
            ));
            for port in &candidate.boundary_ports {
                let socket = port.door.socket();
                assert!(super::super::compositional_route_cut::
                    compositional_route_cut_socket_in_inventory(socket));
                assert!(super::super::compositional_route_cut::
                    compositional_route_cut_socket_in_inventory(socket.mate()));
            }
        }
    }

    #[test]
    fn fixed_seed_block_reports_graph_rewrite_coverage() {
        for profile in [
            CompositionalAbilityGateProfile::WallJump,
            CompositionalAbilityGateProfile::Dash,
            CompositionalAbilityGateProfile::Both,
        ] {
            let mut successes = 0_usize;
            let mut topology_signatures = HashSet::new();
            let mut insufficient_edges = 0_usize;
            let mut unseparated_pairs = 0_usize;
            for seed in 0..512 {
                let mission = base_mission(seed);
                let key = CompositionalAbilityEdgeRewriteKey::new(mission.key, profile);
                match key.rewrite(&mission) {
                    Ok(rewritten) => {
                        successes += 1;
                        topology_signatures.insert(rewritten.topology_signature());
                    }
                    Err(CompositionalAbilityEdgeRewriteError {
                        cause:
                            CompositionalAbilityEdgeRewriteFailure::InsufficientUnavoidableEdges {
                                ..
                            },
                        ..
                    }) => insufficient_edges += 1,
                    Err(CompositionalAbilityEdgeRewriteError {
                        cause:
                            CompositionalAbilityEdgeRewriteFailure::
                                NoSeparatedUnavoidableEdgeArrangement { .. },
                        ..
                    }) => unseparated_pairs += 1,
                    Err(error) => panic!("unexpected fixed-block failure: {error}"),
                }
            }
            eprintln!(
                "ability-edge-v1 {profile:?}: successes={successes}/512 distinct_topologies={} insufficient_edges={insufficient_edges} unseparated_pairs={unseparated_pairs}",
                topology_signatures.len(),
            );
            assert_eq!(
                successes + insufficient_edges + unseparated_pairs,
                512,
                "every exact key has one recorded outcome"
            );
            assert!(successes >= 64, "graph rewrite coverage regressed");
            assert!(
                topology_signatures.len() >= 48,
                "ability rewrite topology diversity regressed"
            );
        }
    }
}
