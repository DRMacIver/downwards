//! Bounded, deterministic smoke report for the generator-neutral corpus-v2 path.
//!
//! The default mode stops after the shared four-loadout route matrices. Deep
//! analysis is available only through an explicit, bounded room limit.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    error::Error,
};

use downwards_core::{BoundarySide, DoorSocket};
use downwards_lab::CollisionTopologyDescriptor;
use serde::Serialize;

#[path = "../corpus/mod.rs"]
#[allow(dead_code, unused_imports, clippy::enum_variant_names)]
mod corpus;
#[path = "../structural.rs"]
#[allow(dead_code)]
pub mod structural;

use corpus::{
    AbilityPromotionClaimV2, AbilityPromotionDecisionV2, AbilityPromotionDirectAuditStateV2,
    AbilityPromotionIntendedEvidenceSourceV2, AbilityPromotionMatrixStateV2, CorpusBuildConfigV2,
    CorpusConstructionFailureClassV2, CorpusConstructionOutcomeV2, CorpusRoomAnalysisConfig,
    CorpusRoomAnalysisError, EvaluatedCorpusBatchV2, EvaluationLoadout, GenerationBatchSummaryV2,
    RoomId, RouteMatrixSummary, SocketPackageRoom, VariantAbilityPromotionEvidenceV2,
    analyze_corpus_room_v2, audit_socket_mate_coverage, evaluate_route_matrices_v2,
    generate_seed_block_v2,
};

const PILOT_REPORT_VERSION: u32 = 1;
const USAGE: &str = "usage: cargo run --bin corpus_v2_pilot -- <start-seed> <seed-count> [--deep-room-limit <positive-count>]";

