//! Replay-only rehydration for verified generator-neutral corpus shards.
//!
//! No solver search runs here. Construction is regenerated exactly, every
//! persisted positive is replayed through validation's untrusted-input seam,
//! and bounded-inconclusive cells remain bounded-inconclusive.

use std::{collections::BTreeSet, error::Error, fmt, path::PathBuf};

use downwards_ai::{ReachedTarget, Replay, SearchTarget, TargetSolution};
use downwards_core::Simulation;
use downwards_validation::{
    BoundedInconclusiveEvidence, BoundedTargetEvidence, DoorTargetEvidenceRehydrationError,
    RecordedDoorRouteEvidence, RecordedPickupRouteEvidence, RecordedTargetEvidence,
    rehydrate_generated_door_target_evidence,
};

use super::{
    CorpusArtifactBundleV3, CorpusArtifactV3Error, CorpusBuildConfigV2,
    CorpusConstructionAttemptV2, CorpusGenerationV2Error, CorpusMetricInputV2Error,
    CorpusSeedCheckpointV3, EvaluatedCorpusBatchV2, EvaluatedCorpusRoomV2, EvaluationLoadout,
    GenerationBatchSummaryV2, RoomId, RouteEvaluationConfigV2, RouteMatrixSummary,
    artifact_v3::{
        ArtifactRehydrationDoorRowV3, ArtifactRehydrationEvidenceV3, ArtifactRehydrationMatrixV3,
        ArtifactRehydrationPickupRowV3, VerifiedRehydratedSeedShardV3,
        load_verified_rehydrated_seed_shard_v3, verified_rehydration_data_v3,
    },
};

/// Rehydrate one independently verified artifact bundle into the exact rich
/// in-memory value used by deep analysis and selection.
pub fn rehydrate_artifact_bundle_v3(
    bundle: &CorpusArtifactBundleV3,
) -> Result<EvaluatedCorpusBatchV2, CorpusArtifactV3RehydrationError> {
    let data = verified_rehydration_data_v3(bundle)?;
    if data.evaluated.config != data.verified.config
        || data.evaluated.evaluation_configs != data.verified.evaluation_configs
        || data.evaluated.generation_summary != data.verified.generation
        || data
            .evaluated
            .rooms
            .iter()
            .map(|room| &room.generated.id)
            .ne(&data.verified.room_ids)
    {
        return Err(invalid(
            "process-local rehydrated batch differs from its verified artifact identity",
        ));
    }
    Ok(data.evaluated)
}

pub(super) fn rehydrate_matrix_v3(
    room_id: &RoomId,
    generated: &downwards_gen::GeneratedLevel,
    evaluation_config: &RouteEvaluationConfigV2,
    matrix: ArtifactRehydrationMatrixV3,
) -> Result<downwards_validation::DoorTargetEvidenceBatch, CorpusArtifactV3RehydrationError> {
    let loadout = matrix.loadout;
    if evaluation_config.loadout != loadout {
        return Err(invalid(format!(
            "room {:?} matrix/config loadout mismatch: {} != {}",
            room_id.0,
            loadout.slug(),
            evaluation_config.loadout.slug()
        )));
    }
    let solver_config = evaluation_config.solver.to_solver_config();
    let door_routes = matrix
        .door_routes
        .into_iter()
        .map(|row| rehydrate_door_row_v3(generated, loadout, row))
        .collect::<Result<Vec<_>, _>>()?;
    let pickup_routes = matrix
        .pickup_routes
        .into_iter()
        .map(|row| rehydrate_pickup_row_v3(generated, loadout, row))
        .collect::<Result<Vec<_>, _>>()?;
    rehydrate_generated_door_target_evidence(
        generated,
        loadout.abilities(),
        &solver_config,
        door_routes,
        pickup_routes,
        matrix.source_search_effort,
        matrix.aggregate_search_effort,
    )
    .map_err(|source| CorpusArtifactV3RehydrationError::Evidence {
        room_id: room_id.clone(),
        loadout,
        source,
    })
}

