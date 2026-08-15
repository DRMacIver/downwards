//! Positive-only terrain-use and controller-ablation diagnostics.
//!
//! This module deliberately distinguishes three kinds of observation:
//!
//! - generator structure can attribute terrain to route-plan supports;
//! - replay-certified controllers can place traversal near terrain; and
//! - replaying the same controller after a single-feature ablation can prove
//!   that the controller still succeeds without that feature.
//!
//! None of the complementary observations prove that terrain is unused,
//! unreachable, or necessary. In particular, an ablated controller failure
//! is not a reachability result for the modified room.

use std::{collections::BTreeSet, error::Error, fmt};

use downwards_ai::{ReachedTarget, ReplayDivergence, SearchTarget, TargetSolution};
use downwards_core::{AbilitySet, DeathReason, DoorEntryError, Room, Simulation, SimulationEvent};
use downwards_gen::{GeneratedLevel, StagedCompositionalCandidate, experimental::RoutePlan};
use downwards_lab::{
    ROOM_ABLATION_VERSION, RoomAblationError, RoomAblationKind, TraversalCell, TraversalGrid,
    TraversalSpan, TraversalTrace, room_ablation_variants,
};
use downwards_validation::{BoundedTargetEvidence, DoorTargetEvidenceBatch};

use crate::structural::{
    STRUCTURAL_DESCRIPTOR_VERSION, StructuralDescriptorError, TerrainUtilityDescriptor,
    describe_terrain_utility,
};

/// Version of the report schema and outcome policy in this module.
pub const TERRAIN_AUDIT_VERSION: u32 = 1;

/// Interpretation boundary for every [`TerrainAuditReport`].
pub const TERRAIN_AUDIT_EVIDENCE_DISCLAIMER: &str = "terrain coverage uses generator attribution and replay-certified positive witnesses only; uncorroborated terrain is not thereby unused or unreachable, a surviving ablated replay is positive redundancy evidence for that exact controller only, and any other ablated outcome is not proof that the removed feature is necessary";

/// Exact target identity of one stored positive controller.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PositiveControllerTarget {
    Door(String),
    Pickup(String),
}

impl PositiveControllerTarget {
    fn search_target(&self) -> SearchTarget {
        match self {
            Self::Door(id) => SearchTarget::door(id),
            Self::Pickup(id) => SearchTarget::pickup(id),
        }
    }

    fn reached_target(&self) -> ReachedTarget {
        match self {
            Self::Door(id) => ReachedTarget::Door(id.clone()),
            Self::Pickup(id) => ReachedTarget::Pickup(id.clone()),
        }
    }
}

/// Stable identity of an exact replay-certified controller.
///
/// The witness fingerprint binds the original room, source arrival, loadout,
/// typed target, actions, authoritative replay digests, and solver statistics.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveControllerId {
    pub source_door_id: String,
    pub target: PositiveControllerTarget,
    pub witness_fingerprint: u64,
}

/// Terrain evidence observed from one replay-certified positive controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositiveTerrainWitness {
    pub controller: PositiveControllerId,
    pub replay_ticks: usize,
    pub completion_tick: usize,
    pub traversal: TraversalTrace,
    /// Attribution and proximity evaluated with only this witness's trace.
    pub terrain_utility: TerrainUtilityDescriptor,
}

/// Aggregate structural and positive-traversal coverage.
///
/// "Uncorroborated" means neither statically attributed nor near any supplied
/// positive traversal. It deliberately does not mean unused or unreachable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PositiveTerrainCoverage {
    pub positive_controller_count: usize,
    pub interior_component_count: usize,
    pub interior_tile_count: usize,
    pub structurally_attributed_component_count: usize,
    pub structurally_attributed_tile_count: usize,
    pub traversal_near_component_count: usize,
    pub traversal_near_tile_count: usize,
    pub positively_corroborated_component_count: usize,
    pub positively_corroborated_tile_count: usize,
    pub uncorroborated_component_count: usize,
    pub uncorroborated_tile_count: usize,
}

impl PositiveTerrainCoverage {
    fn from_utility(utility: &TerrainUtilityDescriptor, positive_controller_count: usize) -> Self {
        let traversal_near_component_count = utility
            .components
            .iter()
            .filter(|component| component.certified_traversal_near_tiles > 0)
            .count();
        Self {
            positive_controller_count,
            interior_component_count: utility.components.len(),
            interior_tile_count: utility.interior_terrain_tiles,
            structurally_attributed_component_count: utility.components.len()
                - utility.components_without_static_attribution,
            structurally_attributed_tile_count: utility.static_attributed_tiles,
            traversal_near_component_count,
            traversal_near_tile_count: utility.certified_traversal_near_tiles,
            positively_corroborated_component_count: utility.components.len()
                - utility.components_without_positive_corroboration,
            positively_corroborated_tile_count: utility.positively_corroborated_tiles,
            uncorroborated_component_count: utility.components_without_positive_corroboration,
            uncorroborated_tile_count: utility.tiles_without_positive_corroboration,
        }
    }
}

