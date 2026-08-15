//! Exact authoritative audit for the two promotable physical-v2 ability keys.
//!
//! This deliberately is not a corpus enumerator.  Its key set and solver
//! configurations are frozen below so a failed claim is reported rather than
//! retried with another seed, embedding attempt, rewrite attempt, or budget.

use std::error::Error;

use downwards_ai::{
    DIRECT_PROBE_AUDIT_VERSION, DirectProbeAuditStatus, DirectProbeBudgetLimit, Replay,
    SearchStats, SearchTarget, SolverConfig, TargetSolveOutcome, audit_direct_controller_probes,
    solve_target,
};
use downwards_core::{
    AbilitySet, JumpKind, PLAYER_HEIGHT, PLAYER_WIDTH, Rect, Simulation, SimulationEvent, Tile,
};
use downwards_gen::{
    TILE_SIZE,
    experimental::{
        AbilityGateEmbeddingPendingReason, AbilityGateEmbeddingState,
        COMPOSITIONAL_ABILITY_GENERATION_VERSION, ChallengeIntent, CompositionalAbilityCandidate,
        CompositionalAbilityGateProfile, CompositionalAbilityGenerationKey,
        DirectedTraversalRequirement, GateAbility, RouteVerb,
        compositional_route_cut_socket_in_inventory,
    },
};
use downwards_lab::{TraversalGrid, observe_successful_replay};
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};

const SOURCE_DOOR: &str = "port-0";
const SINK_DOOR: &str = "port-1";

