//! Authoritative bounded smoke audit for the recursive-partition generator.
//!
//! A solver non-success is reported as inconclusive evidence and never as an
//! unreachable route. This executable does not rewrite or relax geometry.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
};

use downwards_ai::SolverConfig;
use downwards_core::{AbilitySet, Action, BoundarySide, Simulation, Tile};
use downwards_gen::{
    GeneratedLevel,
    experimental::{
        ChallengeIntent, PARTITION_ROUTE_GENERATION_VERSION, PartitionRouteCandidate,
        PartitionRouteKey, PartitionRouteProfile,
    },
};
use downwards_lab::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, TraversalGrid, TraversalTrace,
    observe_successful_replay,
};
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
use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};
use structural::{describe_port_path, describe_terrain_utility};

const USAGE: &str = "usage: cargo run --bin partition_route_experiment -- <start-seed> <seed-count> [baseline|wall-jump|dash|both|all] [mixed|columnar|branching|all] [gentle|standard|technical|all] [embedding-attempt]";

#[derive(Clone, Copy, Debug, Default)]
struct MatrixAggregate {
    expected_doors: usize,
    positive_doors: usize,
    expected_pickups: usize,
    positive_pickups: usize,
    positive_rooms: usize,
}

impl MatrixAggregate {
    fn observe(&mut self, batch: &DoorTargetEvidenceBatch) -> bool {
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
        self.expected_doors += batch.door_routes().len();
        self.positive_doors += positive_doors;
        self.expected_pickups += batch.pickup_routes().len();
        self.positive_pickups += positive_pickups;
        let all_positive = positive_doors == batch.door_routes().len()
            && positive_pickups == batch.pickup_routes().len();
        self.positive_rooms += usize::from(all_positive);
        all_positive
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct DirectAggregate {
    expected_routes: usize,
    exact_known_routes: usize,
    complete_without_positive: usize,
    bounded_without_positive: usize,
    run_only_routes: usize,
    monotone_simple_only_routes: usize,
    other_routes: usize,
    horizontal_reversals: usize,
    vertical_decisions: usize,
    duration_ticks: usize,
}

impl DirectAggregate {
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
    }
}

#[derive(Debug, Default)]
struct AuditAggregate {
    attempted_rooms: usize,
    constructed_rooms: usize,
    hard_gate_positive_rooms: usize,
    construction: MatrixAggregate,
    both: MatrixAggregate,
    construction_direct: DirectAggregate,
    both_direct: DirectAggregate,
    directional: DirectionalAggregate,
    graph_signatures: BTreeSet<u64>,
    route_signatures: BTreeSet<u64>,
    derivation_fingerprints: BTreeSet<u64>,
    coordinate_route_signatures: BTreeSet<u64>,
    terrain_components: usize,
    interior_terrain_tiles: usize,
    statically_attributed_terrain_tiles: usize,
    positively_corroborated_terrain_tiles: usize,
    uncorroborated_terrain_components: usize,
    uncorroborated_terrain_tiles: usize,
    floor_ports_seen: usize,
    floor_entries_checked: usize,
    floor_immediate_self_triggers: usize,
    floor_matrix_source_rows: usize,
    construction_inconclusive_causes: BTreeMap<String, usize>,
    both_inconclusive_causes: BTreeMap<String, usize>,
    construction_failures: BTreeMap<String, usize>,
}

impl AuditAggregate {
    fn observe_candidate_identity(&mut self, candidate: &PartitionRouteCandidate) {
        self.graph_signatures
            .insert(candidate.derivation.graph_topology_signature);
        self.route_signatures
            .insert(candidate.derivation.route_derivation_signature);
        self.derivation_fingerprints
            .insert(candidate.derivation.derivation_fingerprint);
        self.coordinate_route_signatures
            .insert(candidate.route_summary.signature);
    }