/// What happened when one exact stored controller was applied unchanged to an
/// ablated room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AblatedControllerOutcome {
    /// The exact typed target was reached. This is positive redundancy
    /// evidence for this controller and removed feature only.
    Succeeded { completion_tick: usize },
    /// The controller encountered a death before reaching its exact target.
    Died { tick: usize, reason: DeathReason },
    /// The controller reached a different terminal door or legacy exit.
    WrongTarget { tick: usize, reached: ReachedTarget },
    /// The stored action sequence ended without target, death, or a different
    /// terminal trigger. This is a diagnostic, not an impossibility claim.
    Diverged { frames_replayed: usize },
}

/// Result of replaying one exact controller against one ablation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerAblationObservation {
    pub controller: PositiveControllerId,
    pub outcome: AblatedControllerOutcome,
    /// First tick at which player state, room-attempt state, collected
    /// pickups, or emitted events differed from the original run. Zero denotes
    /// an initial-state difference; later ticks are one-based action ticks.
    /// Room identity itself is intentionally excluded from this comparison.
    pub first_behavior_divergence_tick: Option<usize>,
}

/// Outcome totals for one ablation. These are counts of exact stored
/// controllers, not estimates of modified-room reachability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AblationOutcomeSummary {
    pub controller_count: usize,
    pub succeeded: usize,
    pub died: usize,
    pub wrong_target: usize,
    pub diverged: usize,
}

impl AblationOutcomeSummary {
    fn from_observations(observations: &[ControllerAblationObservation]) -> Self {
        let mut summary = Self {
            controller_count: observations.len(),
            ..Self::default()
        };
        for observation in observations {
            match &observation.outcome {
                AblatedControllerOutcome::Succeeded { .. } => summary.succeeded += 1,
                AblatedControllerOutcome::Died { .. } => summary.died += 1,
                AblatedControllerOutcome::WrongTarget { .. } => summary.wrong_target += 1,
                AblatedControllerOutcome::Diverged { .. } => summary.diverged += 1,
            }
        }
        summary
    }
}

/// Positive-controller experiment for one canonical single-feature removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureAblationAudit {
    pub kind: RoomAblationKind,
    pub controllers: Vec<ControllerAblationObservation>,
    pub summary: AblationOutcomeSummary,
}

/// Versioned terrain-use and ablation report for one candidate/loadout pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainAuditReport {
    pub version: u32,
    pub structural_descriptor_version: u32,
    pub room_ablation_version: u32,
    pub loadout: AbilitySet,
    pub positive_door_controller_count: usize,
    pub positive_pickup_controller_count: usize,
    pub positive_witnesses: Vec<PositiveTerrainWitness>,
    /// Utility recomputed using every positive trace together.
    pub aggregate_terrain_utility: TerrainUtilityDescriptor,
    pub coverage: PositiveTerrainCoverage,
    pub ablations: Vec<FeatureAblationAudit>,
}

#[derive(Debug)]
pub enum TerrainAuditError {
    InvalidEvidenceMatrix {
        detail: String,
    },
    PositiveTargetContract {
        controller: Box<PositiveControllerId>,
        expected: Box<SearchTarget>,
        reported_target: Box<SearchTarget>,
        reported_reached: Box<ReachedTarget>,
    },
    DoorEntry {
        controller: PositiveControllerId,
        source: DoorEntryError,
    },
    OriginalReplayDiverged {
        controller: PositiveControllerId,
        source: Box<ReplayDivergence>,
    },
    OriginalControllerDied {
        controller: PositiveControllerId,
        tick: usize,
        reason: DeathReason,
    },
    OriginalControllerHitWrongTarget {
        controller: PositiveControllerId,
        tick: usize,
        reached: ReachedTarget,
    },
    OriginalControllerMissedTarget {
        controller: PositiveControllerId,
    },
    AblationObjectContract {
        kind: RoomAblationKind,
        detail: String,
    },
    Structural(StructuralDescriptorError),
    Ablation(RoomAblationError),
}

