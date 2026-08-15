use std::collections::{HashMap, HashSet};

use downwards_gen::{
    CompositionalKey, CompositionalProfile, StagedCompositionalCandidate, StagedCompositionalKey,
    experimental::{ChallengeIntent, GenerationStrategy},
    generate_staged_compositional,
};
use downwards_lab::{SimulationGeometryDescriptor, StaticVisualDescriptor};

use super::{
    CandidateKeyRecord, ConstructionRecord, CorpusBuildConfigV1, CorpusConfigError,
    EvaluationLoadout, GenerationAttemptRecord, GenerationBatchSummary, RoomId,
    fingerprints::fingerprint_static_visual,
};

// This module is the historical staged-v6 pilot source. Its concrete
// `StagedCompositionalCandidate` storage is intentionally retained so v1/v2
// evidence shards can be regenerated exactly. The final multi-generator
// corpus path uses a tagged native-candidate schema; new generator families
// must not be flattened into this legacy key or made authoritative by the
// insertion order of `variants`.

/// Version of the stable room identity derivation.
///
/// Version 2 uses explicit canonical feature-stage slugs rather than Rust
/// `Debug` output, so identity does not depend on enum spelling.
pub const CORPUS_ROOM_ID_VERSION: u32 = 2;

#[derive(Clone, Debug)]
pub struct GeneratedCorpusRoom {
    pub id: RoomId,
    pub static_visual: StaticVisualDescriptor,
    pub simulation_geometry: SimulationGeometryDescriptor,
    /// Same exact static geometry can have multiple constructive explanations.
    /// They remain available for structural audits rather than being silently
    /// discarded during physical-room deduplication.
    pub variants: Vec<StagedCompositionalCandidate>,
}

#[derive(Clone, Debug)]
pub struct GeneratedCorpusBatch {
    pub config: CorpusBuildConfigV1,
    pub records: Vec<GenerationAttemptRecord>,
    pub rooms: Vec<GeneratedCorpusRoom>,
    pub summary: GenerationBatchSummary,
}

