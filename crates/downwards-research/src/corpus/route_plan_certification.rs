//! Opt-in replay certification along generator-declared route-plan supports.
//!
//! The route plan is used only to choose deterministic intermediate search
//! objectives.  Positive evidence still comes exclusively from exact core
//! simulation replay, including a final replay from the requested entry door
//! to the exact requested target door.
//!
//! A selected path that must cross a third boundary-port node is refused
//! before search because its trigger may terminate the segmented simulation.
//! That refusal is not physical-unreachability evidence: independently found
//! monolithic or generous-search positives remain valid but outside this
//! terminal-unaware certification policy.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use downwards_ai::{
    GroundedStandingRegion, GroundedSupportSolveError, GroundedSupportSolveOutcome,
    GroundedSupportTarget, GroundedSupportTargetError, InconclusiveReason, ReachedTarget, Replay,
    ReplayDivergence, SOLVER_POLICY_VERSION, SearchStats, SearchTarget, SolverConfig,
    TargetSolveError, TargetSolveOutcome, WAYPOINT_DIAGNOSTIC_POLICY_VERSION,
    solve_grounded_support, solve_target,
};
use downwards_core::{
    DeathReason, DoorEntryError, PLAYER_HEIGHT, PLAYER_WIDTH, Rect, Room, Simulation,
    SimulationEvent, Tile,
};
use downwards_gen::experimental::{
    BoundaryPort, RouteEdge, RoutePlan, RouteVerb, SupportKind, SupportSpec,
};

use crate::structural::{
    STRUCTURAL_DESCRIPTOR_VERSION, StructuralBypassDescriptor, StructuralDescriptorError,
    describe_port_path,
};

use super::{
    CorpusCandidate, CorpusCandidateKeyRecord, CorpusPhysicalRoomDescriptorV3, EvaluationLoadout,
    RoomId,
};

/// Version of structural selection, support validation, segment composition,
/// and final authoritative replay semantics in this module.
pub const ROUTE_PLAN_CERTIFICATION_VERSION: u32 = 1;

/// Version of the loadout-aware, third-port-avoiding structural path policy.
pub const ROUTE_PLAN_CERTIFICATION_PATH_POLICY_VERSION: u32 = 1;

/// Stable byte encoding used by `RoutePlanCertificationConfigFingerprint`.
pub const ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION: u32 = 1;

