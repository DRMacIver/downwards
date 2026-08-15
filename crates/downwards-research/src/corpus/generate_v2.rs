//! Construction and exact-geometry grouping for the final corpus path.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fmt,
};

use downwards_gen::experimental::{
    ChallengeIntent, CompositionalAbilityEdgeRewriteFailure, CompositionalAbilityGateProfile,
    CompositionalAbilityGenerationFailure, CompositionalAbilityGenerationKey,
    CompositionalRouteCutGenerationFailure, CompositionalRouteCutGrammar, CompositionalRouteCutKey,
    MissionDerivationFailure, PartitionRouteFailure, PartitionRouteKey, PartitionRouteProfile,
};
use serde::{Deserialize, Serialize};

use super::{
    CompositionalAbilityKeyRecord, CompositionalRouteCutKeyRecord, CorpusBuildConfigV2,
    CorpusBuildConfigV2Error, CorpusCandidate, CorpusCandidateKeyRecord,
    CorpusCandidateRegenerationError, CorpusPhysicalRoomDescriptorV3, EvaluationLoadout,
    PartitionRouteKeyRecord, RoomId, fingerprints::fingerprint_static_visual,
};

/// One exact key's construction result. Generation failures are evidence, not
/// reasons to omit an attempted coordinate from the batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum CorpusConstructionOutcomeV2 {
    Constructed {
        room_id: RoomId,
        static_visual_fingerprint: String,
        simulation_geometry_fingerprint: String,
        port_count: usize,
        pickup_count: usize,
    },
    Rejected {
        generator: super::CorpusCandidateGenerator,
        failure_class: CorpusConstructionFailureClassV2,
        detail: String,
    },
}

/// Stable rejection category for deficit accounting. Exact native errors stay
/// in `detail`; downstream code never has to parse their wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusConstructionFailureClassV2 {
    KeyVersionMismatch,
    UnsupportedEmbeddingAttempt,
    GraphDerivationExhausted,
    MissionDerivationExhausted,
    EmbeddingExhausted,
    RhythmExhausted,
    MissingEmbeddedMissionNode,
    SupportConstraintExhausted,
    ForkEmbeddingExhausted,
    AbilityRewriteExhausted,
    AbilityRewriteContract,
    AbilityConstraintSearchExhausted,
    AbilityGateContract,
    PortContract,
    RoomInvariant,
    DoorInvariant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusConstructionAttemptV2 {
    pub key: CorpusCandidateKeyRecord,
    pub outcome: CorpusConstructionOutcomeV2,
}