fn rehydrate_door_row_v3(
    generated: &downwards_gen::GeneratedLevel,
    loadout: EvaluationLoadout,
    row: ArtifactRehydrationDoorRowV3,
) -> Result<RecordedDoorRouteEvidence, CorpusArtifactV3RehydrationError> {
    Ok(RecordedDoorRouteEvidence {
        evidence: recorded_evidence_v3(
            generated,
            loadout,
            &row.source_door_id,
            SearchTarget::door(&row.target_door_id),
            ReachedTarget::Door(row.target_door_id.clone()),
            row.evidence,
        )?,
        source_door_id: row.source_door_id,
        target_door_id: row.target_door_id,
    })
}

fn rehydrate_pickup_row_v3(
    generated: &downwards_gen::GeneratedLevel,
    loadout: EvaluationLoadout,
    row: ArtifactRehydrationPickupRowV3,
) -> Result<RecordedPickupRouteEvidence, CorpusArtifactV3RehydrationError> {
    Ok(RecordedPickupRouteEvidence {
        evidence: recorded_evidence_v3(
            generated,
            loadout,
            &row.source_door_id,
            SearchTarget::pickup(&row.required_pickup_id),
            ReachedTarget::Pickup(row.required_pickup_id.clone()),
            row.evidence,
        )?,
        source_door_id: row.source_door_id,
        required_pickup_id: row.required_pickup_id,
    })
}

fn recorded_evidence_v3(
    generated: &downwards_gen::GeneratedLevel,
    loadout: EvaluationLoadout,
    source_door_id: &str,
    target: SearchTarget,
    reached: ReachedTarget,
    evidence: ArtifactRehydrationEvidenceV3,
) -> Result<RecordedTargetEvidence, CorpusArtifactV3RehydrationError> {
    match evidence {
        ArtifactRehydrationEvidenceV3::BoundedInconclusive {
            reason,
            search_effort,
        } => Ok(RecordedTargetEvidence::BoundedInconclusive(
            BoundedInconclusiveEvidence {
                reason,
                search_effort,
            },
        )),
        ArtifactRehydrationEvidenceV3::PositiveReplay {
            witness_id,
            initial_digest,
            actions,
            search_effort,
        } => {
            let initial = Simulation::enter_via_door(
                generated.room.clone(),
                loadout.abilities(),
                source_door_id,
            )
            .map_err(|error| {
                invalid(format!(
                    "could not enter source door {source_door_id:?} during rehydration: {error}"
                ))
            })?;
            if initial.digest() != initial_digest {
                return Err(invalid(format!(
                    "persisted initial digest differs at source door {source_door_id:?}"
                )));
            }
            Ok(RecordedTargetEvidence::PositiveReplay {
                solution: TargetSolution {
                    target,
                    reached,
                    replay: Replay::record(&initial, actions),
                    stats: search_effort,
                },
                recorded_witness_id: witness_id,
            })
        }
    }
}

pub(super) fn summarize_rehydrated_evidence(
    evidence: &downwards_validation::DoorTargetEvidenceBatch,
) -> RouteMatrixSummary {
    let positive_door_rows = evidence
        .door_routes()
        .iter()
        .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        .count();
    let positive_pickup_rows = evidence
        .pickup_routes()
        .iter()
        .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
        .count();
    RouteMatrixSummary {
        door_rows: evidence.door_routes().len(),
        positive_door_rows,
        inconclusive_door_rows: evidence.door_routes().len() - positive_door_rows,
        pickup_rows: evidence.pickup_routes().len(),
        positive_pickup_rows,
        inconclusive_pickup_rows: evidence.pickup_routes().len() - positive_pickup_rows,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RehydratedCorpusV3 {
    pub seeds: Vec<u64>,
    pub build_configs: Vec<CorpusBuildConfigV2>,
    pub evaluation_configs: Vec<RouteEvaluationConfigV2>,
    pub ability_promotion_audit_config: super::CorpusRoomAnalysisConfigRecord,
    pub generation_summaries: Vec<(u64, GenerationBatchSummaryV2)>,
    pub construction_records: Vec<CorpusConstructionAttemptV2>,
    pub rooms: Vec<EvaluatedCorpusRoomV2>,
}

/// Exact durable source identity retained beside a process-local rehydrated
/// corpus. The checkpoint bytes are needed by downstream operational caches;
/// they remain meaningful only because the corresponding shard was fully
/// verified and byte-stability checked in the same load operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VerifiedRehydratedCorpusSourceV3 {
    pub seed: u64,
    pub config: CorpusBuildConfigV2,
    pub checkpoint: CorpusSeedCheckpointV3,
    pub checkpoint_bytes: Vec<u8>,
}

/// Process-local result of loading a directory set exactly once. This is not
/// serialized and cannot replace durable artifact verification.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VerifiedRehydratedCorpusV3 {
    pub corpus: RehydratedCorpusV3,
    pub sources: Vec<VerifiedRehydratedCorpusSourceV3>,
}

