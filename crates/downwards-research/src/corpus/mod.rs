//! Room-centric corpus construction.
//!
//! This is deliberately separate from the legacy representative-band
//! curator. A corpus room owns a complete directed route/loadout matrix; no
//! single witness or scalar band stands in for the room.

use std::collections::BTreeSet;
use std::{error::Error, path::Path};

mod ability_promotion_v2;
mod analysis;
mod analysis_artifact;
mod archive;
mod artifact;
mod artifact_v3;
mod artifact_v3_rehydrate;
mod candidate;
mod challenge_audit;
mod checkpoint;
mod config;
mod config_v2;
mod deep_cli;
mod descriptors;
mod evaluate;
mod evaluate_v2;
mod extended_selection_metrics;
mod fingerprints;
mod generate;
mod generate_v2;
mod inconclusive_audit;
mod metrics;
mod model;
mod offline_selection_cache;
mod pickup_detour;
mod playtest_export;
mod raw_audit;
mod room_metrics;
mod route_assessment;
mod route_choice_diversity;
mod route_fusion;
mod route_plan_certification;
mod selection_metrics;
mod shaky_analysis;
mod socket_packages;
mod support_face_ablation;
mod terrain_audit;
mod visual_audit;

pub use ability_promotion_v2::*;
pub use analysis::{
    CORPUS_ROOM_ANALYSIS_CONFIG_ID_VERSION, CORPUS_ROOM_ANALYSIS_VERSION, CorpusRoomAnalysis,
    CorpusRoomAnalysisConfig, CorpusRoomAnalysisConfigError, CorpusRoomAnalysisConfigRecord,
    CorpusRoomAnalysisError, analyze_corpus_room, analyze_corpus_room_v2,
};
pub use analysis_artifact::{
    CORPUS_ANALYSIS_ACTION_ENCODING_VERSION, CORPUS_ANALYSIS_ARTIFACT_VERSION,
    CorpusAnalysisArtifactBundle, CorpusAnalysisArtifactError, CorpusAnalysisArtifactRoomRef,
    VerifiedCorpusAnalysisArtifact, parse_and_verify_seed_analysis_artifact,
    render_seed_analysis_artifact,
};
pub use archive::{
    ArchiveBuildSummary, ArchiveCandidate, ArchiveCell, ArchiveCellKey, ArchiveConfig,
    ArchiveError, ObjectiveDirection, ProjectedCell, QualityDiversityArchive, SelectionError,
    SelectionPackage, SelectionResult, SelectionStep,
};
pub use artifact::{
    CorpusArtifactBundle, CorpusArtifactError, render_evaluated_artifacts,
    write_new_artifact_bundle,
};
pub use artifact_v3::{
    CORPUS_ARTIFACT_V3_ACTION_ENCODING_VERSION, CORPUS_ARTIFACT_V3_CHECKPOINT_FILE,
    CORPUS_ARTIFACT_V3_CHECKPOINT_VERSION, CORPUS_ARTIFACT_V3_SCHEMA_VERSION,
    CORPUS_ARTIFACT_V3_SEARCH_OBSERVATION_POLICY_VERSION, CorpusArtifactBundleV3,
    CorpusArtifactSummaryV3, CorpusArtifactV3Error, CorpusSeedCheckpointV3,
    CorpusSeedShardOutcomeV3, VerifiedCorpusArtifactV3, WaypointRescueEvidencePolicyV3,
    build_or_verify_seed_shard_v3, corpus_config_id_v3, read_artifact_bundle_v3,
    render_evaluated_artifact_v3, render_seed_checkpoint_v3, seed_shard_directory_v3,
    verify_artifact_bundle_v3, verify_artifact_directory_v3, verify_seed_checkpoint_v3,
    verify_seed_shard_directory_v3, write_new_artifact_bundle_v3, write_new_seed_checkpoint_v3,
    write_or_verify_seed_shard_v3,
};
pub use artifact_v3_rehydrate::{
    CorpusArtifactV3RehydrationError, RehydratedCorpusV3, load_rehydrated_artifact_v3_shards,
    rehydrate_artifact_bundle_v3,
};
pub use candidate::{
    CORPUS_CANDIDATE_KEY_RECORD_VERSION, CORPUS_PHYSICAL_ROOM_ID_VERSION, ChallengeIntentRecord,
    CompositionalAbilityGateProfileRecord, CompositionalAbilityKeyRecord,
    CompositionalRouteCutGrammarRecord, CompositionalRouteCutKeyRecord, CorpusCandidate,
    CorpusCandidateGenerator, CorpusCandidateKeyError, CorpusCandidateKeyRecord,
    CorpusCandidateProvenance, CorpusCandidateRegenerationError, CorpusCandidateView,
    CorpusPhysicalRoomDescriptorV3, PartitionRouteKeyRecord, PartitionRouteProfileRecord,
};
pub use challenge_audit::*;
pub use checkpoint::{
    CORPUS_SEED_CHECKPOINT_VERSION, CorpusVerificationError, SeedShardCheckpoint,
    VerifiedCorpusArtifact, corpus_config_hash, read_artifact_bundle, render_seed_shard_checkpoint,
    verify_artifact_bundle, verify_artifact_directory, verify_seed_shard_checkpoint,
    write_new_seed_shard_checkpoint,
};
pub use config::{CORPUS_SCHEMA_VERSION, CorpusBuildConfigV1, CorpusConfigError};
pub use config_v2::{
    CORPUS_BUILD_CONFIG_V2_EMBEDDING_ATTEMPT, CORPUS_BUILD_CONFIG_V2_REWRITE_ATTEMPT,
    CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION, CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION,
    CorpusBuildConfigV2, CorpusBuildConfigV2Error, CorpusBuildConfigV2VersionField,
    CorpusSourceCapabilityEnumerationPolicy,
};
pub use deep_cli::{run_deep_shard_cli, run_verify_deep_shard_cli};
pub use descriptors::{
    MORPHOLOGY_DIMENSIONS, ROOM_EMBEDDING_PREFIX_VERSION, RoomEmbeddingPrefix,
    RoomEmbeddingPrefixDistance, TOPOLOGY_DIMENSIONS, room_embedding_prefix,
    room_embedding_prefix_distance, room_embedding_prefix_from_parts,
};
pub use evaluate::{
    CorpusEvaluationError, EvaluatedCorpusBatch, EvaluatedCorpusRoom, LoadoutRouteMatrix,
    RouteMatrixSummary, evaluate_route_matrices, evaluate_route_matrices_with,
};
pub use evaluate_v2::{
    CORPUS_CANONICAL_REGENERATION_POLICY_VERSION, CORPUS_FEASIBILITY_GATE_VERSION,
    CORPUS_ROUTE_EVALUATION_CONFIG_ID_VERSION, CanonicalRegenerationPolicy,
    CanonicalRegenerationSelectionV2, CorpusEvaluationV2Error, CorpusFeasibilityGateState,
    CorpusMetricInputV2Error, EvaluatedCorpusBatchV2, EvaluatedCorpusRoomV2,
    RouteEvaluationActionV2, RouteEvaluationConfigV2, RouteEvaluationConfigV2Error,
    RouteEvaluationDifficultyConfigV2, RouteEvaluationMacroV2, RouteEvaluationSolverConfigV2,
    VariantConstructionGateV2, evaluate_route_matrices_v2, evaluate_route_matrices_v2_with,
    rerun_validate_evaluated_ability_promotions_v2, resolve_corpus_metric_candidate_v2,
    select_canonical_regeneration_v2, validate_evaluated_corpus_room_v2,
    validate_route_evaluation_configs_v2,
};
pub use extended_selection_metrics::{
    ExtendedSelectionMetricError, PICKUP_DETOUR_SELECTION_DISCLAIMER,
    PICKUP_DETOUR_SELECTION_METRICS_VERSION, PickupChallengeCoordinateDistributions,
    PickupCrossLoadoutSelectionAggregate, PickupDetourCoordinateDistributions,
    PickupDetourSelectionAggregate, PickupDetourSelectionLoadoutMetric,
    PickupDetourSelectionMetricSummary, PickupStructuralSelectionAggregate,
    ROUTE_CHOICE_SELECTION_DISCLAIMER, ROUTE_CHOICE_SELECTION_METRICS_VERSION,
    RouteChoiceSelectionAggregate, RouteChoiceSelectionLoadoutMetric,
    RouteChoiceSelectionMetricSummary, summarize_pickup_detour_selection_metrics,
    summarize_route_choice_selection_metrics,
};
pub use generate::{
    CORPUS_ROOM_ID_VERSION, GeneratedCorpusBatch, GeneratedCorpusRoom, generate_seed_block,
};
pub use generate_v2::{
    CorpusConstructionAttemptV2, CorpusConstructionFailureClassV2, CorpusConstructionOutcomeV2,
    CorpusGenerationV2Error, GeneratedCorpusBatchV2, GeneratedCorpusRoomV2,
    GenerationBatchSummaryV2, generate_seed_block_v2,
};
pub use inconclusive_audit::*;
pub use metrics::{
    CORPUS_ROUTE_MEASUREMENT_VERSION, CorpusRouteMeasurement, CorpusRouteMeasurementError,
    measure_door_route,
};
pub use model::{
    CandidateKeyRecord, ConstructionRecord, EvaluationLoadout, FeatureStageRecord,
    GenerationAttemptRecord, GenerationBatchSummary, RoomId,
};
pub use offline_selection_cache::*;
pub use pickup_detour::*;
pub use playtest_export::*;
pub use raw_audit::{
    DistanceDistribution, RAW_CORPUS_DIVERSITY_AUDIT_VERSION, RawCorpusDiversityAudit,
    audit_raw_corpus_diversity,
};
pub use room_metrics::*;
pub use route_assessment::{
    BoundedIncompleteLoadout, CONTROLLER_DEMAND_POLICY_VERSION,
    CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, ControllerDemand, ControllerDemandCoordinates,
    LoadoutControllerAudit, LoadoutControllerAuditStatus, PositiveBypassEvidence,
    ROUTE_CONTROLLER_ASSESSMENT_DISCLAIMER, ROUTE_CONTROLLER_ASSESSMENT_POLICY_VERSION,
    ROUTE_CONTROLLER_TRACE_VERSION, RouteControllerAssessment, RouteControllerAssessmentError,
    RouteControllerAssessmentPolicy, RouteControllerAuditCompleteness, RouteControllerWitness,
    SharedLoadoutControllerAudit, SourceRouteControllerAssessmentBatch, assess_easiest_known_route,
    assess_easiest_known_routes_from_source,
};
pub use route_choice_diversity::{
    AcceptedGateEvent, AuthoredEdgeOrientation, DirectedRouteChoiceCell, GatePathStyleSignature,
    LoadoutRouteChoiceSummary, NormalizedDistanceSamples, OrientedRouteVerb,
    PositiveAlternativeAuditStatus, ROUTE_CHOICE_DIVERSITY_DISCLAIMER,
    ROUTE_CHOICE_DIVERSITY_VERSION, RoomRouteChoiceDiversity, RouteAlternativeClass,
    RouteAlternativeDistanceReport, RouteAlternativeSet, RouteChoiceAggregate,
    RouteChoiceCellEvidence, RouteChoiceDiversityError, RouteChoiceOperationalCost,
    SpatialPathSignature, StructuralPathStep, VerticalExcursion, WithinCellDistanceDistribution,
    analyze_route_choice_diversity, analyze_route_choice_diversity_for_corpus_candidate,
    analyze_route_choice_diversity_v2,
};
pub use route_fusion::{
    CURRENT_EASIEST_KNOWN_ROUTE_FUSION_POLICY, CanonicalMatrixCellStatus, ControllerEaseOrdering,
    EASIEST_KNOWN_ROUTE_DISCLAIMER, EASIEST_KNOWN_ROUTE_FUSION_VERSION,
    EASIEST_KNOWN_ROUTE_REPLAY_IDENTITY_VERSION, EASIEST_KNOWN_ROUTE_SELECTION_VERSION,
    EasiestKnownRouteFusionPolicy, EasiestKnownRouteSelection, EasiestKnownRouteSelectionStatus,
    FusedDirectAuditStatus, FusedRouteCandidate, FusedRouteCandidateMeasurement,
    FusedRouteCandidateProvenance, FusedRouteCellAssessment, FusedRouteEaseEvidence,
    RouteFusionCandidateOrigin, RouteFusionError, fuse_easiest_known_route_cell,
};
pub use route_plan_certification::{
    ROUTE_PLAN_CERTIFICATION_CONFIG_FINGERPRINT_VERSION,
    ROUTE_PLAN_CERTIFICATION_EVIDENCE_DISCLAIMER, ROUTE_PLAN_CERTIFICATION_PATH_POLICY_VERSION,
    ROUTE_PLAN_CERTIFICATION_VERSION, RoutePlanCertificate, RoutePlanCertificationBoundedReason,
    RoutePlanCertificationConfigFingerprint, RoutePlanCertificationError,
    RoutePlanCertificationInconclusive, RoutePlanCertificationOutcome,
    RoutePlanCertificationPathCost, RoutePlanCertificationProvenance,
    RoutePlanCertificationSegment, RoutePlanCertificationSegmentTarget,
    RoutePlanCertificationStructuralPath, certify_candidate_route_plan,
    route_plan_certification_config_fingerprint,
};
pub use selection_metrics::*;
pub use shaky_analysis::*;
pub use socket_packages::{
    SocketClosurePolicy, SocketMateCoverageReport, SocketPackage, SocketPackageError,
    SocketPackagePlan, SocketPackageRequest, SocketPackageRoom, UncoveredSocket,
    audit_socket_mate_coverage, build_socket_packages, is_pairwise_mate_closed,
};
pub use support_face_ablation::*;
pub use terrain_audit::{
    AblatedControllerOutcome, AblationOutcomeSummary, ControllerAblationObservation,
    FeatureAblationAudit, PositiveControllerId, PositiveControllerTarget, PositiveTerrainCoverage,
    PositiveTerrainWitness, TERRAIN_AUDIT_EVIDENCE_DISCLAIMER, TERRAIN_AUDIT_VERSION,
    TerrainAuditError, TerrainAuditReport, audit_candidate_terrain, audit_generated_terrain,
};
pub use visual_audit::*;