/// One exact physical room and all native constructive explanations for it.
///
/// Variants are sorted by their exact serialized key. Their order carries no
/// feasibility or canonical-presentation authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCorpusRoomV2 {
    pub id: RoomId,
    pub physical_descriptor: CorpusPhysicalRoomDescriptorV3,
    pub variants: Vec<CorpusCandidate>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationBatchSummaryV2 {
    pub attempted: usize,
    pub constructed: usize,
    pub rejected: usize,
    pub physical_rooms: usize,
    pub exact_static_rooms: usize,
    pub alias_candidates: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCorpusBatchV2 {
    pub config: CorpusBuildConfigV2,
    pub construction_records: Vec<CorpusConstructionAttemptV2>,
    pub rooms: Vec<GeneratedCorpusRoomV2>,
    pub summary: GenerationBatchSummaryV2,
}

/// Enumerate the complete exact attempt-zero key set required by a v2 batch.
impl CorpusBuildConfigV2 {
    pub fn exact_candidate_keys(
        &self,
    ) -> Result<Vec<CorpusCandidateKeyRecord>, CorpusBuildConfigV2Error> {
        self.validate()?;
        let mut keys = Vec::with_capacity(self.seed_count.saturating_mul(15));
        for seed_offset in 0..self.seed_count {
            let seed_offset = u64::try_from(seed_offset)
                .expect("validated corpus-v2 seed count fits the u64 seed domain");
            let source_seed = self
                .start_seed
                .checked_add(seed_offset)
                .expect("validated corpus-v2 seed range does not overflow");
            for intent in ChallengeIntent::ALL {
                for profile in PartitionRouteProfile::ALL {
                    let key = PartitionRouteKey::new(
                        source_seed,
                        EvaluationLoadout::Baseline.abilities(),
                        intent,
                        profile,
                    )
                    .with_embedding_attempt(self.embedding_attempt);
                    keys.push(CorpusCandidateKeyRecord::PartitionRoute(
                        PartitionRouteKeyRecord::from(key),
                    ));
                }
                let key = CompositionalRouteCutKey::new(
                    source_seed,
                    EvaluationLoadout::Baseline.abilities(),
                    intent,
                )
                .with_embedding(
                    CompositionalRouteCutGrammar::RecursiveMissionCutsV1,
                    self.embedding_attempt,
                );
                keys.push(CorpusCandidateKeyRecord::CompositionalRouteCut(
                    CompositionalRouteCutKeyRecord::from(key),
                ));
                let key = CompositionalAbilityGenerationKey::new(
                    source_seed,
                    CompositionalAbilityGateProfile::Dash,
                    intent,
                )
                .with_attempts(self.embedding_attempt, self.ability_rewrite_attempt);
                keys.push(CorpusCandidateKeyRecord::CompositionalAbility(
                    CompositionalAbilityKeyRecord::try_from(key)
                        .expect("enumeration contains the certified Dash profile only"),
                ));
            }
        }
        keys.sort_unstable_by_key(CorpusCandidateKeyRecord::stable_slug);
        Ok(keys)
    }
}

/// Construct every explicit key, retaining typed success/failure records and
/// grouping aliases only by complete descriptor equality.
pub fn generate_seed_block_v2(
    config: CorpusBuildConfigV2,
) -> Result<GeneratedCorpusBatchV2, CorpusGenerationV2Error> {
    let keys = config
        .exact_candidate_keys()
        .map_err(CorpusGenerationV2Error::Config)?;
    let mut successes = Vec::new();
    let mut construction_records = Vec::with_capacity(keys.len());
    for key in keys {
        match key.regenerate() {
            Ok(candidate) => successes.push(candidate),
            Err(error) => {
                let failure_class = classify_construction_failure(&error);
                construction_records.push(CorpusConstructionAttemptV2 {
                    outcome: CorpusConstructionOutcomeV2::Rejected {
                        generator: key.generator(),
                        failure_class,
                        detail: error.to_string(),
                    },
                    key,
                });
            }
        }
    }

    let (rooms, key_to_room) =
        group_constructed_candidates_with_id(successes, |descriptor| descriptor.room_id())?;
    let mut exact_static_rooms = HashSet::new();
    for room in &rooms {
        exact_static_rooms.insert(room.physical_descriptor.static_visual.clone());
        for candidate in &room.variants {
            let key = candidate.exact_key();
            debug_assert_eq!(key_to_room.get(&key), Some(&room.id));
            construction_records.push(CorpusConstructionAttemptV2 {
                key,
                outcome: CorpusConstructionOutcomeV2::Constructed {
                    room_id: room.id.clone(),
                    static_visual_fingerprint: format!(
                        "downwards-corpus-static-v1-{:016x}",
                        fingerprint_static_visual(&room.physical_descriptor.static_visual)
                    ),
                    simulation_geometry_fingerprint: format!(
                        "downwards-simulation-geometry-v1-{:016x}",
                        room.physical_descriptor.simulation_geometry.stable_digest()
                    ),
                    port_count: candidate.generated().room.doors().len(),
                    pickup_count: candidate.generated().room.pickups().len(),
                },
            });
        }
    }
    construction_records.sort_unstable_by_key(|record| record.key.stable_slug());

    let constructed = key_to_room.len();
    let summary = GenerationBatchSummaryV2 {
        attempted: construction_records.len(),
        constructed,
        rejected: construction_records.len() - constructed,
        physical_rooms: rooms.len(),
        exact_static_rooms: exact_static_rooms.len(),
        alias_candidates: constructed - rooms.len(),
    };
    Ok(GeneratedCorpusBatchV2 {
        config,
        construction_records,
        rooms,
        summary,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusGenerationV2Error {
    Config(CorpusBuildConfigV2Error),
    DuplicateExactKey {
        key: CorpusCandidateKeyRecord,
    },
    CompactRoomIdCollision {
        room_id: RoomId,
        existing_static_fingerprint: u64,
        existing_simulation_fingerprint: u64,
        colliding_static_fingerprint: u64,
        colliding_simulation_fingerprint: u64,
    },
}

impl fmt::Display for CorpusGenerationV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => error.fmt(formatter),
            Self::DuplicateExactKey { key } => {
                write!(
                    formatter,
                    "duplicate exact corpus-v2 key {}",
                    key.stable_slug()
                )
            }
            Self::CompactRoomIdCollision {
                room_id,
                existing_static_fingerprint,
                existing_simulation_fingerprint,
                colliding_static_fingerprint,
                colliding_simulation_fingerprint,
            } => write!(
                formatter,
                "compact room ID {} collided between exact descriptor pairs {:016x}/{:016x} and {:016x}/{:016x}",
                room_id.0,
                existing_static_fingerprint,
                existing_simulation_fingerprint,
                colliding_static_fingerprint,
                colliding_simulation_fingerprint,
            ),
        }
    }
}

impl Error for CorpusGenerationV2Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::DuplicateExactKey { .. } | Self::CompactRoomIdCollision { .. } => None,
        }
    }
}

