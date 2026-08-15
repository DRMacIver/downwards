//! Bounded correctness audit for the experimental physical-v3 wall gate.
//!
//! The exact key block is fixed to attempt-zero seeds 0..=4 crossed with all
//! challenge intents.  This is deliberately not a corpus enumerator and does
//! not retry a failed construction with another embedding or rewrite attempt.

use std::{collections::BTreeSet, error::Error, fmt};

use downwards_ai::{
    DIRECT_PROBE_AUDIT_VERSION, DirectProbeAuditStatus, DirectProbeBudgetLimit, Replay,
    SearchStats, SearchTarget, SolverConfig, audit_direct_controller_probes,
};
use downwards_core::{
    AbilitySet, JumpKind, PLAYER_HEIGHT, PLAYER_WIDTH, Rect, Simulation, SimulationEvent, Tile,
};
use downwards_gen::{
    TILE_SIZE,
    experimental::{
        AbilityGateEmbeddingPendingReason, AbilityGateEmbeddingState,
        COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
        COMPOSITIONAL_ABILITY_GENERATION_VERSION, ChallengeIntent, CompositionalAbilityCandidate,
        CompositionalAbilityGateProfile, CompositionalAbilityGenerationFailure,
        CompositionalAbilityGenerationKey, DirectedTraversalRequirement, GateAbility, RouteVerb,
        compositional_route_cut_socket_in_inventory,
    },
};
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};

const SOURCE_DOOR: &str = "port-0";
const SINK_DOOR: &str = "port-1";
const FIRST_SEED: u64 = 0;
const LAST_SEED: u64 = 4;
const PROFILES: [CompositionalAbilityGateProfile; 1] = [CompositionalAbilityGateProfile::WallJump];
const INTENDED_WALL_LOADOUTS: [AbilitySet; 2] = [AbilitySet::new(true, false), AbilitySet::ALL];
const MISSING_WALL_LOADOUTS: [AbilitySet; 2] = [AbilitySet::NONE, AbilitySet::new(false, true)];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExactKey {
    profile: CompositionalAbilityGateProfile,
    intent: ChallengeIntent,
    seed: u64,
}

impl ExactKey {
    fn generation_key(self) -> CompositionalAbilityGenerationKey {
        CompositionalAbilityGenerationKey::new(self.seed, self.profile, self.intent)
    }
}