const CORPUS_USAGE: &str = "\
USAGE:
    downwards-research corpus pilot <start-seed> <seed-count> <output-directory>
    downwards-research corpus scan <start-seed> <seed-count>
    downwards-research corpus verify <artifact-directory>
    downwards-research corpus verify-shards <shard-root> <start-seed> <seed-count>
    downwards-research corpus inconclusive-audit <start-seed> <seed-count> <new-output-directory>
    downwards-research corpus socket-audit <shard-root> <start-seed> <seed-count> <minimum> <maximum>
    downwards-research corpus socket-coverage <shard-root> <start-seed> <seed-count>
    downwards-research corpus visual-audit <start-seed> <seed-count> <output-directory>
    downwards-research corpus deep-pilot <start-seed> <seed-count> <room-limit>
    downwards-research corpus deep-shard <seed> <new-output-directory>
    downwards-research corpus verify-deep-shard <artifact-directory>
    downwards-research corpus build-v3-shards <shard-root> <start-seed> <seed-count>
    downwards-research corpus verify-v3-shards <shard-root> <start-seed> <seed-count>

The pilot command evaluates terrain-only rooms under all four loadouts and
writes a new, non-overwriting JSON/JSONL evidence bundle. Scan performs only
the selection-independent generation/diversity audit. Verify regenerates and
replays an existing evidence bundle. Deep-pilot writes per-room JSON to stdout
and a non-scalar aggregate challenge sanity audit to stderr.
";