fn group_constructed_candidates_with_id(
    mut candidates: Vec<CorpusCandidate>,
    mut id_for_descriptor: impl FnMut(&CorpusPhysicalRoomDescriptorV3) -> RoomId,
) -> Result<
    (
        Vec<GeneratedCorpusRoomV2>,
        BTreeMap<CorpusCandidateKeyRecord, RoomId>,
    ),
    CorpusGenerationV2Error,
> {
    candidates.sort_unstable_by_key(|candidate| candidate.exact_key().stable_slug());
    let mut descriptor_to_room = HashMap::<CorpusPhysicalRoomDescriptorV3, usize>::new();
    let mut id_to_descriptor = HashMap::<RoomId, CorpusPhysicalRoomDescriptorV3>::new();
    let mut key_to_room = BTreeMap::new();
    let mut rooms = Vec::<GeneratedCorpusRoomV2>::new();

    for candidate in candidates {
        let key = candidate.exact_key();
        if key_to_room.contains_key(&key) {
            return Err(CorpusGenerationV2Error::DuplicateExactKey { key });
        }
        let descriptor = candidate.physical_room_descriptor_v3();
        let room_id = id_for_descriptor(&descriptor);
        if let Some(existing) = id_to_descriptor.get(&room_id)
            && existing != &descriptor
        {
            return Err(compact_id_collision(room_id, existing, &descriptor));
        }
        id_to_descriptor
            .entry(room_id.clone())
            .or_insert_with(|| descriptor.clone());

        let room_index = if let Some(&room_index) = descriptor_to_room.get(&descriptor) {
            debug_assert_eq!(rooms[room_index].id, room_id);
            room_index
        } else {
            let room_index = rooms.len();
            descriptor_to_room.insert(descriptor.clone(), room_index);
            rooms.push(GeneratedCorpusRoomV2 {
                id: room_id.clone(),
                physical_descriptor: descriptor,
                variants: Vec::new(),
            });
            room_index
        };
        rooms[room_index].variants.push(candidate);
        key_to_room.insert(key, room_id);
    }

    for room in &mut rooms {
        room.variants
            .sort_unstable_by_key(|candidate| candidate.exact_key().stable_slug());
    }
    rooms.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    Ok((rooms, key_to_room))
}

