//! Authoritative bounded audit for the independent switchback-cut grammar.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
};

use downwards_ai::{SearchTarget, SolverConfig, audit_direct_controller_probes};
use downwards_core::{AbilitySet, Simulation, Tile};
use downwards_gen::GeneratedLevel;
use downwards_gen::experimental::{
    ChallengeIntent, RoutePlan, SwitchbackCutGrammar, SwitchbackCutKey,
};
use downwards_lab::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, SimulationGeometryDescriptor,
    StaticVisualDescriptor, TraversalGrid, TraversalTrace, observe_successful_replay,
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
#[path = "../structural.rs"]
#[allow(dead_code)]
mod structural;
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};
use route_assessment::{
    ControllerDemand, LoadoutControllerAuditStatus, assess_easiest_known_routes_from_source,
};
use structural::describe_terrain_utility;

const USAGE: &str = "usage: cargo run --bin switchback_cut_experiment -- <start-seed> <seed-count> [baseline|both|all] [embedding-attempt]";

#[derive(Clone, Copy, Debug, Default)]
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
struct CanonicalDemand {
    horizontal_reversals: usize,
    vertical_decisions: usize,
    duration_ticks: usize,
}

struct CanonicalObservation {
    demands: BTreeMap<(String, String), CanonicalDemand>,
    traversals: Vec<TraversalTrace>,
}

#[derive(Clone, Copy, Debug, Default)]
struct DirectionalAggregate {
    bidirectional_pairs: usize,
    asymmetric_pairs: usize,
    total_duration_difference: usize,
    maximum_duration_difference: usize,
    total_reversal_difference: usize,
    maximum_reversal_difference: usize,
    bottom_to_ceiling_routes: usize,
    bottom_to_ceiling_reversals: usize,
    bottom_to_ceiling_vertical_decisions: usize,
    bottom_to_ceiling_duration: usize,
    ceiling_to_bottom_routes: usize,
    ceiling_to_bottom_reversals: usize,
    ceiling_to_bottom_vertical_decisions: usize,
    ceiling_to_bottom_duration: usize,
}