impl fmt::Display for TerrainAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEvidenceMatrix { detail } => {
                write!(formatter, "invalid route evidence matrix: {detail}")
            }
            Self::PositiveTargetContract {
                controller,
                expected,
                reported_target,
                reported_reached,
            } => write!(
                formatter,
                "positive controller {controller:?} expected {expected:?}, but its solution reports target {reported_target:?} and reached trigger {reported_reached:?}",
            ),
            Self::DoorEntry { controller, source } => write!(
                formatter,
                "cannot recreate source arrival for positive controller {controller:?}: {source}"
            ),
            Self::OriginalReplayDiverged { controller, source } => write!(
                formatter,
                "positive controller {controller:?} no longer verifies on its original candidate: {source}"
            ),
            Self::OriginalControllerDied {
                controller,
                tick,
                reason,
            } => write!(
                formatter,
                "positive controller {controller:?} died at tick {tick} on its original candidate: {reason:?}"
            ),
            Self::OriginalControllerHitWrongTarget {
                controller,
                tick,
                reached,
            } => write!(
                formatter,
                "positive controller {controller:?} hit {reached:?} at tick {tick} before its claimed target"
            ),
            Self::OriginalControllerMissedTarget { controller } => write!(
                formatter,
                "positive controller {controller:?} exhausted its replay without reaching its claimed target"
            ),
            Self::AblationObjectContract { kind, detail } => write!(
                formatter,
                "ablation {kind:?} did not preserve the room/object reconstruction contract: {detail}"
            ),
            Self::Structural(source) => write!(formatter, "terrain structure is invalid: {source}"),
            Self::Ablation(source) => {
                write!(formatter, "cannot construct room ablations: {source}")
            }
        }
    }
}

impl Error for TerrainAuditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorEntry { source, .. } => Some(source),
            Self::OriginalReplayDiverged { source, .. } => Some(source.as_ref()),
            Self::Structural(source) => Some(source),
            Self::Ablation(source) => Some(source),
            Self::InvalidEvidenceMatrix { .. }
            | Self::PositiveTargetContract { .. }
            | Self::OriginalControllerDied { .. }
            | Self::OriginalControllerHitWrongTarget { .. }
            | Self::OriginalControllerMissedTarget { .. }
            | Self::AblationObjectContract { .. } => None,
        }
    }
}

impl From<StructuralDescriptorError> for TerrainAuditError {
    fn from(source: StructuralDescriptorError) -> Self {
        Self::Structural(source)
    }
}

impl From<RoomAblationError> for TerrainAuditError {
    fn from(source: RoomAblationError) -> Self {
        Self::Ablation(source)
    }
}

#[derive(Clone)]
struct StoredPositiveController {
    id: PositiveControllerId,
    solution: TargetSolution,
}

/// Audit structural terrain coverage and exact-controller ablations for one
/// staged candidate under one explicit loadout.
///
/// All positive rows are re-certified from their exact source-door arrival on
/// `candidate` before they contribute. Inconclusive rows contribute no
/// traversal evidence and are never converted into negative claims.
pub fn audit_candidate_terrain(
    candidate: &StagedCompositionalCandidate,
    evidence: &DoorTargetEvidenceBatch,
) -> Result<TerrainAuditReport, TerrainAuditError> {
    audit_generated_terrain(&candidate.generated, &candidate.route_plan, evidence)
}