impl fmt::Display for ExactKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:e00:r000:{:016x}",
            self.profile.slug(),
            self.intent.slug(),
            self.seed,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AcceptedAbilityEvents {
    wall_jumps: usize,
    wall_jumps_in_wall_gate: usize,
    dashes: usize,
    dashes_in_wall_gate: usize,
    dashes_in_dash_gate: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FiniteDirectOutcome {
    Positive {
        witness_count: usize,
        witnesses_with_all_gate_events: usize,
        status: DirectProbeAuditStatus,
        stats: SearchStats,
    },
    CompleteNoPositive {
        stats: SearchStats,
    },
    BoundedNoPositive {
        limit: DirectProbeBudgetLimit,
        stats: SearchStats,
    },
}

#[derive(Clone, Debug)]
struct MatrixReport {
    loadout: AbilitySet,
    door_positive: usize,
    door_total: usize,
    pickup_positive: usize,
    pickup_total: usize,
    forward_positive: bool,
    reverse_positive: bool,
    aggregate_effort: SearchStats,
}

#[derive(Clone, Debug)]
struct MissingWallReport {
    matrix: MatrixReport,
    direct: FiniteDirectOutcome,
}

#[derive(Clone, Debug)]
struct IntendedWallReport {
    matrix: MatrixReport,
    advertised_events: Option<AcceptedAbilityEvents>,
    direct: FiniteDirectOutcome,
    exact_action_bypasses: Vec<AbilitySet>,
}

#[derive(Clone, Debug)]
struct CandidateReport {
    key: ExactKey,
    intended_wall: Vec<IntendedWallReport>,
    baseline_reverse_positive: bool,
    missing_wall: Vec<MissingWallReport>,
    violations: Vec<String>,
}

impl CandidateReport {
    fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let construction_only = parse_args()?;
    println!(
        "compositional-wall-gate-v3-audit physical-generation-version={} gate-contract-version={} direct-probe-version={} seeds={}..={} intents={} profiles={} construction-only={construction_only}",
        COMPOSITIONAL_ABILITY_GENERATION_VERSION,
        COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
        DIRECT_PROBE_AUDIT_VERSION,
        FIRST_SEED,
        LAST_SEED,
        ChallengeIntent::ALL.len(),
        PROFILES.len(),
    );

    let mut constructed = Vec::new();
    let mut construction_failures = Vec::new();
    let mut correctness_failures = Vec::new();
    let mut geometry_signatures = BTreeSet::new();

    for profile in PROFILES {
        for intent in ChallengeIntent::ALL {
            for seed in FIRST_SEED..=LAST_SEED {
                let exact = ExactKey {
                    profile,
                    intent,
                    seed,
                };
                match exact.generation_key().generate() {
                    Ok(candidate) => {
                        let regenerated = candidate.key.generate()?;
                        let mut violations = structural_and_raster_violations(&candidate);
                        if regenerated != candidate {
                            violations.push("exact regeneration changed the candidate".to_owned());
                        }
                        let signature = geometry_signature(&candidate);
                        geometry_signatures.insert(signature.clone());
                        println!(
                            "construction key={exact} outcome=positive gates={} cuts={} sockets={:?} geometry={signature}",
                            candidate.embedding.gate_realizations.len(),
                            candidate.embedding.cut_realizations.len(),
                            candidate.embedding.socket_columns,
                        );
                        for violation in &violations {
                            println!("  construction-violation={violation}");
                            correctness_failures.push(format!("{exact}: {violation}"));
                        }
                        if violations.is_empty() {
                            constructed.push((exact, candidate));
                        }
                    }
                    Err(error) => {
                        let cause = construction_failure_slug(&error.cause);
                        println!(
                            "construction key={exact} outcome=refused cause={cause} detail={error}"
                        );
                        construction_failures.push((exact, cause));
                    }
                }
            }
        }
    }

    print_construction_summary(
        &constructed,
        &construction_failures,
        geometry_signatures.len(),
    );
    for profile in PROFILES {
        if !constructed.iter().any(|(key, _)| key.profile == profile) {
            correctness_failures.push(format!(
                "the bounded block constructed no {} candidate",
                profile.slug()
            ));
        }
    }
    if !correctness_failures.is_empty() {
        return Err(format!(
            "construction correctness failed: {}",
            correctness_failures.join("; ")
        )
        .into());
    }
    if construction_only {
        return Ok(());
    }

    let mut reports = Vec::with_capacity(constructed.len());
    for (key, candidate) in &constructed {
        println!("authoritative-audit key={key} phase=start");
        let report = audit_candidate(*key, candidate)?;
        print_candidate_report(&report);
        if !report.passed() {
            return Err(format!("exact authoritative audit failed for {key}").into());
        }
        reports.push(report);
    }

    let failed = reports.iter().filter(|report| !report.passed()).count();
    println!(
        "authoritative-summary audited={} passed={} failed={} geometry-diversity={}",
        reports.len(),
        reports.len() - failed,
        failed,
        geometry_signatures.len(),
    );
    if failed > 0 {
        return Err(format!(
            "{failed} of {} constructed exact keys failed",
            reports.len()
        )
        .into());
    }
    Ok(())
}

fn parse_args() -> Result<bool, Box<dyn Error>> {
    let mut construction_only = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--construction-only" => construction_only = true,
            _ => return Err(format!("unknown argument {argument:?}").into()),
        }
    }
    Ok(construction_only)
}