const EXACT_KEYS: [(CompositionalAbilityGateProfile, u64); 2] = [
    (CompositionalAbilityGateProfile::WallJump, 0),
    (CompositionalAbilityGateProfile::Dash, 0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FiniteDirectOutcome {
    Positive {
        witness_count: usize,
        status: DirectProbeAuditStatus,
        stats: SearchStats,
    },
    CompleteNoPositive {
        stats: SearchStats,
    },
    Bounded {
        limit: DirectProbeBudgetLimit,
        stats: SearchStats,
    },
}

#[derive(Clone, Debug)]
struct MissingLoadoutAudit {
    loadout: AbilitySet,
    outcome: FiniteDirectOutcome,
}

#[derive(Clone, Copy, Debug, Default)]
struct AcceptedAbilityEvents {
    wall_jumps: usize,
    dashes: usize,
}

#[derive(Clone, Debug)]
struct ExactKeyReport {
    profile: CompositionalAbilityGateProfile,
    seed: u64,
    intended_matrix_door_routes: usize,
    intended_matrix_pickup_routes: usize,
    intended_matrix_all_positive: bool,
    canonical_gate_events: Option<AcceptedAbilityEvents>,
    intended_direct_events: Vec<AcceptedAbilityEvents>,
    reverse_baseline_positive: bool,
    missing_loadouts: Vec<MissingLoadoutAudit>,
    violations: Vec<String>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SpatialTraceSummary {
    loadout: AbilitySet,
    reached_exit: Option<String>,
    collected_pickups: Vec<String>,
    support_sequence: Vec<Vec<u16>>,
    accepted_events: Vec<String>,
    gate_dash_events: Vec<(u16, usize)>,
    gate_wall_jump_events: Vec<(u16, usize)>,
    gate_vertical_coverage: Vec<(u16, i32, i32)>,
    cut_crossings: Vec<String>,
}

impl ExactKeyReport {
    fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "compositional-ability-gate-audit physical-generation-version={} direct-probe-version={} exact-keys={}",
        COMPOSITIONAL_ABILITY_GENERATION_VERSION,
        DIRECT_PROBE_AUDIT_VERSION,
        EXACT_KEYS.len(),
    );
    let mut reports = Vec::new();
    for (profile, seed) in EXACT_KEYS {
        let key = CompositionalAbilityGenerationKey::new(seed, profile, ChallengeIntent::Standard);
        let candidate = key.generate()?;
        reports.push(audit_exact_candidate(&candidate)?);
    }

    for report in &reports {
        println!(
            "key profile={} seed={} intended-door-routes={} intended-pickup-routes={} intended-all-positive={} canonical-events={:?} reverse-baseline-positive={} passed={}",
            report.profile.slug(),
            report.seed,
            report.intended_matrix_door_routes,
            report.intended_matrix_pickup_routes,
            report.intended_matrix_all_positive,
            report.canonical_gate_events,
            report.reverse_baseline_positive,
            report.passed(),
        );
        for (index, events) in report.intended_direct_events.iter().enumerate() {
            println!(
                "  intended-direct-witness index={index} wall-jumps={} dashes={}",
                events.wall_jumps, events.dashes,
            );
        }
        for audit in &report.missing_loadouts {
            println!(
                "  missing-loadout loadout={} outcome={:?}",
                loadout_slug(audit.loadout),
                audit.outcome,
            );
        }
        for violation in &report.violations {
            println!("  violation={violation}");
        }
    }

    let failed = reports.iter().filter(|report| !report.passed()).count();
    if failed > 0 {
        return Err(format!("{failed} of {} exact ability keys failed", reports.len()).into());
    }
    Ok(())
}

fn audit_exact_candidate(
    candidate: &CompositionalAbilityCandidate,
) -> Result<ExactKeyReport, Box<dyn Error>> {
    let profile = candidate.key.rewrite_key.profile;
    let seed = candidate.key.rewrite_key.base_key.source_seed;
    let intended = profile.abilities();
    let mut violations = structural_and_raster_violations(candidate);

    let intended_matrix = evaluate_generated_door_targets_for_loadout(
        &candidate.generated,
        intended,
        &ValidationConfig::for_loadout(intended),
    )?;
    let intended_matrix_all_positive = matrix_is_all_positive(&intended_matrix);
    if !intended_matrix_all_positive {
        append_matrix_misses(&intended_matrix, &mut violations);
    }

    let intended_initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), intended, SOURCE_DOOR)?;
    let canonical_positive = intended_matrix
        .door_routes()
        .iter()
        .find(|route| route.source_door_id == SOURCE_DOOR && route.target_door_id == SINK_DOOR)
        .and_then(|route| route.evidence.positive());
    let canonical_gate_events = canonical_positive
        .map(|positive| observe_ability_events(&intended_initial, &positive.solution().replay))
        .transpose()?;

    let target = [SearchTarget::door(SINK_DOOR)];
    let intended_direct = audit_direct_controller_probes(
        &intended_initial,
        &target,
        &SolverConfig::for_abilities(intended),
    )?;
    let intended_direct_events = intended_direct
        .witnesses
        .iter()
        .map(|witness| observe_ability_events(&intended_initial, &witness.replay))
        .collect::<Result<Vec<_>, _>>()?;
    let mut missing_event_abilities = Vec::new();
    for ability in candidate
        .rewritten_mission
        .plan
        .gates
        .iter()
        .map(|gate| gate.required_ability)
    {
        let canonical_observed =
            canonical_gate_events.is_some_and(|events| events_include(events, ability));
        let direct_observed = intended_direct_events
            .iter()
            .copied()
            .any(|events| events_include(events, ability));
        if !canonical_observed && !direct_observed {
            missing_event_abilities.push(ability);
            violations.push(format!(
                "no accepted {ability:?} event was observed on an exact {SOURCE_DOOR}->{SINK_DOOR} intended-loadout replay"
            ));
        }
    }
    if !missing_event_abilities.is_empty()
        && let Some(positive) = canonical_positive
    {
        diagnose_exact_canonical_trace(
            candidate,
            &positive.solution().replay,
            &missing_event_abilities,
        )?;
    }

    let reverse_initial = Simulation::enter_via_door(
        candidate.generated.room.clone(),
        AbilitySet::NONE,
        SINK_DOOR,
    )?;
    let reverse = solve_target(
        &reverse_initial,
        SearchTarget::door(SOURCE_DOOR),
        &SolverConfig::for_abilities(AbilitySet::NONE),
    )?;
    let reverse_baseline_positive = match reverse {
        TargetSolveOutcome::Solved(solution) => {
            solution.replay.verify(&reverse_initial)?;
            true
        }
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            violations.push(format!(
                "baseline reverse {SINK_DOOR}->{SOURCE_DOOR} is inconclusive: reason={reason:?} stats={stats:?}"
            ));
            false
        }
    };

    let mut missing_loadouts = Vec::new();
    for loadout in loadouts_missing_claimed_ability(profile) {
        let initial =
            Simulation::enter_via_door(candidate.generated.room.clone(), loadout, SOURCE_DOOR)?;
        let audit = audit_direct_controller_probes(
            &initial,
            &target,
            &SolverConfig::for_abilities(loadout),
        )?;
        for witness in &audit.witnesses {
            witness.replay.verify(&initial)?;
        }
        let outcome = classify_direct_audit(audit.witnesses.len(), audit.status, audit.stats);
        if matches!(outcome, FiniteDirectOutcome::Positive { .. }) {
            violations.push(format!(
                "missing-ability loadout {} has a positive direct-controller {SOURCE_DOOR}->{SINK_DOOR} bypass",
                loadout_slug(loadout),
            ));
        }
        missing_loadouts.push(MissingLoadoutAudit { loadout, outcome });
    }

    Ok(ExactKeyReport {
        profile,
        seed,
        intended_matrix_door_routes: intended_matrix.door_routes().len(),
        intended_matrix_pickup_routes: intended_matrix.pickup_routes().len(),
        intended_matrix_all_positive,
        canonical_gate_events,
        intended_direct_events,
        reverse_baseline_positive,
        missing_loadouts,
        violations,
    })
}