fn compact_id_collision(
    room_id: RoomId,
    existing: &CorpusPhysicalRoomDescriptorV3,
    colliding: &CorpusPhysicalRoomDescriptorV3,
) -> CorpusGenerationV2Error {
    CorpusGenerationV2Error::CompactRoomIdCollision {
        room_id,
        existing_static_fingerprint: fingerprint_static_visual(&existing.static_visual),
        existing_simulation_fingerprint: existing.simulation_geometry.stable_digest(),
        colliding_static_fingerprint: fingerprint_static_visual(&colliding.static_visual),
        colliding_simulation_fingerprint: colliding.simulation_geometry.stable_digest(),
    }
}

fn classify_construction_failure(
    error: &CorpusCandidateRegenerationError,
) -> CorpusConstructionFailureClassV2 {
    match error {
        CorpusCandidateRegenerationError::Key(_) => {
            CorpusConstructionFailureClassV2::KeyVersionMismatch
        }
        CorpusCandidateRegenerationError::PartitionRoute(error) => match &error.cause {
            PartitionRouteFailure::UnsupportedEmbeddingAttempt { .. } => {
                CorpusConstructionFailureClassV2::UnsupportedEmbeddingAttempt
            }
            PartitionRouteFailure::GraphExhausted(_) => {
                CorpusConstructionFailureClassV2::GraphDerivationExhausted
            }
            PartitionRouteFailure::EmbeddingExhausted(_) => {
                CorpusConstructionFailureClassV2::EmbeddingExhausted
            }
            PartitionRouteFailure::PortContract(_) => {
                CorpusConstructionFailureClassV2::PortContract
            }
            PartitionRouteFailure::Room(_) => CorpusConstructionFailureClassV2::RoomInvariant,
            PartitionRouteFailure::Door(_) => CorpusConstructionFailureClassV2::DoorInvariant,
        },
        CorpusCandidateRegenerationError::CompositionalRouteCut(error) => {
            classify_compositional_route_cut_failure(&error.cause)
        }
        CorpusCandidateRegenerationError::CompositionalAbility(error) => {
            classify_compositional_ability_failure(&error.cause)
        }
    }
}

fn classify_mission_derivation_failure(
    cause: &MissionDerivationFailure,
) -> CorpusConstructionFailureClassV2 {
    match cause {
        MissionDerivationFailure::UnsupportedEmbeddingAttempt { .. } => {
            CorpusConstructionFailureClassV2::UnsupportedEmbeddingAttempt
        }
        MissionDerivationFailure::CutPlacementExhausted { .. }
        | MissionDerivationFailure::ForkPlacementExhausted { .. }
        | MissionDerivationFailure::NoPickupNode
        | MissionDerivationFailure::NoPortAttachmentNode => {
            CorpusConstructionFailureClassV2::MissionDerivationExhausted
        }
    }
}

fn classify_compositional_route_cut_failure(
    cause: &CompositionalRouteCutGenerationFailure,
) -> CorpusConstructionFailureClassV2 {
    match cause {
        CompositionalRouteCutGenerationFailure::Mission(cause) => {
            classify_mission_derivation_failure(cause)
        }
        CompositionalRouteCutGenerationFailure::RhythmExhausted => {
            CorpusConstructionFailureClassV2::RhythmExhausted
        }
        CompositionalRouteCutGenerationFailure::MissingMissionNode { .. } => {
            CorpusConstructionFailureClassV2::MissingEmbeddedMissionNode
        }
        CompositionalRouteCutGenerationFailure::SupportConstraintExhausted { .. } => {
            CorpusConstructionFailureClassV2::SupportConstraintExhausted
        }
        CompositionalRouteCutGenerationFailure::ForkEmbeddingExhausted { .. } => {
            CorpusConstructionFailureClassV2::ForkEmbeddingExhausted
        }
        CompositionalRouteCutGenerationFailure::PortContract(_) => {
            CorpusConstructionFailureClassV2::PortContract
        }
        CompositionalRouteCutGenerationFailure::Room(_) => {
            CorpusConstructionFailureClassV2::RoomInvariant
        }
        CompositionalRouteCutGenerationFailure::Door(_) => {
            CorpusConstructionFailureClassV2::DoorInvariant
        }
    }
}