pub fn run_cli(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    match arguments {
        [command, start_seed, seed_count, output_directory] if command == "pilot" => {
            run_pilot(start_seed, seed_count, output_directory)
        }
        [command, start_seed, seed_count] if command == "scan" => run_scan(start_seed, seed_count),
        [command, directory] if command == "verify" => {
            let verified = verify_artifact_directory(Path::new(directory))?;
            println!(
                "verified corpus artifact: seeds={}..{} rooms={} routes={} pickups={} positive-witnesses={} config={}",
                verified.config.start_seed,
                verified
                    .config
                    .start_seed
                    .saturating_add(verified.config.seed_count.saturating_sub(1) as u64),
                verified.rooms,
                verified.route_rows,
                verified.pickup_rows,
                verified.positive_witnesses,
                verified.config_hash,
            );
            Ok(())
        }
        [command, root, start_seed, seed_count] if command == "verify-shards" => {
            run_verify_shards(root, start_seed, seed_count)
        }
        [command, start_seed, seed_count, output_directory] if command == "inconclusive-audit" => {
            run_inconclusive_audit_cli(start_seed, seed_count, output_directory)
        }
        [command, root, start_seed, seed_count] if command == "socket-coverage" => {
            run_socket_coverage(root, start_seed, seed_count)
        }
        [command, start_seed, seed_count, output_directory] if command == "visual-audit" => {
            run_visual_audit(start_seed, seed_count, output_directory)
        }
        [command, start_seed, seed_count, room_limit] if command == "deep-pilot" => {
            run_deep_pilot(start_seed, seed_count, room_limit)
        }
        [command, seed, output_directory] if command == "deep-shard" => {
            run_deep_shard_cli(seed, output_directory)
        }
        [command, artifact_directory] if command == "verify-deep-shard" => {
            run_verify_deep_shard_cli(artifact_directory)
        }
        [command, root, start_seed, seed_count] if command == "build-v3-shards" => {
            run_build_v3_shards(root, start_seed, seed_count)
        }
        [command, root, start_seed, seed_count] if command == "verify-v3-shards" => {
            run_verify_v3_shards(root, start_seed, seed_count)
        }
        [command, root, start_seed, seed_count, minimum, maximum] if command == "socket-audit" => {
            run_socket_audit(root, start_seed, seed_count, minimum, maximum)
        }
        _ => Err(CORPUS_USAGE.into()),
    }
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct DeepPilotRoomRow {
    room_id: String,
    seed: u64,
    strategy: String,
    intent: String,
    construction_loadout: EvaluationLoadout,
    port_count: usize,
    canonical_positive_routes: usize,
    canonical_inconclusive_routes: usize,
    complete_kit_direct_positive_routes: usize,
    complete_kit_direct_no_positive_after_complete_audit: usize,
    complete_kit_direct_inconclusive_without_positive: usize,
    complete_kit_run_only_routes: Option<usize>,
    complete_kit_monotone_simple_routes: Option<usize>,
    complete_kit_other_controller_routes: Option<usize>,
    maximum_reverse_duration_difference: Option<usize>,
    maximum_reverse_span_difference: Option<usize>,
    reverse_pairs_with_different_ability_use: usize,
    terrain_components: usize,
    uncorroborated_terrain_components: usize,
    terrain_tiles: usize,
    uncorroborated_terrain_tiles: usize,
    ablation_controller_trials: usize,
    ablation_controller_survivals: usize,
    ablations_all_stored_controllers_survived: usize,
    ablations_some_stored_controller_affected: usize,
    removed_tiles_all_stored_controllers_survived: usize,
    removed_tiles_some_stored_controller_affected: usize,
    direct_controller_simulated_ticks: usize,
}

fn run_deep_pilot(
    start_seed: &str,
    seed_count: &str,
    room_limit: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let room_limit = room_limit.parse::<usize>()?;
    if room_limit == 0 {
        return Err("room-limit must be positive".into());
    }
    let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(
        start_seed, seed_count,
    ))?;
    let evaluated = evaluate_route_matrices(generated)?;
    let candidates = evaluated
        .rooms
        .iter()
        .filter(|room| room.construction_loadout_gate_passes() && room.complete_kit_gate_passes())
        .take(room_limit)
        .collect::<Vec<_>>();
    eprintln!(
        "deep pilot: analyzing {} construction+all-positive rooms",
        candidates.len()
    );
    let config = CorpusRoomAnalysisConfig::default();
    let mut analyses = Vec::with_capacity(candidates.len());
    for (index, room) in candidates.iter().enumerate() {
        let analysis = analyze_corpus_room(room, &config)?;
        let metrics = summarize_room_metrics(&analysis)?;
        let canonical = &room.generated.variants[0];
        let complete_kit = metrics
            .direct_controllers
            .by_loadout
            .iter()
            .find(|summary| summary.loadout == EvaluationLoadout::Both)
            .expect("room metrics retain all four loadouts");
        let (run_only, monotone_simple, other) = match &complete_kit.easiest_controller_fractions {
            MetricEvidence::Observed(fractions) => (
                Some(fractions.run_only_class_fraction.numerator),
                Some(fractions.monotone_simple_class_fraction.numerator),
                Some(fractions.other_controller_class_fraction.numerator),
            ),
            MetricEvidence::Missing { .. } | MetricEvidence::NotApplicable { .. } => {
                (None, None, None)
            }
        };
        let observed_asymmetries = metrics
            .directional_asymmetry
            .iter()
            .filter(|asymmetry| asymmetry.loadout == EvaluationLoadout::Both)
            .filter_map(|asymmetry| match &asymmetry.comparison {
                MetricEvidence::Observed(comparison) => Some(comparison),
                MetricEvidence::Missing { .. } | MetricEvidence::NotApplicable { .. } => None,
            })
            .collect::<Vec<_>>();
        let row = DeepPilotRoomRow {
            room_id: room.generated.id.0.clone(),
            seed: canonical.key.source.seed,
            strategy: canonical.key.source.profile.strategy.slug().to_owned(),
            intent: canonical.key.source.profile.intent.slug().to_owned(),
            construction_loadout: CandidateKeyRecord::from_staged_key(canonical.key)
                .construction_loadout,
            port_count: canonical.generated.room.doors().len(),
            canonical_positive_routes: metrics.canonical_routes.positive_route_count,
            canonical_inconclusive_routes: metrics
                .canonical_routes
                .bounded_inconclusive_route_count,
            complete_kit_direct_positive_routes: complete_kit.known_positive_directed_routes,
            complete_kit_direct_no_positive_after_complete_audit: complete_kit
                .no_positive_in_complete_finite_vocabulary,
            complete_kit_direct_inconclusive_without_positive: complete_kit
                .inconclusive_without_positive,
            complete_kit_run_only_routes: run_only,
            complete_kit_monotone_simple_routes: monotone_simple,
            complete_kit_other_controller_routes: other,
            maximum_reverse_duration_difference: observed_asymmetries
                .iter()
                .map(|comparison| comparison.duration_ticks.absolute_difference)
                .max(),
            maximum_reverse_span_difference: observed_asymmetries
                .iter()
                .map(|comparison| comparison.semantic_spans.absolute_difference)
                .max(),
            reverse_pairs_with_different_ability_use: observed_asymmetries
                .iter()
                .filter(|comparison| {
                    comparison.ability_use.wall_jump_use_differs
                        || comparison.ability_use.dash_use_differs
                })
                .count(),
            terrain_components: metrics.terrain.coverage.interior_component_count,
            uncorroborated_terrain_components: metrics
                .terrain
                .coverage
                .uncorroborated_component_count,
            terrain_tiles: metrics.terrain.coverage.interior_tile_count,
            uncorroborated_terrain_tiles: metrics.terrain.coverage.uncorroborated_tile_count,
            ablation_controller_trials: metrics
                .terrain
                .ablations
                .iter()
                .map(|ablation| ablation.outcomes.controller_count)
                .sum(),
            ablation_controller_survivals: metrics
                .terrain
                .ablations
                .iter()
                .map(|ablation| ablation.outcomes.succeeded)
                .sum(),
            ablations_all_stored_controllers_survived: metrics
                .terrain
                .ablation_utility
                .all_stored_controllers_survived,
            ablations_some_stored_controller_affected: metrics
                .terrain
                .ablation_utility
                .at_least_one_stored_controller_affected,
            removed_tiles_all_stored_controllers_survived: metrics
                .terrain
                .ablation_utility
                .removed_tiles_all_survived,
            removed_tiles_some_stored_controller_affected: metrics
                .terrain
                .ablation_utility
                .removed_tiles_some_affected,
            direct_controller_simulated_ticks: metrics
                .operational_cost
                .direct_controller_reported_total
                .simulated_ticks,
        };
        println!("{}", serde_json::to_string(&row)?);
        analyses.push(analysis);
        eprintln!("deep pilot: completed {}/{}", index + 1, candidates.len());
    }
    let audited = EvaluatedCorpusBatch {
        generated: evaluated.generated.clone(),
        rooms: candidates.iter().map(|room| (*room).clone()).collect(),
    };
    let challenge =
        audit_challenge_sanity(&audited, &analyses, ChallengeSanityThresholds::default())?;
    eprint!("{}", render_challenge_sanity_text(&challenge));
    Ok(())
}