fn matrix_is_all_positive(matrix: &DoorTargetEvidenceBatch) -> bool {
    matrix
        .door_routes()
        .iter()
        .all(|route| route.evidence.positive().is_some())
        && matrix
            .pickup_routes()
            .iter()
            .all(|route| route.evidence.positive().is_some())
}

fn append_matrix_misses(matrix: &DoorTargetEvidenceBatch, violations: &mut Vec<String>) {
    for route in matrix.door_routes() {
        if let BoundedTargetEvidence::Inconclusive(evidence) = &route.evidence {
            violations.push(format!(
                "intended door route {}->{} is inconclusive: reason={:?} stats={:?}",
                route.source_door_id, route.target_door_id, evidence.reason, evidence.search_effort,
            ));
        }
    }
    for route in matrix.pickup_routes() {
        if let BoundedTargetEvidence::Inconclusive(evidence) = &route.evidence {
            violations.push(format!(
                "intended pickup route {}->{} is inconclusive: reason={:?} stats={:?}",
                route.source_door_id,
                route.required_pickup_id,
                evidence.reason,
                evidence.search_effort,
            ));
        }
    }
}

fn observe_ability_events(
    initial: &Simulation,
    replay: &downwards_ai::Replay,
) -> Result<AcceptedAbilityEvents, Box<dyn Error>> {
    let observation = observe_successful_replay(initial, replay, TraversalGrid::default())?;
    Ok(AcceptedAbilityEvents {
        wall_jumps: observation.actions.successful_wall_jumps,
        dashes: observation.actions.successful_dashes,
    })
}

const fn events_include(events: AcceptedAbilityEvents, ability: GateAbility) -> bool {
    match ability {
        GateAbility::WallJump => events.wall_jumps > 0,
        GateAbility::Dash => events.dashes > 0,
    }
}

const fn classify_direct_audit(
    witness_count: usize,
    status: DirectProbeAuditStatus,
    stats: SearchStats,
) -> FiniteDirectOutcome {
    if witness_count > 0 {
        FiniteDirectOutcome::Positive {
            witness_count,
            status,
            stats,
        }
    } else {
        match status {
            DirectProbeAuditStatus::Complete => FiniteDirectOutcome::CompleteNoPositive { stats },
            DirectProbeAuditStatus::BudgetLimited(limit) => {
                FiniteDirectOutcome::Bounded { limit, stats }
            }
        }
    }
}

fn loadouts_missing_claimed_ability(profile: CompositionalAbilityGateProfile) -> Vec<AbilitySet> {
    [
        AbilitySet::NONE,
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::ALL,
    ]
    .into_iter()
    .filter(|loadout| match profile {
        CompositionalAbilityGateProfile::WallJump => !loadout.wall_jump,
        CompositionalAbilityGateProfile::Dash => !loadout.dash,
        CompositionalAbilityGateProfile::Both => false,
    })
    .collect()
}