fn construction_failure_slug(cause: &CompositionalAbilityGenerationFailure) -> String {
    match cause {
        CompositionalAbilityGenerationFailure::Mission(_) => "mission".to_owned(),
        CompositionalAbilityGenerationFailure::Rewrite(_) => "rewrite".to_owned(),
        CompositionalAbilityGenerationFailure::BaselineEmbedding(_) => {
            "baseline-embedding".to_owned()
        }
        CompositionalAbilityGenerationFailure::ConstraintSearchExhausted { phase, explored } => {
            format!("{}-search-exhausted-{explored}", phase.slug())
        }
        CompositionalAbilityGenerationFailure::GateContract {
            gate_ordinal,
            violation,
        } => format!("gate-{gate_ordinal}-{violation}"),
    }
}

fn print_construction_summary(
    constructed: &[(ExactKey, CompositionalAbilityCandidate)],
    failures: &[(ExactKey, String)],
    geometry_diversity: usize,
) {
    for profile in PROFILES {
        for intent in ChallengeIntent::ALL {
            let successes = constructed
                .iter()
                .filter(|(key, _)| key.profile == profile && key.intent == intent)
                .count();
            let refusals = failures
                .iter()
                .filter(|(key, _)| key.profile == profile && key.intent == intent)
                .count();
            println!(
                "construction-summary profile={} intent={} positive={successes}/{} refused={refusals}",
                profile.slug(),
                intent.slug(),
                LAST_SEED - FIRST_SEED + 1,
            );
        }
    }
    let mut causes = failures
        .iter()
        .map(|(_, cause)| cause.clone())
        .collect::<BTreeSet<_>>();
    if causes.is_empty() {
        causes.insert("none".to_owned());
    }
    println!(
        "construction-summary total-positive={} total-refused={} geometry-diversity={} refusal-causes={causes:?}",
        constructed.len(),
        failures.len(),
        geometry_diversity,
    );
}

fn geometry_signature(candidate: &CompositionalAbilityCandidate) -> String {
    let gates = candidate
        .embedding
        .gate_realizations
        .iter()
        .map(|gate| {
            format!(
                "{}:spine{}:rise{}:empty{}:rows{}->{}:bounds({},{},{},{}):supports{}..{}:{}..{}",
                gate.gate.required_ability.slug(),
                gate.gate.spine_edge_index,
                gate.lower_support.row - gate.upper_support.row,
                gate.required_empty_tiles.len(),
                gate.lower_support.row,
                gate.upper_support.row,
                gate.ascent_bounds.x,
                gate.ascent_bounds.y,
                gate.ascent_bounds.width,
                gate.ascent_bounds.height,
                gate.lower_support.start_x,
                gate.lower_support.end_x,
                gate.upper_support.start_x,
                gate.upper_support.end_x,
            )
        })
        .collect::<Vec<_>>()
        .join("+");
    let cuts = candidate
        .embedding
        .cut_realizations
        .iter()
        .map(|cut| format!("{}:{}..{}", cut.row, cut.opening_start_x, cut.opening_end_x))
        .collect::<Vec<_>>()
        .join("+");
    format!(
        "gates=[{gates}];cuts=[{cuts}];sockets={:?}",
        candidate.embedding.socket_columns
    )
}