fn run_visual_audit(
    start_seed: &str,
    seed_count: &str,
    output_directory: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(
        start_seed, seed_count,
    ))?;
    let records = visual_audit_records(&generated)?;
    let bundle = render_generated_visual_audit(&generated, VisualAuditOptions::default())?;
    let suspicious_static = render_visual_audit_suspicious_pair_sheets(
        &records,
        VisualAuditNeighborMetric::StaticVisual,
        20,
        VisualAuditOptions::new(20, 4)?,
    )?;
    let suspicious_collision = render_visual_audit_suspicious_pair_sheets(
        &records,
        VisualAuditNeighborMetric::CollisionTopology,
        20,
        VisualAuditOptions::new(20, 4)?,
    )?;
    let output_directory = Path::new(output_directory);
    std::fs::create_dir(output_directory)?;
    for page in &bundle.pages {
        std::fs::write(output_directory.join(page.suggested_file_name()), &page.svg)?;
    }
    for (prefix, pages) in [
        ("suspicious-static", suspicious_static),
        ("suspicious-collision", suspicious_collision),
    ] {
        for page in pages {
            std::fs::write(
                output_directory.join(format!(
                    "{prefix}-page-{:03}-of-{:03}.svg",
                    page.page_index + 1,
                    page.page_count
                )),
                page.svg,
            )?;
        }
    }
    let mut nearest = String::from(
        "room_id\tstatic_neighbor\tstatic_distance\tcollision_neighbor\tcollision_distance\n",
    );
    for row in &bundle.nearest_neighbors {
        let static_neighbor = row.static_visual.as_ref();
        let collision_neighbor = row.collision_topology.as_ref();
        nearest.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.room_id.0,
            static_neighbor.map_or("", |neighbor| neighbor.room_id.0.as_str()),
            static_neighbor
                .map_or_else(String::new, |neighbor| format!("{:.9}", neighbor.distance)),
            collision_neighbor.map_or("", |neighbor| neighbor.room_id.0.as_str()),
            collision_neighbor
                .map_or_else(String::new, |neighbor| format!("{:.9}", neighbor.distance)),
        ));
    }
    std::fs::write(output_directory.join("nearest-neighbors.tsv"), nearest)?;
    println!(
        "visual audit written: rooms={} catalogue-pages={} suspicious-static-pairs=20 suspicious-collision-pairs=20 output={}",
        records.len(),
        bundle.pages.len(),
        output_directory.display(),
    );
    Ok(())
}