/// Load immutable checkpointed shards into one deterministic deep-analysis
/// pool. Seed/config policy and room identity collisions fail closed.
pub fn load_rehydrated_artifact_v3_shards(
    directories: &[PathBuf],
) -> Result<RehydratedCorpusV3, CorpusArtifactV3RehydrationError> {
    Ok(load_verified_rehydrated_artifact_v3_shards(directories)?.corpus)
}

pub(super) fn load_verified_rehydrated_artifact_v3_shards(
    directories: &[PathBuf],
) -> Result<VerifiedRehydratedCorpusV3, CorpusArtifactV3RehydrationError> {
    if directories.is_empty() {
        return Err(invalid("at least one artifact-v3 shard is required"));
    }
    let mut batches = Vec::with_capacity(directories.len());
    for directory in directories {
        let loaded = load_verified_rehydrated_seed_shard_v3(directory, None)?;
        batches.push(loaded);
    }
    batches.sort_unstable_by_key(|loaded| loaded.artifact.evaluated.config.start_seed);
    let mut seeds = BTreeSet::new();
    let reference = normalized_build_policy(&batches[0].artifact.evaluated.config);
    let evaluation_configs = batches[0].artifact.evaluated.evaluation_configs.clone();
    let ability_promotion_audit_config = batches[0]
        .artifact
        .verified
        .ability_promotion_audit_config
        .clone();
    let mut build_configs = Vec::with_capacity(batches.len());
    let mut generation_summaries = Vec::with_capacity(batches.len());
    let mut construction_records = Vec::new();
    let mut rooms = Vec::new();
    let mut room_ids = BTreeSet::new();
    let mut sources = Vec::with_capacity(batches.len());
    for VerifiedRehydratedSeedShardV3 {
        checkpoint,
        checkpoint_bytes,
        artifact,
    } in batches
    {
        let promotion_config = artifact.verified.ability_promotion_audit_config;
        let mut batch = artifact.evaluated;
        let seed = batch.config.start_seed;
        if !seeds.insert(seed) {
            return Err(invalid(format!("duplicate artifact-v3 seed {seed}")));
        }
        if normalized_build_policy(&batch.config) != reference
            || batch.evaluation_configs != evaluation_configs
            || promotion_config != ability_promotion_audit_config
        {
            return Err(invalid(format!(
                "artifact-v3 shard {seed} has a different build/evaluation policy"
            )));
        }
        for room in &batch.rooms {
            if !room_ids.insert(room.generated.id.clone()) {
                return Err(invalid(format!(
                    "duplicate physical room ID {:?} across artifact-v3 shards",
                    room.generated.id.0
                )));
            }
        }
        sources.push(VerifiedRehydratedCorpusSourceV3 {
            seed,
            config: batch.config.clone(),
            checkpoint,
            checkpoint_bytes,
        });
        generation_summaries.push((seed, batch.generation_summary));
        build_configs.push(batch.config);
        construction_records.append(&mut batch.construction_records);
        rooms.append(&mut batch.rooms);
    }
    construction_records.sort_unstable_by_key(|record| record.key.stable_slug());
    rooms.sort_unstable_by(|left, right| left.generated.id.cmp(&right.generated.id));
    Ok(VerifiedRehydratedCorpusV3 {
        corpus: RehydratedCorpusV3 {
            seeds: seeds.into_iter().collect(),
            build_configs,
            evaluation_configs,
            ability_promotion_audit_config,
            generation_summaries,
            construction_records,
            rooms,
        },
        sources,
    })
}

fn normalized_build_policy(config: &CorpusBuildConfigV2) -> CorpusBuildConfigV2 {
    let mut normalized = config.clone();
    normalized.start_seed = 0;
    normalized
}