fn audit_candidate(
    key: ExactKey,
    candidate: &CompositionalAbilityCandidate,
) -> Result<CandidateReport, Box<dyn Error>> {
    let mut violations = structural_and_raster_violations(candidate);
    let required_events = candidate
        .rewritten_mission
        .plan
        .gates
        .iter()
        .map(|gate| gate.required_ability)
        .collect::<Vec<_>>();
    let mut intended_wall = Vec::new();
    for loadout in INTENDED_WALL_LOADOUTS {
        let matrix = evaluate_generated_door_targets_for_loadout(
            &candidate.generated,
            loadout,
            &ValidationConfig::for_loadout(loadout),
        )?;
        let matrix_report = summarize_matrix(&matrix);
        append_intended_matrix_misses(&matrix, &mut violations);
        let intended_initial =
            Simulation::enter_via_door(candidate.generated.room.clone(), loadout, SOURCE_DOOR)?;
        let advertised_positive =
            exact_door_row(&matrix, SOURCE_DOOR, SINK_DOOR).and_then(|row| row.evidence.positive());
        let advertised_events = advertised_positive
            .map(|positive| {
                observe_gate_events(candidate, &intended_initial, &positive.solution().replay)
            })
            .transpose()?;
        match advertised_events {
            Some(events) => append_missing_gate_events(
                candidate,
                events,
                &format!("{} advertised matrix", loadout_slug(loadout)),
                &mut violations,
            ),
            None => violations.push(format!(
                "{} advertised pair {SOURCE_DOOR}->{SINK_DOOR} is not replay-positive",
                loadout_slug(loadout),
            )),
        }

        let mut exact_action_bypasses = Vec::new();
        if let Some(positive) = advertised_positive {
            for missing in MISSING_WALL_LOADOUTS {
                if exact_actions_reach_sink(candidate, &positive.solution().replay, missing)? {
                    exact_action_bypasses.push(missing);
                    violations.push(format!(
                        "{} advertised replay actions reach the sink under missing-Wall loadout {}",
                        loadout_slug(loadout),
                        loadout_slug(missing),
                    ));
                }
            }
        }

        let direct = audit_direct(candidate, loadout, &required_events)?;
        if let FiniteDirectOutcome::Positive {
            witness_count,
            witnesses_with_all_gate_events,
            ..
        } = direct
            && witnesses_with_all_gate_events != witness_count
        {
            violations.push(format!(
                "{} direct audit has {} of {witness_count} positive witnesses missing one or more accepted gate events",
                loadout_slug(loadout),
                witness_count - witnesses_with_all_gate_events,
            ));
        }
        intended_wall.push(IntendedWallReport {
            matrix: matrix_report,
            advertised_events,
            direct,
            exact_action_bypasses,
        });
    }

    let mut missing_wall = Vec::new();
    let mut baseline_reverse_positive = false;
    for loadout in MISSING_WALL_LOADOUTS {
        let matrix = evaluate_generated_door_targets_for_loadout(
            &candidate.generated,
            loadout,
            &ValidationConfig::for_loadout(loadout),
        )?;
        let matrix_report = summarize_matrix(&matrix);
        if matrix_report.forward_positive {
            let positive = exact_door_row(&matrix, SOURCE_DOOR, SINK_DOOR)
                .and_then(|row| row.evidence.positive())
                .expect("positive summary has exact positive evidence");
            let initial =
                Simulation::enter_via_door(candidate.generated.room.clone(), loadout, SOURCE_DOOR)?;
            let events = observe_gate_events(candidate, &initial, &positive.solution().replay)?;
            let trace =
                trace_missing_wall_bypass(candidate, &initial, &positive.solution().replay)?;
            violations.push(format!(
                "missing-Wall loadout {} has replay-positive advertised-pair matrix bypass with events {events:?}; {trace}",
                loadout_slug(loadout),
            ));
        }
        if loadout == AbilitySet::NONE {
            baseline_reverse_positive = matrix_report.reverse_positive;
            if !baseline_reverse_positive {
                violations.push(format!(
                    "baseline reverse {SINK_DOOR}->{SOURCE_DOOR} is not matrix-positive"
                ));
            } else {
                let positive = exact_door_row(&matrix, SINK_DOOR, SOURCE_DOOR)
                    .and_then(|row| row.evidence.positive())
                    .expect("positive reverse summary has exact positive evidence");
                let initial = Simulation::enter_via_door(
                    candidate.generated.room.clone(),
                    loadout,
                    SINK_DOOR,
                )?;
                positive.solution().replay.verify(&initial)?;
            }
        }
        let direct = audit_direct(candidate, loadout, &[])?;
        if matches!(direct, FiniteDirectOutcome::Positive { .. }) {
            violations.push(format!(
                "missing-Wall loadout {} has a positive finite direct-controller advertised-pair bypass",
                loadout_slug(loadout),
            ));
        }
        missing_wall.push(MissingWallReport {
            matrix: matrix_report,
            direct,
        });
    }

    Ok(CandidateReport {
        key,
        intended_wall,
        baseline_reverse_positive,
        missing_wall,
        violations,
    })
}