fn run_socket_coverage(
    root: &str,
    start_seed: &str,
    seed_count: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let eligible_ids = verified_passing_room_ids(Path::new(root), start_seed, seed_count)?;
    let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(
        start_seed, seed_count,
    ))?;
    let socket_storage = generated
        .rooms
        .iter()
        .map(|room| {
            (
                room.id.clone(),
                room.variants[0]
                    .generated
                    .room
                    .doors()
                    .iter()
                    .map(|door| door.socket())
                    .collect::<Vec<_>>(),
                eligible_ids.contains(&room.id),
            )
        })
        .collect::<Vec<_>>();
    let rooms = socket_storage
        .iter()
        .map(|(id, sockets, eligible)| SocketPackageRoom {
            stable_id: id,
            sockets,
            eligible: *eligible,
        })
        .collect::<Vec<_>>();
    let report = audit_socket_mate_coverage(&rooms)?;
    println!(
        "socket mate coverage: rooms={}/{} sockets={}/{} uncovered={} complete={}",
        report.rooms_with_complete_mate_coverage,
        report.eligible_rooms,
        report.covered_socket_occurrences,
        report.eligible_socket_occurrences,
        report.uncovered.len(),
        report.is_complete(),
    );
    for uncovered in report.uncovered.iter().take(32) {
        println!(
            "  room={} socket={:?} occurrences={}",
            uncovered.room_id.0, uncovered.socket, uncovered.occurrences
        );
    }
    Ok(())
}