fn structural_and_raster_violations(candidate: &CompositionalAbilityCandidate) -> Vec<String> {
    let mut violations = Vec::new();
    let mission = &candidate.rewritten_mission.plan;
    let source_port = candidate
        .boundary_ports
        .iter()
        .find(|port| port.door.id == SOURCE_DOOR);
    let sink_port = candidate
        .boundary_ports
        .iter()
        .find(|port| port.door.id == SINK_DOOR);
    let mission_route_id = |mission_node_id| {
        candidate
            .mission_route_nodes
            .iter()
            .find(|mapping| mapping.mission_node_id == mission_node_id)
            .map(|mapping| mapping.route_node_id)
    };
    if source_port.map(|port| port.node_id) != mission_route_id(mission.source_node_id()) {
        violations.push("port-0 is not the rewritten mission source".to_owned());
    }
    if sink_port.map(|port| port.node_id) != mission_route_id(mission.sink_node_id()) {
        violations.push("port-1 is not the rewritten mission sink".to_owned());
    }
    if !mission.graph_can_reach(
        mission.sink_node_id(),
        mission.source_node_id(),
        AbilitySet::NONE,
    ) {
        violations.push("graph reverse is not baseline-reachable".to_owned());
    }
    if !matches!(
        candidate.evidence_state,
        AbilityGateEmbeddingState::Pending { ref reasons }
            if reasons == &[
                AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
            ]
    ) {
        violations
            .push("candidate did not retain the exact replay-pending evidence state".to_owned());
    }

    for gate in &mission.gates {
        if mission.graph_can_reach_without_gate_edge(gate.ordinal) != Some(false) {
            violations.push(format!(
                "gate {} is not an all-path graph bridge",
                gate.ordinal
            ));
        }
        let mut missing = candidate.key.rewrite_key.profile.abilities();
        match gate.required_ability {
            GateAbility::WallJump => missing.wall_jump = false,
            GateAbility::Dash => missing.dash = false,
        }
        if mission.graph_can_reach(mission.source_node_id(), mission.sink_node_id(), missing) {
            violations.push(format!(
                "gate {} graph reaches sink without {:?}",
                gate.ordinal, gate.required_ability,
            ));
        }
        let Some(realization) = candidate
            .embedding
            .gate_realizations
            .iter()
            .find(|realization| realization.gate.ordinal == gate.ordinal)
        else {
            violations.push(format!("gate {} has no physical realization", gate.ordinal));
            continue;
        };
        let expected_verb = match gate.required_ability {
            GateAbility::WallJump => RouteVerb::WallClimb,
            GateAbility::Dash => RouteVerb::DashUp,
        };
        let route_edge = candidate.route_plan.edges.iter().find(|edge| {
            edge.from == realization.from_route_node_id
                && edge.to == realization.to_route_node_id
                && edge.critical
        });
        if route_edge.map(|edge| edge.verb) != Some(expected_verb) {
            violations.push(format!(
                "gate {} physical route edge does not retain {:?}",
                gate.ordinal, expected_verb,
            ));
        }
        let directed = candidate
            .rewritten_mission
            .plan
            .edges
            .get(usize::from(gate.mission_edge_index));
        if directed.map(|edge| edge.forward_requirement)
            != Some(DirectedTraversalRequirement::Ability(gate.required_ability))
        {
            violations.push(format!("gate {} lost directed provenance", gate.ordinal));
        }
        for cell in &realization.required_solid_tiles {
            if candidate.generated.room.tile(cell.x, cell.row) != Some(Tile::Solid) {
                violations.push(format!(
                    "gate {} solid reservation ({},{}) mutated",
                    gate.ordinal, cell.x, cell.row,
                ));
            }
        }
        for cell in &realization.required_empty_tiles {
            if candidate.generated.room.tile(cell.x, cell.row) != Some(Tile::Empty) {
                violations.push(format!(
                    "gate {} empty reservation ({},{}) mutated",
                    gate.ordinal, cell.x, cell.row,
                ));
            }
        }
    }

    for cut in &candidate.embedding.cut_realizations {
        for x in cut.shelf_start_x..cut.shelf_end_x {
            if candidate.generated.room.tile(x, cut.row) != Some(Tile::Solid) {
                violations.push(format!(
                    "cut {} shelf tile ({},{}) mutated",
                    cut.order, x, cut.row,
                ));
            }
        }
        for gate in &candidate.embedding.gate_realizations {
            if gate
                .required_solid_tiles
                .iter()
                .chain(&gate.required_empty_tiles)
                .any(|cell| {
                    cell.row == cut.row && (cut.shelf_start_x..cut.shelf_end_x).contains(&cell.x)
                })
            {
                violations.push(format!(
                    "gate {} reservation intersects cut {} shelf",
                    gate.gate.ordinal, cut.order,
                ));
            }
        }
    }

    for port in &candidate.boundary_ports {
        let socket = port.door.socket();
        if !compositional_route_cut_socket_in_inventory(socket)
            || !compositional_route_cut_socket_in_inventory(socket.mate())
        {
            violations.push(format!("door {} left mate-closed inventory", port.door.id));
        }
        let arrival = Rect::new(
            port.door.arrival.x,
            port.door.arrival.y,
            PLAYER_WIDTH,
            PLAYER_HEIGHT,
        );
        if !room_rect_is_clear(&candidate.generated.room, arrival) {
            violations.push(format!("door {} arrival is blocked", port.door.id));
        }
        for gate in &candidate.embedding.gate_realizations {
            if gate.ascent_bounds.intersects(arrival) {
                violations.push(format!(
                    "gate {} ascent reservation intersects door {} arrival",
                    gate.gate.ordinal, port.door.id,
                ));
            }
            if gate.required_solid_tiles.iter().any(|cell| {
                tile_rect(cell.x, cell.row).intersects(port.door.trigger_bounds)
                    || tile_rect(cell.x, cell.row).intersects(arrival)
            }) {
                violations.push(format!(
                    "gate {} solid reservation mutates door {} trigger/arrival",
                    gate.gate.ordinal, port.door.id,
                ));
            }
        }
    }
    violations
}

