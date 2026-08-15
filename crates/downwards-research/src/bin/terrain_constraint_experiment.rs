//! Bounded before/after audit for the opt-in grounded-route-pier experiment.

use std::{collections::BTreeMap, env, error::Error};

use downwards_ai::SolverConfig;
use downwards_core::{AbilitySet, Simulation, Tile};
use downwards_gen::{
    CompositionalFeatureSet, CompositionalKey, CompositionalProfile, GeneratedLevel,
    StagedCompositionalKey,
    experimental::{
        ChallengeIntent, GenerationStrategy, RoutePlan, TerrainConstraintExperiment,
        generate_terrain_constrained_candidate,
    },
    generate_staged_compositional,
};
use downwards_lab::{TraversalGrid, TraversalTrace, observe_successful_replay};
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
    BoundedTargetEvidence, ValidationConfig, evaluate_generated_door_targets_for_loadout,
};
use route_assessment::{LoadoutControllerAuditStatus, assess_easiest_known_routes_from_source};
use structural::describe_terrain_utility;

const USAGE: &str = "usage: cargo run --bin terrain_constraint_experiment -- <start-seed> <seed-count> [baseline|all] [cyclic|growth|rhythm|all]";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Variant {
    LegacyTerrainOnly,
    GroundedRoutePierV1,
}

impl Variant {
    const ALL: [Self; 2] = [Self::LegacyTerrainOnly, Self::GroundedRoutePierV1];