fn run_socket_audit(
    root: &str,
    start_seed: &str,
    seed_count: &str,
    minimum: &str,
    maximum: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let minimum = minimum.parse::<usize>()?;
    let maximum = maximum.parse::<usize>()?;
    let eligible_ids = verified_passing_room_ids(Path::new(root), start_seed, seed_count)?;
    let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(
        start_seed, seed_count,
    ))?;
    let socket_storage = generated
        .rooms
        .iter()
        .map(|room| {
            (
                room.id.clone(),
                room.variants[0]
                    .generated
                    .room
                    .doors()
                    .iter()
                    .map(|door| door.socket())
                    .collect::<Vec<_>>(),
                eligible_ids.contains(&room.id),
            )
        })
        .collect::<Vec<_>>();
    let rooms = socket_storage
        .iter()
        .map(|(id, sockets, eligible)| SocketPackageRoom {
            stable_id: id,
            sockets,
            eligible: *eligible,
        })
        .collect::<Vec<_>>();
    let mut inventory =
        std::collections::BTreeMap::<downwards_core::DoorSocket, (usize, usize)>::new();
    for (_, sockets, eligible) in &socket_storage {
        if !eligible {
            continue;
        }
        for &socket in sockets {
            let mate = socket.mate();
            let signature = socket.min(mate);
            let counts = inventory.entry(signature).or_default();
            if socket == signature {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        }
    }
    let imbalanced = inventory
        .iter()
        .filter(|(_, (left, right))| left != right)
        .map(|(socket, counts)| (*socket, *counts))
        .collect::<Vec<_>>();
    eprintln!(
        "socket inventory: signatures={} imbalanced={} total-absolute-imbalance={}",
        inventory.len(),
        imbalanced.len(),
        imbalanced
            .iter()
            .map(|(_, (left, right))| left.abs_diff(*right))
            .sum::<usize>()
    );
    for (socket, (left, right)) in imbalanced.iter().take(32) {
        eprintln!("  {socket:?}: {left} vs {right}");
    }
    let plan = build_socket_packages(&rooms, SocketPackageRequest::new(minimum, maximum))?;
    let mut sizes = plan
        .packages
        .iter()
        .map(|package| package.room_ids.len())
        .collect::<Vec<_>>();
    sizes.sort_unstable();
    println!(
        "socket closure verified: eligible={} selected={} packages={} package-sizes={sizes:?} explored-nodes={}",
        eligible_ids.len(),
        plan.total_rooms,
        plan.packages.len(),
        plan.explored_nodes,
    );
    Ok(())
}