fn summarize_matrix(matrix: &DoorTargetEvidenceBatch) -> MatrixReport {
    MatrixReport {
        loadout: matrix.loadout(),
        door_positive: matrix
            .door_routes()
            .iter()
            .filter(|route| route.evidence.positive().is_some())
            .count(),
        door_total: matrix.door_routes().len(),
        pickup_positive: matrix
            .pickup_routes()
            .iter()
            .filter(|route| route.evidence.positive().is_some())
            .count(),
        pickup_total: matrix.pickup_routes().len(),
        forward_positive: exact_door_row(matrix, SOURCE_DOOR, SINK_DOOR)
            .is_some_and(|route| route.evidence.positive().is_some()),
        reverse_positive: exact_door_row(matrix, SINK_DOOR, SOURCE_DOOR)
            .is_some_and(|route| route.evidence.positive().is_some()),
        aggregate_effort: matrix.aggregate_search_effort(),
    }
}

fn exact_door_row<'a>(
    matrix: &'a DoorTargetEvidenceBatch,
    source: &str,
    target: &str,
) -> Option<&'a downwards_validation::DoorRouteEvidence> {
    matrix
        .door_routes()
        .iter()
        .find(|route| route.source_door_id == source && route.target_door_id == target)
}

fn append_intended_matrix_misses(matrix: &DoorTargetEvidenceBatch, violations: &mut Vec<String>) {
    for route in matrix.door_routes() {
        if let BoundedTargetEvidence::Inconclusive(evidence) = route.evidence {
            violations.push(format!(
                "intended door route {}->{} is bounded: reason={:?} stats={:?}",
                route.source_door_id, route.target_door_id, evidence.reason, evidence.search_effort,
            ));
        }
    }
    for route in matrix.pickup_routes() {
        if let BoundedTargetEvidence::Inconclusive(evidence) = route.evidence {
            violations.push(format!(
                "intended pickup route {}->{} is bounded: reason={:?} stats={:?}",
                route.source_door_id,
                route.required_pickup_id,
                evidence.reason,
                evidence.search_effort,
            ));
        }
    }
}

fn audit_direct(
    candidate: &CompositionalAbilityCandidate,
    loadout: AbilitySet,
    required_events: &[GateAbility],
) -> Result<FiniteDirectOutcome, Box<dyn Error>> {
    let initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), loadout, SOURCE_DOOR)?;
    let audit = audit_direct_controller_probes(
        &initial,
        &[SearchTarget::door(SINK_DOOR)],
        &SolverConfig::for_abilities(loadout),
    )?;
    let mut witnesses_with_all_gate_events = 0;
    for witness in &audit.witnesses {
        witness.replay.verify(&initial)?;
        let events = observe_gate_events(candidate, &initial, &witness.replay)?;
        if required_events
            .iter()
            .all(|ability| events_include(events, *ability))
        {
            witnesses_with_all_gate_events += 1;
        }
    }
    Ok(if audit.witnesses.is_empty() {
        match audit.status {
            DirectProbeAuditStatus::Complete => {
                FiniteDirectOutcome::CompleteNoPositive { stats: audit.stats }
            }
            DirectProbeAuditStatus::BudgetLimited(limit) => {
                FiniteDirectOutcome::BoundedNoPositive {
                    limit,
                    stats: audit.stats,
                }
            }
        }
    } else {
        FiniteDirectOutcome::Positive {
            witness_count: audit.witnesses.len(),
            witnesses_with_all_gate_events,
            status: audit.status,
            stats: audit.stats,
        }
    })
}