fn main() -> Result<(), Box<dyn Error>> {
    let options = PilotOptions::parse(env::args().skip(1).collect())?;
    let report = run_pilot(options)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PilotOptions {
    start_seed: u64,
    seed_count: usize,
    deep_room_limit: Option<usize>,
}

impl PilotOptions {
    fn parse(arguments: Vec<String>) -> Result<Self, Box<dyn Error>> {
        let (start_seed, seed_count, deep_room_limit) = match arguments.as_slice() {
            [start_seed, seed_count] => (start_seed, seed_count, None),
            [start_seed, seed_count, flag, deep_room_limit] if flag == "--deep-room-limit" => {
                (start_seed, seed_count, Some(deep_room_limit))
            }
            _ => return Err(USAGE.into()),
        };
        let start_seed = start_seed.parse::<u64>()?;
        let seed_count = seed_count.parse::<usize>()?;
        if seed_count == 0 {
            return Err("seed-count must be positive".into());
        }
        let deep_room_limit = deep_room_limit
            .map(|limit| limit.parse::<usize>())
            .transpose()?;
        if deep_room_limit == Some(0) {
            return Err("deep-room-limit must be positive".into());
        }
        Ok(Self {
            start_seed,
            seed_count,
            deep_room_limit,
        })
    }
}

fn run_pilot(options: PilotOptions) -> Result<PilotReport, Box<dyn Error>> {
    let config = CorpusBuildConfigV2::attempt_zero(options.start_seed, options.seed_count);
    let generated = generate_seed_block_v2(config)?;
    let evaluated = evaluate_route_matrices_v2(generated)?;
    summarize_pilot(&evaluated, options.deep_room_limit)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct PilotReport {
    report_version: u32,
    config: CorpusBuildConfigV2,
    construction: ConstructionReport,
    physical_aliases: PhysicalAliasReport,
    route_matrices: BTreeMap<String, RouteMatrixAggregate>,
    feasibility_gates: FeasibilityGateReport,
    ability_promotion: AbilityPromotionReport,
    socket_mate_coverage: SocketMateCoverageSummary,
    diversity: DiversityCountReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    deep_analysis: Option<DeepAnalysisReport>,
}

fn summarize_pilot(
    evaluated: &EvaluatedCorpusBatchV2,
    deep_room_limit: Option<usize>,
) -> Result<PilotReport, Box<dyn Error>> {
    let construction = summarize_construction(evaluated);
    let physical_aliases = summarize_aliases(evaluated);
    let route_matrices = summarize_route_matrices(evaluated);
    let feasibility_gates = summarize_feasibility_gates(evaluated);
    let ability_promotion = summarize_ability_promotion(evaluated);
    let socket_storage = evaluated
        .rooms
        .iter()
        .map(|room| {
            let sockets = room
                .generated
                .variants
                .first()
                .expect("constructed physical rooms retain at least one native variant")
                .generated()
                .room
                .doors()
                .iter()
                .map(|door| door.socket())
                .collect::<Vec<_>>();
            (room.generated.id.clone(), sockets)
        })
        .collect::<Vec<_>>();
    let socket_mate_coverage = summarize_socket_coverage(&socket_storage)?;
    let diversity = summarize_diversity(evaluated);
    let deep_analysis = deep_room_limit.map(|limit| summarize_deep_analysis(evaluated, limit));
    Ok(PilotReport {
        report_version: PILOT_REPORT_VERSION,
        config: evaluated.config.clone(),
        construction,
        physical_aliases,
        route_matrices,
        feasibility_gates,
        ability_promotion,
        socket_mate_coverage,
        diversity,
        deep_analysis,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct GeneratorConstructionReport {
    attempted: usize,
    constructed: usize,
    rejected: usize,
    failures_by_cause: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ConstructionReport {
    summary: GenerationBatchSummaryV2,
    by_generator: BTreeMap<String, GeneratorConstructionReport>,
}

fn summarize_construction(evaluated: &EvaluatedCorpusBatchV2) -> ConstructionReport {
    let mut by_generator = BTreeMap::<String, GeneratorConstructionReport>::new();
    for record in &evaluated.construction_records {
        let generator = record.key.generator();
        let aggregate = by_generator.entry(generator.slug().to_owned()).or_default();
        aggregate.attempted += 1;
        match &record.outcome {
            CorpusConstructionOutcomeV2::Constructed { .. } => aggregate.constructed += 1,
            CorpusConstructionOutcomeV2::Rejected {
                generator: recorded_generator,
                failure_class,
                ..
            } => {
                debug_assert_eq!(*recorded_generator, generator);
                aggregate.rejected += 1;
                *aggregate
                    .failures_by_cause
                    .entry(construction_failure_slug(*failure_class).to_owned())
                    .or_default() += 1;
            }
        }
    }
    ConstructionReport {
        summary: evaluated.generation_summary,
        by_generator,
    }
}

const fn construction_failure_slug(failure: CorpusConstructionFailureClassV2) -> &'static str {
    match failure {
        CorpusConstructionFailureClassV2::KeyVersionMismatch => "key-version-mismatch",
        CorpusConstructionFailureClassV2::UnsupportedEmbeddingAttempt => {
            "unsupported-embedding-attempt"
        }
        CorpusConstructionFailureClassV2::GraphDerivationExhausted => "graph-derivation-exhausted",
        CorpusConstructionFailureClassV2::MissionDerivationExhausted => {
            "mission-derivation-exhausted"
        }
        CorpusConstructionFailureClassV2::EmbeddingExhausted => "embedding-exhausted",
        CorpusConstructionFailureClassV2::RhythmExhausted => "rhythm-exhausted",
        CorpusConstructionFailureClassV2::MissingEmbeddedMissionNode => {
            "missing-embedded-mission-node"
        }
        CorpusConstructionFailureClassV2::SupportConstraintExhausted => {
            "support-constraint-exhausted"
        }
        CorpusConstructionFailureClassV2::ForkEmbeddingExhausted => "fork-embedding-exhausted",
        CorpusConstructionFailureClassV2::AbilityRewriteExhausted => "ability-rewrite-exhausted",
        CorpusConstructionFailureClassV2::AbilityRewriteContract => "ability-rewrite-contract",
        CorpusConstructionFailureClassV2::AbilityConstraintSearchExhausted => {
            "ability-constraint-search-exhausted"
        }
        CorpusConstructionFailureClassV2::AbilityGateContract => "ability-gate-contract",
        CorpusConstructionFailureClassV2::PortContract => "port-contract",
        CorpusConstructionFailureClassV2::RoomInvariant => "room-invariant",
        CorpusConstructionFailureClassV2::DoorInvariant => "door-invariant",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct PhysicalAliasReport {
    physical_rooms: usize,
    constructed_variants: usize,
    alias_candidates: usize,
    rooms_with_multiple_variants: usize,
    cross_generator_alias_rooms: usize,
    largest_variant_group: usize,
    variant_count_distribution: BTreeMap<usize, usize>,
}

fn summarize_aliases(evaluated: &EvaluatedCorpusBatchV2) -> PhysicalAliasReport {
    let mut variant_count_distribution = BTreeMap::new();
    let mut rooms_with_multiple_variants = 0;
    let mut cross_generator_alias_rooms = 0;
    let mut largest_variant_group = 0;
    for room in &evaluated.rooms {
        let variant_count = room.generated.variants.len();
        *variant_count_distribution.entry(variant_count).or_default() += 1;
        rooms_with_multiple_variants += usize::from(variant_count > 1);
        largest_variant_group = largest_variant_group.max(variant_count);
        let generators = room
            .generated
            .variants
            .iter()
            .map(|candidate| candidate.generator())
            .collect::<BTreeSet<_>>();
        cross_generator_alias_rooms += usize::from(generators.len() > 1);
    }
    PhysicalAliasReport {
        physical_rooms: evaluated.rooms.len(),
        constructed_variants: evaluated.generation_summary.constructed,
        alias_candidates: evaluated.generation_summary.alias_candidates,
        rooms_with_multiple_variants,
        cross_generator_alias_rooms,
        largest_variant_group,
        variant_count_distribution,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct RouteMatrixAggregate {
    physical_rooms: usize,
    all_targets_positive_rooms: usize,
    bounded_inconclusive_rooms: usize,
    door_rows: usize,
    positive_door_rows: usize,
    inconclusive_door_rows: usize,
    pickup_rows: usize,
    positive_pickup_rows: usize,
    inconclusive_pickup_rows: usize,
}

impl RouteMatrixAggregate {
    fn observe(&mut self, summary: RouteMatrixSummary) {
        self.physical_rooms += 1;
        let all_positive =
            summary.inconclusive_door_rows == 0 && summary.inconclusive_pickup_rows == 0;
        self.all_targets_positive_rooms += usize::from(all_positive);
        self.bounded_inconclusive_rooms += usize::from(!all_positive);
        self.door_rows += summary.door_rows;
        self.positive_door_rows += summary.positive_door_rows;
        self.inconclusive_door_rows += summary.inconclusive_door_rows;
        self.pickup_rows += summary.pickup_rows;
        self.positive_pickup_rows += summary.positive_pickup_rows;
        self.inconclusive_pickup_rows += summary.inconclusive_pickup_rows;
    }
}

fn summarize_route_matrices(
    evaluated: &EvaluatedCorpusBatchV2,
) -> BTreeMap<String, RouteMatrixAggregate> {
    let mut result = EvaluationLoadout::ALL
        .into_iter()
        .map(|loadout| (loadout.slug().to_owned(), RouteMatrixAggregate::default()))
        .collect::<BTreeMap<_, _>>();
    for room in &evaluated.rooms {
        for matrix in &room.matrices {
            result
                .get_mut(matrix.loadout.slug())
                .expect("all evaluated loadouts belong to the frozen four-loadout inventory")
                .observe(matrix.summary);
        }
    }
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct FeasibilityGateReport {
    construction_variant_gates: usize,
    construction_variant_passes: usize,
    construction_variant_bounded_inconclusive: usize,
    physical_rooms_with_any_construction_pass: usize,
    physical_rooms_with_all_construction_variants_passing: usize,
    complete_kit_physical_gates: usize,
    complete_kit_passes: usize,
    complete_kit_bounded_inconclusive: usize,
    canonical_regeneration_selected: usize,
}

fn summarize_feasibility_gates(evaluated: &EvaluatedCorpusBatchV2) -> FeasibilityGateReport {
    let mut result = FeasibilityGateReport::default();
    for room in &evaluated.rooms {
        result.complete_kit_physical_gates += 1;
        if room.complete_kit_gate.passes() {
            result.complete_kit_passes += 1;
        } else {
            result.complete_kit_bounded_inconclusive += 1;
        }
        result.canonical_regeneration_selected +=
            usize::from(room.canonical_regeneration.selected_key.is_some());

        let passes = room
            .variant_construction_gates
            .iter()
            .filter(|gate| gate.state.passes())
            .count();
        result.construction_variant_gates += room.variant_construction_gates.len();
        result.construction_variant_passes += passes;
        result.construction_variant_bounded_inconclusive +=
            room.variant_construction_gates.len() - passes;
        result.physical_rooms_with_any_construction_pass += usize::from(passes > 0);
        result.physical_rooms_with_all_construction_variants_passing +=
            usize::from(passes == room.variant_construction_gates.len());
    }
    result
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct AbilityPromotionClaimReport {
    aliases: usize,
    promoted: usize,
    unpromoted: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct AbilityPromotionReport {
    ordinary_not_applicable_aliases: usize,
    ability_aliases: usize,
    promoted_ability_aliases: usize,
    unpromoted_ability_aliases: usize,
    physical_rooms_with_promoted_ability_alias: usize,
    by_claim: BTreeMap<String, AbilityPromotionClaimReport>,
    decisions: BTreeMap<String, usize>,
    intended_evidence_sources: BTreeMap<String, usize>,
    missing_ability_matrix_positives_by_loadout: BTreeMap<String, usize>,
    missing_ability_direct_positives_by_loadout: BTreeMap<String, usize>,
    missing_ability_bounded_direct_audits_by_loadout: BTreeMap<String, usize>,
}

fn summarize_ability_promotion(evaluated: &EvaluatedCorpusBatchV2) -> AbilityPromotionReport {
    let mut report = AbilityPromotionReport::default();
    for room in &evaluated.rooms {
        let mut room_has_promoted_ability = false;
        for gate in &room.variant_ability_promotion_gates {
            match &gate.evidence {
                VariantAbilityPromotionEvidenceV2::NotApplicable => {
                    report.ordinary_not_applicable_aliases += 1;
                }
                VariantAbilityPromotionEvidenceV2::Ability {
                    claim,
                    intended_evidence_source,
                    missing_ability_loadouts,
                    ..
                } => {
                    report.ability_aliases += 1;
                    let promoted = matches!(
                        gate.decision,
                        AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass
                    );
                    report.promoted_ability_aliases += usize::from(promoted);
                    report.unpromoted_ability_aliases += usize::from(!promoted);
                    room_has_promoted_ability |= promoted;

                    let claim_report = report
                        .by_claim
                        .entry(ability_claim_slug(*claim).to_owned())
                        .or_default();
                    claim_report.aliases += 1;
                    claim_report.promoted += usize::from(promoted);
                    claim_report.unpromoted += usize::from(!promoted);

                    *report
                        .decisions
                        .entry(ability_decision_slug(gate.decision).to_owned())
                        .or_default() += 1;
                    *report
                        .intended_evidence_sources
                        .entry(
                            intended_evidence_source
                                .as_ref()
                                .map_or("unavailable", intended_evidence_source_slug)
                                .to_owned(),
                        )
                        .or_default() += 1;

                    for evidence in missing_ability_loadouts {
                        let loadout = evidence.loadout.slug().to_owned();
                        if evidence.matrix == AbilityPromotionMatrixStateV2::ReplayCertifiedPositive
                        {
                            *report
                                .missing_ability_matrix_positives_by_loadout
                                .entry(loadout.clone())
                                .or_default() += 1;
                        }
                        match evidence.direct_audit {
                            AbilityPromotionDirectAuditStateV2::PositiveBypass { .. } => {
                                *report
                                    .missing_ability_direct_positives_by_loadout
                                    .entry(loadout)
                                    .or_default() += 1;
                            }
                            AbilityPromotionDirectAuditStateV2::BoundedInconclusive { .. } => {
                                *report
                                    .missing_ability_bounded_direct_audits_by_loadout
                                    .entry(loadout)
                                    .or_default() += 1;
                            }
                            AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive => {}
                        }
                    }
                }
            }
        }
        report.physical_rooms_with_promoted_ability_alias += usize::from(room_has_promoted_ability);
    }
    report
}

const fn ability_claim_slug(claim: AbilityPromotionClaimV2) -> &'static str {
    match claim {
        AbilityPromotionClaimV2::WallJump => "wall-jump",
        AbilityPromotionClaimV2::Dash => "dash",
    }
}

const fn intended_evidence_source_slug(
    source: &AbilityPromotionIntendedEvidenceSourceV2,
) -> &'static str {
    match source {
        AbilityPromotionIntendedEvidenceSourceV2::DirectController { .. } => "direct-controller",
        AbilityPromotionIntendedEvidenceSourceV2::CanonicalMatrix => "canonical-matrix",
    }
}

const fn ability_decision_slug(decision: AbilityPromotionDecisionV2) -> &'static str {
    match decision {
        AbilityPromotionDecisionV2::NotApplicable => "not-applicable",
        AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass => {
            "promoted-structural-no-known-bypass"
        }
        AbilityPromotionDecisionV2::RefusedAliasRoomMismatch => "refused-alias-room-mismatch",
        AbilityPromotionDecisionV2::BoundedIntendedRoute => "bounded-intended-route",
        AbilityPromotionDecisionV2::IntendedRouteMissingRequiredEvent => {
            "intended-route-missing-required-event"
        }
        AbilityPromotionDecisionV2::BoundedBaselineReverse => "bounded-baseline-reverse",
        AbilityPromotionDecisionV2::PositiveMatrixBypass { .. } => "positive-matrix-bypass",
        AbilityPromotionDecisionV2::PositiveDirectBypass { .. } => "positive-direct-bypass",
        AbilityPromotionDecisionV2::BoundedDirectAudit { .. } => "bounded-direct-audit",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketMateCoverageSummary {
    physical_rooms: usize,
    socket_occurrences: usize,
    occurrences_with_different_room_mate: usize,
    occurrences_without_different_room_mate: usize,
    rooms_with_complete_different_room_mate_coverage: usize,
    distinct_socket_kinds: usize,
    distinct_socket_kinds_with_different_room_mate: usize,
    distinct_socket_kinds_without_different_room_mate: usize,
}

fn summarize_socket_coverage(
    socket_storage: &[(RoomId, Vec<DoorSocket>)],
) -> Result<SocketMateCoverageSummary, Box<dyn Error>> {
    let rooms = socket_storage
        .iter()
        .map(|(room_id, sockets)| SocketPackageRoom {
            stable_id: room_id,
            sockets,
            eligible: true,
        })
        .collect::<Vec<_>>();
    let coverage = audit_socket_mate_coverage(&rooms)?;

    let mut providers = BTreeMap::<DoorSocket, BTreeSet<&RoomId>>::new();
    for (room_id, sockets) in socket_storage {
        for &socket in sockets {
            providers.entry(socket).or_default().insert(room_id);
        }
    }
    let distinct_socket_kinds_with_different_room_mate = providers
        .iter()
        .filter(|(socket, room_ids)| {
            providers.get(&socket.mate()).is_some_and(|mate_room_ids| {
                room_ids
                    .iter()
                    .any(|room_id| mate_room_ids.iter().any(|mate_id| mate_id != room_id))
            })
        })
        .count();
    Ok(SocketMateCoverageSummary {
        physical_rooms: coverage.eligible_rooms,
        socket_occurrences: coverage.eligible_socket_occurrences,
        occurrences_with_different_room_mate: coverage.covered_socket_occurrences,
        occurrences_without_different_room_mate: coverage
            .eligible_socket_occurrences
            .saturating_sub(coverage.covered_socket_occurrences),
        rooms_with_complete_different_room_mate_coverage: coverage
            .rooms_with_complete_mate_coverage,
        distinct_socket_kinds: providers.len(),
        distinct_socket_kinds_with_different_room_mate,
        distinct_socket_kinds_without_different_room_mate: providers
            .len()
            .saturating_sub(distinct_socket_kinds_with_different_room_mate),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct DiversityCountReport {
    exact_physical_rooms: usize,
    exact_static_visuals: usize,
    exact_simulation_geometries: usize,
    same_static_different_simulation_rooms: usize,
    exact_collision_topologies: usize,
    exact_authored_route_plans: usize,
    distinct_route_signature_digests: usize,
    physical_rooms_with_multiple_authored_route_plans: usize,
    source_seeds: usize,
    static_shape_count: usize,
    comparable_variable_tile_cells: Option<usize>,
    port_count_distribution: BTreeMap<usize, usize>,
    boundary_side_mask_distribution: BTreeMap<u8, usize>,
    variant_generator_distribution: BTreeMap<String, usize>,
    physical_generator_membership_distribution: BTreeMap<String, usize>,
}

fn summarize_diversity(evaluated: &EvaluatedCorpusBatchV2) -> DiversityCountReport {
    let mut static_visuals = HashSet::new();
    let mut simulation_geometries = HashSet::new();
    let mut collision_topologies = HashSet::new();
    let mut route_plans = HashSet::new();
    let mut route_signatures = HashSet::new();
    let mut source_seeds = BTreeSet::new();
    let mut static_shapes = BTreeSet::new();
    let mut port_count_distribution = BTreeMap::new();
    let mut boundary_side_mask_distribution = BTreeMap::new();
    let mut variant_generator_distribution = BTreeMap::new();
    let mut physical_generator_membership_distribution = BTreeMap::new();
    let mut physical_rooms_with_multiple_authored_route_plans = 0;
    let mut tile_state_masks = Vec::<u8>::new();

    for room in &evaluated.rooms {
        let descriptor = &room.generated.physical_descriptor;
        static_visuals.insert(&descriptor.static_visual);
        simulation_geometries.insert(&descriptor.simulation_geometry);
        static_shapes.insert((
            descriptor.static_visual.width,
            descriptor.static_visual.height,
            descriptor.static_visual.tile_size,
            descriptor.static_visual.tiles.len(),
        ));
        let candidate = room
            .generated
            .variants
            .first()
            .expect("constructed physical rooms retain at least one native variant");
        let core_room = &candidate.generated().room;
        collision_topologies.insert(CollisionTopologyDescriptor::from_room(core_room));
        *port_count_distribution
            .entry(core_room.doors().len())
            .or_default() += 1;
        let side_mask = core_room.doors().iter().fold(0_u8, |mask, door| {
            mask | match door.side {
                BoundarySide::Left => 1,
                BoundarySide::Right => 2,
                BoundarySide::Ceiling => 4,
                BoundarySide::Floor => 8,
            }
        });
        *boundary_side_mask_distribution
            .entry(side_mask)
            .or_default() += 1;

        let mut generators = BTreeSet::new();
        let mut room_route_plans = HashSet::new();
        for variant in &room.generated.variants {
            let generator_slug = variant.generator().slug();
            generators.insert(generator_slug);
            *variant_generator_distribution
                .entry(generator_slug.to_owned())
                .or_default() += 1;
            source_seeds.insert(variant.exact_key().source_seed());
            route_plans.insert(variant.route_plan());
            room_route_plans.insert(variant.route_plan());
            route_signatures.insert(variant.route_plan_summary().signature);
        }
        physical_rooms_with_multiple_authored_route_plans +=
            usize::from(room_route_plans.len() > 1);
        let membership = generators.into_iter().collect::<Vec<_>>().join("+");
        *physical_generator_membership_distribution
            .entry(membership)
            .or_default() += 1;
    }

    let comparable_variable_tile_cells = if static_shapes.len() == 1 {
        for descriptor in &static_visuals {
            if tile_state_masks.is_empty() {
                tile_state_masks.resize(descriptor.tiles.len(), 0);
            }
            for (&tile, mask) in descriptor.tiles.iter().zip(&mut tile_state_masks) {
                *mask |= 1_u8 << (tile as u8);
            }
        }
        Some(
            tile_state_masks
                .iter()
                .filter(|mask| mask.count_ones() > 1)
                .count(),
        )
    } else {
        None
    };

    DiversityCountReport {
        exact_physical_rooms: evaluated.rooms.len(),
        exact_static_visuals: static_visuals.len(),
        exact_simulation_geometries: simulation_geometries.len(),
        same_static_different_simulation_rooms: evaluated
            .rooms
            .len()
            .saturating_sub(static_visuals.len()),
        exact_collision_topologies: collision_topologies.len(),
        exact_authored_route_plans: route_plans.len(),
        distinct_route_signature_digests: route_signatures.len(),
        physical_rooms_with_multiple_authored_route_plans,
        source_seeds: source_seeds.len(),
        static_shape_count: static_shapes.len(),
        comparable_variable_tile_cells,
        port_count_distribution,
        boundary_side_mask_distribution,
        variant_generator_distribution,
        physical_generator_membership_distribution,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct DeepOperationalStats {
    expanded_nodes: usize,
    generated_nodes: usize,
    simulated_ticks: usize,
    deepest_path_ticks: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct DeepAnalysisReport {
    requested_room_limit: usize,
    eligible_canonical_rooms: usize,
    attempted_rooms: usize,
    analyzed_rooms: usize,
    omitted_eligible_rooms: usize,
    failures_by_cause: BTreeMap<String, usize>,
    source_door_batches: usize,
    directed_routes_assessed: usize,
    canonical_route_measurements: usize,
    positive_terrain_witnesses: usize,
    terrain_ablations: usize,
    operational_stats: DeepOperationalStats,
}

fn summarize_deep_analysis(
    evaluated: &EvaluatedCorpusBatchV2,
    room_limit: usize,
) -> DeepAnalysisReport {
    let eligible = evaluated
        .rooms
        .iter()
        .filter(|room| room.canonical_regeneration.selected_key.is_some())
        .collect::<Vec<_>>();
    let attempted_rooms = eligible.len().min(room_limit);
    let mut report = DeepAnalysisReport {
        requested_room_limit: room_limit,
        eligible_canonical_rooms: eligible.len(),
        attempted_rooms,
        omitted_eligible_rooms: eligible.len().saturating_sub(attempted_rooms),
        ..DeepAnalysisReport::default()
    };
    let config = CorpusRoomAnalysisConfig::default();
    for room in eligible.into_iter().take(room_limit) {
        match analyze_corpus_room_v2(room, &config) {
            Ok(analysis) => {
                report.analyzed_rooms += 1;
                report.source_door_batches += analysis.source_route_assessments.len();
                report.directed_routes_assessed += analysis
                    .source_route_assessments
                    .iter()
                    .map(|batch| batch.routes.len())
                    .sum::<usize>();
                report.canonical_route_measurements += analysis.canonical_route_measurements.len();
                report.positive_terrain_witnesses +=
                    analysis.terrain_audit.positive_witnesses.len();
                report.terrain_ablations += analysis.terrain_audit.ablations.len();
                report.operational_stats.expanded_nodes = report
                    .operational_stats
                    .expanded_nodes
                    .saturating_add(analysis.direct_controller_operational_stats.expanded_nodes);
                report.operational_stats.generated_nodes = report
                    .operational_stats
                    .generated_nodes
                    .saturating_add(analysis.direct_controller_operational_stats.generated_nodes);
                report.operational_stats.simulated_ticks = report
                    .operational_stats
                    .simulated_ticks
                    .saturating_add(analysis.direct_controller_operational_stats.simulated_ticks);
                report.operational_stats.deepest_path_ticks =
                    report.operational_stats.deepest_path_ticks.max(
                        analysis
                            .direct_controller_operational_stats
                            .deepest_path_ticks,
                    );
            }
            Err(error) => {
                *report
                    .failures_by_cause
                    .entry(deep_analysis_error_slug(&error).to_owned())
                    .or_default() += 1;
            }
        }
    }
    report
}

const fn deep_analysis_error_slug(error: &CorpusRoomAnalysisError) -> &'static str {
    match error {
        CorpusRoomAnalysisError::InvalidAnalysisConfig { .. } => "invalid-analysis-config",
        CorpusRoomAnalysisError::MissingCanonicalVariant { .. } => "missing-canonical-variant",
        CorpusRoomAnalysisError::MissingPostFeasibilityCanonical { .. } => {
            "missing-post-feasibility-canonical"
        }
        CorpusRoomAnalysisError::PostFeasibilityCanonicalMismatch { .. } => {
            "post-feasibility-canonical-mismatch"
        }
        CorpusRoomAnalysisError::CanonicalRegeneration { .. } => "canonical-regeneration",
        CorpusRoomAnalysisError::CanonicalEvidenceSourceMismatch { .. } => {
            "canonical-evidence-source-mismatch"
        }
        CorpusRoomAnalysisError::DuplicateDoorId { .. } => "duplicate-door-id",
        CorpusRoomAnalysisError::MissingLoadoutMatrix { .. } => "missing-loadout-matrix",
        CorpusRoomAnalysisError::DuplicateLoadoutMatrix { .. } => "duplicate-loadout-matrix",
        CorpusRoomAnalysisError::MatrixLoadoutMismatch { .. } => "matrix-loadout-mismatch",
        CorpusRoomAnalysisError::DoorRouteCardinality { .. } => "door-route-cardinality",
        CorpusRoomAnalysisError::DoorRouteOrder { .. } => "door-route-order",
        CorpusRoomAnalysisError::SourceRouteAssessment { .. } => "source-route-assessment",
        CorpusRoomAnalysisError::CanonicalRouteMeasurement { .. } => "canonical-route-measurement",
        CorpusRoomAnalysisError::CanonicalWitnessTargetMismatch { .. } => {
            "canonical-witness-target-mismatch"
        }
        CorpusRoomAnalysisError::RouteFusion { .. } => "route-fusion",
        CorpusRoomAnalysisError::TerrainAudit { .. } => "terrain-audit",
    }
}

#[cfg(test)]
mod tests {
    use downwards_validation::ValidationConfig;

    use super::*;

    #[test]
    fn options_require_positive_bounded_counts() {
        assert_eq!(
            PilotOptions::parse(vec!["7".to_owned(), "3".to_owned()]).unwrap(),
            PilotOptions {
                start_seed: 7,
                seed_count: 3,
                deep_room_limit: None,
            }
        );
        assert_eq!(
            PilotOptions::parse(vec![
                "7".to_owned(),
                "3".to_owned(),
                "--deep-room-limit".to_owned(),
                "2".to_owned(),
            ])
            .unwrap()
            .deep_room_limit,
            Some(2)
        );
        assert!(PilotOptions::parse(vec!["7".to_owned(), "0".to_owned()]).is_err());
        assert!(
            PilotOptions::parse(vec![
                "7".to_owned(),
                "3".to_owned(),
                "--deep-room-limit".to_owned(),
                "0".to_owned(),
            ])
            .is_err()
        );
    }

    #[test]
    fn route_aggregate_calls_every_non_positive_row_inconclusive() {
        let mut aggregate = RouteMatrixAggregate::default();
        aggregate.observe(RouteMatrixSummary {
            door_rows: 6,
            positive_door_rows: 4,
            inconclusive_door_rows: 2,
            pickup_rows: 3,
            positive_pickup_rows: 2,
            inconclusive_pickup_rows: 1,
        });
        assert_eq!(aggregate.all_targets_positive_rooms, 0);
        assert_eq!(aggregate.bounded_inconclusive_rooms, 1);
        assert_eq!(aggregate.inconclusive_door_rows, 2);
        assert_eq!(aggregate.inconclusive_pickup_rows, 1);
    }

    #[test]
    fn socket_coverage_requires_a_mate_on_another_room() {
        let left = DoorSocket {
            side: BoundarySide::Left,
            offset: 20,
            span: 20,
        };
        let right = left.mate();
        let same_room_only = vec![(RoomId("a".to_owned()), vec![left, right])];
        let report = summarize_socket_coverage(&same_room_only).unwrap();
        assert_eq!(report.occurrences_with_different_room_mate, 0);
        assert_eq!(report.occurrences_without_different_room_mate, 2);

        let different_rooms = vec![
            (RoomId("a".to_owned()), vec![left]),
            (RoomId("b".to_owned()), vec![right]),
        ];
        let report = summarize_socket_coverage(&different_rooms).unwrap();
        assert_eq!(report.occurrences_with_different_room_mate, 2);
        assert_eq!(report.distinct_socket_kinds_with_different_room_mate, 2);
        assert_eq!(report.rooms_with_complete_different_room_mate_coverage, 2);
    }

    #[test]
    fn generation_accounting_is_complete_and_generator_partitioned() {
        let generated = generate_seed_block_v2(CorpusBuildConfigV2::attempt_zero(0, 1)).unwrap();
        let fake_evaluated = EvaluatedCorpusBatchV2 {
            config: generated.config,
            evaluation_configs: EvaluationLoadout::ALL
                .into_iter()
                .map(|loadout| {
                    corpus::RouteEvaluationConfigV2::from_validation_config(
                        loadout,
                        &ValidationConfig::for_loadout(loadout.abilities()),
                    )
                    .unwrap()
                })
                .collect(),
            construction_records: generated.construction_records,
            generation_summary: generated.summary,
            rooms: Vec::new(),
        };
        let report = summarize_construction(&fake_evaluated);
        assert_eq!(
            report
                .by_generator
                .values()
                .map(|generator| generator.attempted)
                .sum::<usize>(),
            report.summary.attempted
        );
        assert_eq!(
            report
                .by_generator
                .values()
                .map(|generator| generator.constructed)
                .sum::<usize>(),
            report.summary.constructed
        );
        assert_eq!(
            report
                .by_generator
                .values()
                .map(|generator| generator.rejected)
                .sum::<usize>(),
            report.summary.rejected
        );
        assert_eq!(report.by_generator.len(), 2);
    }

    #[test]
    fn generator_slugs_are_stable_report_keys() {
        assert_eq!(
            corpus::CorpusCandidateGenerator::PartitionRoute.slug(),
            "partition-route"
        );
        assert_eq!(
            corpus::CorpusCandidateGenerator::CompositionalRouteCut.slug(),
            "compositional-route-cut"
        );
        assert_eq!(
            construction_failure_slug(CorpusConstructionFailureClassV2::EmbeddingExhausted),
            "embedding-exhausted"
        );
    }
}