fn verified_passing_room_ids(
    root: &Path,
    start_seed: u64,
    seed_count: usize,
) -> Result<BTreeSet<RoomId>, Box<dyn Error>> {
    let mut result = BTreeSet::new();
    for offset in 0..seed_count {
        let seed = start_seed.wrapping_add(offset as u64);
        let directory = root.join(format!("seed-{seed:04}"));
        let verified = verify_artifact_directory(&directory)?;
        result.extend(verified.construction_and_complete_kit_room_ids);
    }
    Ok(result)
}

fn run_verify_shards(root: &str, start_seed: &str, seed_count: &str) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let mut rooms = 0_usize;
    let mut passing_rooms = 0_usize;
    let mut route_rows = 0_usize;
    let mut positive_route_rows = 0_usize;
    let mut pickup_rows = 0_usize;
    let mut positive_pickup_rows = 0_usize;
    let mut witnesses = 0_usize;
    for offset in 0..seed_count {
        let seed = start_seed.wrapping_add(offset as u64);
        let directory = Path::new(root).join(format!("seed-{seed:04}"));
        let bundle = read_artifact_bundle(&directory)?;
        let expected_config = CorpusBuildConfigV1::terrain_only_pilot(seed, 1);
        let checkpoint = std::fs::read(directory.join("checkpoint.json"))?;
        verify_seed_shard_checkpoint(&checkpoint, &expected_config, &bundle)?;
        let verified = verify_artifact_bundle(&bundle)?;
        rooms += verified.rooms;
        passing_rooms += verified.construction_and_complete_kit_rooms;
        route_rows += verified.route_rows;
        positive_route_rows += verified.positive_route_rows;
        pickup_rows += verified.pickup_rows;
        positive_pickup_rows += verified.positive_pickup_rows;
        witnesses += verified.positive_witnesses;
    }
    println!(
        "verified {seed_count} immutable seed shards: rooms={rooms} construction+all-positive={passing_rooms} routes={positive_route_rows}/{route_rows} pickups={positive_pickup_rows}/{pickup_rows} witnesses={witnesses}"
    );
    Ok(())
}