/// Required interpretation of every route-plan-guided certificate.
///
/// A certificate is positive reachability evidence only.  The authored path
/// deliberately guides search, so it is never evidence that this is the
/// easiest route, that no lower-demand bypass exists, or that any difficulty
/// metric has a particular value.  Independent easiest-controller assessment
/// remains authoritative for those questions.
pub const ROUTE_PLAN_CERTIFICATION_EVIDENCE_DISCLAIMER: &str = "route-plan-guided replay is positive reachability evidence only; it is not easiest-route, no-bypass, or difficulty evidence";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificationConfigFingerprint {
    pub version: u32,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificationProvenance {
    pub certification_version: u32,
    pub path_policy_version: u32,
    pub waypoint_policy_version: u32,
    pub solver_policy_version: u32,
    pub structural_descriptor_version: u32,
    pub evidence_disclaimer: String,
    pub candidate_key: CorpusCandidateKeyRecord,
    /// Full geometry equality remains authoritative; this compact ID is a
    /// label and is never used alone to establish candidate equivalence.
    pub physical_room_id: RoomId,
    pub physical_room_descriptor: CorpusPhysicalRoomDescriptorV3,
    pub generated_room_id: String,
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub solver_config: RoutePlanCertificationConfigFingerprint,
    pub structural_path: StructuralBypassDescriptor,
    pub certification_path: RoutePlanCertificationStructuralPath,
}

/// Lexicographic policy cost for the actual waypoint path.
///
/// Avoiding an intermediate boundary-port node dominates all other fields,
/// because accidentally touching that port terminates authoritative
/// simulation.  Ability use then dominates length, and critical authored
/// edges break otherwise similar choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RoutePlanCertificationPathCost {
    pub intermediate_boundary_port_nodes: usize,
    pub ability_edges: usize,
    pub edge_count: usize,
    pub noncritical_edges: usize,
    pub vertical_transitions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificationStructuralPath {
    pub source_node_id: u16,
    pub target_node_id: u16,
    pub node_path: Vec<u16>,
    pub edge_indices: Vec<usize>,
    pub cost: RoutePlanCertificationPathCost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutePlanCertificationSegmentTarget {
    GroundedSupport {
        route_node_id: u16,
        authored_support: SupportSpec,
        exact_target: GroundedSupportTarget,
    },
    ExactDoor {
        door_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificationSegment {
    pub segment_index: usize,
    pub target: RoutePlanCertificationSegmentTarget,
    pub replay: Replay,
    pub stats: SearchStats,
    pub composed_action_start: usize,
    pub composed_action_end: usize,
}

/// Positive reachability evidence, not an easiest-known-route certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificate {
    pub provenance: RoutePlanCertificationProvenance,
    pub segments: Vec<RoutePlanCertificationSegment>,
    pub total_stats: SearchStats,
    pub replay: Replay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutePlanCertificationBoundedReason {
    GroundedSupport {
        route_node_id: u16,
        reason: InconclusiveReason,
    },
    ExactDoor {
        door_id: String,
        reason: InconclusiveReason,
    },
}

/// A bounded miss retains completed positive segments and exact work totals.
/// It deliberately makes no unreachable/impossible claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlanCertificationInconclusive {
    pub provenance: RoutePlanCertificationProvenance,
    pub completed_segments: Vec<RoutePlanCertificationSegment>,
    pub failed_segment_index: usize,
    pub failed_segment_target: RoutePlanCertificationSegmentTarget,
    pub reason: RoutePlanCertificationBoundedReason,
    pub failed_segment_stats: SearchStats,
    pub total_stats: SearchStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutePlanCertificationOutcome {
    Certified(RoutePlanCertificate),
    BoundedInconclusive(RoutePlanCertificationInconclusive),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutePlanCertificationError {
    Structural(StructuralDescriptorError),
    NoLoadoutCompatibleStructuralPath {
        loadout: EvaluationLoadout,
        source_node_id: u16,
        target_node_id: u16,
    },
    /// Certification refuses to guide a replay through a third boundary-port
    /// node because its trigger can terminate simulation. This is a
    /// route-guidance preflight failure, never a claim of physical
    /// unreachability for the requested door pair.
    IntermediateBoundaryPortPreflightRefusal {
        source_node_id: u16,
        target_node_id: u16,
        intermediate_port_node_ids: Vec<u16>,
    },
    MissingRouteNode {
        node_id: u16,
    },
    SupportOutOfBounds {
        node_id: u16,
        support: SupportSpec,
    },
    SupportMaterialMismatch {
        node_id: u16,
        tile_x: u16,
        tile_y: u16,
        expected: Tile,
        actual: Option<Tile>,
    },
    NoClearStandingRegion {
        node_id: u16,
        support: SupportSpec,
    },
    InvalidGroundedTarget {
        node_id: u16,
        source: GroundedSupportTargetError,
    },
    DoorEntry(DoorEntryError),
    WaypointSolve {
        segment_index: usize,
        node_id: u16,
        source: GroundedSupportSolveError,
    },
    DoorSolve {
        segment_index: usize,
        source: TargetSolveError,
    },
    SegmentReplayDiverged {
        segment_index: usize,
        source: ReplayDivergence,
    },
    SegmentDidNotEndOnSupport {
        segment_index: usize,
        node_id: u16,
    },
    AuthoritativeDeath {
        segment_index: usize,
        composed_frame_index: usize,
        reason: DeathReason,
    },
    WrongDoor {
        segment_index: usize,
        composed_frame_index: usize,
        expected_door_id: String,
        reached_door_id: String,
    },
    PrematureTargetDoor {
        segment_index: usize,
        composed_frame_index: usize,
        target_door_id: String,
    },
    FinalReplayDiverged(ReplayDivergence),
    FinalDoorNotReached {
        expected_door_id: String,
        reached_door_id: Option<String>,
    },
}

impl fmt::Display for RoutePlanCertificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Structural(error) => write!(formatter, "invalid structural route: {error}"),
            Self::NoLoadoutCompatibleStructuralPath {
                loadout,
                source_node_id,
                target_node_id,
            } => write!(
                formatter,
                "route-plan nodes {source_node_id}->{target_node_id} have no structural path compatible with {loadout:?}"
            ),
            Self::IntermediateBoundaryPortPreflightRefusal {
                source_node_id,
                target_node_id,
                intermediate_port_node_ids,
            } => write!(
                formatter,
                "route-guided certification preflight refused nodes {source_node_id}->{target_node_id} because every selected compatible path crosses third boundary-port nodes {intermediate_port_node_ids:?}; this is not a physical-unreachability claim"
            ),
            Self::MissingRouteNode { node_id } => {
                write!(
                    formatter,
                    "selected path references missing route node {node_id}"
                )
            }
            Self::SupportOutOfBounds { node_id, support } => write!(
                formatter,
                "route node {node_id} support is outside the room: {support:?}"
            ),
            Self::SupportMaterialMismatch {
                node_id,
                tile_x,
                tile_y,
                expected,
                actual,
            } => write!(
                formatter,
                "route node {node_id} support material differs at ({tile_x}, {tile_y}): expected {expected:?}, got {actual:?}"
            ),
            Self::NoClearStandingRegion { node_id, support } => write!(
                formatter,
                "route node {node_id} has no exact clear standing region on {support:?}"
            ),
            Self::InvalidGroundedTarget { node_id, source } => write!(
                formatter,
                "route node {node_id} produced an invalid grounded target: {source}"
            ),
            Self::DoorEntry(error) => write!(formatter, "could not enter source door: {error}"),
            Self::WaypointSolve {
                segment_index,
                node_id,
                source,
            } => write!(
                formatter,
                "waypoint segment {segment_index} for route node {node_id} failed before search: {source}"
            ),
            Self::DoorSolve {
                segment_index,
                source,
            } => write!(
                formatter,
                "exact-door segment {segment_index} failed before search: {source}"
            ),
            Self::SegmentReplayDiverged {
                segment_index,
                source,
            } => write!(
                formatter,
                "segment {segment_index} replay diverged from its exact initial state: {source}"
            ),
            Self::SegmentDidNotEndOnSupport {
                segment_index,
                node_id,
            } => write!(
                formatter,
                "segment {segment_index} replay did not end stably grounded on route node {node_id}"
            ),
            Self::AuthoritativeDeath {
                segment_index,
                composed_frame_index,
                reason,
            } => write!(
                formatter,
                "segment {segment_index} died at composed frame {composed_frame_index}: {reason:?}"
            ),
            Self::WrongDoor {
                segment_index,
                composed_frame_index,
                expected_door_id,
                reached_door_id,
            } => write!(
                formatter,
                "segment {segment_index} reached wrong door {reached_door_id:?} instead of {expected_door_id:?} at composed frame {composed_frame_index}"
            ),
            Self::PrematureTargetDoor {
                segment_index,
                composed_frame_index,
                target_door_id,
            } => write!(
                formatter,
                "waypoint segment {segment_index} reached target door {target_door_id:?} prematurely at composed frame {composed_frame_index}"
            ),
            Self::FinalReplayDiverged(error) => {
                write!(formatter, "composed authoritative replay diverged: {error}")
            }
            Self::FinalDoorNotReached {
                expected_door_id,
                reached_door_id,
            } => write!(
                formatter,
                "composed replay did not reach exact door {expected_door_id:?}; reached {reached_door_id:?}"
            ),
        }
    }
}

impl Error for RoutePlanCertificationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Structural(error) => Some(error),
            Self::InvalidGroundedTarget { source, .. } => Some(source),
            Self::DoorEntry(error) => Some(error),
            Self::WaypointSolve { source, .. } => Some(source),
            Self::DoorSolve { source, .. } => Some(source),
            Self::SegmentReplayDiverged { source, .. } => Some(source),
            Self::FinalReplayDiverged(error) => Some(error),
            _ => None,
        }
    }
}