fn invalid(message: impl Into<String>) -> CorpusArtifactV3RehydrationError {
    CorpusArtifactV3RehydrationError::Invalid(message.into())
}

#[derive(Debug)]
pub enum CorpusArtifactV3RehydrationError {
    Artifact(CorpusArtifactV3Error),
    Generation(CorpusGenerationV2Error),
    RoomValidation {
        room_id: RoomId,
        source: CorpusMetricInputV2Error,
    },
    Evidence {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source: DoorTargetEvidenceRehydrationError,
    },
    Invalid(String),
}

impl fmt::Display for CorpusArtifactV3RehydrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact(source) => source.fmt(formatter),
            Self::Generation(source) => source.fmt(formatter),
            Self::RoomValidation { room_id, source } => {
                write!(
                    formatter,
                    "rehydrated room {} is invalid: {source}",
                    room_id.0
                )
            }
            Self::Evidence {
                room_id,
                loadout,
                source,
            } => write!(
                formatter,
                "could not rehydrate evidence for {} under {}: {source}",
                room_id.0,
                loadout.slug()
            ),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for CorpusArtifactV3RehydrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Artifact(source) => Some(source),
            Self::Generation(source) => Some(source),
            Self::RoomValidation { source, .. } => Some(source),
            Self::Evidence { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<CorpusArtifactV3Error> for CorpusArtifactV3RehydrationError {
    fn from(source: CorpusArtifactV3Error) -> Self {
        Self::Artifact(source)
    }
}

impl From<CorpusGenerationV2Error> for CorpusArtifactV3RehydrationError {
    fn from(source: CorpusGenerationV2Error) -> Self {
        Self::Generation(source)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::OnceLock};

    use downwards_validation::ValidationConfig;

    use super::*;
    use crate::corpus::artifact_v3::{
        reset_semantic_verifier_passes_v3, semantic_verifier_passes_v3,
    };
    use crate::corpus::{
        evaluate_route_matrices_v2_with, generate_seed_block_v2, render_evaluated_artifact_v3,
        write_or_verify_seed_shard_v3,
    };

    fn evaluated_and_bundle() -> &'static (EvaluatedCorpusBatchV2, CorpusArtifactBundleV3) {
        static VALUE: OnceLock<(EvaluatedCorpusBatchV2, CorpusArtifactBundleV3)> = OnceLock::new();
        VALUE.get_or_init(|| {
            let generated =
                generate_seed_block_v2(CorpusBuildConfigV2::attempt_zero(0, 1)).unwrap();
            let evaluated = evaluate_route_matrices_v2_with(generated, |loadout| {
                let mut config = ValidationConfig::for_loadout(loadout.abilities());
                config.solver.max_expanded_nodes = 1;
                config.solver.max_simulated_ticks = 2_000;
                config
            })
            .unwrap();
            let bundle = render_evaluated_artifact_v3(&evaluated).unwrap();
            (evaluated, bundle)
        })
    }

    #[test]
    fn verified_bundle_rehydrates_to_the_exact_evaluated_batch_without_search() {
        let (evaluated, bundle) = evaluated_and_bundle();
        let rehydrated = rehydrate_artifact_bundle_v3(bundle).unwrap();
        assert_eq!(&rehydrated, evaluated);
    }

    #[test]
    fn multi_shard_loader_rejects_duplicate_seed_and_is_deterministic() {
        let (evaluated, bundle) = evaluated_and_bundle();
        let directory = std::env::temp_dir().join(format!(
            "downwards-corpus-v3-rehydrate-{}-{}",
            std::process::id(),
            evaluated.config.start_seed
        ));
        write_or_verify_seed_shard_v3(&directory, &evaluated.config, bundle).unwrap();
        reset_semantic_verifier_passes_v3();
        let one = load_rehydrated_artifact_v3_shards(std::slice::from_ref(&directory)).unwrap();
        assert_eq!(semantic_verifier_passes_v3(), 1);
        assert_eq!(one.rooms, evaluated.rooms);
        let error = load_rehydrated_artifact_v3_shards(&[directory.clone(), directory.clone()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate artifact-v3 seed"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }
}