impl DirectionalAggregate {
    fn observe(&mut self, demands: &BTreeMap<(String, String), CanonicalDemand>) {
        for ((source, target), forward) in demands {
            if source >= target {
                continue;
            }
            let Some(reverse) = demands.get(&(target.clone(), source.clone())) else {
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
        if let Some(demand) = demands.get(&("port-bottom".to_owned(), "port-ceiling".to_owned())) {
            self.bottom_to_ceiling_routes += 1;
            self.bottom_to_ceiling_reversals += demand.horizontal_reversals;
            self.bottom_to_ceiling_vertical_decisions += demand.vertical_decisions;
            self.bottom_to_ceiling_duration += demand.duration_ticks;
        }
        if let Some(demand) = demands.get(&("port-ceiling".to_owned(), "port-bottom".to_owned())) {
            self.ceiling_to_bottom_routes += 1;
            self.ceiling_to_bottom_reversals += demand.horizontal_reversals;
            self.ceiling_to_bottom_vertical_decisions += demand.vertical_decisions;
            self.ceiling_to_bottom_duration += demand.duration_ticks;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Aggregate {
    attempted_rooms: usize,
    constructed_rooms: usize,
    construction_all_target_positive_rooms: usize,
    complete_kit_all_target_positive_rooms: usize,
    hard_gate_positive_rooms: usize,
    construction_expected_doors: usize,
    construction_positive_doors: usize,
    construction_expected_pickups: usize,
    construction_positive_pickups: usize,
    both_expected_doors: usize,
    both_positive_doors: usize,
    both_expected_pickups: usize,
    both_positive_pickups: usize,
    construction_direct: RouteDemandAggregate,
    both_direct: RouteDemandAggregate,
    canonical_both_routes: usize,
    canonical_both_reversals: usize,
    canonical_both_vertical_decisions: usize,
    canonical_both_duration: usize,
    directional: DirectionalAggregate,
    solid_tiles: usize,
    one_way_tiles: usize,
    terrain_components: usize,
    interior_terrain_tiles: usize,
    statically_attributed_terrain_tiles: usize,
    positively_corroborated_terrain_tiles: usize,
    uncorroborated_terrain_components: usize,
    uncorroborated_terrain_tiles: usize,
}

#[derive(Debug, Default)]
struct Expressivity {
    route_signatures: BTreeSet<u64>,
    static_visual_fingerprints: BTreeSet<u64>,
    simulation_geometry_fingerprints: BTreeSet<u64>,
}

impl Expressivity {
    fn observe(&mut self, route_plan: &RoutePlan, generated: &GeneratedLevel) {
        self.route_signatures.insert(route_plan.summary().signature);
        self.static_visual_fingerprints
            .insert(fingerprint_static_visual(
                &StaticVisualDescriptor::from_room(&generated.room),
            ));
        self.simulation_geometry_fingerprints
            .insert(SimulationGeometryDescriptor::from_room(&generated.room).stable_digest());
    }
}

impl Aggregate {
    fn observe_matrix(&mut self, batch: &DoorTargetEvidenceBatch, construction: bool) -> bool {
        let positive_doors = batch
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        let positive_pickups = batch
            .pickup_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        if construction {
            self.construction_expected_doors += batch.door_routes().len();
            self.construction_positive_doors += positive_doors;
            self.construction_expected_pickups += batch.pickup_routes().len();
            self.construction_positive_pickups += positive_pickups;
        } else {
            self.both_expected_doors += batch.door_routes().len();
            self.both_positive_doors += positive_doors;
            self.both_expected_pickups += batch.pickup_routes().len();
            self.both_positive_pickups += positive_pickups;
        }
        positive_doors == batch.door_routes().len()
            && positive_pickups == batch.pickup_routes().len()
    }

    fn observe_terrain(
        &mut self,
        generated: &GeneratedLevel,
        route_plan: &RoutePlan,
        traversals: &[TraversalTrace],
    ) -> Result<(), Box<dyn Error>> {
        let traversal_refs = traversals.iter().collect::<Vec<_>>();
        let terrain = describe_terrain_utility(&generated.room, route_plan, &traversal_refs)?;
        self.terrain_components += terrain.components.len();
        self.interior_terrain_tiles += terrain.interior_terrain_tiles;
        self.statically_attributed_terrain_tiles += terrain.static_attributed_tiles;
        self.positively_corroborated_terrain_tiles += terrain.positively_corroborated_tiles;
        self.uncorroborated_terrain_components += terrain.components_without_positive_corroboration;
        self.uncorroborated_terrain_tiles += terrain.tiles_without_positive_corroboration;
        Ok(())
    }

    fn print(self, loadout_label: &str, attempt: u8, expressivity: &Expressivity) {
        println!(
            concat!(
                "grammar=shelf-return-v1 attempt={} construction-loadouts={} ",
                "attempted={} constructed={} construction-all-target-positive={} ",
                "both-all-target-positive={} hard-gate-positive={} ",
                "construction-doors={}/{} construction-pickups={}/{} ",
                "both-doors={}/{} both-pickups={}/{} ",
                "construction-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "both-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "canonical-both=routes:{} reversals:{} vertical:{} duration:{} ",
                "directional=pairs:{} asymmetric:{} duration-diff-total:{} duration-diff-max:{} ",
                "reversal-diff-total:{} reversal-diff-max:{} ",
                "bottom-ceiling=routes:{} reversals:{} vertical:{} duration:{} ",
                "ceiling-bottom=routes:{} reversals:{} vertical:{} duration:{} ",
                "expressivity=route-signatures:{} static-visuals:{} simulation-geometries:{} ",
                "terrain=solid:{} one-way:{} components:{} interior:{} static-attributed:{} ",
                "positive-corroborated:{} uncorroborated-components:{} uncorroborated-tiles:{}"
            ),
            attempt,
            loadout_label,
            self.attempted_rooms,
            self.constructed_rooms,
            self.construction_all_target_positive_rooms,
            self.complete_kit_all_target_positive_rooms,
            self.hard_gate_positive_rooms,
            self.construction_positive_doors,
            self.construction_expected_doors,
            self.construction_positive_pickups,
            self.construction_expected_pickups,
            self.both_positive_doors,
            self.both_expected_doors,
            self.both_positive_pickups,
            self.both_expected_pickups,
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
            self.directional.bidirectional_pairs,
            self.directional.asymmetric_pairs,
            self.directional.total_duration_difference,
            self.directional.maximum_duration_difference,
            self.directional.total_reversal_difference,
            self.directional.maximum_reversal_difference,
            self.directional.bottom_to_ceiling_routes,
            self.directional.bottom_to_ceiling_reversals,
            self.directional.bottom_to_ceiling_vertical_decisions,
            self.directional.bottom_to_ceiling_duration,
            self.directional.ceiling_to_bottom_routes,
            self.directional.ceiling_to_bottom_reversals,
            self.directional.ceiling_to_bottom_vertical_decisions,
            self.directional.ceiling_to_bottom_duration,
            expressivity.route_signatures.len(),
            expressivity.static_visual_fingerprints.len(),
            expressivity.simulation_geometry_fingerprints.len(),
            self.solid_tiles,
            self.one_way_tiles,
            self.terrain_components,
            self.interior_terrain_tiles,
            self.statically_attributed_terrain_tiles,
            self.positively_corroborated_terrain_tiles,
            self.uncorroborated_terrain_components,
            self.uncorroborated_terrain_tiles,
        );
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [start_seed, seed_count, tail @ ..] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    if tail.len() > 2 {
        return Err(USAGE.into());
    }
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let (loadouts, loadout_label): (&[EvaluationLoadout], &str) =
        match tail.first().map(String::as_str) {
            None | Some("baseline") => (&[EvaluationLoadout::Baseline], "baseline"),
            Some("both") => (&[EvaluationLoadout::Both], "both"),
            Some("all") => (&EvaluationLoadout::ALL, "all"),
            Some(_) => return Err(USAGE.into()),
        };
    let embedding_attempt = tail.get(1).map_or(Ok(0), |value| value.parse::<u8>())?;
    let mut aggregate = Aggregate::default();
    let mut expressivity = Expressivity::default();

    for &loadout in loadouts {
        for intent in ChallengeIntent::ALL {
            for offset in 0..seed_count {
                let seed = start_seed.wrapping_add(offset as u64);
                aggregate.attempted_rooms += 1;
                let key = SwitchbackCutKey::new(seed, loadout.abilities(), intent)
                    .with_embedding(SwitchbackCutGrammar::ShelfReturnV1, embedding_attempt);
                let candidate = match key.regenerate() {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        eprintln!(
                            "construct-failure loadout={} intent={} seed={seed}: {error}",
                            loadout.slug(),
                            intent.slug(),
                        );
                        continue;
                    }
                };
                aggregate.constructed_rooms += 1;
                expressivity.observe(&candidate.route_plan, &candidate.generated);
                aggregate.solid_tiles += candidate
                    .generated
                    .room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile == Tile::Solid)
                    .count();
                aggregate.one_way_tiles += candidate
                    .generated
                    .room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile == Tile::OneWay)
                    .count();

                let construction_matrix = evaluate_generated_door_targets_for_loadout(
                    &candidate.generated,
                    loadout.abilities(),
                    &ValidationConfig::for_loadout(loadout.abilities()),
                )?;
                let construction_positive = aggregate.observe_matrix(&construction_matrix, true);
                aggregate.construction_all_target_positive_rooms +=
                    usize::from(construction_positive);
                let both_matrix = if loadout == EvaluationLoadout::Both {
                    construction_matrix.clone()
                } else {
                    evaluate_generated_door_targets_for_loadout(
                        &candidate.generated,
                        AbilitySet::ALL,
                        &ValidationConfig::for_loadout(AbilitySet::ALL),
                    )?
                };
                let both_positive = aggregate.observe_matrix(&both_matrix, false);
                aggregate.complete_kit_all_target_positive_rooms += usize::from(both_positive);
                observe_direct_demand(
                    &candidate.generated,
                    loadout,
                    &mut aggregate.construction_direct,
                )?;
                observe_direct_demand(
                    &candidate.generated,
                    EvaluationLoadout::Both,
                    &mut aggregate.both_direct,
                )?;
                if !(construction_positive && both_positive) {
                    eprintln!(
                        "hard-gate-inconclusive loadout={} intent={} seed={seed} construction={} both={}",
                        loadout.slug(),
                        intent.slug(),
                        construction_positive,
                        both_positive,
                    );
                    report_inconclusive_targets(&construction_matrix, "construction");
                    report_inconclusive_targets(&both_matrix, "both");
                    report_bottom_direct_targets(&candidate.generated, loadout)?;
                    continue;
                }
                aggregate.hard_gate_positive_rooms += 1;
                let canonical =
                    observe_canonical_both(&candidate.generated, &both_matrix, &mut aggregate)?;
                aggregate.directional.observe(&canonical.demands);
                aggregate.observe_terrain(
                    &candidate.generated,
                    &candidate.route_plan,
                    &canonical.traversals,
                )?;
            }
        }
        eprintln!("completed construction-loadout={}", loadout.slug());
    }

    aggregate.print(loadout_label, embedding_attempt, &expressivity);
    Ok(())
}

fn report_inconclusive_targets(matrix: &DoorTargetEvidenceBatch, label: &str) {
    for row in matrix.door_routes() {
        if let Some(inconclusive) = row.evidence.inconclusive() {
            eprintln!(
                "  {label}-door-inconclusive {} -> {} reason={:?}",
                row.source_door_id, row.target_door_id, inconclusive.reason,
            );
            eprintln!(
                "    effort=expanded:{} generated:{} simulated:{} deepest:{}",
                inconclusive.search_effort.expanded_nodes,
                inconclusive.search_effort.generated_nodes,
                inconclusive.search_effort.simulated_ticks,
                inconclusive.search_effort.deepest_path_ticks,
            );
        }
    }
    for row in matrix.pickup_routes() {
        if let Some(inconclusive) = row.evidence.inconclusive() {
            eprintln!(
                "  {label}-pickup-inconclusive {} -> {} reason={:?}",
                row.source_door_id, row.required_pickup_id, inconclusive.reason,
            );
            eprintln!(
                "    effort=expanded:{} generated:{} simulated:{} deepest:{}",
                inconclusive.search_effort.expanded_nodes,
                inconclusive.search_effort.generated_nodes,
                inconclusive.search_effort.simulated_ticks,
                inconclusive.search_effort.deepest_path_ticks,
            );
        }
    }
}

fn report_bottom_direct_targets(
    generated: &GeneratedLevel,
    loadout: EvaluationLoadout,
) -> Result<(), Box<dyn Error>> {
    let initial =
        Simulation::enter_via_door(generated.room.clone(), loadout.abilities(), "port-bottom")?;
    let targets = [
        SearchTarget::door("port-ceiling"),
        SearchTarget::pickup("switchback-cache"),
    ];
    let audit = audit_direct_controller_probes(
        &initial,
        &targets,
        &SolverConfig::for_abilities(loadout.abilities()),
    )?;
    for (target_index, target) in targets.iter().enumerate() {
        let witnesses = audit
            .witnesses
            .iter()
            .filter(|witness| witness.target_index == target_index)
            .collect::<Vec<_>>();
        let shortest = witnesses
            .iter()
            .map(|witness| witness.replay.frames.len())
            .min();
        eprintln!(
            "  direct-bottom target={target:?} positives={} shortest={shortest:?} status={:?}",
            witnesses.len(),
            audit.status,
        );
    }
    eprintln!(
        "    direct-effort=expanded:{} generated:{} simulated:{} deepest:{}",
        audit.stats.expanded_nodes,
        audit.stats.generated_nodes,
        audit.stats.simulated_ticks,
        audit.stats.deepest_path_ticks,
    );
    Ok(())
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
                if source == "port-bottom" && route.target_door_id == "port-ceiling" {
                    eprintln!(
                        "  easiest-exact bottom->ceiling loadout={} demand={:?}",
                        loadout.slug(),
                        exact.demand,
                    );
                }
                aggregate.observe(exact.demand);
                continue;
            }
            let audit = route
                .audits
                .iter()
                .find(|audit| audit.loadout == loadout)
                .expect("the authoritative exact loadout is always audited");
            if source == "port-bottom" && route.target_door_id == "port-ceiling" {
                eprintln!(
                    "  easiest-exact bottom->ceiling loadout={} no-positive status={:?} effort={:?}",
                    loadout.slug(),
                    audit.status,
                    audit.operational_stats,
                );
            }
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
    aggregate: &mut Aggregate,
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
        aggregate.canonical_both_routes += 1;
        aggregate.canonical_both_reversals += demand.horizontal_reversals;
        aggregate.canonical_both_vertical_decisions += demand.vertical_decisions;
        aggregate.canonical_both_duration += demand.duration_ticks;
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
