//! Authoritative bounded audit for the compositional route-cut prototype.
//!
//! This executable does not curate or persist corpus artifacts.  It evaluates
//! exact attempt-zero baseline-construction keys, records typed construction
//! failures, and keeps every bounded solver miss explicitly inconclusive.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env,
    error::Error,
    time::Instant,
};

use downwards_ai::{
    DIRECT_PROBE_AUDIT_VERSION, InconclusiveReason, Replay, SearchTarget, SolverConfig,
    TargetSolveOutcome, audit_direct_controller_probes, solve_target,
};
use downwards_core::{AbilitySet, Action, BoundarySide, DoorSocket, Simulation, Tile};
use downwards_gen::{GeneratedLevel, experimental::*};
use downwards_lab::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, SimulationGeometryDescriptor,
    StaticVisualDescriptor, TraversalGrid, TraversalTrace, observe_successful_replay,
};
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};

#[path = "../corpus/fingerprints.rs"]
mod fingerprints;
use fingerprints::fingerprint_static_visual;
#[path = "../corpus/model.rs"]
#[allow(dead_code)]
mod model;
use model::EvaluationLoadout;
#[path = "../corpus/route_assessment.rs"]
#[allow(dead_code)]
mod route_assessment;
use route_assessment::{
    ControllerDemand, LoadoutControllerAuditStatus, assess_easiest_known_routes_from_source,
};
#[path = "../structural.rs"]
#[allow(dead_code)]
mod structural;
use structural::describe_terrain_utility;