fn classify_compositional_ability_failure(
    cause: &CompositionalAbilityGenerationFailure,
) -> CorpusConstructionFailureClassV2 {
    match cause {
        CompositionalAbilityGenerationFailure::Mission(cause) => {
            classify_mission_derivation_failure(cause)
        }
        CompositionalAbilityGenerationFailure::Rewrite(cause) => match cause {
            CompositionalAbilityEdgeRewriteFailure::CandidateSearchBoundExceeded { .. }
            | CompositionalAbilityEdgeRewriteFailure::InsufficientUnavoidableEdges { .. }
            | CompositionalAbilityEdgeRewriteFailure::NoSeparatedUnavoidableEdgeArrangement {
                ..
            }
            | CompositionalAbilityEdgeRewriteFailure::ArrangementAttemptExhausted { .. } => {
                CorpusConstructionFailureClassV2::AbilityRewriteExhausted
            }
            CompositionalAbilityEdgeRewriteFailure::KeyDoesNotMatchMission
            | CompositionalAbilityEdgeRewriteFailure::BaseMissionIsNotBaseline { .. }
            | CompositionalAbilityEdgeRewriteFailure::InvalidMission(_)
            | CompositionalAbilityEdgeRewriteFailure::CertificationFailed { .. } => {
                CorpusConstructionFailureClassV2::AbilityRewriteContract
            }
        },
        CompositionalAbilityGenerationFailure::BaselineEmbedding(cause) => {
            classify_compositional_route_cut_failure(cause)
        }
        CompositionalAbilityGenerationFailure::ConstraintSearchExhausted { .. } => {
            CorpusConstructionFailureClassV2::AbilityConstraintSearchExhausted
        }
        CompositionalAbilityGenerationFailure::GateContract { .. } => {
            CorpusConstructionFailureClassV2::AbilityGateContract
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::AbilitySet;

    use super::*;
    use crate::corpus::{
        ChallengeIntentRecord, CompositionalAbilityGateProfileRecord, CorpusCandidateGenerator,
    };

    #[test]
    fn exact_enumeration_is_deterministic_complete_and_attempt_zero() {
        let config = CorpusBuildConfigV2::attempt_zero(41, 2);
        let first = config.exact_candidate_keys().unwrap();
        let second = config.exact_candidate_keys().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 30);
        assert_eq!(first.iter().collect::<HashSet<_>>().len(), 30);
        assert!(first.iter().all(|key| key.embedding_attempt() == 0));
        assert!(first.iter().all(|key| match key.generator() {
            CorpusCandidateGenerator::CompositionalAbility => key.rewrite_attempt() == Some(0),
            CorpusCandidateGenerator::PartitionRoute
            | CorpusCandidateGenerator::CompositionalRouteCut => key.rewrite_attempt().is_none(),
        }));
        assert_eq!(
            first
                .iter()
                .filter(|key| key.generator() == CorpusCandidateGenerator::PartitionRoute)
                .count(),
            18
        );
        assert_eq!(
            first
                .iter()
                .filter(|key| key.generator() == CorpusCandidateGenerator::CompositionalRouteCut)
                .count(),
            6
        );
        assert_eq!(
            first
                .iter()
                .filter(|key| key.generator() == CorpusCandidateGenerator::CompositionalAbility)
                .count(),
            6
        );
        for seed in [41, 42] {
            let keys = first
                .iter()
                .filter(|key| key.source_seed() == seed)
                .collect::<Vec<_>>();
            assert_eq!(keys.len(), 15);
            for loadout in EvaluationLoadout::ALL {
                assert_eq!(
                    keys.iter()
                        .filter(|key| key.construction_loadout() == loadout)
                        .count(),
                    match loadout {
                        EvaluationLoadout::Baseline => 12,
                        EvaluationLoadout::WallJump => 0,
                        EvaluationLoadout::Dash => 3,
                        EvaluationLoadout::Both => 0,
                    }
                );
            }
            for intent in [
                ChallengeIntentRecord::Gentle,
                ChallengeIntentRecord::Standard,
                ChallengeIntentRecord::Technical,
            ] {
                assert_eq!(
                    keys.iter()
                        .filter(|key| match key {
                            CorpusCandidateKeyRecord::PartitionRoute(record) => {
                                record.intent == intent
                            }
                            CorpusCandidateKeyRecord::CompositionalRouteCut(record) => {
                                record.intent == intent
                            }
                            CorpusCandidateKeyRecord::CompositionalAbility(record) => {
                                record.base_key.intent == intent
                            }
                        })
                        .count(),
                    5
                );
            }
        }
    }

    #[test]
    fn generation_records_every_attempt_and_repeats_exactly() {
        let config = CorpusBuildConfigV2::attempt_zero(0, 1);
        let first = generate_seed_block_v2(config.clone()).unwrap();
        let second = generate_seed_block_v2(config).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.summary.attempted, 15);
        assert_eq!(first.construction_records.len(), 15);
        for record in &first.construction_records {
            if let CorpusConstructionOutcomeV2::Rejected { generator, .. } = &record.outcome {
                assert_eq!(*generator, record.key.generator());
            }
        }
        assert_eq!(
            first.summary.constructed + first.summary.rejected,
            first.summary.attempted
        );
        let frozen = first
            .construction_records
            .iter()
            .find(|record| {
                matches!(
                    &record.key,
                    CorpusCandidateKeyRecord::CompositionalAbility(key)
                        if key.base_key.source_seed == 0
                            && key.base_key.intent == ChallengeIntentRecord::Standard
                            && key.profile == CompositionalAbilityGateProfileRecord::Dash
                )
            })
            .expect("the frozen Standard seed-zero Dash key is enumerated");
        assert!(matches!(
            frozen.outcome,
            CorpusConstructionOutcomeV2::Constructed { .. }
        ));
        assert_eq!(
            first.summary.alias_candidates,
            first.summary.constructed - first.summary.physical_rooms
        );
        for room in &first.rooms {
            assert_eq!(room.id, room.physical_descriptor.room_id());
            assert!(!room.variants.is_empty());
            assert!(room.variants.windows(2).all(|pair| {
                pair[0].exact_key().stable_slug() < pair[1].exact_key().stable_slug()
            }));
            assert!(room.variants.iter().all(|candidate| {
                candidate.physical_room_descriptor_v3() == room.physical_descriptor
            }));
        }
    }

    #[test]
    fn exact_descriptor_aliases_retain_both_sorted_native_variants() {
        let native_key = PartitionRouteKey::new(
            3,
            AbilitySet::NONE,
            ChallengeIntent::Standard,
            PartitionRouteProfile::MixedBsp,
        );
        let first_native = native_key.regenerate().unwrap();
        let mut alias_native = first_native.clone();
        alias_native.key = native_key.with_embedding_attempt(1);
        let first = CorpusCandidate::from(first_native);
        let alias = CorpusCandidate::from(alias_native);
        let (rooms, key_to_room) =
            group_constructed_candidates_with_id(vec![alias, first], |descriptor| {
                descriptor.room_id()
            })
            .unwrap();

        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].variants.len(), 2);
        assert_eq!(key_to_room.len(), 2);
        assert!(
            rooms[0].variants[0].exact_key().stable_slug()
                < rooms[0].variants[1].exact_key().stable_slug()
        );
    }

    #[test]
    fn ability_descriptor_aliases_use_the_same_exact_physical_grouping_truth() {
        let native_key = CompositionalAbilityGenerationKey::new(
            0,
            CompositionalAbilityGateProfile::WallJump,
            ChallengeIntent::Standard,
        );
        let first_native = native_key.generate().unwrap();
        let mut alias_native = first_native.clone();
        alias_native.key = native_key.with_attempts(0, 1);
        let first = CorpusCandidate::try_from(first_native).unwrap();
        let alias = CorpusCandidate::try_from(alias_native).unwrap();
        let (rooms, key_to_room) =
            group_constructed_candidates_with_id(vec![alias, first], |descriptor| {
                descriptor.room_id()
            })
            .unwrap();

        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].variants.len(), 2);
        assert_eq!(key_to_room.len(), 2);
        assert!(rooms[0].variants.iter().all(|candidate| {
            candidate.generator() == CorpusCandidateGenerator::CompositionalAbility
                && candidate.physical_room_descriptor_v3() == rooms[0].physical_descriptor
        }));
    }

    #[test]
    fn compact_id_collision_is_detected_before_descriptor_groups_merge() {
        let first = CorpusCandidate::from(
            CompositionalRouteCutKey::new(0, AbilitySet::ALL, ChallengeIntent::Standard)
                .regenerate()
                .unwrap(),
        );
        let second = CorpusCandidate::from(
            CompositionalRouteCutKey::new(1, AbilitySet::ALL, ChallengeIntent::Standard)
                .regenerate()
                .unwrap(),
        );
        assert_ne!(
            first.physical_room_descriptor_v3(),
            second.physical_room_descriptor_v3()
        );
        let error = group_constructed_candidates_with_id(vec![first, second], |_| {
            RoomId("forced-collision".to_owned())
        })
        .unwrap_err();
        assert!(matches!(
            error,
            CorpusGenerationV2Error::CompactRoomIdCollision { .. }
        ));
    }

    #[test]
    fn rejection_classification_is_typed_independently_of_detail_text() {
        let mut record = PartitionRouteKeyRecord::from(PartitionRouteKey::new(
            0,
            AbilitySet::NONE,
            ChallengeIntent::Gentle,
            PartitionRouteProfile::MixedBsp,
        ));
        record.generator_version += 1;
        let key = CorpusCandidateKeyRecord::PartitionRoute(record);
        let error = key.regenerate().unwrap_err();
        assert_eq!(
            classify_construction_failure(&error),
            CorpusConstructionFailureClassV2::KeyVersionMismatch
        );
        let outcome = CorpusConstructionOutcomeV2::Rejected {
            generator: key.generator(),
            failure_class: classify_construction_failure(&error),
            detail: error.to_string(),
        };
        let encoded = serde_json::to_value(outcome).unwrap();
        assert_eq!(encoded["generator"], "partition-route");
        assert_eq!(encoded["failure_class"], "key-version-mismatch");
        assert!(
            encoded["detail"]
                .as_str()
                .is_some_and(|detail| !detail.is_empty())
        );
    }

    #[test]
    fn exact_ability_failure_is_deterministic_typed_and_never_retried() {
        let native_key = CompositionalAbilityGenerationKey::new(
            0,
            CompositionalAbilityGateProfile::Dash,
            ChallengeIntent::Standard,
        )
        .with_attempts(0, u16::MAX);
        let key = CorpusCandidateKeyRecord::CompositionalAbility(
            CompositionalAbilityKeyRecord::try_from(native_key).unwrap(),
        );
        let first = key.regenerate().unwrap_err();
        let second = key.regenerate().unwrap_err();
        assert_eq!(first.to_string(), second.to_string());
        assert_eq!(
            classify_construction_failure(&first),
            CorpusConstructionFailureClassV2::AbilityRewriteExhausted
        );
        let outcome = CorpusConstructionOutcomeV2::Rejected {
            generator: key.generator(),
            failure_class: classify_construction_failure(&first),
            detail: first.to_string(),
        };
        let encoded = serde_json::to_value(outcome).unwrap();
        assert_eq!(encoded["generator"], "compositional-ability");
        assert_eq!(encoded["failure_class"], "ability-rewrite-exhausted");
    }
}