fn observe_gate_events(
    candidate: &CompositionalAbilityCandidate,
    initial: &Simulation,
    replay: &Replay,
) -> Result<AcceptedAbilityEvents, Box<dyn Error>> {
    replay.verify(initial)?;
    let mut simulation = initial.clone();
    let mut accepted = AcceptedAbilityEvents::default();
    for action in replay.actions() {
        let report = simulation.step(action);
        let player = simulation.player().bounds();
        for event in report.events {
            match event {
                SimulationEvent::Jumped(JumpKind::Wall { .. }) => {
                    accepted.wall_jumps += 1;
                    if candidate.embedding.gate_realizations.iter().any(|gate| {
                        gate.gate.required_ability == GateAbility::WallJump
                            && player.intersects(gate.ascent_bounds)
                    }) {
                        accepted.wall_jumps_in_wall_gate += 1;
                    }
                }
                SimulationEvent::Dashed { .. } => {
                    accepted.dashes += 1;
                    if candidate.embedding.gate_realizations.iter().any(|gate| {
                        gate.gate.required_ability == GateAbility::WallJump
                            && player.intersects(gate.ascent_bounds)
                    }) {
                        accepted.dashes_in_wall_gate += 1;
                    }
                    if candidate.embedding.gate_realizations.iter().any(|gate| {
                        gate.gate.required_ability == GateAbility::Dash
                            && player.intersects(gate.ascent_bounds)
                    }) {
                        accepted.dashes_in_dash_gate += 1;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(accepted)
}

fn exact_actions_reach_sink(
    candidate: &CompositionalAbilityCandidate,
    replay: &Replay,
    loadout: AbilitySet,
) -> Result<bool, Box<dyn Error>> {
    let initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), loadout, SOURCE_DOOR)?;
    let rerecorded = Replay::record(&initial, replay.actions().collect::<Vec<_>>());
    let verification = rerecorded.verify(&initial)?;
    Ok(verification.reached_exit.as_deref() == Some(SINK_DOOR))
}

fn trace_missing_wall_bypass(
    candidate: &CompositionalAbilityCandidate,
    initial: &Simulation,
    replay: &Replay,
) -> Result<String, Box<dyn Error>> {
    replay.verify(initial)?;
    let mut simulation = initial.clone();
    let mut dash_events = Vec::new();
    let mut support_sequence = Vec::<Vec<u16>>::new();
    let mut minimum_bottom = i32::MAX;
    let mut maximum_bottom = i32::MIN;
    for (tick, action) in replay.actions().enumerate() {
        let report = simulation.step(action);
        let player = simulation.player().bounds();
        let inside_wall_gate = candidate.embedding.gate_realizations.iter().any(|gate| {
            gate.gate.required_ability == GateAbility::WallJump
                && player.x < gate.ascent_bounds.right()
                && player.right() > gate.ascent_bounds.x
        });
        if inside_wall_gate {
            minimum_bottom = minimum_bottom.min(player.bottom());
            maximum_bottom = maximum_bottom.max(player.bottom());
        }
        for event in report.events {
            match event {
                SimulationEvent::Dashed { direction } => dash_events.push(format!(
                    "tick{}:{direction:?}@({},{},{},{}) wall-band={inside_wall_gate}",
                    tick + 1,
                    player.x,
                    player.y,
                    player.width,
                    player.height,
                )),
                SimulationEvent::Landed => {
                    let contacts = candidate
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
                        .collect::<Vec<_>>();
                    if !contacts.is_empty() && support_sequence.last() != Some(&contacts) {
                        support_sequence.push(contacts);
                    }
                }
                _ => {}
            }
        }
    }
    let coverage = (minimum_bottom != i32::MAX).then_some((minimum_bottom, maximum_bottom));
    Ok(format!(
        "trace support-sequence={support_sequence:?} wall-band-bottom-coverage={coverage:?} dash-events={dash_events:?}"
    ))
}

fn append_missing_gate_events(
    candidate: &CompositionalAbilityCandidate,
    events: AcceptedAbilityEvents,
    evidence: &str,
    violations: &mut Vec<String>,
) {
    for ability in candidate
        .rewritten_mission
        .plan
        .gates
        .iter()
        .map(|gate| gate.required_ability)
    {
        if !events_include(events, ability) {
            violations.push(format!(
                "{evidence} replay has no accepted gate-local {ability:?} event: {events:?}"
            ));
        }
    }
}

const fn events_include(events: AcceptedAbilityEvents, ability: GateAbility) -> bool {
    match ability {
        GateAbility::WallJump => events.wall_jumps_in_wall_gate > 0,
        GateAbility::Dash => events.dashes_in_dash_gate > 0,
    }
}

fn structural_and_raster_violations(candidate: &CompositionalAbilityCandidate) -> Vec<String> {
    let mut violations = Vec::new();
    let mission = &candidate.rewritten_mission.plan;
    if candidate.embedding.generation_version != COMPOSITIONAL_ABILITY_GENERATION_VERSION {
        violations.push("candidate carries a stale physical generation version".to_owned());
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
        violations.push("candidate did not retain the replay-pending evidence state".to_owned());
    }

    for gate in &mission.gates {
        if gate.embedding_contract.version != COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION
        {
            violations.push(format!(
                "gate {} carries a stale contract version",
                gate.ordinal
            ));
        }
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
                "gate {} graph reaches the sink without {:?}",
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
        let rise = realization.lower_support.row - realization.upper_support.row;
        if rise < gate.embedding_contract.minimum_ascent_rows {
            violations.push(format!(
                "gate {} rise {rise} is below contract minimum {}",
                gate.ordinal, gate.embedding_contract.minimum_ascent_rows,
            ));
        }
        if gate.required_ability == GateAbility::WallJump && rise != 8 {
            violations.push(format!("wall gate {} has non-v3 rise {rise}", gate.ordinal));
        }
        if candidate.route_plan.nodes.iter().any(|node| {
            node.id != realization.from_route_node_id
                && node.id != realization.to_route_node_id
                && realization.upper_support.row < node.support.row
                && node.support.row < realization.lower_support.row
        }) {
            violations.push(format!(
                "gate {} isolation band contains a non-endpoint support",
                gate.ordinal,
            ));
        }
        let expected_verb = match gate.required_ability {
            GateAbility::WallJump => RouteVerb::WallClimb,
            GateAbility::Dash => RouteVerb::DashUp,
        };
        if !candidate.route_plan.edges.iter().any(|edge| {
            edge.from == realization.from_route_node_id
                && edge.to == realization.to_route_node_id
                && edge.critical
                && edge.verb == expected_verb
        }) {
            violations.push(format!(
                "gate {} physical route edge lost {expected_verb:?}",
                gate.ordinal,
            ));
        }
        if candidate
            .rewritten_mission
            .plan
            .edges
            .get(usize::from(gate.mission_edge_index))
            .map(|edge| edge.forward_requirement)
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
                    "cut {} shelf tile ({x},{}) mutated",
                    cut.order, cut.row
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

fn print_candidate_report(report: &CandidateReport) {
    println!(
        "authoritative-result key={} baseline-reverse-positive={} passed={}",
        report.key,
        report.baseline_reverse_positive,
        report.passed(),
    );
    for intended in &report.intended_wall {
        println!(
            "  intended-wall loadout={} matrix-doors={}/{} matrix-pickups={}/{} advertised-events={:?} matrix-effort={:?} direct={:?} exact-action-bypasses={:?}",
            loadout_slug(intended.matrix.loadout),
            intended.matrix.door_positive,
            intended.matrix.door_total,
            intended.matrix.pickup_positive,
            intended.matrix.pickup_total,
            intended.advertised_events,
            intended.matrix.aggregate_effort,
            intended.direct,
            intended
                .exact_action_bypasses
                .iter()
                .copied()
                .map(loadout_slug)
                .collect::<Vec<_>>(),
        );
    }
    for missing in &report.missing_wall {
        println!(
            "  missing-wall loadout={} matrix-doors={}/{} matrix-pickups={}/{} advertised-forward-positive={} reverse-positive={} matrix-effort={:?} direct={:?}",
            loadout_slug(missing.matrix.loadout),
            missing.matrix.door_positive,
            missing.matrix.door_total,
            missing.matrix.pickup_positive,
            missing.matrix.pickup_total,
            missing.matrix.forward_positive,
            missing.matrix.reverse_positive,
            missing.matrix.aggregate_effort,
            missing.direct,
        );
    }
    for violation in &report.violations {
        println!("  violation={violation}");
    }
}

fn room_rect_is_clear(room: &downwards_core::Room, rect: Rect) -> bool {
    room.tiles().iter().enumerate().all(|(index, tile)| {
        if !matches!(tile, Tile::Solid | Tile::Hazard) {
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