    fn observe_terrain(
        &mut self,
        candidate: &PartitionRouteCandidate,
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

    fn print(&self, loadout_label: &str, profile_label: &str, intent_label: &str, attempt: u8) {
        println!(
            concat!(
                "source=partition-route-v{} loadouts={} profiles={} intents={} attempt={} ",
                "attempted={} constructed={} hard-gate-positive={} ",
                "construction=rooms:{}/{} doors:{}/{} pickups:{}/{} ",
                "both=rooms:{}/{} doors:{}/{} pickups:{}/{} ",
                "construction-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "both-direct=known:{}/{} complete-no-positive:{} bounded-no-positive:{} ",
                "classes:run:{} monotone-only:{} other:{} reversals:{} vertical:{} duration:{} ",
                "directional=pairs:{} asymmetric:{} duration-diff-total:{} duration-diff-max:{} ",
                "reversal-diff-total:{} reversal-diff-max:{} ",
                "diversity=graph:{} route:{} full:{} coordinate-route:{} ",
                "terrain=components:{} interior:{} static-attributed:{} positive-corroborated:{} ",
                "uncorroborated-components:{} uncorroborated-tiles:{} ",
                "floor-smoke=ports:{} entries:{} immediate-self-trigger:{} source-rows:{}"
            ),
            PARTITION_ROUTE_GENERATION_VERSION,
            loadout_label,
            profile_label,
            intent_label,
            attempt,
            self.attempted_rooms,
            self.constructed_rooms,
            self.hard_gate_positive_rooms,
            self.construction.positive_rooms,
            self.constructed_rooms,
            self.construction.positive_doors,
            self.construction.expected_doors,
            self.construction.positive_pickups,
            self.construction.expected_pickups,
            self.both.positive_rooms,
            self.constructed_rooms,
            self.both.positive_doors,
            self.both.expected_doors,
            self.both.positive_pickups,
            self.both.expected_pickups,
            self.construction_direct.exact_known_routes,
            self.construction_direct.expected_routes,
            self.construction_direct.complete_without_positive,
            self.construction_direct.bounded_without_positive,
            self.construction_direct.run_only_routes,
            self.construction_direct.monotone_simple_only_routes,
            self.construction_direct.other_routes,
            self.construction_direct.horizontal_reversals,
            self.construction_direct.vertical_decisions,
            self.construction_direct.duration_ticks,
            self.both_direct.exact_known_routes,
            self.both_direct.expected_routes,
            self.both_direct.complete_without_positive,
            self.both_direct.bounded_without_positive,
            self.both_direct.run_only_routes,
            self.both_direct.monotone_simple_only_routes,
            self.both_direct.other_routes,
            self.both_direct.horizontal_reversals,
            self.both_direct.vertical_decisions,
            self.both_direct.duration_ticks,
            self.directional.bidirectional_pairs,
            self.directional.asymmetric_pairs,
            self.directional.total_duration_difference,
            self.directional.maximum_duration_difference,
            self.directional.total_reversal_difference,
            self.directional.maximum_reversal_difference,
            self.graph_signatures.len(),
            self.route_signatures.len(),
            self.derivation_fingerprints.len(),
            self.coordinate_route_signatures.len(),
            self.terrain_components,
            self.interior_terrain_tiles,
            self.statically_attributed_terrain_tiles,
            self.positively_corroborated_terrain_tiles,
            self.uncorroborated_terrain_components,
            self.uncorroborated_terrain_tiles,
            self.floor_ports_seen,
            self.floor_entries_checked,
            self.floor_immediate_self_triggers,
            self.floor_matrix_source_rows,
        );
        print_causes(
            "construction-inconclusive-causes",
            &self.construction_inconclusive_causes,
        );
        print_causes("both-inconclusive-causes", &self.both_inconclusive_causes);
        print_causes("construction-failures", &self.construction_failures);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [start_seed, seed_count, tail @ ..] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    if tail.len() > 4 {
        return Err(USAGE.into());
    }
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let (loadouts, loadout_label) = parse_loadouts(tail.first().map(String::as_str))?;
    let (profiles, profile_label) = parse_profiles(tail.get(1).map(String::as_str))?;
    let (intents, intent_label) = parse_intents(tail.get(2).map(String::as_str))?;
    let embedding_attempt = tail.get(3).map_or(Ok(0), |value| value.parse::<u8>())?;
    let mut aggregate = AuditAggregate::default();

    for &loadout in loadouts {
        for &profile in profiles {
            for &intent in intents {
                for offset in 0..seed_count {
                    let seed = start_seed.wrapping_add(offset as u64);
                    aggregate.attempted_rooms += 1;
                    let key = PartitionRouteKey::new(seed, loadout.abilities(), intent, profile)
                        .with_embedding_attempt(embedding_attempt);
                    let candidate = match key.regenerate() {
                        Ok(candidate) => candidate,
                        Err(error) => {
                            *aggregate
                                .construction_failures
                                .entry(format!("{:?}", error.cause))
                                .or_default() += 1;
                            eprintln!(
                                "construct-failure loadout={} profile={} intent={} seed={seed}: {error}",
                                loadout.slug(),
                                profile.slug(),
                                intent.slug(),
                            );
                            continue;
                        }
                    };
                    aggregate.constructed_rooms += 1;
                    aggregate.observe_candidate_identity(&candidate);
                    inspect_floor_entry(&candidate, loadout.abilities(), &mut aggregate)?;

                    let construction_matrix = evaluate_generated_door_targets_for_loadout(
                        &candidate.generated,
                        loadout.abilities(),
                        &ValidationConfig::for_loadout(loadout.abilities()),
                    )?;
                    inspect_matrix_shape(&candidate.generated, &construction_matrix)?;
                    observe_floor_source_rows(&candidate, &construction_matrix, &mut aggregate);
                    let construction_positive =
                        aggregate.construction.observe(&construction_matrix);
                    observe_inconclusive_causes(
                        &construction_matrix,
                        &mut aggregate.construction_inconclusive_causes,
                    );

                    let both_matrix = if loadout == EvaluationLoadout::Both {
                        construction_matrix.clone()
                    } else {
                        evaluate_generated_door_targets_for_loadout(
                            &candidate.generated,
                            AbilitySet::ALL,
                            &ValidationConfig::for_loadout(AbilitySet::ALL),
                        )?
                    };
                    inspect_matrix_shape(&candidate.generated, &both_matrix)?;
                    let both_positive = aggregate.both.observe(&both_matrix);
                    observe_inconclusive_causes(
                        &both_matrix,
                        &mut aggregate.both_inconclusive_causes,
                    );
                    aggregate.hard_gate_positive_rooms +=
                        usize::from(construction_positive && both_positive);

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
                    let canonical = observe_canonical_both(&candidate.generated, &both_matrix)?;
                    aggregate.directional.observe(&canonical.demands);
                    aggregate.observe_terrain(&candidate, &canonical.traversals)?;

                    if !(construction_positive && both_positive) {
                        eprintln!(
                            "hard-gate-inconclusive loadout={} profile={} intent={} seed={seed} construction={} both={}",
                            loadout.slug(),
                            profile.slug(),
                            intent.slug(),
                            construction_positive,
                            both_positive,
                        );
                        report_inconclusive_targets(&construction_matrix, "construction");
                        report_inconclusive_targets(&both_matrix, "both");
                        report_ceiling_structure(&candidate, &construction_matrix)?;
                    }
                }
            }
        }
        eprintln!("completed construction-loadout={}", loadout.slug());
    }

    aggregate.print(
        loadout_label,
        profile_label,
        intent_label,
        embedding_attempt,
    );
    Ok(())
}

fn parse_loadouts(
    value: Option<&str>,
) -> Result<(&'static [EvaluationLoadout], &'static str), Box<dyn Error>> {
    Ok(match value {
        None | Some("baseline") => (&[EvaluationLoadout::Baseline], "baseline"),
        Some("wall-jump") => (&[EvaluationLoadout::WallJump], "wall-jump"),
        Some("dash") => (&[EvaluationLoadout::Dash], "dash"),
        Some("both") => (&[EvaluationLoadout::Both], "both"),
        Some("all") => (&EvaluationLoadout::ALL, "all"),
        Some(_) => return Err(USAGE.into()),
    })
}

fn parse_profiles(
    value: Option<&str>,
) -> Result<(&'static [PartitionRouteProfile], &'static str), Box<dyn Error>> {
    Ok(match value {
        None | Some("mixed") => (&[PartitionRouteProfile::MixedBsp], "mixed"),
        Some("columnar") => (&[PartitionRouteProfile::Columnar], "columnar"),
        Some("branching") => (&[PartitionRouteProfile::Branching], "branching"),
        Some("all") => (&PartitionRouteProfile::ALL, "all"),
        Some(_) => return Err(USAGE.into()),
    })
}

fn parse_intents(
    value: Option<&str>,
) -> Result<(&'static [ChallengeIntent], &'static str), Box<dyn Error>> {
    Ok(match value {
        None | Some("all") => (&ChallengeIntent::ALL, "all"),
        Some("gentle") => (&[ChallengeIntent::Gentle], "gentle"),
        Some("standard") => (&[ChallengeIntent::Standard], "standard"),
        Some("technical") => (&[ChallengeIntent::Technical], "technical"),
        Some(_) => return Err(USAGE.into()),
    })
}

fn inspect_matrix_shape(
    generated: &GeneratedLevel,
    matrix: &DoorTargetEvidenceBatch,
) -> Result<(), Box<dyn Error>> {
    let door_count = generated.room.doors().len();
    let pickup_count = generated.room.pickups().len();
    let expected_doors = door_count * door_count.saturating_sub(1);
    let expected_pickups = door_count * pickup_count;
    if matrix.door_routes().len() != expected_doors
        || matrix.pickup_routes().len() != expected_pickups
        || matrix.source_search_effort().len() != door_count
    {
        return Err(format!(
            "matrix shape mismatch for {}: doors {}/{expected_doors}, pickups {}/{expected_pickups}, sources {}/{door_count}",
            generated.room.id(),
            matrix.door_routes().len(),
            matrix.pickup_routes().len(),
            matrix.source_search_effort().len(),
        )
        .into());
    }
    for source in generated.room.doors() {
        let source_doors = matrix
            .door_routes()
            .iter()
            .filter(|row| row.source_door_id == source.id)
            .count();
        let source_pickups = matrix
            .pickup_routes()
            .iter()
            .filter(|row| row.source_door_id == source.id)
            .count();
        let contains_self = matrix
            .door_routes()
            .iter()
            .any(|row| row.source_door_id == source.id && row.target_door_id == source.id);
        if source_doors != door_count - 1 || source_pickups != pickup_count || contains_self {
            return Err(format!(
                "source-row suppression mismatch for {} door {:?}: door rows {}/{}, pickup rows {}/{}, contains-self={contains_self}",
                generated.room.id(),
                source.id,
                source_doors,
                door_count - 1,
                source_pickups,
                pickup_count,
            )
            .into());
        }
    }
    Ok(())
}

fn inspect_floor_entry(
    candidate: &PartitionRouteCandidate,
    abilities: AbilitySet,
    aggregate: &mut AuditAggregate,
) -> Result<(), Box<dyn Error>> {
    let Some(floor) = candidate
        .generated
        .room
        .doors()
        .iter()
        .find(|door| door.side == BoundarySide::Floor)
    else {
        return Ok(());
    };
    aggregate.floor_ports_seen += 1;
    let arrival_bounds = downwards_core::Rect::new(
        floor.arrival.x,
        floor.arrival.y,
        downwards_core::PLAYER_WIDTH,
        downwards_core::PLAYER_HEIGHT,
    );
    if arrival_bounds.intersects(floor.trigger_bounds) {
        return Err(format!(
            "floor arrival overlaps its trigger in {}",
            candidate.generated.room.id()
        )
        .into());
    }
    let mut simulation =
        Simulation::enter_via_door(candidate.generated.room.clone(), abilities, &floor.id)?;
    if simulation.entry_door() != Some(floor.id.as_str()) {
        return Err(format!(
            "floor entry identity was not retained in {}",
            candidate.generated.room.id()
        )
        .into());
    }
    aggregate.floor_entries_checked += 1;
    simulation.step(Action::default());
    if simulation.reached_exit() == Some(floor.id.as_str()) {
        aggregate.floor_immediate_self_triggers += 1;
        return Err(format!(
            "floor source immediately retriggered itself in {}",
            candidate.generated.room.id()
        )
        .into());
    }
    Ok(())
}

fn observe_floor_source_rows(
    candidate: &PartitionRouteCandidate,
    matrix: &DoorTargetEvidenceBatch,
    aggregate: &mut AuditAggregate,
) {
    let Some(floor) = candidate
        .generated
        .room
        .doors()
        .iter()
        .find(|door| door.side == BoundarySide::Floor)
    else {
        return;
    };
    aggregate.floor_matrix_source_rows += matrix
        .source_search_effort()
        .iter()
        .filter(|row| row.source_door_id == floor.id)
        .count();
}

fn observe_direct_demand(
    generated: &GeneratedLevel,
    loadout: EvaluationLoadout,
    aggregate: &mut DirectAggregate,
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
                .expect("the exact requested loadout is always audited");
            if audit.status == LoadoutControllerAuditStatus::CompleteFiniteVocabulary {
                aggregate.complete_without_positive += 1;
            } else {
                aggregate.bounded_without_positive += 1;
            }
        }
    }
    Ok(())
}

fn observe_canonical_both(
    generated: &GeneratedLevel,
    matrix: &DoorTargetEvidenceBatch,
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
        demands.insert(
            (row.source_door_id.clone(), row.target_door_id.clone()),
            canonical_demand(&observation.actions),
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

fn observe_inconclusive_causes(
    matrix: &DoorTargetEvidenceBatch,
    causes: &mut BTreeMap<String, usize>,
) {
    for evidence in matrix
        .door_routes()
        .iter()
        .map(|row| &row.evidence)
        .chain(matrix.pickup_routes().iter().map(|row| &row.evidence))
    {
        if let Some(inconclusive) = evidence.inconclusive() {
            *causes
                .entry(format!("{:?}", inconclusive.reason))
                .or_default() += 1;
        }
    }
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

fn report_ceiling_structure(
    candidate: &PartitionRouteCandidate,
    matrix: &DoorTargetEvidenceBatch,
) -> Result<(), Box<dyn Error>> {
    let Some(ceiling) = candidate
        .boundary_ports
        .iter()
        .find(|port| port.door.side == BoundarySide::Ceiling)
    else {
        return Ok(());
    };
    let ceiling_support = candidate.route_plan.nodes[usize::from(ceiling.node_id)].support;
    let connector_anchor = ceiling_connector_anchor(candidate, ceiling.node_id);
    eprintln!(
        "  ceiling-structure ceiling-slot={} floor-slot={:?} node={} support={:?} socket={:?} trigger={:?} arrival={:?} connector-anchor={:?}",
        candidate.derivation.vertical_socket_slot,
        candidate.derivation.floor_socket_slot,
        ceiling.node_id,
        ceiling_support,
        ceiling.door.socket(),
        ceiling.door.trigger_bounds,
        ceiling.door.arrival,
        connector_anchor.map(|node_id| (
            node_id,
            candidate.route_plan.nodes[usize::from(node_id)].support,
        )),
    );
    for row in matrix.door_routes().iter().filter(|row| {
        row.target_door_id == ceiling.door.id && row.evidence.inconclusive().is_some()
    }) {
        let path = describe_port_path(
            &candidate.route_plan,
            &candidate.boundary_ports,
            &row.source_door_id,
            &row.target_door_id,
        )?;
        eprintln!(
            "    segmented {} -> {} cost={:?} nodes={:?}",
            row.source_door_id, row.target_door_id, path.cost, path.node_path,
        );
        for (index, node_id) in path.node_path.iter().enumerate() {
            let node = &candidate.route_plan.nodes[usize::from(*node_id)];
            let overhead = overhead_tiles(&candidate.generated, node.support);
            if let Some(step) = index.checked_sub(1).and_then(|index| path.steps.get(index)) {
                eprintln!(
                    "      node={} role={:?} support={:?} overhead={} via={:?}/{:?} critical={}",
                    node.id,
                    node.role,
                    node.support,
                    overhead,
                    step.declared_verb,
                    step.traversal_verb,
                    step.critical,
                );
            } else {
                eprintln!(
                    "      node={} role={:?} support={:?} overhead={} source",
                    node.id, node.role, node.support, overhead,
                );
            }
        }
    }
    Ok(())
}

fn ceiling_connector_anchor(
    candidate: &PartitionRouteCandidate,
    ceiling_node_id: u16,
) -> Option<u16> {
    let mut cursor = ceiling_node_id;
    for _ in 0..candidate.route_plan.nodes.len() {
        let edge = candidate
            .route_plan
            .edges
            .iter()
            .rev()
            .find(|edge| edge.to == cursor)?;
        if edge.from < ceiling_node_id {
            return Some(edge.from);
        }
        cursor = edge.from;
    }
    None
}

fn overhead_tiles(
    generated: &GeneratedLevel,
    support: downwards_gen::experimental::SupportSpec,
) -> String {
    let mut result = String::new();
    for row in [support.row.saturating_sub(2), support.row.saturating_sub(1)] {
        for x in support.start_x..support.end_x {
            result.push(match generated.room.tile(x, row) {
                Some(Tile::Empty) => '.',
                Some(Tile::Solid) => '#',
                Some(Tile::OneWay) => '=',
                Some(Tile::HazardUp) => '^',
                Some(Tile::HazardDown) => 'v',
                Some(Tile::HazardLeft) => '<',
                Some(Tile::HazardRight) => '>',
                None => '?',
            });
        }
        result.push('/');
    }
    result
}

fn print_causes(label: &str, causes: &BTreeMap<String, usize>) {
    if causes.is_empty() {
        println!("{label}=none");
        return;
    }
    for (cause, count) in causes {
        println!("{label} count={count} cause={cause}");
    }
}