fn run_build_v3_shards(
    root: &str,
    start_seed: &str,
    seed_count: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let root = Path::new(root);
    let mut written = 0usize;
    let mut already_verified = 0usize;
    let mut rooms = 0usize;
    for offset in 0..seed_count {
        let seed = checked_seed_offset_v3(start_seed, offset)?;
        let outcome = build_or_verify_seed_shard_v3(root, seed)?;
        match &outcome {
            CorpusSeedShardOutcomeV3::Written(_) => written += 1,
            CorpusSeedShardOutcomeV3::AlreadyVerified(_) => already_verified += 1,
        }
        rooms += outcome.verified().summary.physical_rooms;
        println!(
            "corpus-v3 seed {seed}: {} rooms={} config={}",
            match &outcome {
                CorpusSeedShardOutcomeV3::Written(_) => "written",
                CorpusSeedShardOutcomeV3::AlreadyVerified(_) => "already-verified",
            },
            outcome.verified().summary.physical_rooms,
            outcome.verified().config_id,
        );
    }
    println!(
        "corpus-v3 shard run complete: seeds={seed_count} written={written} already-verified={already_verified} rooms={rooms}"
    );
    Ok(())
}

fn run_verify_v3_shards(
    root: &str,
    start_seed: &str,
    seed_count: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let root = Path::new(root);
    let mut rooms = 0usize;
    let mut route_rows = 0usize;
    let mut positive_routes = 0usize;
    let mut pickup_rows = 0usize;
    let mut positive_pickups = 0usize;
    for offset in 0..seed_count {
        let seed = checked_seed_offset_v3(start_seed, offset)?;
        let config = CorpusBuildConfigV2::attempt_zero(seed, 1);
        let verified =
            verify_seed_shard_directory_v3(&seed_shard_directory_v3(root, seed), &config)?;
        rooms += verified.summary.physical_rooms;
        route_rows += verified.summary.route_rows;
        positive_routes += verified.summary.positive_route_rows;
        pickup_rows += verified.summary.pickup_rows;
        positive_pickups += verified.summary.positive_pickup_rows;
    }
    println!(
        "verified {seed_count} corpus-v3 shards: rooms={rooms} routes={positive_routes}/{route_rows} pickups={positive_pickups}/{pickup_rows}"
    );
    Ok(())
}

fn checked_seed_offset_v3(start_seed: u64, offset: usize) -> Result<u64, Box<dyn Error>> {
    let offset = u64::try_from(offset)?;
    start_seed
        .checked_add(offset)
        .ok_or_else(|| "corpus-v3 seed range overflows u64".into())
}

fn run_scan(start_seed: &str, seed_count: &str) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let config = CorpusBuildConfigV1::terrain_only_pilot(start_seed, seed_count);
    let generated = generate_seed_block(config)?;
    let report = audit_raw_corpus_diversity(&generated);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn run_pilot(
    start_seed: &str,
    seed_count: &str,
    output_directory: &str,
) -> Result<(), Box<dyn Error>> {
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    let config = CorpusBuildConfigV1::terrain_only_pilot(start_seed, seed_count);
    eprintln!(
        "corpus: generating {} terrain-only candidate profiles from {} seeds",
        seed_count.saturating_mul(36),
        seed_count
    );
    let generated = generate_seed_block(config)?;
    eprintln!(
        "corpus: {} constructed candidates, {} exact static rooms; evaluating all directed targets under four loadouts",
        generated.summary.constructed, generated.summary.exact_static_rooms
    );
    let evaluated = evaluate_route_matrices(generated)?;
    let bundle = render_evaluated_artifacts(&evaluated)?;
    verify_artifact_bundle(&bundle)?;
    write_new_artifact_bundle(Path::new(output_directory), &bundle)?;
    if seed_count == 1 {
        write_new_seed_shard_checkpoint(
            &Path::new(output_directory).join("checkpoint.json"),
            &bundle,
        )?;
    }
    let positive_rooms = evaluated
        .rooms
        .iter()
        .filter(|room| room.construction_loadout_gate_passes() && room.complete_kit_gate_passes())
        .count();
    println!(
        "corpus pilot complete: rooms={} construction+all-positive={} output={output_directory}",
        evaluated.rooms.len(),
        positive_rooms
    );
    Ok(())
}