/// Generator-neutral terrain and exact-controller ablation audit.
///
/// `generated` supplies the authoritative simulation room while `route_plan`
/// supplies only structural attribution. Positive controller evidence is
/// replayed exactly as in [`audit_candidate_terrain`].
pub fn audit_generated_terrain(
    generated: &GeneratedLevel,
    route_plan: &RoutePlan,
    evidence: &DoorTargetEvidenceBatch,
) -> Result<TerrainAuditReport, TerrainAuditError> {
    validate_matrix_shape(&generated.room, evidence)?;
    let mut controllers = collect_positive_controllers(evidence)?;
    controllers.sort_unstable_by(|left, right| left.id.cmp(&right.id));

    let mut positive_witnesses = Vec::with_capacity(controllers.len());
    for stored in &controllers {
        let initial = Simulation::enter_via_door(
            generated.room.clone(),
            evidence.loadout(),
            &stored.id.source_door_id,
        )
        .map_err(|source| TerrainAuditError::DoorEntry {
            controller: stored.id.clone(),
            source,
        })?;
        stored.solution.replay.verify(&initial).map_err(|source| {
            TerrainAuditError::OriginalReplayDiverged {
                controller: stored.id.clone(),
                source: Box::new(source),
            }
        })?;
        let (traversal, completion_tick) = observe_original_controller(
            &initial,
            &stored.solution,
            &stored.id,
            TraversalGrid::default(),
        )?;
        let terrain_utility = describe_terrain_utility(&generated.room, route_plan, &[&traversal])?;
        positive_witnesses.push(PositiveTerrainWitness {
            controller: stored.id.clone(),
            replay_ticks: stored.solution.replay.frames.len(),
            completion_tick,
            traversal,
            terrain_utility,
        });
    }

    let traversal_refs = positive_witnesses
        .iter()
        .map(|witness| &witness.traversal)
        .collect::<Vec<_>>();
    let aggregate_terrain_utility =
        describe_terrain_utility(&generated.room, route_plan, &traversal_refs)?;
    let coverage =
        PositiveTerrainCoverage::from_utility(&aggregate_terrain_utility, controllers.len());

    let mut ablations = Vec::new();
    for ablation in room_ablation_variants(&generated.room)? {
        validate_ablation_object_contract(&generated.room, &ablation.room, ablation.kind)?;
        let mut observations = Vec::with_capacity(controllers.len());
        for stored in &controllers {
            observations.push(replay_controller_on_ablation(
                &generated.room,
                &ablation.room,
                evidence.loadout(),
                stored,
            )?);
        }
        let summary = AblationOutcomeSummary::from_observations(&observations);
        ablations.push(FeatureAblationAudit {
            kind: ablation.kind,
            controllers: observations,
            summary,
        });
    }

    let positive_door_controller_count = controllers
        .iter()
        .filter(|controller| matches!(&controller.id.target, PositiveControllerTarget::Door(_)))
        .count();
    let positive_pickup_controller_count = controllers.len() - positive_door_controller_count;
    Ok(TerrainAuditReport {
        version: TERRAIN_AUDIT_VERSION,
        structural_descriptor_version: STRUCTURAL_DESCRIPTOR_VERSION,
        room_ablation_version: ROOM_ABLATION_VERSION,
        loadout: evidence.loadout(),
        positive_door_controller_count,
        positive_pickup_controller_count,
        positive_witnesses,
        aggregate_terrain_utility,
        coverage,
        ablations,
    })
}

fn validate_ablation_object_contract(
    original: &Room,
    ablated: &Room,
    kind: RoomAblationKind,
) -> Result<(), TerrainAuditError> {
    if original.id() != ablated.id()
        || original.name() != ablated.name()
        || original.width() != ablated.width()
        || original.height() != ablated.height()
        || original.tile_size() != ablated.tile_size()
        || original.spawn() != ablated.spawn()
        || original.exits() != ablated.exits()
    {
        return Err(TerrainAuditError::AblationObjectContract {
            kind,
            detail: "room identity, dimensions, spawn, or legacy exits changed".to_owned(),
        });
    }
    if original.doors() != ablated.doors() {
        return Err(TerrainAuditError::AblationObjectContract {
            kind,
            detail: "door definitions or exact arrivals changed".to_owned(),
        });
    }
    if original.pickups() != ablated.pickups() {
        return Err(TerrainAuditError::AblationObjectContract {
            kind,
            detail: "pickup definitions changed".to_owned(),
        });
    }
    let expected_timed_hazards = match kind {
        RoomAblationKind::InteriorTerrainComponent { .. }
        | RoomAblationKind::StaticHazardComponent { .. } => original.timed_hazards().to_vec(),
        RoomAblationKind::TimedHazard { hazard_index } => original
            .timed_hazards()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != hazard_index)
            .map(|(_, hazard)| hazard.clone())
            .collect(),
    };
    if ablated.timed_hazards() != expected_timed_hazards {
        return Err(TerrainAuditError::AblationObjectContract {
            kind,
            detail: "timed-hazard reconstruction changed objects other than the selected hazard"
                .to_owned(),
        });
    }
    Ok(())
}