pub fn generate_seed_block(
    config: CorpusBuildConfigV1,
) -> Result<GeneratedCorpusBatch, CorpusConfigError> {
    config.validate()?;
    let mut successes = Vec::new();
    let mut rejected = Vec::new();

    for seed_offset in 0..config.seed_count {
        let seed = config.start_seed.wrapping_add(seed_offset as u64);
        for loadout in EvaluationLoadout::ALL {
            for strategy in GenerationStrategy::ALL {
                for intent in ChallengeIntent::ALL {
                    let source = CompositionalKey::new(
                        seed,
                        CompositionalProfile::new(loadout.abilities(), strategy, intent),
                    );
                    let key =
                        StagedCompositionalKey::new(source, config.feature_stage.feature_set());
                    let key_record = CandidateKeyRecord::from_staged_key(key);
                    match generate_staged_compositional(key) {
                        Ok(candidate) => successes.push((key_record, candidate)),
                        Err(error) => rejected.push(GenerationAttemptRecord {
                            key: key_record,
                            construction: ConstructionRecord::Rejected {
                                reason: error.to_string(),
                            },
                        }),
                    }
                }
            }
        }
    }

    // Static preview equality is the eventual distinct-geometry selection
    // gate, but it is not safe for evaluation deduplication: door arrivals
    // and timed schedules can change a rollout without changing the preview.
    // Evaluate each exact simulation geometry separately and retain the
    // static count as an independent selection diagnostic.
    let mut descriptor_to_room =
        HashMap::<(StaticVisualDescriptor, SimulationGeometryDescriptor), usize>::new();
    let mut exact_static_visuals = HashSet::<StaticVisualDescriptor>::new();
    let mut rooms = Vec::<GeneratedCorpusRoom>::new();
    let mut pending_records = Vec::with_capacity(successes.len());
    for (key, candidate) in successes {
        let static_visual = StaticVisualDescriptor::from_room(&candidate.generated.room);
        let simulation_geometry =
            SimulationGeometryDescriptor::from_room(&candidate.generated.room);
        exact_static_visuals.insert(static_visual.clone());
        let visual_fingerprint = fingerprint_static_visual(&static_visual);
        let descriptor_key = (static_visual.clone(), simulation_geometry.clone());
        let room_index = if let Some(index) = descriptor_to_room.get(&descriptor_key) {
            let index = *index;
            rooms[index].variants.push(candidate);
            index
        } else {
            let id = RoomId(format!(
                "room-v{CORPUS_ROOM_ID_VERSION}-{visual_fingerprint:016x}-{:016x}-{}",
                simulation_geometry.stable_digest(),
                key.stable_slug()
            ));
            let index = rooms.len();
            descriptor_to_room.insert(descriptor_key, index);
            rooms.push(GeneratedCorpusRoom {
                id,
                static_visual,
                simulation_geometry,
                variants: vec![candidate],
            });
            index
        };
        pending_records.push((key, room_index));
    }

    let mut records = Vec::with_capacity(pending_records.len() + rejected.len());
    for (key, room_index) in pending_records {
        let room = &rooms[room_index];
        let candidate = room
            .variants
            .iter()
            .find(|candidate| CandidateKeyRecord::from_staged_key(candidate.key) == key)
            .expect("the inserted candidate must remain in its exact-geometry group");
        let canonical = CandidateKeyRecord::from_staged_key(room.variants[0].key) == key;
        records.push(GenerationAttemptRecord {
            key,
            construction: ConstructionRecord::Constructed {
                room_id: room.id.clone(),
                static_visual_fingerprint: format!(
                    "downwards-corpus-static-v1-{:016x}",
                    fingerprint_static_visual(&room.static_visual)
                ),
                simulation_geometry_fingerprint: format!(
                    "downwards-simulation-geometry-v1-{:016x}",
                    room.simulation_geometry.stable_digest()
                ),
                port_count: candidate.generated.room.doors().len(),
                pickup_count: candidate.generated.room.pickups().len(),
                route_signature: format!("{:016x}", candidate.route_summary.signature),
                cycle_rank: candidate.route_summary.cycle_rank,
                canonical,
            },
        });
    }
    records.extend(rejected);
    records.sort_unstable_by(|left, right| left.key.cmp(&right.key));

    let constructed = records
        .iter()
        .filter(|record| matches!(record.construction, ConstructionRecord::Constructed { .. }))
        .count();
    let summary = GenerationBatchSummary {
        attempted: records.len(),
        constructed,
        rejected: records.len() - constructed,
        exact_static_rooms: exact_static_visuals.len(),
        alias_candidates: constructed - rooms.len(),
    };

    Ok(GeneratedCorpusBatch {
        config,
        records,
        rooms,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_seed_enumerates_every_profile_and_is_repeatable() {
        let config = CorpusBuildConfigV1::terrain_only_pilot(0, 1);
        let first = generate_seed_block(config.clone()).unwrap();
        let second = generate_seed_block(config).unwrap();

        assert_eq!(first.summary.attempted, 36);
        assert_eq!(first.summary.constructed + first.summary.rejected, 36);
        assert_eq!(first.records, second.records);
        assert_eq!(first.summary, second.summary);
        assert_eq!(first.rooms.len(), second.rooms.len());
        assert!(first.rooms.iter().all(|room| {
            room.variants.iter().all(|variant| {
                variant.key.features == downwards_gen::CompositionalFeatureSet::TerrainOnly
                    && variant.generated.room.timed_hazards().is_empty()
                    && !variant
                        .generated
                        .room
                        .tiles()
                        .iter()
                        .any(|tile| tile.is_hazard())
            })
        }));
    }

    #[test]
    fn invalid_config_is_rejected_before_generation() {
        let mut config = CorpusBuildConfigV1::terrain_only_pilot(0, 0);
        assert_eq!(
            generate_seed_block(config.clone()).unwrap_err(),
            CorpusConfigError::EmptySeedBlock
        );
        config.seed_count = 1;
        config.target_min_rooms = 10;
        config.target_max_rooms = 9;
        assert!(matches!(
            generate_seed_block(config).unwrap_err(),
            CorpusConfigError::InvalidTargetRange { .. }
        ));
    }
}