const USAGE: &str = concat!(
    "usage: cargo run --bin compositional_route_cut_experiment -- <start-seed> <seed-count>\n",
    "       cargo run --bin compositional_route_cut_experiment -- diagnose <intent> <seed>",
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RouteDemandAggregate {
    expected_routes: usize,
    exact_known_routes: usize,
    complete_without_exact_positive: usize,
    bounded_without_exact_positive: usize,
    run_only_routes: usize,
    monotone_simple_only_routes: usize,
    other_routes: usize,
    horizontal_reversals: usize,
    vertical_decisions: usize,
    duration_ticks: usize,
}

impl RouteDemandAggregate {
    fn observe(&mut self, demand: ControllerDemand) {
        self.exact_known_routes += 1;
        if demand.run_only {
            self.run_only_routes += 1;
        } else if demand.monotone_simple {
            self.monotone_simple_only_routes += 1;
        } else {
            self.other_routes += 1;
        }
        self.horizontal_reversals += demand.horizontal_reversals;
        self.vertical_decisions += demand.vertical_decisions;
        self.duration_ticks += demand.duration_ticks;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TargetAggregate {
    expected_doors: usize,
    positive_doors: usize,
    expected_pickups: usize,
    positive_pickups: usize,
}

impl TargetAggregate {
    fn all_positive(self) -> bool {
        self.positive_doors == self.expected_doors && self.positive_pickups == self.expected_pickups
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CanonicalDemand {
    horizontal_reversals: usize,
    vertical_decisions: usize,
    duration_ticks: usize,
}

struct CanonicalObservation {
    demands: BTreeMap<(String, String), CanonicalDemand>,
    traversals: Vec<TraversalTrace>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DirectionalAggregate {
    expected_pairs: usize,
    bidirectional_pairs: usize,
    one_direction_positive_pairs: usize,
    neither_direction_positive_pairs: usize,
    asymmetric_pairs: usize,
    total_duration_difference: usize,
    maximum_duration_difference: usize,
    total_reversal_difference: usize,
    maximum_reversal_difference: usize,
}

impl DirectionalAggregate {
    fn observe(
        &mut self,
        demands: &BTreeMap<(String, String), CanonicalDemand>,
        door_ids: &[String],
    ) {
        for (source_index, source) in door_ids.iter().enumerate() {
            for target in &door_ids[source_index + 1..] {
                self.expected_pairs += 1;
                let forward = demands.get(&(source.clone(), target.clone()));
                let reverse = demands.get(&(target.clone(), source.clone()));
                let (Some(forward), Some(reverse)) = (forward, reverse) else {
                    if forward.is_some() || reverse.is_some() {
                        self.one_direction_positive_pairs += 1;
                    } else {
                        self.neither_direction_positive_pairs += 1;
                    }
                    continue;
                };
                self.bidirectional_pairs += 1;
                self.asymmetric_pairs += usize::from(forward != reverse);
                let duration_difference = forward.duration_ticks.abs_diff(reverse.duration_ticks);
                let reversal_difference = forward
                    .horizontal_reversals
                    .abs_diff(reverse.horizontal_reversals);
                self.total_duration_difference += duration_difference;
                self.maximum_duration_difference =
                    self.maximum_duration_difference.max(duration_difference);
                self.total_reversal_difference += reversal_difference;
                self.maximum_reversal_difference =
                    self.maximum_reversal_difference.max(reversal_difference);
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PortAudit {
    side_occurrences: BTreeMap<&'static str, usize>,
    socket_occurrences: BTreeMap<DoorSocket, usize>,
    inventory_misses: usize,
}

impl PortAudit {
    fn observe(&mut self, candidate: &CompositionalRouteCutCandidate) {
        for door in candidate.generated.room.doors() {
            *self
                .side_occurrences
                .entry(boundary_side_slug(door.side))
                .or_default() += 1;
            *self.socket_occurrences.entry(door.socket()).or_default() += 1;
            self.inventory_misses +=
                usize::from(!compositional_route_cut_socket_in_inventory(door.socket()));
        }
    }

    fn signatures_with_observed_mates(&self) -> usize {
        self.socket_occurrences
            .keys()
            .filter(|socket| self.socket_occurrences.contains_key(&socket.mate()))
            .count()
    }

    fn occurrences(&self) -> usize {
        self.socket_occurrences.values().sum()
    }

    fn occurrences_with_observed_mates(&self) -> usize {
        self.socket_occurrences
            .iter()
            .filter(|(socket, _)| self.socket_occurrences.contains_key(&socket.mate()))
            .map(|(_, count)| count)
            .sum()
    }

    fn unmatched_signatures(&self) -> Vec<String> {
        self.socket_occurrences
            .keys()
            .filter(|socket| !self.socket_occurrences.contains_key(&socket.mate()))
            .map(|socket| socket_label(*socket))
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Expressivity {
    topology_signatures: BTreeSet<u64>,
    derivation_signatures: BTreeSet<u64>,
    route_signatures: BTreeSet<u64>,
    static_visual_fingerprints: BTreeSet<u64>,
    simulation_geometry_fingerprints: BTreeSet<u64>,
}

impl Expressivity {
    fn observe(&mut self, candidate: &CompositionalRouteCutCandidate) {
        self.topology_signatures
            .insert(candidate.mission.topology_signature());
        self.derivation_signatures
            .insert(candidate.mission.derivation_signature());
        self.route_signatures
            .insert(candidate.route_summary.signature);
        self.static_visual_fingerprints
            .insert(fingerprint_static_visual(
                &StaticVisualDescriptor::from_room(&candidate.generated.room),
            ));
        self.simulation_geometry_fingerprints.insert(
            SimulationGeometryDescriptor::from_room(&candidate.generated.room).stable_digest(),
        );
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuditSummary {
    attempted_rooms: usize,
    constructed_rooms: usize,
    generation_failures: BTreeMap<&'static str, usize>,
    construction_all_target_positive_rooms: usize,
    both_all_target_positive_rooms: usize,
    hard_gate_positive_rooms: usize,
    construction_targets: TargetAggregate,
    both_targets: TargetAggregate,
    inconclusive_classes: BTreeMap<String, usize>,
    construction_direct: RouteDemandAggregate,
    both_direct: RouteDemandAggregate,
    canonical_both_routes: usize,
    canonical_both_reversals: usize,
    canonical_both_vertical_decisions: usize,
    canonical_both_duration: usize,
    directional: DirectionalAggregate,
    terrain_components: usize,
    interior_terrain_tiles: usize,
    statically_attributed_terrain_tiles: usize,
    positively_corroborated_terrain_tiles: usize,
    uncorroborated_terrain_components: usize,
    uncorroborated_terrain_tiles: usize,
    zero_cut_rooms: usize,
    one_cut_rooms: usize,
    multiple_cut_rooms: usize,
    ports: PortAudit,
    expressivity: Expressivity,
}

impl AuditSummary {
    fn observe_matrix(
        &mut self,
        matrix: &DoorTargetEvidenceBatch,
        label: &str,
        construction: bool,
    ) -> bool {
        let mut observed = TargetAggregate {
            expected_doors: matrix.door_routes().len(),
            expected_pickups: matrix.pickup_routes().len(),
            ..TargetAggregate::default()
        };
        for row in matrix.door_routes() {
            match &row.evidence {
                BoundedTargetEvidence::Positive(_) => observed.positive_doors += 1,
                BoundedTargetEvidence::Inconclusive(inconclusive) => {
                    *self
                        .inconclusive_classes
                        .entry(format!(
                            "{label}:door:{}",
                            inconclusive_reason_slug(inconclusive.reason)
                        ))
                        .or_default() += 1;
                }
            }
        }
        for row in matrix.pickup_routes() {
            match &row.evidence {
                BoundedTargetEvidence::Positive(_) => observed.positive_pickups += 1,
                BoundedTargetEvidence::Inconclusive(inconclusive) => {
                    *self
                        .inconclusive_classes
                        .entry(format!(
                            "{label}:pickup:{}",
                            inconclusive_reason_slug(inconclusive.reason)
                        ))
                        .or_default() += 1;
                }
            }
        }
        let aggregate = if construction {
            &mut self.construction_targets
        } else {
            &mut self.both_targets
        };
        aggregate.expected_doors += observed.expected_doors;
        aggregate.positive_doors += observed.positive_doors;
        aggregate.expected_pickups += observed.expected_pickups;
        aggregate.positive_pickups += observed.positive_pickups;
        observed.all_positive()
    }

    fn observe_generation(&mut self, candidate: &CompositionalRouteCutCandidate) {
        self.constructed_rooms += 1;
        match candidate.embedding.cut_realizations.len() {
            0 => self.zero_cut_rooms += 1,
            1 => self.one_cut_rooms += 1,
            _ => self.multiple_cut_rooms += 1,
        }
        self.ports.observe(candidate);
        self.expressivity.observe(candidate);
    }

    fn observe_terrain(
        &mut self,
        candidate: &CompositionalRouteCutCandidate,
        traversals: &[TraversalTrace],
    ) -> Result<(), Box<dyn Error>> {
        let traversal_refs = traversals.iter().collect::<Vec<_>>();
        let terrain = describe_terrain_utility(
            &candidate.generated.room,
            &candidate.route_plan,
            &traversal_refs,
        )?;
        self.terrain_components += terrain.components.len();
        self.interior_terrain_tiles += terrain.interior_terrain_tiles;
        self.statically_attributed_terrain_tiles += terrain.static_attributed_tiles;
        self.positively_corroborated_terrain_tiles += terrain.positively_corroborated_tiles;
        self.uncorroborated_terrain_components += terrain.components_without_positive_corroboration;
        self.uncorroborated_terrain_tiles += terrain.tiles_without_positive_corroboration;
        Ok(())
    }

    fn render(&self) -> String {
        format!(
            concat!(
                "source=compositional-route-cut-v{} derivation-v{} socket-inventory-v{} ",
                "direct-probe-audit-v{} attempt=0 construction-loadout=baseline ",
                "attempted={} constructed={} generation-failures={:?} ",
                "construction-all-target-positive={} both-all-target-positive={} hard-gate-positive={} ",
                "construction-targets=doors:{}/{} pickups:{}/{} ",
                "both-targets=doors:{}/{} pickups:{}/{} inconclusive-classes={:?} ",
                "construction-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "both-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "canonical-both=routes:{} reversals:{} vertical:{} duration:{} ",
                "directional=expected-pairs:{} bidirectional-positive:{} one-direction-positive:{} ",
                "neither-positive:{} demand-asymmetric:{} duration-diff-total:{} duration-diff-max:{} ",
                "reversal-diff-total:{} reversal-diff-max:{} ",
                "cuts=zero:{} one:{} multiple:{} ",
                "expressivity=topology:{} derivation:{} route:{} static:{} simulation:{} ",
                "terrain=components:{} interior:{} static-attributed:{} positive-corroborated:{} ",
                "uncorroborated-components:{} uncorroborated-tiles:{} ",
                "ports=total:{} sides:{:?} sockets:{:?} unique-sockets:{} ",
                "observed-mate-covered-signatures:{} observed-mate-covered-occurrences:{} ",
                "inventory-misses:{} unmatched:{:?}"
            ),
            COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            DIRECT_PROBE_AUDIT_VERSION,
            self.attempted_rooms,
            self.constructed_rooms,
            self.generation_failures,
            self.construction_all_target_positive_rooms,
            self.both_all_target_positive_rooms,
            self.hard_gate_positive_rooms,
            self.construction_targets.positive_doors,
            self.construction_targets.expected_doors,
            self.construction_targets.positive_pickups,
            self.construction_targets.expected_pickups,
            self.both_targets.positive_doors,
            self.both_targets.expected_doors,
            self.both_targets.positive_pickups,
            self.both_targets.expected_pickups,
            self.inconclusive_classes,
            self.construction_direct.exact_known_routes,
            self.construction_direct.expected_routes,
            self.construction_direct.complete_without_exact_positive,
            self.construction_direct.bounded_without_exact_positive,
            self.construction_direct.run_only_routes,
            self.construction_direct.monotone_simple_only_routes,
            self.construction_direct.other_routes,
            self.construction_direct.horizontal_reversals,
            self.construction_direct.vertical_decisions,
            self.construction_direct.duration_ticks,
            self.both_direct.exact_known_routes,
            self.both_direct.expected_routes,
            self.both_direct.complete_without_exact_positive,
            self.both_direct.bounded_without_exact_positive,
            self.both_direct.run_only_routes,
            self.both_direct.monotone_simple_only_routes,
            self.both_direct.other_routes,
            self.both_direct.horizontal_reversals,
            self.both_direct.vertical_decisions,
            self.both_direct.duration_ticks,
            self.canonical_both_routes,
            self.canonical_both_reversals,
            self.canonical_both_vertical_decisions,
            self.canonical_both_duration,
            self.directional.expected_pairs,
            self.directional.bidirectional_pairs,
            self.directional.one_direction_positive_pairs,
            self.directional.neither_direction_positive_pairs,
            self.directional.asymmetric_pairs,
            self.directional.total_duration_difference,
            self.directional.maximum_duration_difference,
            self.directional.total_reversal_difference,
            self.directional.maximum_reversal_difference,
            self.zero_cut_rooms,
            self.one_cut_rooms,
            self.multiple_cut_rooms,
            self.expressivity.topology_signatures.len(),
            self.expressivity.derivation_signatures.len(),
            self.expressivity.route_signatures.len(),
            self.expressivity.static_visual_fingerprints.len(),
            self.expressivity.simulation_geometry_fingerprints.len(),
            self.terrain_components,
            self.interior_terrain_tiles,
            self.statically_attributed_terrain_tiles,
            self.positively_corroborated_terrain_tiles,
            self.uncorroborated_terrain_components,
            self.uncorroborated_terrain_tiles,
            self.ports.occurrences(),
            self.ports.side_occurrences,
            self.ports.socket_occurrences,
            self.ports.socket_occurrences.len(),
            self.ports.signatures_with_observed_mates(),
            self.ports.occurrences_with_observed_mates(),
            self.ports.inventory_misses,
            self.ports.unmatched_signatures(),
        )
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let [command, intent, seed] = arguments.as_slice()
        && command == "diagnose"
    {
        return diagnose_exact_key(parse_intent(intent)?, seed.parse()?);
    }
    let [start_seed, seed_count] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let started = Instant::now();
    let mut summary = AuditSummary::default();

    for intent in ChallengeIntent::ALL {
        for offset in 0..seed_count {
            let seed = start_seed.wrapping_add(offset as u64);
            summary.attempted_rooms += 1;
            let key = CompositionalRouteCutKey::new(seed, AbilitySet::NONE, intent);
            let candidate = match key.regenerate() {
                Ok(candidate) => candidate,
                Err(error) => {
                    *summary
                        .generation_failures
                        .entry(generation_failure_class(&error.cause))
                        .or_default() += 1;
                    eprintln!(
                        "construct-failure intent={} seed={seed} attempt=0 class={} error={error}",
                        intent.slug(),
                        generation_failure_class(&error.cause),
                    );
                    continue;
                }
            };
            summary.observe_generation(&candidate);

            let construction_matrix = evaluate_generated_door_targets_for_loadout(
                &candidate.generated,
                AbilitySet::NONE,
                &ValidationConfig::for_loadout(AbilitySet::NONE),
            )?;
            let construction_positive =
                summary.observe_matrix(&construction_matrix, "construction", true);
            summary.construction_all_target_positive_rooms += usize::from(construction_positive);

            let both_matrix = evaluate_generated_door_targets_for_loadout(
                &candidate.generated,
                AbilitySet::ALL,
                &ValidationConfig::for_loadout(AbilitySet::ALL),
            )?;
            let both_positive = summary.observe_matrix(&both_matrix, "both", false);
            summary.both_all_target_positive_rooms += usize::from(both_positive);

            observe_direct_demand(
                &candidate.generated,
                EvaluationLoadout::Baseline,
                &mut summary.construction_direct,
            )?;
            observe_direct_demand(
                &candidate.generated,
                EvaluationLoadout::Both,
                &mut summary.both_direct,
            )?;

            let canonical =
                observe_canonical_both(&candidate.generated, &both_matrix, &mut summary)?;
            let mut door_ids = candidate
                .generated
                .room
                .doors()
                .iter()
                .map(|door| door.id.clone())
                .collect::<Vec<_>>();
            door_ids.sort_unstable();
            summary.directional.observe(&canonical.demands, &door_ids);
            summary.observe_terrain(&candidate, &canonical.traversals)?;

            if construction_positive && both_positive {
                summary.hard_gate_positive_rooms += 1;
            } else {
                eprintln!(
                    "hard-gate-inconclusive intent={} seed={seed} construction={} both={}",
                    intent.slug(),
                    construction_positive,
                    both_positive,
                );
                report_inconclusive_targets(&construction_matrix, "construction");
                report_inconclusive_targets(&both_matrix, "both");
            }
            eprintln!(
                "completed intent={} seed={seed} ports={} cuts={} construction={} both={}",
                intent.slug(),
                candidate.boundary_ports.len(),
                candidate.embedding.cut_realizations.len(),
                construction_positive,
                both_positive,
            );
        }
    }

    println!("{}", summary.render());
    println!("runtime-ms={}", started.elapsed().as_millis());
    Ok(())
}

fn parse_intent(value: &str) -> Result<ChallengeIntent, Box<dyn Error>> {
    match value {
        "gentle" => Ok(ChallengeIntent::Gentle),
        "standard" => Ok(ChallengeIntent::Standard),
        "technical" => Ok(ChallengeIntent::Technical),
        _ => {
            Err(format!("unknown intent {value:?}; expected gentle, standard, or technical").into())
        }
    }
}

/// Inspect one exact candidate without altering its key or geometry.  The
/// generous search is a diagnostic only: a bounded miss remains inconclusive.
fn diagnose_exact_key(intent: ChallengeIntent, seed: u64) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let key = CompositionalRouteCutKey::new(seed, AbilitySet::NONE, intent);
    let candidate = key.regenerate()?;
    println!(
        "diagnostic-key intent={} seed={} attempt={} construction=baseline topology={:016x} derivation={:016x}",
        intent.slug(),
        seed,
        key.embedding_attempt,
        candidate.mission.topology_signature(),
        candidate.mission.derivation_signature(),
    );
    report_exact_geometry(&candidate)?;

    let source = "port-0";
    let target = "port-1";
    let default_solver = SolverConfig::for_abilities(AbilitySet::NONE);
    let direct = assess_easiest_known_routes_from_source(
        &candidate.generated.room,
        source,
        vec![target.to_owned()],
        EvaluationLoadout::Baseline,
        &default_solver,
    )?;
    let route = &direct.routes[0];
    let audit = route
        .audits
        .iter()
        .find(|audit| audit.loadout == EvaluationLoadout::Baseline)
        .expect("the exact baseline loadout is audited");
    println!(
        "direct-controller source={source} target={target} status={:?} stats={:?} raw-positive={} retained-positive={} easiest-positive={}",
        audit.status,
        audit.operational_stats,
        audit.raw_positive_witnesses,
        audit.retained_semantic_witnesses,
        route.easiest_known().is_some(),
    );

    let initial =
        Simulation::enter_via_door(candidate.generated.room.clone(), AbilitySet::NONE, source)?;
    let default_outcome = solve_target(&initial, SearchTarget::door(target), &default_solver)?;
    report_solve_outcome("default-full", &default_outcome, &initial)?;

    let generous_solver = generous_diagnostic_solver();
    let generous_outcome = solve_target(&initial, SearchTarget::door(target), &generous_solver)?;
    report_solve_outcome("generous-full", &generous_outcome, &initial)?;

    let matrix = evaluate_generated_door_targets_for_loadout(
        &candidate.generated,
        AbilitySet::NONE,
        &ValidationConfig::for_loadout(AbilitySet::NONE),
    )?;
    let both_matrix = evaluate_generated_door_targets_for_loadout(
        &candidate.generated,
        AbilitySet::ALL,
        &ValidationConfig::for_loadout(AbilitySet::ALL),
    )?;
    if let Some(positive) = both_matrix
        .door_routes()
        .iter()
        .find(|row| row.source_door_id == source && row.target_door_id == target)
        .and_then(|row| row.evidence.positive())
    {
        let both_initial =
            Simulation::enter_via_door(candidate.generated.room.clone(), AbilitySet::ALL, source)?;
        let observation = observe_successful_replay(
            &both_initial,
            &positive.solution().replay,
            TraversalGrid::default(),
        )?;
        println!(
            "both-witness ticks={} jumps={} wall-jumps={} dashes={} reversals={} events={:?}",
            observation.completion_ticks,
            observation.actions.successful_jumps,
            observation.actions.successful_wall_jumps,
            observation.actions.successful_dashes,
            direction_reversals(&observation.actions.spans, |action| action.move_x),
            observation.actions.events,
        );
    } else {
        println!("both-witness positive=false");
    }
    let pickup = matrix
        .pickup_routes()
        .iter()
        .find(|row| row.source_door_id == source)
        .and_then(|row| row.evidence.positive());
    if let Some(pickup) = pickup {
        let pickup_actions = pickup.solution().replay.actions().collect::<Vec<_>>();
        let mut intermediate = initial.clone();
        for action in &pickup_actions {
            intermediate.step(*action);
        }
        let position = intermediate.player().position_subpixels();
        println!(
            "segment-source-to-pickup positive=true ticks={} position-subpixels=({}, {}) collected={:?}",
            pickup_actions.len(),
            position.x,
            position.y,
            intermediate
                .collected_pickups()
                .map(|pickup| pickup.id().to_owned())
                .collect::<Vec<_>>(),
        );
        let continuation =
            solve_target(&intermediate, SearchTarget::door(target), &generous_solver)?;
        report_solve_outcome("segment-pickup-to-target", &continuation, &intermediate)?;
        if let TargetSolveOutcome::Solved(solution) = continuation {
            let continuation_ticks = solution.replay.frames.len();
            let mut actions = pickup_actions.clone();
            actions.extend(solution.replay.actions());
            let replay = Replay::record(&initial, actions);
            let verification = replay.verify(&initial)?;
            println!(
                "composed-replay ticks={} continuation-ticks={} reached={:?} pickups={:?}",
                replay.frames.len(),
                continuation_ticks,
                verification.reached_exit,
                verification.collected_pickup_ids,
            );
        }
        let pickup_route_node = candidate
            .mission_route_nodes
            .iter()
            .find(|mapping| mapping.mission_node_id == candidate.mission.plan.pickup_node_id)
            .map(|mapping| mapping.route_node_id)
            .ok_or("pickup mission node has no route-node mapping")?;
        let target_route_node = candidate
            .boundary_ports
            .iter()
            .find(|port| port.door.id == target)
            .map(|port| port.node_id)
            .ok_or("candidate has no port-1")?;
        if let Some(landing_path) = conservative_landing_path(
            &candidate.route_plan,
            pickup_route_node,
            target_route_node,
            4,
        ) {
            println!(
                "static-landing-path max-gap-tiles=4 route-nodes={landing_path:?} clearances={:?}",
                landing_path_clearances(&candidate.route_plan, &landing_path),
            );
            if let Some(controller_replay) = waypoint_controller_replay(
                &intermediate,
                &candidate.route_plan,
                &landing_path,
                target,
                None,
            ) {
                let verification = controller_replay.verify(&intermediate)?;
                println!(
                    "waypoint-controller positive=true ticks={} reached={:?}",
                    controller_replay.frames.len(),
                    verification.reached_exit,
                );
                let mut actions = pickup_actions.clone();
                actions.extend(controller_replay.actions());
                let composed = Replay::record(&initial, actions);
                let verification = composed.verify(&initial)?;
                println!(
                    "waypoint-composed-replay ticks={} reached={:?} pickups={:?}",
                    composed.frames.len(),
                    verification.reached_exit,
                    verification.collected_pickup_ids,
                );
            } else {
                println!("waypoint-controller positive=false bounded-ticks=1200");
            }
            if let Some(checkpoint_replay) = waypoint_controller_replay(
                &intermediate,
                &candidate.route_plan,
                &landing_path,
                target,
                Some(landing_path.len() - 1),
            ) {
                let mut checkpoint = intermediate.clone();
                for action in checkpoint_replay.actions() {
                    checkpoint.step(action);
                }
                let player = checkpoint.player().bounds();
                println!(
                    "penultimate-checkpoint ticks={} player=({}, {}, {}, {}) grounded={}",
                    checkpoint_replay.frames.len(),
                    player.x,
                    player.y,
                    player.width,
                    player.height,
                    checkpoint.player().grounded(),
                );
                let target_request = SearchTarget::door(target);
                let checkpoint_direct = audit_direct_controller_probes(
                    &checkpoint,
                    std::slice::from_ref(&target_request),
                    &default_solver,
                )?;
                println!(
                    "penultimate-direct-controller status={:?} stats={:?} positives={}",
                    checkpoint_direct.status,
                    checkpoint_direct.stats,
                    checkpoint_direct.witnesses.len(),
                );
                let checkpoint_outcome =
                    solve_target(&checkpoint, target_request, &generous_solver)?;
                report_solve_outcome(
                    "segment-penultimate-to-target",
                    &checkpoint_outcome,
                    &checkpoint,
                )?;
                if let Some((coyote_replay, delay, hold_ticks)) =
                    coyote_transfer_controller(&checkpoint, target)
                {
                    let verification = coyote_replay.verify(&checkpoint)?;
                    println!(
                        "penultimate-coyote-controller positive=true delay={} hold={} ticks={} reached={:?}",
                        delay,
                        hold_ticks,
                        coyote_replay.frames.len(),
                        verification.reached_exit,
                    );
                    let mut actions = pickup_actions.clone();
                    actions.extend(checkpoint_replay.actions());
                    actions.extend(coyote_replay.actions());
                    let composed = Replay::record(&initial, actions);
                    let verification = composed.verify(&initial)?;
                    println!(
                        "coyote-composed-replay ticks={} reached={:?} pickups={:?}",
                        composed.frames.len(),
                        verification.reached_exit,
                        verification.collected_pickup_ids,
                    );
                } else {
                    println!("penultimate-coyote-controller positive=false variants=36");
                }
            } else {
                println!("penultimate-checkpoint positive=false");
            }
        } else {
            println!("static-landing-path max-gap-tiles=4 found=false");
        }
    } else {
        println!("segment-source-to-pickup positive=false");
    }
    println!("diagnostic-runtime-ms={}", started.elapsed().as_millis());
    Ok(())
}

fn generous_diagnostic_solver() -> SolverConfig {
    let mut solver = SolverConfig::for_abilities(AbilitySet::NONE);
    solver.max_expanded_nodes = 500_000;
    solver.max_simulated_ticks = 20_000_000;
    solver.max_ticks_per_path = 2_400;
    solver.beam_width = 256;
    solver
}

fn report_solve_outcome(
    label: &str,
    outcome: &TargetSolveOutcome,
    initial: &Simulation,
) -> Result<(), Box<dyn Error>> {
    match outcome {
        TargetSolveOutcome::Solved(solution) => {
            let verification = solution.replay.verify(initial)?;
            println!(
                "{label} positive=true ticks={} stats={:?} replay-reached={:?}",
                solution.replay.frames.len(),
                solution.stats,
                verification.reached_exit,
            );
        }
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            println!("{label} positive=false reason={reason:?} stats={stats:?}");
        }
    }
    Ok(())
}

fn report_exact_geometry(candidate: &CompositionalRouteCutCandidate) -> Result<(), Box<dyn Error>> {
    for port in &candidate.boundary_ports {
        let node = &candidate.route_plan.nodes[usize::from(port.node_id)];
        let mission_node = candidate
            .mission_route_nodes
            .iter()
            .find(|mapping| mapping.route_node_id == port.node_id)
            .map(|mapping| mapping.mission_node_id)
            .ok_or_else(|| format!("route port {} has no mission-node mapping", port.node_id))?;
        println!(
            "port id={} side={} socket={} arrival=({}, {}) trigger=({}, {}, {}, {}) mission-node={} route-node={} support=({}, {}, row={}, kind={:?})",
            port.door.id,
            boundary_side_slug(port.door.side),
            socket_label(port.door.socket()),
            port.door.arrival.x,
            port.door.arrival.y,
            port.door.trigger_bounds.x,
            port.door.trigger_bounds.y,
            port.door.trigger_bounds.width,
            port.door.trigger_bounds.height,
            mission_node,
            port.node_id,
            node.support.start_x,
            node.support.end_x,
            node.support.row,
            node.support.kind,
        );
    }
    let source = candidate
        .boundary_ports
        .iter()
        .find(|port| port.door.id == "port-0")
        .ok_or("candidate has no port-0")?;
    let target = candidate
        .boundary_ports
        .iter()
        .find(|port| port.door.id == "port-1")
        .ok_or("candidate has no port-1")?;
    let path = shortest_route_path(&candidate.route_plan, source.node_id, target.node_id)
        .ok_or("route graph has no structural port-0 to port-1 path")?;
    println!(
        "structural-path source=port-0 target=port-1 edges={} route-nodes={:?}",
        path.len(),
        structural_path_nodes(source.node_id, &path),
    );
    for (index, step) in path.iter().enumerate() {
        let from = &candidate.route_plan.nodes[usize::from(step.from)];
        let to = &candidate.route_plan.nodes[usize::from(step.to)];
        println!(
            "  structural-step index={} traversal={}->{} authored={}->{} verb={:?} critical={} from-support=({}, {}, row={}, {:?}) to-support=({}, {}, row={}, {:?}) row-delta={} center-x-delta={}",
            index,
            step.from,
            step.to,
            step.edge.from,
            step.edge.to,
            step.edge.verb,
            step.edge.critical,
            from.support.start_x,
            from.support.end_x,
            from.support.row,
            from.support.kind,
            to.support.start_x,
            to.support.end_x,
            to.support.row,
            to.support.kind,
            i32::from(to.support.row) - i32::from(from.support.row),
            i32::from(to.support.center_x()) - i32::from(from.support.center_x()),
        );
    }
    println!(
        "pickup id={} bounds={:?} mission-node={} cuts={} forks={}",
        candidate.generated.room.pickups()[0].id(),
        candidate.generated.room.pickups()[0].bounds(),
        candidate.mission.plan.pickup_node_id,
        candidate.embedding.cut_realizations.len(),
        candidate.mission.plan.forks.len(),
    );
    println!("room-tiles:");
    for row in 0..candidate.generated.room.height() {
        let line = (0..candidate.generated.room.width())
            .map(|column| match candidate.generated.room.tile(column, row) {
                Some(Tile::Empty) => '.',
                Some(Tile::Solid) => '#',
                Some(Tile::OneWay) => '=',
                Some(Tile::Hazard) => '!',
                None => '?',
            })
            .collect::<String>();
        println!("{row:02} {line}");
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct StructuralPathStep<'a> {
    from: u16,
    to: u16,
    edge: &'a RouteEdge,
}

fn shortest_route_path(
    plan: &RoutePlan,
    source: u16,
    target: u16,
) -> Option<Vec<StructuralPathStep<'_>>> {
    let mut frontier = VecDeque::from([source]);
    let mut predecessor = vec![None; plan.nodes.len()];
    predecessor[usize::from(source)] = Some((source, usize::MAX));
    while let Some(node) = frontier.pop_front() {
        if node == target {
            break;
        }
        for (edge_index, edge) in plan.edges.iter().enumerate() {
            let adjacent = if edge.from == node {
                Some(edge.to)
            } else if edge.to == node {
                Some(edge.from)
            } else {
                None
            };
            if let Some(adjacent) = adjacent
                && predecessor[usize::from(adjacent)].is_none()
            {
                predecessor[usize::from(adjacent)] = Some((node, edge_index));
                frontier.push_back(adjacent);
            }
        }
    }
    predecessor[usize::from(target)]?;
    let mut reversed = Vec::new();
    let mut current = target;
    while current != source {
        let (previous, edge_index) = predecessor[usize::from(current)]?;
        reversed.push(StructuralPathStep {
            from: previous,
            to: current,
            edge: &plan.edges[edge_index],
        });
        current = previous;
    }
    reversed.reverse();
    Some(reversed)
}

fn structural_path_nodes(source: u16, path: &[StructuralPathStep<'_>]) -> Vec<u16> {
    std::iter::once(source)
        .chain(path.iter().map(|step| step.to))
        .collect()
}

fn support_gap(left: SupportSpec, right: SupportSpec) -> u16 {
    right
        .start_x
        .saturating_sub(left.end_x)
        .max(left.start_x.saturating_sub(right.end_x))
}

/// Coordinate-only diagnostic path.  This does not certify a transfer; it
/// merely finds upward landing candidates for the exact controller probe.
fn conservative_landing_path(
    plan: &RoutePlan,
    source: u16,
    target: u16,
    maximum_gap_tiles: u16,
) -> Option<Vec<u16>> {
    let mut frontier = VecDeque::from([source]);
    let mut predecessor = vec![None; plan.nodes.len()];
    predecessor[usize::from(source)] = Some(source);
    while let Some(node_id) = frontier.pop_front() {
        if node_id == target {
            break;
        }
        let node = &plan.nodes[usize::from(node_id)];
        for candidate in &plan.nodes {
            let rise = node.support.row.saturating_sub(candidate.support.row);
            if !(1..=2).contains(&rise)
                || support_gap(node.support, candidate.support) > maximum_gap_tiles
                || predecessor[usize::from(candidate.id)].is_some()
            {
                continue;
            }
            predecessor[usize::from(candidate.id)] = Some(node_id);
            frontier.push_back(candidate.id);
        }
    }
    predecessor[usize::from(target)]?;
    let mut path = vec![target];
    while *path.last()? != source {
        path.push(predecessor[usize::from(*path.last()?)]?);
    }
    path.reverse();
    Some(path)
}

fn landing_path_clearances(plan: &RoutePlan, path: &[u16]) -> Vec<(u16, u16, u16, u16)> {
    path.windows(2)
        .map(|pair| {
            let from = plan.nodes[usize::from(pair[0])].support;
            let to = plan.nodes[usize::from(pair[1])].support;
            (
                pair[0],
                pair[1],
                from.row.saturating_sub(to.row),
                support_gap(from, to),
            )
        })
        .collect()
}

fn waypoint_controller_replay(
    initial: &Simulation,
    plan: &RoutePlan,
    landing_path: &[u16],
    target_door_id: &str,
    stop_at_waypoint: Option<usize>,
) -> Option<Replay> {
    let target = initial
        .room()
        .doors()
        .iter()
        .find(|door| door.id == target_door_id)?;
    let mut simulation = initial.clone();
    let mut actions = Vec::new();
    let mut waypoint = 1;
    let mut launched = false;
    for _ in 0..1_200 {
        if simulation.reached_exit() == Some(target_door_id) {
            return Some(Replay::record(initial, actions));
        }
        let player = simulation.player().bounds();
        while waypoint < landing_path.len() {
            let support = plan.nodes[usize::from(landing_path[waypoint])].support;
            let standing_y = i32::from(support.row) * simulation.room().tile_size() - player.height;
            if simulation.player().grounded() && player.y <= standing_y + 1 {
                waypoint += 1;
                launched = false;
            } else {
                break;
            }
        }
        if stop_at_waypoint == Some(waypoint) {
            return Some(Replay::record(initial, actions));
        }
        let tile_size = simulation.room().tile_size();
        let (target_center_x, jump) = if waypoint < landing_path.len() {
            let from = plan.nodes[usize::from(landing_path[waypoint - 1])].support;
            let to = plan.nodes[usize::from(landing_path[waypoint])].support;
            let moving_right = to.center_x() >= from.center_x();
            if launched {
                let landing_center = if moving_right {
                    i32::from(to.start_x) * tile_size - player.width / 2 + 1
                } else {
                    i32::from(to.end_x) * tile_size + player.width / 2 - 1
                };
                (
                    landing_center,
                    simulation.player().jump_hold_ticks_remaining() > 0,
                )
            } else if simulation.player().grounded() {
                let takeoff_center = if moving_right {
                    i32::from(from.end_x) * tile_size - player.width / 2 - 1
                } else {
                    i32::from(from.start_x) * tile_size + player.width / 2 + 1
                };
                let ready = (takeoff_center - (player.x + player.width / 2)).abs() <= 10;
                if ready {
                    launched = true;
                }
                (takeoff_center, ready)
            } else {
                (i32::from(from.center_x()) * tile_size, false)
            }
        } else {
            let center = target.trigger_bounds.x + target.trigger_bounds.width / 2;
            if launched {
                (center, simulation.player().jump_hold_ticks_remaining() > 0)
            } else {
                let ready = simulation.player().grounded()
                    && (center - (player.x + player.width / 2)).abs() <= 3;
                if ready {
                    launched = true;
                }
                (center, ready)
            }
        };
        let player_center_x = player.x + player.width / 2;
        let offset = target_center_x - player_center_x;
        let action = Action {
            move_x: if offset.abs() <= 3 {
                0
            } else {
                i8::try_from(offset.signum()).expect("a horizontal sign fits the input axis")
            },
            jump,
            ..Action::default()
        };
        simulation.step(action);
        actions.push(action);
    }
    let player = simulation.player().bounds();
    println!(
        "waypoint-controller-final waypoint={}/{} player=({}, {}, {}, {}) grounded={} deaths={} reached={:?}",
        waypoint,
        landing_path.len(),
        player.x,
        player.y,
        player.width,
        player.height,
        simulation.player().grounded(),
        simulation.deaths(),
        simulation.reached_exit(),
    );
    None
}

/// Exact diagnostic controller for a ledge-edge transfer.  It enumerates a
/// small finite set of coyote-delay/jump-hold timings and returns only a replay
/// that the authoritative simulation actually completed.
fn coyote_transfer_controller(
    initial: &Simulation,
    target_door_id: &str,
) -> Option<(Replay, usize, usize)> {
    let target = initial
        .room()
        .doors()
        .iter()
        .find(|door| door.id == target_door_id)?;
    let target_center = target.trigger_bounds.x + target.trigger_bounds.width / 2;
    for delay in 0..=5 {
        for hold_ticks in 5..=10 {
            let mut simulation = initial.clone();
            let mut actions = Vec::new();
            let mut departed = false;
            let mut air_ticks = 0;
            let mut jumped = false;
            let mut held = 0;
            let mut previous_jump = false;
            for _ in 0..240 {
                if simulation.reached_exit() == Some(target_door_id) {
                    return Some((Replay::record(initial, actions), delay, hold_ticks));
                }
                let player = simulation.player().bounds();
                let on_target_support = simulation.player().grounded() && player.y <= 19;
                let action = if on_target_support {
                    let offset = target_center - (player.x + player.width / 2);
                    let aligned = offset.abs() <= 3;
                    Action {
                        move_x: if aligned {
                            0
                        } else {
                            i8::try_from(offset.signum())
                                .expect("a horizontal sign fits the input axis")
                        },
                        jump: aligned && !previous_jump,
                        ..Action::default()
                    }
                } else if !departed {
                    if !simulation.player().grounded() {
                        departed = true;
                    }
                    Action {
                        move_x: 1,
                        ..Action::default()
                    }
                } else if !jumped {
                    let press = air_ticks >= delay;
                    if press {
                        jumped = true;
                        held = 1;
                    } else {
                        air_ticks += 1;
                    }
                    Action {
                        move_x: 1,
                        jump: press,
                        ..Action::default()
                    }
                } else {
                    let hold = held < hold_ticks;
                    held += usize::from(hold);
                    Action {
                        move_x: 1,
                        jump: hold,
                        ..Action::default()
                    }
                };
                previous_jump = action.jump;
                simulation.step(action);
                actions.push(action);
                if jumped && simulation.player().grounded() && simulation.player().bounds().y > 19 {
                    break;
                }
            }
        }
    }
    None
}

fn observe_direct_demand(
    generated: &GeneratedLevel,
    loadout: EvaluationLoadout,
    aggregate: &mut RouteDemandAggregate,
) -> Result<(), Box<dyn Error>> {
    let mut doors = generated
        .room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<Vec<_>>();
    doors.sort_unstable();
    let solver = SolverConfig::for_abilities(loadout.abilities());
    for source in &doors {
        let targets = doors
            .iter()
            .filter(|target| *target != source)
            .cloned()
            .collect::<Vec<_>>();
        let batch = assess_easiest_known_routes_from_source(
            &generated.room,
            source,
            targets,
            loadout,
            &solver,
        )?;
        for route in batch.routes {
            aggregate.expected_routes += 1;
            let exact = route
                .easiest_first_witnesses
                .iter()
                .find(|witness| witness.loadout == loadout);
            if let Some(exact) = exact {
                aggregate.observe(exact.demand);
                continue;
            }
            let audit = route
                .audits
                .iter()
                .find(|audit| audit.loadout == loadout)
                .expect("the authoritative exact loadout is always audited");
            if audit.status == LoadoutControllerAuditStatus::CompleteFiniteVocabulary {
                aggregate.complete_without_exact_positive += 1;
            } else {
                aggregate.bounded_without_exact_positive += 1;
            }
        }
    }
    Ok(())
}

fn observe_canonical_both(
    generated: &GeneratedLevel,
    matrix: &DoorTargetEvidenceBatch,
    summary: &mut AuditSummary,
) -> Result<CanonicalObservation, Box<dyn Error>> {
    let mut demands = BTreeMap::new();
    let mut traversals = Vec::new();
    for row in matrix.door_routes() {
        let Some(positive) = row.evidence.positive() else {
            continue;
        };
        let initial = Simulation::enter_via_door(
            generated.room.clone(),
            AbilitySet::ALL,
            &row.source_door_id,
        )?;
        let observation = observe_successful_replay(
            &initial,
            &positive.solution().replay,
            TraversalGrid::default(),
        )?;
        let demand = canonical_demand(&observation.actions);
        summary.canonical_both_routes += 1;
        summary.canonical_both_reversals += demand.horizontal_reversals;
        summary.canonical_both_vertical_decisions += demand.vertical_decisions;
        summary.canonical_both_duration += demand.duration_ticks;
        demands.insert(
            (row.source_door_id.clone(), row.target_door_id.clone()),
            demand,
        );
        traversals.push(observation.traversal);
    }
    Ok(CanonicalObservation {
        demands,
        traversals,
    })
}

fn canonical_demand(actions: &SemanticActionTrace) -> CanonicalDemand {
    let horizontal_reversals = direction_reversals(&actions.spans, |action| action.move_x);
    let vertical_input_changes = input_direction_changes(&actions.spans, |action| action.move_y);
    let dash_direction_changes = accepted_dash_direction_changes(actions);
    CanonicalDemand {
        horizontal_reversals,
        vertical_decisions: actions
            .jump_presses
            .saturating_add(vertical_input_changes)
            .saturating_add(dash_direction_changes),
        duration_ticks: actions.total_ticks,
    }
}

fn direction_reversals(spans: &[ActionSpan], direction: impl Fn(SemanticAction) -> i8) -> usize {
    let mut previous_nonzero = 0;
    let mut reversals = 0;
    for span in spans {
        let current = direction(span.action);
        if current == 0 {
            continue;
        }
        if previous_nonzero != 0 && current != previous_nonzero {
            reversals += 1;
        }
        previous_nonzero = current;
    }
    reversals
}

fn input_direction_changes(
    spans: &[ActionSpan],
    direction: impl Fn(SemanticAction) -> i8,
) -> usize {
    let Some(first) = spans.first() else {
        return 0;
    };
    let mut previous = direction(first.action);
    spans[1..]
        .iter()
        .filter(|span| {
            let current = direction(span.action);
            let changed = current != previous && (current != 0 || previous != 0);
            previous = current;
            changed
        })
        .count()
}

fn accepted_dash_direction_changes(actions: &SemanticActionTrace) -> usize {
    let mut previous = None;
    let mut changes = 0;
    for direction in actions.events.iter().filter_map(|event| match event.event {
        SemanticEvent::Dash(direction) => Some(direction),
        _ => None,
    }) {
        if previous.is_some_and(|previous| previous != direction) {
            changes += 1;
        }
        previous = Some(direction);
    }
    changes
}

fn report_inconclusive_targets(matrix: &DoorTargetEvidenceBatch, label: &str) {
    for row in matrix.door_routes() {
        if let Some(inconclusive) = row.evidence.inconclusive() {
            eprintln!(
                "  {label}-door-inconclusive {} -> {} reason={:?} effort={:?}",
                row.source_door_id,
                row.target_door_id,
                inconclusive.reason,
                inconclusive.search_effort,
            );
        }
    }
    for row in matrix.pickup_routes() {
        if let Some(inconclusive) = row.evidence.inconclusive() {
            eprintln!(
                "  {label}-pickup-inconclusive {} -> {} reason={:?} effort={:?}",
                row.source_door_id,
                row.required_pickup_id,
                inconclusive.reason,
                inconclusive.search_effort,
            );
        }
    }
}

const fn inconclusive_reason_slug(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::NoExitsDefined => "no-exits-defined",
        InconclusiveReason::ExpandedNodeBudget => "expanded-node-budget",
        InconclusiveReason::SimulatedTickBudget => "simulated-tick-budget",
        InconclusiveReason::PathHorizon => "path-horizon",
        InconclusiveReason::FrontierExhausted => "frontier-exhausted",
    }
}

const fn boundary_side_slug(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "left",
        BoundarySide::Right => "right",
        BoundarySide::Ceiling => "ceiling",
        BoundarySide::Floor => "floor",
    }
}

fn socket_label(socket: DoorSocket) -> String {
    format!(
        "{}:{}:{}",
        boundary_side_slug(socket.side),
        socket.offset,
        socket.span
    )
}

const fn generation_failure_class(
    failure: &CompositionalRouteCutGenerationFailure,
) -> &'static str {
    match failure {
        CompositionalRouteCutGenerationFailure::Mission(
            MissionDerivationFailure::UnsupportedEmbeddingAttempt { .. },
        ) => "mission:unsupported-attempt",
        CompositionalRouteCutGenerationFailure::Mission(
            MissionDerivationFailure::CutPlacementExhausted { .. },
        ) => "mission:cut-placement",
        CompositionalRouteCutGenerationFailure::Mission(
            MissionDerivationFailure::ForkPlacementExhausted { .. },
        ) => "mission:fork-placement",
        CompositionalRouteCutGenerationFailure::Mission(MissionDerivationFailure::NoPickupNode) => {
            "mission:no-pickup-node"
        }
        CompositionalRouteCutGenerationFailure::Mission(
            MissionDerivationFailure::NoPortAttachmentNode,
        ) => "mission:no-port-node",
        CompositionalRouteCutGenerationFailure::RhythmExhausted => "embedding:rhythm",
        CompositionalRouteCutGenerationFailure::MissingMissionNode { .. } => {
            "embedding:missing-node"
        }
        CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
            phase: SupportConstraintPhase::Spine,
            ..
        } => "embedding:constraint-spine",
        CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
            phase: SupportConstraintPhase::Fork,
            ..
        } => "embedding:constraint-fork",
        CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
            phase: SupportConstraintPhase::FinalRouteContract,
            ..
        } => "embedding:constraint-final",
        CompositionalRouteCutGenerationFailure::SupportConstraintExhausted {
            phase: SupportConstraintPhase::RasterizedRouteContract,
            ..
        } => "embedding:constraint-raster",
        CompositionalRouteCutGenerationFailure::ForkEmbeddingExhausted { .. } => "embedding:fork",
        CompositionalRouteCutGenerationFailure::PortContract(_) => "embedding:port-contract",
        CompositionalRouteCutGenerationFailure::Room(_) => "room",
        CompositionalRouteCutGenerationFailure::Door(_) => "door",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_key_block_is_exact_and_deterministic() {
        let keys = ChallengeIntent::ALL
            .into_iter()
            .flat_map(|intent| {
                (0..4)
                    .map(move |seed| CompositionalRouteCutKey::new(seed, AbilitySet::NONE, intent))
            })
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 12);
        for key in keys {
            assert_eq!(key.embedding_attempt, 0);
            assert_eq!(key.construction_abilities, AbilitySet::NONE);
            assert_eq!(key.regenerate(), key.regenerate());
        }
    }

    #[test]
    fn port_audit_distinguishes_inventory_from_observed_mates() {
        let mut audit = PortAudit::default();
        audit.socket_occurrences.insert(
            DoorSocket {
                side: BoundarySide::Ceiling,
                offset: 60,
                span: 20,
            },
            2,
        );
        assert_eq!(audit.signatures_with_observed_mates(), 0);
        assert_eq!(audit.occurrences_with_observed_mates(), 0);
        audit.socket_occurrences.insert(
            DoorSocket {
                side: BoundarySide::Floor,
                offset: 60,
                span: 20,
            },
            1,
        );
        assert_eq!(audit.signatures_with_observed_mates(), 2);
        assert_eq!(audit.occurrences_with_observed_mates(), 3);
        assert!(audit.unmatched_signatures().is_empty());
    }

    #[test]
    fn summary_render_is_deterministic_and_keeps_failures_typed() {
        let mut summary = AuditSummary {
            attempted_rooms: 2,
            constructed_rooms: 1,
            ..AuditSummary::default()
        };
        summary
            .generation_failures
            .insert("embedding:port-contract", 1);
        summary
            .inconclusive_classes
            .insert("both:door:path-horizon".to_owned(), 2);
        assert_eq!(summary.render(), summary.clone().render());
        assert!(summary.render().contains("embedding:port-contract"));
        assert!(summary.render().contains("both:door:path-horizon"));
    }

    #[test]
    fn directional_audit_separates_reachability_from_demand_asymmetry() {
        let door_ids = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let mut demands = BTreeMap::new();
        demands.insert(
            ("a".to_owned(), "b".to_owned()),
            CanonicalDemand {
                duration_ticks: 10,
                ..CanonicalDemand::default()
            },
        );
        demands.insert(
            ("b".to_owned(), "a".to_owned()),
            CanonicalDemand {
                duration_ticks: 12,
                ..CanonicalDemand::default()
            },
        );
        demands.insert(("a".to_owned(), "c".to_owned()), CanonicalDemand::default());
        let mut aggregate = DirectionalAggregate::default();
        aggregate.observe(&demands, &door_ids);
        assert_eq!(aggregate.expected_pairs, 3);
        assert_eq!(aggregate.bidirectional_pairs, 1);
        assert_eq!(aggregate.one_direction_positive_pairs, 1);
        assert_eq!(aggregate.neither_direction_positive_pairs, 1);
        assert_eq!(aggregate.asymmetric_pairs, 1);
        assert_eq!(aggregate.total_duration_difference, 2);
    }
}