fn diagnose_exact_canonical_trace(
    candidate: &CompositionalAbilityCandidate,
    canonical: &Replay,
    missing_event_abilities: &[GateAbility],
) -> Result<(), Box<dyn Error>> {
    println!(
        "exact-canonical-diagnostic profile={} seed={} missing-events={missing_event_abilities:?} action-ticks={}",
        candidate.key.rewrite_key.profile.slug(),
        candidate.key.rewrite_key.base_key.source_seed,
        canonical.frames.len(),
    );
    for (spine_index, &mission_node_id) in candidate
        .rewritten_mission
        .plan
        .base
        .spine
        .iter()
        .enumerate()
    {
        let route_node_id = candidate
            .mission_route_nodes
            .iter()
            .find(|mapping| mapping.mission_node_id == mission_node_id)
            .map(|mapping| mapping.route_node_id)
            .ok_or("spine node has no route mapping")?;
        let support = candidate.route_plan.nodes[usize::from(route_node_id)].support;
        println!(
            "  spine index={spine_index} mission-node={mission_node_id} route-node={route_node_id} support=({}, {}, row={}, kind={:?})",
            support.start_x, support.end_x, support.row, support.kind,
        );
    }
    for realization in &candidate.embedding.gate_realizations {
        println!(
            "  gate ordinal={} ability={:?} spine-edge={} mission-edge={} route={}->{} lower=({}, {}, row={}) upper=({}, {}, row={}) ascent=({}, {}, {}, {}) solids={:?} empty-count={}",
            realization.gate.ordinal,
            realization.gate.required_ability,
            realization.gate.spine_edge_index,
            realization.gate.mission_edge_index,
            realization.from_route_node_id,
            realization.to_route_node_id,
            realization.lower_support.start_x,
            realization.lower_support.end_x,
            realization.lower_support.row,
            realization.upper_support.start_x,
            realization.upper_support.end_x,
            realization.upper_support.row,
            realization.ascent_bounds.x,
            realization.ascent_bounds.y,
            realization.ascent_bounds.width,
            realization.ascent_bounds.height,
            realization.required_solid_tiles,
            realization.required_empty_tiles.len(),
        );
    }
    for cut in &candidate.embedding.cut_realizations {
        println!(
            "  cut order={} row={} shelf={}..{} opening={}..{}",
            cut.order,
            cut.row,
            cut.shelf_start_x,
            cut.shelf_end_x,
            cut.opening_start_x,
            cut.opening_end_x,
        );
    }

    let intended = candidate.key.rewrite_key.profile.abilities();
    let intended_initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), intended, SOURCE_DOOR)?;
    let actions = canonical.actions().collect::<Vec<_>>();
    let intended_trace = spatial_trace(candidate, intended_initial, &actions);
    println!("  canonical-intended-trace={intended_trace:#?}");

    let dash_only = AbilitySet::new(false, true);
    let dash_initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), dash_only, SOURCE_DOOR)?;
    let dash_replay = Replay::record(&dash_initial, actions.clone());
    let dash_verification = dash_replay.verify(&dash_initial)?;
    let dash_trace = spatial_trace(candidate, dash_initial, &actions);
    println!(
        "  canonical-actions-dash-only verification-reached={:?} trace={dash_trace:#?}",
        dash_verification.reached_exit,
    );
    if dash_verification.reached_exit.as_deref() == Some(SINK_DOOR) {
        let wall_gate_ordinals = candidate
            .embedding
            .gate_realizations
            .iter()
            .filter(|gate| gate.gate.required_ability == GateAbility::WallJump)
            .map(|gate| gate.gate.ordinal)
            .collect::<Vec<_>>();
        let dash_inside_wall_gate = dash_trace
            .gate_dash_events
            .iter()
            .any(|(ordinal, count)| wall_gate_ordinals.contains(ordinal) && *count > 0);
        println!(
            "  localized-cause=dash-only-canonical-trace-reaches-sink dash-inside-wall-gate={dash_inside_wall_gate} wall-gates={wall_gate_ordinals:?}"
        );
    }
    Ok(())
}