/// Certify a deterministic conservative route-plan path for an exact ordered
/// door pair and loadout.
///
/// This returns positive reachability evidence only.  Because the authored
/// route guides the search, callers must not use the resulting witness as the
/// easiest-known route or as difficulty/no-bypass evidence.  Run the
/// independent easiest-controller audit for those interpretations.
pub fn certify_candidate_route_plan(
    candidate: &CorpusCandidate,
    source_door_id: &str,
    target_door_id: &str,
    loadout: EvaluationLoadout,
    solver_config: &SolverConfig,
) -> Result<RoutePlanCertificationOutcome, RoutePlanCertificationError> {
    let view = candidate.view();
    let structural_path = describe_port_path(
        view.route_plan,
        view.boundary_ports,
        source_door_id,
        target_door_id,
    )
    .map_err(RoutePlanCertificationError::Structural)?;
    let abilities = loadout.abilities();
    let certification_path = select_certification_path(
        view.route_plan,
        view.boundary_ports,
        structural_path.source_node_id,
        structural_path.target_node_id,
        loadout,
    )?;
    refuse_intermediate_boundary_port_path(view.boundary_ports, &certification_path)?;

    let physical_room_descriptor = candidate.physical_room_descriptor_v3();
    let provenance = RoutePlanCertificationProvenance {
        certification_version: ROUTE_PLAN_CERTIFICATION_VERSION,
        path_policy_version: ROUTE_PLAN_CERTIFICATION_PATH_POLICY_VERSION,
        waypoint_policy_version: WAYPOINT_DIAGNOSTIC_POLICY_VERSION,
        solver_policy_version: SOLVER_POLICY_VERSION,
        structural_descriptor_version: STRUCTURAL_DESCRIPTOR_VERSION,
        evidence_disclaimer: ROUTE_PLAN_CERTIFICATION_EVIDENCE_DISCLAIMER.to_owned(),
        candidate_key: candidate.exact_key(),
        physical_room_id: physical_room_descriptor.room_id(),
        physical_room_descriptor,
        generated_room_id: view.generated.room.id().to_owned(),
        source_door_id: source_door_id.to_owned(),
        target_door_id: target_door_id.to_owned(),
        loadout,
        solver_config: route_plan_certification_config_fingerprint(solver_config),
        structural_path,
        certification_path,
    };
    let room = &view.generated.room;
    let all_path_supports = provenance
        .certification_path
        .node_path
        .iter()
        .copied()
        .map(|node_id| {
            let node = view
                .route_plan
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .ok_or(RoutePlanCertificationError::MissingRouteNode { node_id })?;
            let exact_target = grounded_target_for_support(room, node_id, node.support)?;
            Ok(RoutePlanCertificationSegmentTarget::GroundedSupport {
                route_node_id: node_id,
                authored_support: node.support,
                exact_target,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The source arrival is already owned by `enter_via_door`, while the
    // exact-door segment owns the terminal port edge. In particular, a
    // lateral/floor target support is never rejected merely because stable
    // standing there overlaps its own terminal trigger.
    let waypoint_count = all_path_supports.len().saturating_sub(2);
    let waypoint_targets = all_path_supports
        .into_iter()
        .skip(1)
        .take(waypoint_count)
        .collect::<Vec<_>>();

    let initial = Simulation::enter_via_door(room.clone(), abilities, source_door_id)
        .map_err(RoutePlanCertificationError::DoorEntry)?;
    let mut simulation = initial.clone();
    let mut actions = Vec::new();
    let mut segments = Vec::new();
    let mut total_stats = SearchStats::default();

    for target in waypoint_targets {
        let (route_node_id, exact_target) = match &target {
            RoutePlanCertificationSegmentTarget::GroundedSupport {
                route_node_id,
                exact_target,
                ..
            } => (*route_node_id, exact_target.clone()),
            RoutePlanCertificationSegmentTarget::ExactDoor { .. } => {
                unreachable!("waypoint list contains only support targets")
            }
        };
        let segment_index = segments.len();
        let outcome = solve_grounded_support(&simulation, &exact_target, solver_config).map_err(
            |source| RoutePlanCertificationError::WaypointSolve {
                segment_index,
                node_id: route_node_id,
                source,
            },
        )?;
        match outcome {
            GroundedSupportSolveOutcome::Solved(solution) => {
                add_stats(&mut total_stats, solution.stats);
                let action_start = actions.len();
                apply_segment_replay(
                    &mut simulation,
                    &solution.replay,
                    &mut actions,
                    segment_index,
                    target_door_id,
                    false,
                )?;
                if !exact_target.is_reached(&simulation) {
                    return Err(RoutePlanCertificationError::SegmentDidNotEndOnSupport {
                        segment_index,
                        node_id: route_node_id,
                    });
                }
                segments.push(RoutePlanCertificationSegment {
                    segment_index,
                    target,
                    replay: solution.replay,
                    stats: solution.stats,
                    composed_action_start: action_start,
                    composed_action_end: actions.len(),
                });
            }
            GroundedSupportSolveOutcome::Inconclusive { reason, stats } => {
                add_stats(&mut total_stats, stats);
                return Ok(RoutePlanCertificationOutcome::BoundedInconclusive(
                    RoutePlanCertificationInconclusive {
                        provenance,
                        completed_segments: segments,
                        failed_segment_index: segment_index,
                        failed_segment_target: target,
                        reason: RoutePlanCertificationBoundedReason::GroundedSupport {
                            route_node_id,
                            reason,
                        },
                        failed_segment_stats: stats,
                        total_stats,
                    },
                ));
            }
        }
    }

    let segment_index = segments.len();
    let final_target = RoutePlanCertificationSegmentTarget::ExactDoor {
        door_id: target_door_id.to_owned(),
    };
    let outcome = solve_target(
        &simulation,
        SearchTarget::door(target_door_id),
        solver_config,
    )
    .map_err(|source| RoutePlanCertificationError::DoorSolve {
        segment_index,
        source,
    })?;
    match outcome {
        TargetSolveOutcome::Solved(solution) => {
            add_stats(&mut total_stats, solution.stats);
            if solution.reached != ReachedTarget::Door(target_door_id.to_owned()) {
                return Err(RoutePlanCertificationError::FinalDoorNotReached {
                    expected_door_id: target_door_id.to_owned(),
                    reached_door_id: match solution.reached {
                        ReachedTarget::Door(id) | ReachedTarget::Exit(id) => Some(id),
                        ReachedTarget::Pickup(_) => None,
                    },
                });
            }
            let action_start = actions.len();
            apply_segment_replay(
                &mut simulation,
                &solution.replay,
                &mut actions,
                segment_index,
                target_door_id,
                true,
            )?;
            segments.push(RoutePlanCertificationSegment {
                segment_index,
                target: final_target,
                replay: solution.replay,
                stats: solution.stats,
                composed_action_start: action_start,
                composed_action_end: actions.len(),
            });
        }
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            add_stats(&mut total_stats, stats);
            return Ok(RoutePlanCertificationOutcome::BoundedInconclusive(
                RoutePlanCertificationInconclusive {
                    provenance,
                    completed_segments: segments,
                    failed_segment_index: segment_index,
                    failed_segment_target: final_target,
                    reason: RoutePlanCertificationBoundedReason::ExactDoor {
                        door_id: target_door_id.to_owned(),
                        reason,
                    },
                    failed_segment_stats: stats,
                    total_stats,
                },
            ));
        }
    }

    let replay = Replay::record(&initial, actions);
    let verification = replay
        .verify(&initial)
        .map_err(RoutePlanCertificationError::FinalReplayDiverged)?;
    if verification.reached_exit.as_deref() != Some(target_door_id) {
        return Err(RoutePlanCertificationError::FinalDoorNotReached {
            expected_door_id: target_door_id.to_owned(),
            reached_door_id: verification.reached_exit,
        });
    }
    Ok(RoutePlanCertificationOutcome::Certified(
        RoutePlanCertificate {
            provenance,
            segments,
            total_stats,
            replay,
        },
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CertificationPathRank {
    cost: RoutePlanCertificationPathCost,
    edge_indices: Vec<usize>,
    node_path: Vec<u16>,
}

#[derive(Clone, Copy)]
enum CertificationRequiredAbility {
    WallJump,
    Dash,
}

fn select_certification_path(
    plan: &RoutePlan,
    ports: &[BoundaryPort],
    source_node_id: u16,
    target_node_id: u16,
    loadout: EvaluationLoadout,
) -> Result<RoutePlanCertificationStructuralPath, RoutePlanCertificationError> {
    let nodes = plan
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect::<BTreeMap<_, _>>();
    let intermediate_port_nodes = ports
        .iter()
        .map(|port| port.node_id)
        .filter(|node_id| *node_id != source_node_id && *node_id != target_node_id)
        .collect::<BTreeSet<_>>();
    let initial = CertificationPathRank {
        cost: RoutePlanCertificationPathCost::default(),
        edge_indices: Vec::new(),
        node_path: vec![source_node_id],
    };
    let mut best = BTreeMap::from([(source_node_id, initial.clone())]);
    let mut frontier = BTreeSet::from([(initial, source_node_id)]);
    let abilities = loadout.abilities();

    while let Some((rank, node_id)) = frontier.pop_first() {
        if best.get(&node_id) != Some(&rank) {
            continue;
        }
        if node_id == target_node_id {
            return Ok(RoutePlanCertificationStructuralPath {
                source_node_id,
                target_node_id,
                node_path: rank.node_path,
                edge_indices: rank.edge_indices,
                cost: rank.cost,
            });
        }
        for (edge_index, edge) in plan.edges.iter().enumerate() {
            let adjacent = if edge.from == node_id {
                Some(edge.to)
            } else if edge.to == node_id {
                Some(edge.from)
            } else {
                None
            };
            let Some(adjacent) = adjacent else {
                continue;
            };
            let requirement = certification_required_ability(edge, node_id);
            let available = match requirement {
                None => true,
                Some(CertificationRequiredAbility::WallJump) => abilities.wall_jump,
                Some(CertificationRequiredAbility::Dash) => abilities.dash,
            };
            if !available {
                continue;
            }
            let from = nodes[&node_id].support;
            let to = nodes[&adjacent].support;
            let mut next = rank.clone();
            next.cost.intermediate_boundary_port_nodes +=
                usize::from(intermediate_port_nodes.contains(&adjacent));
            next.cost.ability_edges += usize::from(requirement.is_some());
            next.cost.edge_count += 1;
            next.cost.noncritical_edges += usize::from(!edge.critical);
            next.cost.vertical_transitions += usize::from(from.row != to.row);
            next.edge_indices.push(edge_index);
            next.node_path.push(adjacent);
            if best.get(&adjacent).is_some_and(|known| *known <= next) {
                continue;
            }
            if let Some(old) = best.insert(adjacent, next.clone()) {
                frontier.remove(&(old, adjacent));
            }
            frontier.insert((next, adjacent));
        }
    }

    Err(
        RoutePlanCertificationError::NoLoadoutCompatibleStructuralPath {
            loadout,
            source_node_id,
            target_node_id,
        },
    )
}

fn refuse_intermediate_boundary_port_path(
    ports: &[BoundaryPort],
    path: &RoutePlanCertificationStructuralPath,
) -> Result<(), RoutePlanCertificationError> {
    if path.cost.intermediate_boundary_port_nodes == 0 {
        return Ok(());
    }
    let boundary_port_nodes = ports
        .iter()
        .map(|port| port.node_id)
        .filter(|node_id| *node_id != path.source_node_id && *node_id != path.target_node_id)
        .collect::<BTreeSet<_>>();
    let intermediate_port_node_ids = path
        .node_path
        .iter()
        .copied()
        .filter(|node_id| boundary_port_nodes.contains(node_id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    debug_assert_eq!(
        path.cost.intermediate_boundary_port_nodes,
        intermediate_port_node_ids.len(),
        "the minimum-cost path cannot repeat an intermediate port node"
    );
    Err(
        RoutePlanCertificationError::IntermediateBoundaryPortPreflightRefusal {
            source_node_id: path.source_node_id,
            target_node_id: path.target_node_id,
            intermediate_port_node_ids,
        },
    )
}

const fn certification_required_ability(
    edge: &RouteEdge,
    traversal_from: u16,
) -> Option<CertificationRequiredAbility> {
    let follows_declaration = edge.from == traversal_from;
    match edge.verb {
        RouteVerb::WallClimb if follows_declaration => Some(CertificationRequiredAbility::WallJump),
        RouteVerb::DashAcross => Some(CertificationRequiredAbility::Dash),
        RouteVerb::DashUp if follows_declaration => Some(CertificationRequiredAbility::Dash),
        RouteVerb::Run
        | RouteVerb::Jump
        | RouteVerb::Drop
        | RouteVerb::WallClimb
        | RouteVerb::DashUp => None,
    }
}

#[must_use]
pub fn route_plan_certification_config_fingerprint(
    config: &SolverConfig,
) -> RoutePlanCertificationConfigFingerprint {
    let mut hash = StableHash::domain(b"downwards-route-plan-certification-config");
    hash.u32(ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION);
    hash.u32(ROUTE_PLAN_CERTIFICATION_VERSION);
    hash.u32(ROUTE_PLAN_CERTIFICATION_PATH_POLICY_VERSION);
    hash.u32(WAYPOINT_DIAGNOSTIC_POLICY_VERSION);
    hash.u32(SOLVER_POLICY_VERSION);
    hash.usize(config.max_expanded_nodes);
    hash.usize(config.max_simulated_ticks);
    hash.usize(config.max_ticks_per_path);
    hash.usize(config.beam_width);
    hash.i32(config.position_quantum);
    hash.i32(config.velocity_quantum);
    hash.bool(config.probe_direct_routes);
    hash.usize(config.baseline_preview_max_expanded_nodes);
    hash.usize(config.baseline_preview_max_simulated_ticks);
    hash.usize(config.macros.len());
    for action_macro in &config.macros {
        hash.string(&action_macro.name);
        hash.usize(action_macro.actions.len());
        for action in &action_macro.actions {
            hash.i8(action.move_x);
            hash.i8(action.move_y);
            hash.bool(action.jump);
            hash.bool(action.dash);
            hash.bool(action.restart);
        }
    }
    RoutePlanCertificationConfigFingerprint {
        version: ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION,
        id: format!(
            "downwards-route-plan-certification-config-v{}-{:016x}",
            ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION,
            hash.finish()
        ),
    }
}

fn grounded_target_for_support(
    room: &Room,
    node_id: u16,
    support: SupportSpec,
) -> Result<GroundedSupportTarget, RoutePlanCertificationError> {
    if support.start_x >= support.end_x
        || support.end_x > room.width()
        || support.row >= room.height()
    {
        return Err(RoutePlanCertificationError::SupportOutOfBounds { node_id, support });
    }
    let expected = match support.kind {
        SupportKind::Solid => Tile::Solid,
        SupportKind::OneWay => Tile::OneWay,
    };
    for tile_x in support.start_x..support.end_x {
        let actual = room.tile(tile_x, support.row);
        if actual != Some(expected) {
            return Err(RoutePlanCertificationError::SupportMaterialMismatch {
                node_id,
                tile_x,
                tile_y: support.row,
                expected,
                actual,
            });
        }
    }

    let tile_size = room.tile_size();
    let surface_left = i32::from(support.start_x) * tile_size;
    let surface_right = i32::from(support.end_x) * tile_size;
    let surface_y = i32::from(support.row) * tile_size;
    let standing_y = surface_y - PLAYER_HEIGHT;
    if standing_y < 0 || surface_right - surface_left < PLAYER_WIDTH {
        return Err(RoutePlanCertificationError::NoClearStandingRegion { node_id, support });
    }

    let mut clear_x = Vec::new();
    for player_x in surface_left..=surface_right - PLAYER_WIDTH {
        let player = Rect::new(player_x, standing_y, PLAYER_WIDTH, PLAYER_HEIGHT);
        let blocked_by_tile = (0..room.height()).any(|tile_y| {
            (0..room.width()).any(|tile_x| {
                matches!(room.tile(tile_x, tile_y), Some(Tile::Solid | Tile::Hazard))
                    && player.intersects(room.tile_bounds(tile_x, tile_y))
            })
        });
        let blocked_by_timed_hazard = room
            .timed_hazards()
            .iter()
            .any(|hazard| player.intersects(hazard.bounds()));
        if !blocked_by_tile && !blocked_by_timed_hazard {
            clear_x.push(player_x);
        }
    }
    if clear_x.is_empty() {
        return Err(RoutePlanCertificationError::NoClearStandingRegion { node_id, support });
    }
    let mut regions = Vec::new();
    let mut start = clear_x[0];
    let mut end = start;
    for player_x in clear_x.into_iter().skip(1) {
        if player_x == end + 1 {
            end = player_x;
        } else {
            regions.push(
                GroundedStandingRegion::new(start, end)
                    .expect("ordered clear positions form a valid region"),
            );
            start = player_x;
            end = player_x;
        }
    }
    regions.push(
        GroundedStandingRegion::new(start, end)
            .expect("ordered clear positions form a valid region"),
    );
    GroundedSupportTarget::new(surface_left, surface_right, surface_y, regions)
        .map_err(|source| RoutePlanCertificationError::InvalidGroundedTarget { node_id, source })
}

fn apply_segment_replay(
    simulation: &mut Simulation,
    replay: &Replay,
    composed_actions: &mut Vec<downwards_core::Action>,
    segment_index: usize,
    target_door_id: &str,
    final_door_segment: bool,
) -> Result<(), RoutePlanCertificationError> {
    replay.verify(simulation).map_err(|source| {
        RoutePlanCertificationError::SegmentReplayDiverged {
            segment_index,
            source,
        }
    })?;
    for (local_frame_index, action) in replay.actions().enumerate() {
        let composed_frame_index = composed_actions.len();
        let report = simulation.step(action);
        composed_actions.push(action);
        for event in report.events {
            match event {
                SimulationEvent::Died(reason) => {
                    return Err(RoutePlanCertificationError::AuthoritativeDeath {
                        segment_index,
                        composed_frame_index,
                        reason,
                    });
                }
                SimulationEvent::ExitReached { id } if id != target_door_id => {
                    return Err(RoutePlanCertificationError::WrongDoor {
                        segment_index,
                        composed_frame_index,
                        expected_door_id: target_door_id.to_owned(),
                        reached_door_id: id,
                    });
                }
                SimulationEvent::ExitReached { id }
                    if !final_door_segment || local_frame_index + 1 != replay.frames.len() =>
                {
                    return Err(RoutePlanCertificationError::PrematureTargetDoor {
                        segment_index,
                        composed_frame_index,
                        target_door_id: id,
                    });
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn add_stats(total: &mut SearchStats, segment: SearchStats) {
    total.expanded_nodes = total.expanded_nodes.saturating_add(segment.expanded_nodes);
    total.generated_nodes = total
        .generated_nodes
        .saturating_add(segment.generated_nodes);
    total.simulated_ticks = total
        .simulated_ticks
        .saturating_add(segment.simulated_ticks);
    total.deepest_path_ticks = total.deepest_path_ticks.max(segment.deepest_path_ticks);
}

struct StableHash(u64);

impl StableHash {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn domain(domain: &[u8]) -> Self {
        let mut hash = Self(Self::OFFSET);
        hash.bytes(domain);
        hash
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        self.usize(values.len());
        for &value in values {
            self.byte(value);
        }
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn i8(&mut self, value: i8) {
        self.byte(value as u8);
    }

    fn i32(&mut self, value: i32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn usize(&mut self, value: usize) {
        for byte in (value as u64).to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{SOLVER_POLICY_VERSION, solve_target};
    use downwards_core::{AbilitySet, BoundarySide, Door, Point};
    use downwards_gen::experimental::{
        ChallengeIntent, CompositionalRouteCutKey, PartitionRouteKey, PartitionRouteProfile,
    };

    use super::*;

    fn assert_default_solver_remains_bounded(candidate: &CorpusCandidate) {
        let initial = Simulation::enter_via_door(
            candidate.generated().room.clone(),
            AbilitySet::NONE,
            "port-0",
        )
        .unwrap();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("port-1"),
            &SolverConfig::for_abilities(AbilitySet::NONE),
        )
        .unwrap();
        assert!(matches!(outcome, TargetSolveOutcome::Inconclusive { .. }));
    }

    fn assert_frozen_preflight_refusal(
        candidate: &CorpusCandidate,
        expected_intermediate_port_node_ids: Vec<u16>,
    ) {
        let config = SolverConfig::for_abilities(AbilitySet::NONE);
        let first = certify_candidate_route_plan(
            candidate,
            "port-0",
            "port-1",
            EvaluationLoadout::Baseline,
            &config,
        )
        .unwrap_err();
        let second = certify_candidate_route_plan(
            candidate,
            "port-0",
            "port-1",
            EvaluationLoadout::Baseline,
            &config,
        )
        .unwrap_err();
        assert_eq!(first, second);
        assert_eq!(
            first,
            RoutePlanCertificationError::IntermediateBoundaryPortPreflightRefusal {
                source_node_id: 0,
                target_node_id: 1,
                intermediate_port_node_ids: expected_intermediate_port_node_ids,
            }
        );
        assert!(
            first
                .to_string()
                .contains("not a physical-unreachability claim")
        );
    }

    #[test]
    fn frozen_gentle_zero_cut_seed_two_is_certified_under_baseline() {
        let native = CompositionalRouteCutKey::new(2, AbilitySet::NONE, ChallengeIntent::Gentle)
            .regenerate()
            .unwrap();
        assert_eq!(native.mission.provenance.cut_rewrites, 0);
        let candidate = native.into();
        assert_default_solver_remains_bounded(&candidate);
        // A separately observed generous monolithic search has a positive
        // replay for this room. That evidence is outside this certificate:
        // route-guided segmentation refuses its unavoidable third ports.
        assert_frozen_preflight_refusal(&candidate, vec![5, 10]);
    }

    #[test]
    fn frozen_gentle_cut_fork_seed_three_is_certified_under_baseline() {
        let native = CompositionalRouteCutKey::new(3, AbilitySet::NONE, ChallengeIntent::Gentle)
            .regenerate()
            .unwrap();
        assert!(native.mission.provenance.cut_rewrites > 0);
        assert!(native.mission.provenance.fork_rewrites > 0);
        let candidate = native.into();
        assert_default_solver_remains_bounded(&candidate);
        // As with seed two, separate generous-search positive evidence does
        // not authorize a terminal-unaware route-plan certificate.
        assert_frozen_preflight_refusal(&candidate, vec![3, 6]);
    }

    #[test]
    fn ordinary_solver_output_and_policy_are_unchanged_by_certification() {
        const FROZEN_SOLVER_POLICY_VERSION: u32 = 3;
        assert_eq!(SOLVER_POLICY_VERSION, FROZEN_SOLVER_POLICY_VERSION);
        let candidate: CorpusCandidate =
            CompositionalRouteCutKey::new(2, AbilitySet::NONE, ChallengeIntent::Gentle)
                .regenerate()
                .unwrap()
                .into();
        let initial = Simulation::enter_via_door(
            candidate.generated().room.clone(),
            AbilitySet::NONE,
            "port-0",
        )
        .unwrap();
        let config = SolverConfig::for_abilities(AbilitySet::NONE);
        let before = solve_target(&initial, SearchTarget::door("port-1"), &config).unwrap();
        let refusal = certify_candidate_route_plan(
            &candidate,
            "port-0",
            "port-1",
            EvaluationLoadout::Baseline,
            &config,
        )
        .unwrap_err();
        assert!(matches!(
            refusal,
            RoutePlanCertificationError::IntermediateBoundaryPortPreflightRefusal { .. }
        ));
        let after = solve_target(&initial, SearchTarget::door("port-1"), &config).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn partition_lateral_terminal_is_owned_by_exact_door_segment() {
        let native = PartitionRouteKey::new(
            0,
            AbilitySet::NONE,
            ChallengeIntent::Gentle,
            PartitionRouteProfile::MixedBsp,
        )
        .regenerate()
        .unwrap();
        let source = "port-west";
        let target = "port-east";
        let candidate: CorpusCandidate = native.into();
        let outcome = certify_candidate_route_plan(
            &candidate,
            source,
            target,
            EvaluationLoadout::Baseline,
            &SolverConfig::for_abilities(AbilitySet::NONE),
        )
        .unwrap();
        let repeated = certify_candidate_route_plan(
            &candidate,
            source,
            target,
            EvaluationLoadout::Baseline,
            &SolverConfig::for_abilities(AbilitySet::NONE),
        )
        .unwrap();
        assert_eq!(outcome, repeated);
        let RoutePlanCertificationOutcome::Certified(certificate) = outcome else {
            panic!("lateral terminal remained bounded: {outcome:?}");
        };
        assert!(matches!(
            certificate.segments.last().map(|segment| &segment.target),
            Some(RoutePlanCertificationSegmentTarget::ExactDoor { door_id })
                if door_id == target
        ));
        let mut summed = SearchStats::default();
        for segment in &certificate.segments {
            add_stats(&mut summed, segment.stats);
        }
        assert_eq!(certificate.total_stats, summed);
    }

    #[test]
    fn partition_floor_terminal_is_owned_by_exact_door_segment() {
        let native = PartitionRouteKey::new(
            0,
            AbilitySet::NONE,
            ChallengeIntent::Technical,
            PartitionRouteProfile::MixedBsp,
        )
        .regenerate()
        .unwrap();
        let floor = native
            .boundary_ports
            .iter()
            .find(|port| port.door.side == BoundarySide::Floor)
            .expect("technical partition candidate has a floor port");
        let target = floor.door.id.clone();
        let target_node_id = floor.node_id;
        let candidate: CorpusCandidate = native.into();
        let outcome = certify_candidate_route_plan(
            &candidate,
            "port-west",
            &target,
            EvaluationLoadout::Baseline,
            &SolverConfig::for_abilities(AbilitySet::NONE),
        )
        .unwrap();
        let RoutePlanCertificationOutcome::Certified(certificate) = outcome else {
            panic!("floor terminal remained bounded: {outcome:?}");
        };
        assert!(certificate.segments.iter().all(|segment| !matches!(
            segment.target,
            RoutePlanCertificationSegmentTarget::GroundedSupport {
                route_node_id,
                ..
            } if route_node_id == target_node_id
        )));
        assert!(matches!(
            certificate.segments.last().map(|segment| &segment.target),
            Some(RoutePlanCertificationSegmentTarget::ExactDoor { door_id })
                if door_id == &target
        ));
    }

    #[test]
    fn certification_path_prefers_an_alternate_without_a_third_port_node() {
        let support = |start_x| SupportSpec {
            start_x,
            end_x: start_x + 4,
            row: 16,
            kind: SupportKind::Solid,
        };
        let plan = RoutePlan {
            nodes: (0..4)
                .map(|id| downwards_gen::experimental::RouteNode {
                    id,
                    role: downwards_gen::experimental::NodeRole::Port,
                    support: support(2 + id * 6),
                })
                .collect(),
            edges: vec![
                RouteEdge {
                    from: 0,
                    to: 1,
                    verb: RouteVerb::Run,
                    critical: true,
                },
                RouteEdge {
                    from: 1,
                    to: 3,
                    verb: RouteVerb::Run,
                    critical: true,
                },
                RouteEdge {
                    from: 0,
                    to: 2,
                    verb: RouteVerb::Run,
                    critical: false,
                },
                RouteEdge {
                    from: 2,
                    to: 3,
                    verb: RouteVerb::Run,
                    critical: false,
                },
            ],
        };
        let door = |id: &str| Door {
            id: id.to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 20, 4, 20),
            arrival: Point::new(12, 20),
            destination_room: None,
            destination_door: None,
        };
        let ports = vec![
            BoundaryPort {
                node_id: 0,
                door: door("source"),
            },
            BoundaryPort {
                node_id: 1,
                door: door("third"),
            },
            BoundaryPort {
                node_id: 3,
                door: door("target"),
            },
        ];
        let selected =
            select_certification_path(&plan, &ports, 0, 3, EvaluationLoadout::Baseline).unwrap();
        assert_eq!(selected.node_path, [0, 2, 3]);
        assert_eq!(selected.cost.intermediate_boundary_port_nodes, 0);
    }

    #[test]
    fn certification_preflight_refuses_an_unavoidable_third_port_without_unreachability_claim() {
        let support = |start_x| SupportSpec {
            start_x,
            end_x: start_x + 4,
            row: 16,
            kind: SupportKind::Solid,
        };
        let plan = RoutePlan {
            nodes: (0..3)
                .map(|id| downwards_gen::experimental::RouteNode {
                    id,
                    role: downwards_gen::experimental::NodeRole::Port,
                    support: support(2 + id * 8),
                })
                .collect(),
            edges: vec![
                RouteEdge {
                    from: 0,
                    to: 1,
                    verb: RouteVerb::Run,
                    critical: true,
                },
                RouteEdge {
                    from: 1,
                    to: 2,
                    verb: RouteVerb::Run,
                    critical: true,
                },
            ],
        };
        let door = |id: &str| Door {
            id: id.to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 20, 4, 20),
            arrival: Point::new(12, 20),
            destination_room: None,
            destination_door: None,
        };
        let ports = vec![
            BoundaryPort {
                node_id: 0,
                door: door("source"),
            },
            BoundaryPort {
                node_id: 1,
                door: door("third"),
            },
            BoundaryPort {
                node_id: 2,
                door: door("target"),
            },
        ];
        let selected =
            select_certification_path(&plan, &ports, 0, 2, EvaluationLoadout::Baseline).unwrap();
        assert_eq!(selected.node_path, [0, 1, 2]);
        assert_eq!(selected.cost.intermediate_boundary_port_nodes, 1);
        let error = refuse_intermediate_boundary_port_path(&ports, &selected).unwrap_err();
        assert_eq!(
            error,
            RoutePlanCertificationError::IntermediateBoundaryPortPreflightRefusal {
                source_node_id: 0,
                target_node_id: 2,
                intermediate_port_node_ids: vec![1],
            }
        );
        assert!(
            error
                .to_string()
                .contains("not a physical-unreachability claim")
        );
    }

    #[test]
    fn config_fingerprint_is_deterministic_and_policy_bound() {
        let config = SolverConfig::for_abilities(AbilitySet::NONE);
        let first = route_plan_certification_config_fingerprint(&config);
        let second = route_plan_certification_config_fingerprint(&config);
        assert_eq!(first, second);
        assert_eq!(
            first.version,
            ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION
        );
    }
}