fn validate_matrix_shape(
    room: &Room,
    evidence: &DoorTargetEvidenceBatch,
) -> Result<(), TerrainAuditError> {
    let door_ids = room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<BTreeSet<_>>();
    let pickup_ids = room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect::<BTreeSet<_>>();
    let expected_door_routes = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect::<BTreeSet<_>>();
    let actual_door_routes = evidence
        .door_routes()
        .iter()
        .map(|route| (route.source_door_id.clone(), route.target_door_id.clone()))
        .collect::<BTreeSet<_>>();
    if actual_door_routes.len() != evidence.door_routes().len()
        || actual_door_routes != expected_door_routes
    {
        return Err(TerrainAuditError::InvalidEvidenceMatrix {
            detail: format!(
                "door rows do not match the candidate's complete directed matrix (expected {}, received {} rows with {} unique identities)",
                expected_door_routes.len(),
                evidence.door_routes().len(),
                actual_door_routes.len()
            ),
        });
    }

    let expected_pickup_routes = door_ids
        .iter()
        .flat_map(|source| {
            pickup_ids
                .iter()
                .map(move |pickup| (source.clone(), pickup.clone()))
        })
        .collect::<BTreeSet<_>>();
    let actual_pickup_routes = evidence
        .pickup_routes()
        .iter()
        .map(|route| {
            (
                route.source_door_id.clone(),
                route.required_pickup_id.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    if actual_pickup_routes.len() != evidence.pickup_routes().len()
        || actual_pickup_routes != expected_pickup_routes
    {
        return Err(TerrainAuditError::InvalidEvidenceMatrix {
            detail: format!(
                "pickup rows do not match the candidate's complete source-by-pickup matrix (expected {}, received {} rows with {} unique identities)",
                expected_pickup_routes.len(),
                evidence.pickup_routes().len(),
                actual_pickup_routes.len()
            ),
        });
    }
    Ok(())
}

fn collect_positive_controllers(
    evidence: &DoorTargetEvidenceBatch,
) -> Result<Vec<StoredPositiveController>, TerrainAuditError> {
    let mut controllers = Vec::new();
    for row in evidence.door_routes() {
        let BoundedTargetEvidence::Positive(positive) = &row.evidence else {
            continue;
        };
        let id = PositiveControllerId {
            source_door_id: row.source_door_id.clone(),
            target: PositiveControllerTarget::Door(row.target_door_id.clone()),
            witness_fingerprint: positive.witness_fingerprint().as_u64(),
        };
        validate_solution_contract(&id, positive.solution())?;
        controllers.push(StoredPositiveController {
            id,
            solution: positive.solution().clone(),
        });
    }
    for row in evidence.pickup_routes() {
        let BoundedTargetEvidence::Positive(positive) = &row.evidence else {
            continue;
        };
        let id = PositiveControllerId {
            source_door_id: row.source_door_id.clone(),
            target: PositiveControllerTarget::Pickup(row.required_pickup_id.clone()),
            witness_fingerprint: positive.witness_fingerprint().as_u64(),
        };
        validate_solution_contract(&id, positive.solution())?;
        controllers.push(StoredPositiveController {
            id,
            solution: positive.solution().clone(),
        });
    }
    Ok(controllers)
}

fn validate_solution_contract(
    controller: &PositiveControllerId,
    solution: &TargetSolution,
) -> Result<(), TerrainAuditError> {
    let expected = controller.target.search_target();
    if solution.target == expected && solution.reached == controller.target.reached_target() {
        return Ok(());
    }
    Err(TerrainAuditError::PositiveTargetContract {
        controller: Box::new(controller.clone()),
        expected: Box::new(expected),
        reported_target: Box::new(solution.target.clone()),
        reported_reached: Box::new(solution.reached.clone()),
    })
}

fn observe_original_controller(
    initial: &Simulation,
    solution: &TargetSolution,
    controller: &PositiveControllerId,
    grid: TraversalGrid,
) -> Result<(TraversalTrace, usize), TerrainAuditError> {
    let mut simulation = initial.clone();
    let mut spans = vec![TraversalSpan {
        cell: traversal_cell(&simulation, grid),
        samples: 1,
    }];
    let mut visited = BTreeSet::from([spans[0].cell]);

    for (frame_index, frame) in solution.replay.frames.iter().enumerate() {
        let tick = frame_index + 1;
        let report = simulation.step(frame.action);
        let cell = traversal_cell(&simulation, grid);
        push_traversal_span(&mut spans, cell);
        visited.insert(cell);

        if let Some(reason) = first_death(&report.events) {
            return Err(TerrainAuditError::OriginalControllerDied {
                controller: controller.clone(),
                tick,
                reason,
            });
        }
        if target_reached(&simulation, &controller.target) {
            return Ok((
                TraversalTrace {
                    grid,
                    sample_count: tick + 1,
                    spans: spans.into_boxed_slice(),
                    visited_cells: visited.into_iter().collect(),
                },
                tick,
            ));
        }
        if let Some(reached) = wrong_terminal_target(&simulation, &controller.target) {
            return Err(TerrainAuditError::OriginalControllerHitWrongTarget {
                controller: controller.clone(),
                tick,
                reached,
            });
        }
    }
    Err(TerrainAuditError::OriginalControllerMissedTarget {
        controller: controller.clone(),
    })
}

fn replay_controller_on_ablation(
    original_room: &Room,
    ablated_room: &Room,
    loadout: AbilitySet,
    stored: &StoredPositiveController,
) -> Result<ControllerAblationObservation, TerrainAuditError> {
    let mut original =
        Simulation::enter_via_door(original_room.clone(), loadout, &stored.id.source_door_id)
            .map_err(|source| TerrainAuditError::DoorEntry {
                controller: stored.id.clone(),
                source,
            })?;
    let mut ablated =
        Simulation::enter_via_door(ablated_room.clone(), loadout, &stored.id.source_door_id)
            .map_err(|source| TerrainAuditError::DoorEntry {
                controller: stored.id.clone(),
                source,
            })?;
    let mut first_behavior_divergence_tick =
        (!same_observable_state(&original, &ablated)).then_some(0);

    for (frame_index, frame) in stored.solution.replay.frames.iter().enumerate() {
        let tick = frame_index + 1;
        let original_report = original.step(frame.action);
        let ablated_report = ablated.step(frame.action);
        if first_behavior_divergence_tick.is_none()
            && (original_report.events != ablated_report.events
                || !same_observable_state(&original, &ablated))
        {
            first_behavior_divergence_tick = Some(tick);
        }
        if let Some(reason) = first_death(&ablated_report.events) {
            return Ok(ControllerAblationObservation {
                controller: stored.id.clone(),
                outcome: AblatedControllerOutcome::Died { tick, reason },
                first_behavior_divergence_tick,
            });
        }
        if target_reached(&ablated, &stored.id.target) {
            return Ok(ControllerAblationObservation {
                controller: stored.id.clone(),
                outcome: AblatedControllerOutcome::Succeeded {
                    completion_tick: tick,
                },
                first_behavior_divergence_tick,
            });
        }
        if let Some(reached) = wrong_terminal_target(&ablated, &stored.id.target) {
            return Ok(ControllerAblationObservation {
                controller: stored.id.clone(),
                outcome: AblatedControllerOutcome::WrongTarget { tick, reached },
                first_behavior_divergence_tick,
            });
        }
    }
    Ok(ControllerAblationObservation {
        controller: stored.id.clone(),
        outcome: AblatedControllerOutcome::Diverged {
            frames_replayed: stored.solution.replay.frames.len(),
        },
        first_behavior_divergence_tick,
    })
}

fn target_reached(simulation: &Simulation, target: &PositiveControllerTarget) -> bool {
    match target {
        PositiveControllerTarget::Door(id) => simulation.reached_exit() == Some(id),
        PositiveControllerTarget::Pickup(id) => simulation
            .room()
            .pickups()
            .iter()
            .position(|pickup| pickup.id() == id)
            .and_then(|index| simulation.pickup_is_collected(index))
            .unwrap_or(false),
    }
}

fn wrong_terminal_target(
    simulation: &Simulation,
    target: &PositiveControllerTarget,
) -> Option<ReachedTarget> {
    let reached_id = simulation.reached_exit()?;
    if matches!(target, PositiveControllerTarget::Door(id) if id == reached_id) {
        return None;
    }
    if simulation
        .room()
        .doors()
        .iter()
        .any(|door| door.id == reached_id)
    {
        Some(ReachedTarget::Door(reached_id.to_owned()))
    } else {
        Some(ReachedTarget::Exit(reached_id.to_owned()))
    }
}

fn first_death(events: &[SimulationEvent]) -> Option<DeathReason> {
    events.iter().find_map(|event| match event {
        SimulationEvent::Died(reason) => Some(*reason),
        _ => None,
    })
}

fn same_observable_state(left: &Simulation, right: &Simulation) -> bool {
    left.player() == right.player()
        && left.room_tick() == right.room_tick()
        && left.deaths() == right.deaths()
        && left.reached_exit() == right.reached_exit()
        && left
            .collected_pickups()
            .map(|pickup| pickup.id())
            .eq(right.collected_pickups().map(|pickup| pickup.id()))
}

fn traversal_cell(simulation: &Simulation, grid: TraversalGrid) -> TraversalCell {
    let room_width = i32::from(simulation.room().width()) * simulation.room().tile_size();
    let room_height = i32::from(simulation.room().height()) * simulation.room().tile_size();
    let bounds = simulation.player().bounds();
    let center_x = (bounds.x + bounds.width / 2).clamp(0, room_width - 1);
    let center_y = (bounds.y + bounds.height / 2).clamp(0, room_height - 1);
    TraversalCell {
        x: ((i64::from(center_x) * i64::from(grid.columns())) / i64::from(room_width)) as u16,
        y: ((i64::from(center_y) * i64::from(grid.rows())) / i64::from(room_height)) as u16,
    }
}

fn push_traversal_span(spans: &mut Vec<TraversalSpan>, cell: TraversalCell) {
    if let Some(last) = spans.last_mut().filter(|last| last.cell == cell) {
        last.samples += 1;
    } else {
        spans.push(TraversalSpan { cell, samples: 1 });
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{BoundarySide, Door, Pickup, Point, Rect, Tile};
    use downwards_gen::{
        AbilityTier, COMPOSITIONAL_GENERATION_VERSION, CompositionalFeatureSet, CompositionalKey,
        CompositionalProfile, GeneratedLevel, GeneratedMetadata, GenerationStats, LayoutFamily,
        StagedCompositionalKey,
        experimental::{
            BoundaryPort, ChallengeIntent, GenerationStrategy, NodeRole, RouteEdge, RouteNode,
            RoutePlan, RouteVerb, SupportKind, SupportSpec,
        },
    };
    use downwards_validation::{ValidationConfig, evaluate_generated_door_targets_for_loadout};

    use super::*;

    const WIDTH: u16 = 32;
    const HEIGHT: u16 = 18;
    const TILE_SIZE: i32 = 10;

    #[test]
    fn decorative_platform_has_only_absent_positive_corroboration_while_route_support_matters_to_stored_controllers()
     {
        let candidate = audit_fixture();
        let mut config = ValidationConfig::for_loadout(AbilitySet::NONE);
        config.solver.max_ticks_per_path = 500;
        let evidence = evaluate_generated_door_targets_for_loadout(
            &candidate.generated,
            AbilitySet::NONE,
            &config,
        )
        .unwrap();
        assert!(
            evidence
                .door_routes()
                .iter()
                .all(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        );
        assert!(
            evidence
                .pickup_routes()
                .iter()
                .all(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        );

        let first = audit_candidate_terrain(&candidate, &evidence).unwrap();
        let second =
            audit_generated_terrain(&candidate.generated, &candidate.route_plan, &evidence)
                .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.version, TERRAIN_AUDIT_VERSION);
        assert_eq!(
            first.structural_descriptor_version,
            STRUCTURAL_DESCRIPTOR_VERSION
        );
        assert_eq!(first.room_ablation_version, ROOM_ABLATION_VERSION);
        assert_eq!(first.positive_door_controller_count, 2);
        assert_eq!(first.positive_pickup_controller_count, 2);
        assert_eq!(first.coverage.positive_controller_count, 4);
        assert_eq!(first.coverage.interior_component_count, 2);
        assert_eq!(first.coverage.structurally_attributed_component_count, 1);
        assert_eq!(first.coverage.uncorroborated_component_count, 1);
        assert!(first.coverage.traversal_near_component_count >= 1);
        assert_eq!(first.ablations.len(), 3);
        let expected_controllers = first
            .positive_witnesses
            .iter()
            .map(|witness| witness.controller.clone())
            .collect::<Vec<_>>();
        for ablation in &first.ablations {
            assert_eq!(
                ablation.summary.controller_count,
                expected_controllers.len()
            );
            assert_eq!(
                ablation.summary.succeeded
                    + ablation.summary.died
                    + ablation.summary.wrong_target
                    + ablation.summary.diverged,
                ablation.summary.controller_count
            );
            assert_eq!(
                ablation
                    .controllers
                    .iter()
                    .map(|observation| observation.controller.clone())
                    .collect::<Vec<_>>(),
                expected_controllers
            );
        }

        let decorative = first
            .ablations
            .iter()
            .find(|audit| {
                audit.kind
                    == RoomAblationKind::InteriorTerrainComponent {
                        component_index: 0,
                        tile_count: 4,
                    }
            })
            .unwrap();
        assert_eq!(decorative.summary.succeeded, 4);
        assert_eq!(decorative.summary.died, 0);
        assert_eq!(decorative.summary.wrong_target, 0);
        assert_eq!(decorative.summary.diverged, 0);
        assert!(
            decorative
                .controllers
                .iter()
                .all(|observation| observation.first_behavior_divergence_tick.is_none())
        );

        let route_support = first
            .ablations
            .iter()
            .find(|audit| {
                matches!(
                    audit.kind,
                    RoomAblationKind::InteriorTerrainComponent {
                        component_index: 1,
                        ..
                    }
                )
            })
            .unwrap();
        assert!(route_support.summary.succeeded < route_support.summary.controller_count);
        assert!(route_support.controllers.iter().any(|observation| {
            !matches!(
                observation.outcome,
                AblatedControllerOutcome::Succeeded { .. }
            ) && observation.first_behavior_divergence_tick.is_some()
        }));

        let removed_hazard = first
            .ablations
            .iter()
            .find(|audit| matches!(audit.kind, RoomAblationKind::StaticHazardComponent { .. }))
            .unwrap();
        assert_eq!(removed_hazard.summary.succeeded, 4);
    }

    fn audit_fixture() -> StagedCompositionalCandidate {
        let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
        for x in 0..WIDTH {
            set_tile(&mut tiles, x, 0, Tile::Solid);
            set_tile(&mut tiles, x, HEIGHT - 1, Tile::Solid);
        }
        for y in 1..HEIGHT - 1 {
            set_tile(&mut tiles, 0, y, Tile::Solid);
            set_tile(&mut tiles, WIDTH - 1, y, Tile::Solid);
        }
        for y in 13..=16 {
            set_tile(&mut tiles, 0, y, Tile::Empty);
            set_tile(&mut tiles, WIDTH - 1, y, Tile::Empty);
        }
        // Canonical interior component 0: high, decorative, and far from all
        // successful baseline traversals in this fixture.
        for x in 3..7 {
            set_tile(&mut tiles, x, 5, Tile::Solid);
        }
        // Canonical interior component 1: the bridge across the hazard basin.
        for x in 7..25 {
            set_tile(&mut tiles, x, 15, Tile::OneWay);
        }
        for x in 8..24 {
            set_tile(&mut tiles, x, HEIGHT - 1, Tile::Hazard);
        }

        let west = Door {
            id: "west".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 130, 4, 40),
            arrival: Point::new(20, 158),
            destination_room: None,
            destination_door: None,
        };
        let east = Door {
            id: "east".to_owned(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 130, 4, 40),
            arrival: Point::new(292, 158),
            destination_room: None,
            destination_door: None,
        };
        let room = Room::new(
            "terrain-audit-fixture",
            "Terrain audit fixture",
            WIDTH,
            HEIGHT,
            TILE_SIZE,
            tiles,
            Point::new(20, 158),
            vec![],
        )
        .unwrap()
        .with_objects(
            vec![],
            vec![Pickup::new("bridge-cache", Rect::new(157, 136, 6, 6)).unwrap()],
        )
        .unwrap()
        .with_doors(vec![west.clone(), east.clone()])
        .unwrap();
        let plan = RoutePlan {
            nodes: vec![
                RouteNode {
                    id: 0,
                    role: NodeRole::Port,
                    support: support(1, 7, 17, SupportKind::Solid),
                },
                RouteNode {
                    id: 1,
                    role: NodeRole::Landing,
                    support: support(7, 25, 15, SupportKind::OneWay),
                },
                RouteNode {
                    id: 2,
                    role: NodeRole::Port,
                    support: support(25, 31, 17, SupportKind::Solid),
                },
            ],
            edges: vec![
                RouteEdge {
                    from: 0,
                    to: 1,
                    verb: RouteVerb::Jump,
                    critical: true,
                },
                RouteEdge {
                    from: 1,
                    to: 2,
                    verb: RouteVerb::Jump,
                    critical: true,
                },
            ],
        };
        let profile = CompositionalProfile::new(
            AbilitySet::NONE,
            GenerationStrategy::ReachabilityGrowth,
            ChallengeIntent::Standard,
        );
        StagedCompositionalCandidate {
            key: StagedCompositionalKey::new(
                CompositionalKey::new(0x7e22_a001, profile),
                CompositionalFeatureSet::StaticHazards,
            ),
            generated: GeneratedLevel {
                room,
                metadata: GeneratedMetadata {
                    generation_version: COMPOSITIONAL_GENERATION_VERSION,
                    seed: 0x7e22_a001,
                    layout_family: LayoutFamily::HazardRun,
                    ability_tier: AbilityTier::Baseline,
                    intended_abilities: AbilitySet::NONE,
                    stats: GenerationStats::default(),
                },
            },
            route_summary: plan.summary(),
            route_plan: plan,
            boundary_ports: vec![
                BoundaryPort {
                    node_id: 0,
                    door: west,
                },
                BoundaryPort {
                    node_id: 2,
                    door: east,
                },
            ],
        }
    }

    const fn support(start_x: u16, end_x: u16, row: u16, kind: SupportKind) -> SupportSpec {
        SupportSpec {
            start_x,
            end_x,
            row,
            kind,
        }
    }

    fn set_tile(tiles: &mut [Tile], x: u16, y: u16, tile: Tile) {
        tiles[usize::from(y) * usize::from(WIDTH) + usize::from(x)] = tile;
    }
}