fn spatial_trace(
    candidate: &CompositionalAbilityCandidate,
    mut simulation: Simulation,
    actions: &[downwards_core::Action],
) -> SpatialTraceSummary {
    let loadout = simulation.abilities();
    let mut support_sequence = Vec::new();
    push_distinct_support_contact(
        &mut support_sequence,
        route_support_contacts(candidate, simulation.player().bounds()),
    );
    let mut accepted_events = Vec::new();
    let mut gate_dash_events = candidate
        .embedding
        .gate_realizations
        .iter()
        .map(|gate| (gate.gate.ordinal, 0_usize))
        .collect::<Vec<_>>();
    let mut gate_wall_jump_events = gate_dash_events.clone();
    let mut gate_vertical_coverage = candidate
        .embedding
        .gate_realizations
        .iter()
        .map(|gate| (gate.gate.ordinal, i32::MAX, i32::MIN))
        .collect::<Vec<_>>();
    let mut cut_crossings = Vec::new();
    let initial = simulation.player().bounds();
    let mut previous_center = (
        initial.x + initial.width / 2,
        initial.y + initial.height / 2,
    );

    for (index, &action) in actions.iter().enumerate() {
        let report = simulation.step(action);
        let bounds = simulation.player().bounds();
        let center = (bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
        for (gate_index, gate) in candidate.embedding.gate_realizations.iter().enumerate() {
            let horizontal =
                bounds.x < gate.ascent_bounds.right() && bounds.right() > gate.ascent_bounds.x;
            if horizontal {
                gate_vertical_coverage[gate_index].1 =
                    gate_vertical_coverage[gate_index].1.min(bounds.bottom());
                gate_vertical_coverage[gate_index].2 =
                    gate_vertical_coverage[gate_index].2.max(bounds.bottom());
            }
        }
        for cut in &candidate.embedding.cut_realizations {
            let cut_y = i32::from(cut.row) * TILE_SIZE;
            if (previous_center.1 < cut_y && center.1 >= cut_y)
                || (previous_center.1 >= cut_y && center.1 < cut_y)
            {
                let tile_x = center.0.div_euclid(TILE_SIZE);
                let through_opening = tile_x >= i32::from(cut.opening_start_x)
                    && tile_x < i32::from(cut.opening_end_x);
                cut_crossings.push(format!(
                    "tick={} cut={} direction={} tile-x={} through-opening={}",
                    index + 1,
                    cut.order,
                    if center.1 < previous_center.1 {
                        "up"
                    } else {
                        "down"
                    },
                    tile_x,
                    through_opening,
                ));
            }
        }
        for event in &report.events {
            match event {
                SimulationEvent::Landed => {
                    let contacts = route_support_contacts(candidate, bounds);
                    push_distinct_support_contact(&mut support_sequence, contacts.clone());
                    accepted_events.push(format!(
                        "tick={} event=Landed bounds={bounds:?} route-supports={contacts:?}",
                        index + 1,
                    ));
                }
                SimulationEvent::Dashed { direction } => {
                    let gates = event_gate_ordinals(candidate, bounds);
                    for ordinal in &gates {
                        if let Some((_, count)) = gate_dash_events
                            .iter_mut()
                            .find(|(gate_ordinal, _)| gate_ordinal == ordinal)
                        {
                            *count += 1;
                        }
                    }
                    accepted_events.push(format!(
                        "tick={} event=Dash({direction:?}) bounds={bounds:?} gates={gates:?}",
                        index + 1,
                    ));
                }
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    let gates = event_gate_ordinals(candidate, bounds);
                    for ordinal in &gates {
                        if let Some((_, count)) = gate_wall_jump_events
                            .iter_mut()
                            .find(|(gate_ordinal, _)| gate_ordinal == ordinal)
                        {
                            *count += 1;
                        }
                    }
                    accepted_events.push(format!(
                        "tick={} event=WallJump({side:?}) bounds={bounds:?} gates={gates:?}",
                        index + 1,
                    ));
                }
                SimulationEvent::Jumped(kind) => accepted_events.push(format!(
                    "tick={} event=Jump({kind:?}) bounds={bounds:?}",
                    index + 1,
                )),
                SimulationEvent::PickupCollected { id } => accepted_events.push(format!(
                    "tick={} event=Pickup({id}) bounds={bounds:?}",
                    index + 1,
                )),
                SimulationEvent::ExitReached { id } => accepted_events.push(format!(
                    "tick={} event=Exit({id}) bounds={bounds:?}",
                    index + 1,
                )),
                SimulationEvent::Died(reason) => accepted_events.push(format!(
                    "tick={} event=Died({reason:?}) bounds={bounds:?}",
                    index + 1,
                )),
                SimulationEvent::Reset => accepted_events
                    .push(format!("tick={} event=Reset bounds={bounds:?}", index + 1,)),
            }
        }
        previous_center = center;
    }
    gate_vertical_coverage.retain(|(_, minimum, _)| *minimum != i32::MAX);
    SpatialTraceSummary {
        loadout,
        reached_exit: simulation.reached_exit().map(str::to_owned),
        collected_pickups: simulation
            .collected_pickups()
            .map(|pickup| pickup.id().to_owned())
            .collect(),
        support_sequence,
        accepted_events,
        gate_dash_events,
        gate_wall_jump_events,
        gate_vertical_coverage,
        cut_crossings,
    }
}