    const fn slug(self) -> &'static str {
        match self {
            Self::LegacyTerrainOnly => "legacy-terrain-only",
            Self::GroundedRoutePierV1 => "grounded-route-pier-v1",
        }
    }

    const fn report_slug(self, strategy: GenerationStrategy) -> &'static str {
        if matches!(self, Self::GroundedRoutePierV1)
            && matches!(strategy, GenerationStrategy::ReachabilityGrowth)
        {
            "unchanged-growth-control"
        } else {
            self.slug()
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Aggregate {
    attempted_rooms: usize,
    constructed_rooms: usize,
    construction_all_target_positive_rooms: usize,
    complete_kit_all_target_positive_rooms: usize,
    expected_door_routes: usize,
    authoritative_positive_door_routes: usize,
    authoritative_inconclusive_door_routes: usize,
    expected_pickup_routes: usize,
    authoritative_positive_pickup_routes: usize,
    authoritative_inconclusive_pickup_routes: usize,
    direct_known_positive_routes: usize,
    direct_complete_without_positive_routes: usize,
    direct_bounded_without_positive_routes: usize,
    run_only_routes: usize,
    monotone_simple_only_routes: usize,
    other_controller_routes: usize,
    routes_with_multiple_front_witnesses: usize,
    retained_direct_witnesses: usize,
    total_easiest_duration_ticks: usize,
    total_easiest_vertical_decisions: usize,
    total_easiest_horizontal_reversals: usize,
    solid_tiles: usize,
    one_way_tiles: usize,
    terrain_components: usize,
    interior_terrain_tiles: usize,
    statically_attributed_terrain_tiles: usize,
    positively_corroborated_terrain_tiles: usize,
    uncorroborated_terrain_components: usize,
    uncorroborated_terrain_tiles: usize,
}

impl Aggregate {
    fn observe_matrix(
        &mut self,
        generated: &GeneratedLevel,
        loadout: EvaluationLoadout,
    ) -> Result<bool, Box<dyn Error>> {
        let evidence = evaluate_generated_door_targets_for_loadout(
            generated,
            loadout.abilities(),
            &ValidationConfig::for_loadout(loadout.abilities()),
        )?;
        let positive_doors = evidence
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        let positive_pickups = evidence
            .pickup_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        if loadout == EvaluationLoadout::Both {
            self.expected_door_routes += evidence.door_routes().len();
            self.authoritative_positive_door_routes += positive_doors;
            self.authoritative_inconclusive_door_routes +=
                evidence.door_routes().len() - positive_doors;
            self.expected_pickup_routes += evidence.pickup_routes().len();
            self.authoritative_positive_pickup_routes += positive_pickups;
            self.authoritative_inconclusive_pickup_routes +=
                evidence.pickup_routes().len() - positive_pickups;
        }
        Ok(positive_doors == evidence.door_routes().len()
            && positive_pickups == evidence.pickup_routes().len())
    }

    fn observe_direct_controllers(
        &mut self,
        generated: &GeneratedLevel,
    ) -> Result<Vec<TraversalTrace>, Box<dyn Error>> {
        let mut traversals = Vec::new();
        let mut doors = generated
            .room
            .doors()
            .iter()
            .map(|door| door.id.clone())
            .collect::<Vec<_>>();
        doors.sort_unstable();
        let solver = SolverConfig::for_abilities(AbilitySet::ALL);
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
                EvaluationLoadout::Both,
                &solver,
            )?;
            for route in batch.routes {
                self.retained_direct_witnesses += route.easiest_first_witnesses.len();
                if route.easiest_known_front.len() > 1 {
                    self.routes_with_multiple_front_witnesses += 1;
                }
                let Some(easiest) = route.easiest_known() else {
                    let complete = route.audits.iter().all(|audit| {
                        audit.status == LoadoutControllerAuditStatus::CompleteFiniteVocabulary
                    });
                    if complete {
                        self.direct_complete_without_positive_routes += 1;
                    } else {
                        self.direct_bounded_without_positive_routes += 1;
                    }
                    continue;
                };
                self.direct_known_positive_routes += 1;
                if easiest.demand.run_only {
                    self.run_only_routes += 1;
                } else if easiest.demand.monotone_simple {
                    self.monotone_simple_only_routes += 1;
                } else {
                    self.other_controller_routes += 1;
                }
                self.total_easiest_duration_ticks += easiest.demand.duration_ticks;
                self.total_easiest_vertical_decisions += easiest.demand.vertical_decisions;
                self.total_easiest_horizontal_reversals += easiest.demand.horizontal_reversals;
                let initial = Simulation::enter_via_door(
                    generated.room.clone(),
                    easiest.loadout.abilities(),
                    source,
                )?;
                traversals.push(
                    observe_successful_replay(&initial, &easiest.replay, TraversalGrid::default())?
                        .traversal,
                );
            }
        }
        Ok(traversals)
    }

    fn observe_terrain(
        &mut self,
        candidate: &GeneratedWithPlan,
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

    fn print(self, variant: Variant, strategy: GenerationStrategy) {
        println!(
            concat!(
                "variant={} strategy={} attempted={} constructed={} ",
                "construction-all-target-positive={} complete-kit-all-target-positive={} ",
                "authoritative-doors={}/{} door-inconclusive={} ",
                "authoritative-pickups={}/{} pickup-inconclusive={} ",
                "direct-known={} direct-complete-no-positive={} direct-bounded-no-positive={} ",
                "classes=run:{} monotone-only:{} other:{} multiple-front:{} retained-witnesses:{} ",
                "easiest-duration-total={} vertical-decisions-total={} reversals-total={} ",
                "terrain=solid:{} one-way:{} components:{} interior:{} static-attributed:{} ",
                "positive-corroborated:{} uncorroborated-components:{} uncorroborated-tiles:{}"
            ),
            variant.report_slug(strategy),
            strategy.slug(),
            self.attempted_rooms,
            self.constructed_rooms,
            self.construction_all_target_positive_rooms,
            self.complete_kit_all_target_positive_rooms,
            self.authoritative_positive_door_routes,
            self.expected_door_routes,
            self.authoritative_inconclusive_door_routes,
            self.authoritative_positive_pickup_routes,
            self.expected_pickup_routes,
            self.authoritative_inconclusive_pickup_routes,
            self.direct_known_positive_routes,
            self.direct_complete_without_positive_routes,
            self.direct_bounded_without_positive_routes,
            self.run_only_routes,
            self.monotone_simple_only_routes,
            self.other_controller_routes,
            self.routes_with_multiple_front_witnesses,
            self.retained_direct_witnesses,
            self.total_easiest_duration_ticks,
            self.total_easiest_vertical_decisions,
            self.total_easiest_horizontal_reversals,
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
    let loadouts: &[EvaluationLoadout] = match tail.first().map(String::as_str) {
        None | Some("baseline") => &[EvaluationLoadout::Baseline],
        Some("all") => &EvaluationLoadout::ALL,
        Some(_) => return Err(USAGE.into()),
    };
    let strategies = match tail.get(1).map(String::as_str) {
        None | Some("all") => GenerationStrategy::ALL.to_vec(),
        Some("cyclic") => vec![GenerationStrategy::CyclicGraph],
        Some("growth") => vec![GenerationStrategy::ReachabilityGrowth],
        Some("rhythm") => vec![GenerationStrategy::RhythmWeave],
        Some(_) => return Err(USAGE.into()),
    };
    let mut aggregates = BTreeMap::<(Variant, GenerationStrategy), Aggregate>::new();

    for variant in Variant::ALL {
        for &strategy in &strategies {
            for loadout in loadouts {
                for intent in ChallengeIntent::ALL {
                    for offset in 0..seed_count {
                        let seed = start_seed.wrapping_add(offset as u64);
                        let aggregate = aggregates.entry((variant, strategy)).or_default();
                        aggregate.attempted_rooms += 1;
                        let candidate = match generate_variant(
                            variant, seed, *loadout, strategy, intent,
                        ) {
                            Ok(generated) => generated,
                            Err(error) => {
                                eprintln!(
                                    "construct-inconclusive variant={} strategy={} loadout={} intent={} seed={seed}: {error}",
                                    variant.report_slug(strategy),
                                    strategy.slug(),
                                    loadout.slug(),
                                    intent.slug(),
                                );
                                continue;
                            }
                        };
                        let generated = &candidate.generated;
                        aggregate.constructed_rooms += 1;
                        aggregate.solid_tiles += generated
                            .room
                            .tiles()
                            .iter()
                            .filter(|&&tile| tile == Tile::Solid)
                            .count();
                        aggregate.one_way_tiles += generated
                            .room
                            .tiles()
                            .iter()
                            .filter(|&&tile| tile == Tile::OneWay)
                            .count();
                        let construction_positive =
                            aggregate.observe_matrix(generated, *loadout)?;
                        if construction_positive {
                            aggregate.construction_all_target_positive_rooms += 1;
                        }
                        let both_positive = if *loadout == EvaluationLoadout::Both {
                            construction_positive
                        } else {
                            aggregate.observe_matrix(generated, EvaluationLoadout::Both)?
                        };
                        if both_positive {
                            aggregate.complete_kit_all_target_positive_rooms += 1;
                        }
                        let traversals = aggregate.observe_direct_controllers(generated)?;
                        aggregate.observe_terrain(&candidate, &traversals)?;
                    }
                }
            }
            eprintln!(
                "completed variant={} strategy={}",
                variant.report_slug(strategy),
                strategy.slug()
            );
        }
    }

    for ((variant, strategy), aggregate) in aggregates {
        aggregate.print(variant, strategy);
    }
    Ok(())
}

struct GeneratedWithPlan {
    generated: GeneratedLevel,
    route_plan: RoutePlan,
}

fn generate_variant(
    variant: Variant,
    seed: u64,
    loadout: EvaluationLoadout,
    strategy: GenerationStrategy,
    intent: ChallengeIntent,
) -> Result<GeneratedWithPlan, Box<dyn Error>> {
    match variant {
        Variant::LegacyTerrainOnly => {
            let source = CompositionalKey::new(
                seed,
                CompositionalProfile::new(loadout.abilities(), strategy, intent),
            );
            let candidate = generate_staged_compositional(StagedCompositionalKey::new(
                source,
                CompositionalFeatureSet::TerrainOnly,
            ))?;
            Ok(GeneratedWithPlan {
                generated: candidate.generated,
                route_plan: candidate.route_plan,
            })
        }
        // The grounded-pier transform is intentionally rejected for growth:
        // its graph is not an ordered arc, and empirical trials found both
        // pickup regressions and no robust controller improvement. Re-running
        // the exact legacy candidate here keeps the growth row as an explicit
        // unchanged control in the A/B report.
        Variant::GroundedRoutePierV1 if strategy == GenerationStrategy::ReachabilityGrowth => {
            generate_variant(Variant::LegacyTerrainOnly, seed, loadout, strategy, intent)
        }
        Variant::GroundedRoutePierV1 => {
            let candidate = generate_terrain_constrained_candidate(
                seed,
                loadout.abilities(),
                strategy,
                intent,
                TerrainConstraintExperiment::GroundedRoutePierV1,
            )?
            .candidate;
            Ok(GeneratedWithPlan {
                generated: candidate.generated,
                route_plan: candidate.route_plan,
            })
        }
    }
}