fn route_support_contacts(candidate: &CompositionalAbilityCandidate, player: Rect) -> Vec<u16> {
    candidate
        .route_plan
        .nodes
        .iter()
        .filter(|node| {
            let support_y = i32::from(node.support.row) * TILE_SIZE;
            player.bottom().abs_diff(support_y) <= 1
                && player.x < i32::from(node.support.end_x) * TILE_SIZE
                && player.right() > i32::from(node.support.start_x) * TILE_SIZE
        })
        .map(|node| node.id)
        .collect()
}

fn push_distinct_support_contact(sequence: &mut Vec<Vec<u16>>, contacts: Vec<u16>) {
    if !contacts.is_empty() && sequence.last() != Some(&contacts) {
        sequence.push(contacts);
    }
}

fn event_gate_ordinals(candidate: &CompositionalAbilityCandidate, player: Rect) -> Vec<u16> {
    candidate
        .embedding
        .gate_realizations
        .iter()
        .filter(|gate| player.intersects(gate.ascent_bounds))
        .map(|gate| gate.gate.ordinal)
        .collect()
}

fn room_rect_is_clear(room: &downwards_core::Room, rect: Rect) -> bool {
    room.tiles().iter().enumerate().all(|(index, tile)| {
        if *tile != Tile::Solid && !tile.is_hazard() {
            return true;
        }
        let width = usize::from(room.width());
        let x = u16::try_from(index % width).expect("room width fits u16");
        let y = u16::try_from(index / width).expect("room height fits u16");
        !rect.intersects(room.tile_bounds(x, y))
    })
}

fn tile_rect(x: u16, row: u16) -> Rect {
    Rect::new(
        i32::from(x) * TILE_SIZE,
        i32::from(row) * TILE_SIZE,
        TILE_SIZE,
        TILE_SIZE,
    )
}

const fn loadout_slug(loadout: AbilitySet) -> &'static str {
    match (loadout.wall_jump, loadout.dash) {
        (false, false) => "baseline",
        (true, false) => "wall-jump",
        (false, true) => "dash",
        (true, true) => "both",
    }
}
