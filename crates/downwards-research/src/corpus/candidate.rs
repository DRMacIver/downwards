//! Generator-neutral candidates for the final corpus pipeline.
//!
//! The historical staged-v6 corpus formats deliberately do not use these
//! types.  A [`CorpusCandidate`] owns its generator's complete native result;
//! the common view borrows shared facts without flattening derivation
//! evidence or inventing a canonical alias.

use std::{error::Error, fmt};

use downwards_core::{AbilitySet, Room};
use downwards_gen::{
    GeneratedLevel,
    experimental::{
        AbilityGateEmbeddingState, AbilityRewrittenMission, BoundaryPort,
        COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
        COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
        COMPOSITIONAL_ABILITY_GENERATION_VERSION, COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
        COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
        COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION, ChallengeIntent,
        CompositionalAbilityCandidate, CompositionalAbilityEdgeRewriteKey,
        CompositionalAbilityEmbeddingSummary, CompositionalAbilityGateProfile,
        CompositionalAbilityGenerationError, CompositionalAbilityGenerationKey,
        CompositionalRouteCutCandidate, CompositionalRouteCutEmbeddingSummary,
        CompositionalRouteCutGenerationError, CompositionalRouteCutGrammar,
        CompositionalRouteCutKey, DerivedMission, MissionRouteNodeMapping,
        PARTITION_ROUTE_GENERATION_VERSION, PartitionDerivation, PartitionRouteCandidate,
        PartitionRouteGenerationError, PartitionRouteKey, PartitionRouteProfile,
        PartitionRouteSummary, RoutePlan, RoutePlanSummary,
    },
};
use downwards_lab::{SimulationGeometryDescriptor, StaticVisualDescriptor};
use serde::{Deserialize, Serialize};

use super::{EvaluationLoadout, RoomId, fingerprints::fingerprint_static_visual};

/// Version of the generator-neutral exact-key serialization.
pub const CORPUS_CANDIDATE_KEY_RECORD_VERSION: u32 = 2;

/// Version of physical-room IDs used by the multi-generator corpus.
///
/// Unlike historical room-v2 IDs, room-v3 IDs contain no candidate key or
/// insertion-order choice.  The ID is a compact display/index value; exact
/// [`CorpusPhysicalRoomDescriptorV3`] equality remains grouping truth.
pub const CORPUS_PHYSICAL_ROOM_ID_VERSION: u32 = 3;

/// Stable source-family tag for diagnostics and version errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusCandidateGenerator {
    PartitionRoute,
    CompositionalRouteCut,
    CompositionalAbility,
}

impl CorpusCandidateGenerator {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::PartitionRoute => "partition-route",
            Self::CompositionalRouteCut => "compositional-route-cut",
            Self::CompositionalAbility => "compositional-ability",
        }
    }
}

/// Serialized mirror of the generator's challenge-intent coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChallengeIntentRecord {
    Gentle,
    Standard,
    Technical,
}

impl ChallengeIntentRecord {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Standard => "standard",
            Self::Technical => "technical",
        }
    }
}

impl From<ChallengeIntent> for ChallengeIntentRecord {
    fn from(value: ChallengeIntent) -> Self {
        match value {
            ChallengeIntent::Gentle => Self::Gentle,
            ChallengeIntent::Standard => Self::Standard,
            ChallengeIntent::Technical => Self::Technical,
        }
    }
}

impl From<ChallengeIntentRecord> for ChallengeIntent {
    fn from(value: ChallengeIntentRecord) -> Self {
        match value {
            ChallengeIntentRecord::Gentle => Self::Gentle,
            ChallengeIntentRecord::Standard => Self::Standard,
            ChallengeIntentRecord::Technical => Self::Technical,
        }
    }
}

/// Serialized mirror of the partition-route graph bias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PartitionRouteProfileRecord {
    MixedBsp,
    Columnar,
    Branching,
}

impl PartitionRouteProfileRecord {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::MixedBsp => "mixed-bsp",
            Self::Columnar => "columnar",
            Self::Branching => "branching",
        }
    }
}

impl From<PartitionRouteProfile> for PartitionRouteProfileRecord {
    fn from(value: PartitionRouteProfile) -> Self {
        match value {
            PartitionRouteProfile::MixedBsp => Self::MixedBsp,
            PartitionRouteProfile::Columnar => Self::Columnar,
            PartitionRouteProfile::Branching => Self::Branching,
        }
    }
}

impl From<PartitionRouteProfileRecord> for PartitionRouteProfile {
    fn from(value: PartitionRouteProfileRecord) -> Self {
        match value {
            PartitionRouteProfileRecord::MixedBsp => Self::MixedBsp,
            PartitionRouteProfileRecord::Columnar => Self::Columnar,
            PartitionRouteProfileRecord::Branching => Self::Branching,
        }
    }
}

/// Serialized mirror of the compositional route-cut grammar coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionalRouteCutGrammarRecord {
    RecursiveMissionCutsV1,
}

impl CompositionalRouteCutGrammarRecord {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::RecursiveMissionCutsV1 => "recursive-mission-cuts-v1",
        }
    }
}

impl From<CompositionalRouteCutGrammar> for CompositionalRouteCutGrammarRecord {
    fn from(value: CompositionalRouteCutGrammar) -> Self {
        match value {
            CompositionalRouteCutGrammar::RecursiveMissionCutsV1 => Self::RecursiveMissionCutsV1,
        }
    }
}

impl From<CompositionalRouteCutGrammarRecord> for CompositionalRouteCutGrammar {
    fn from(value: CompositionalRouteCutGrammarRecord) -> Self {
        match value {
            CompositionalRouteCutGrammarRecord::RecursiveMissionCutsV1 => {
                Self::RecursiveMissionCutsV1
            }
        }
    }
}

/// Promoted single-ability physical profiles. The native combined profile is
/// deliberately absent until it has its own positive promotion evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionalAbilityGateProfileRecord {
    WallJump,
    Dash,
}

impl CompositionalAbilityGateProfileRecord {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
        }
    }

    #[must_use]
    pub const fn construction_loadout(self) -> EvaluationLoadout {
        match self {
            Self::WallJump => EvaluationLoadout::WallJump,
            Self::Dash => EvaluationLoadout::Dash,
        }
    }
}

impl From<CompositionalAbilityGateProfileRecord> for CompositionalAbilityGateProfile {
    fn from(value: CompositionalAbilityGateProfileRecord) -> Self {
        match value {
            CompositionalAbilityGateProfileRecord::WallJump => Self::WallJump,
            CompositionalAbilityGateProfileRecord::Dash => Self::Dash,
        }
    }
}

impl TryFrom<CompositionalAbilityGateProfile> for CompositionalAbilityGateProfileRecord {
    type Error = CorpusCandidateKeyError;

    fn try_from(value: CompositionalAbilityGateProfile) -> Result<Self, Self::Error> {
        match value {
            CompositionalAbilityGateProfile::WallJump => Ok(Self::WallJump),
            CompositionalAbilityGateProfile::Dash => Ok(Self::Dash),
            CompositionalAbilityGateProfile::Both => {
                Err(CorpusCandidateKeyError::UnpromotedAbilityProfile { profile: value })
            }
        }
    }
}

/// Complete serialized regeneration identity for one partition-route room.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionRouteKeyRecord {
    pub record_version: u32,
    pub generator_version: u32,
    pub source_seed: u64,
    pub construction_loadout: EvaluationLoadout,
    pub intent: ChallengeIntentRecord,
    pub profile: PartitionRouteProfileRecord,
    pub embedding_attempt: u8,
}

impl From<PartitionRouteKey> for PartitionRouteKeyRecord {
    fn from(key: PartitionRouteKey) -> Self {
        Self {
            record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            generator_version: PARTITION_ROUTE_GENERATION_VERSION,
            source_seed: key.source_seed,
            construction_loadout: loadout_from_abilities(key.construction_abilities),
            intent: key.intent.into(),
            profile: key.profile.into(),
            embedding_attempt: key.embedding_attempt,
        }
    }
}

impl PartitionRouteKeyRecord {
    /// Validate record versions and recover the exact native key.
    pub fn to_native_key(&self) -> Result<PartitionRouteKey, CorpusCandidateKeyError> {
        validate_record_version(
            CorpusCandidateGenerator::PartitionRoute,
            self.record_version,
        )?;
        validate_generator_version(
            CorpusCandidateGenerator::PartitionRoute,
            PARTITION_ROUTE_GENERATION_VERSION,
            self.generator_version,
        )?;
        Ok(PartitionRouteKey::new(
            self.source_seed,
            self.construction_loadout.abilities(),
            self.intent.into(),
            self.profile.into(),
        )
        .with_embedding_attempt(self.embedding_attempt))
    }
}

/// Complete serialized regeneration identity for one compositional route-cut
/// room.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionalRouteCutKeyRecord {
    pub record_version: u32,
    pub derivation_version: u32,
    pub generator_version: u32,
    pub socket_inventory_version: u32,
    pub source_seed: u64,
    pub construction_loadout: EvaluationLoadout,
    pub intent: ChallengeIntentRecord,
    pub grammar: CompositionalRouteCutGrammarRecord,
    pub embedding_attempt: u8,
}

impl From<CompositionalRouteCutKey> for CompositionalRouteCutKeyRecord {
    fn from(key: CompositionalRouteCutKey) -> Self {
        Self {
            record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            derivation_version: COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            generator_version: COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            socket_inventory_version: COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            source_seed: key.source_seed,
            construction_loadout: loadout_from_abilities(key.construction_abilities),
            intent: key.intent.into(),
            grammar: key.grammar.into(),
            embedding_attempt: key.embedding_attempt,
        }
    }
}

impl CompositionalRouteCutKeyRecord {
    /// Validate record versions and recover the exact native key.
    pub fn to_native_key(&self) -> Result<CompositionalRouteCutKey, CorpusCandidateKeyError> {
        validate_record_version(
            CorpusCandidateGenerator::CompositionalRouteCut,
            self.record_version,
        )?;
        validate_derivation_version(
            CorpusCandidateGenerator::CompositionalRouteCut,
            COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            self.derivation_version,
        )?;
        validate_generator_version(
            CorpusCandidateGenerator::CompositionalRouteCut,
            COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            self.generator_version,
        )?;
        validate_socket_inventory_version(
            CorpusCandidateGenerator::CompositionalRouteCut,
            COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            self.socket_inventory_version,
        )?;
        Ok(CompositionalRouteCutKey::new(
            self.source_seed,
            self.construction_loadout.abilities(),
            self.intent.into(),
        )
        .with_embedding(self.grammar.into(), self.embedding_attempt))
    }
}

/// Complete serialized regeneration identity for one promoted physical
/// single-ability room.
///
/// The nested base key preserves every mission/embedding coordinate and
/// version. The outer fields freeze the separate graph rewrite and physical
/// gate-aware mapping. Combined wall-jump-and-dash keys are not representable
/// in this promoted record.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionalAbilityKeyRecord {
    pub record_version: u32,
    pub generation_version: u32,
    pub edge_rewrite_version: u32,
    pub gate_embedding_contract_version: u32,
    pub base_key: CompositionalRouteCutKeyRecord,
    pub profile: CompositionalAbilityGateProfileRecord,
    pub rewrite_attempt: u16,
}

impl TryFrom<CompositionalAbilityGenerationKey> for CompositionalAbilityKeyRecord {
    type Error = CorpusCandidateKeyError;

    fn try_from(key: CompositionalAbilityGenerationKey) -> Result<Self, Self::Error> {
        Ok(Self {
            record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            edge_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            gate_embedding_contract_version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            base_key: CompositionalRouteCutKeyRecord::from(key.rewrite_key.base_key),
            profile: key.rewrite_key.profile.try_into()?,
            rewrite_attempt: key.rewrite_key.rewrite_attempt,
        })
    }
}

impl CompositionalAbilityKeyRecord {
    /// Validate every contributing mapping version and recover the exact
    /// native key without retry or fallback.
    pub fn to_native_key(
        &self,
    ) -> Result<CompositionalAbilityGenerationKey, CorpusCandidateKeyError> {
        validate_record_version(
            CorpusCandidateGenerator::CompositionalAbility,
            self.record_version,
        )?;
        validate_generator_version(
            CorpusCandidateGenerator::CompositionalAbility,
            COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            self.generation_version,
        )?;
        validate_rewrite_version(
            CorpusCandidateGenerator::CompositionalAbility,
            COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            self.edge_rewrite_version,
        )?;
        validate_gate_embedding_contract_version(
            CorpusCandidateGenerator::CompositionalAbility,
            COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            self.gate_embedding_contract_version,
        )?;
        let base_key = self.base_key.to_native_key()?;
        if base_key.construction_abilities != AbilitySet::NONE {
            return Err(CorpusCandidateKeyError::AbilityBaseKeyIsNotBaseline {
                actual: base_key.construction_abilities,
            });
        }
        Ok(CompositionalAbilityGenerationKey {
            rewrite_key: CompositionalAbilityEdgeRewriteKey {
                base_key,
                profile: self.profile.into(),
                rewrite_attempt: self.rewrite_attempt,
            },
        })
    }
}

/// Tagged, versioned key for every generator admitted to the final corpus.
///
/// This is intentionally distinct from the historical [`super::CandidateKeyRecord`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(
    tag = "generator",
    content = "key",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum CorpusCandidateKeyRecord {
    PartitionRoute(PartitionRouteKeyRecord),
    CompositionalRouteCut(CompositionalRouteCutKeyRecord),
    CompositionalAbility(CompositionalAbilityKeyRecord),
}

impl CorpusCandidateKeyRecord {
    #[must_use]
    pub const fn generator(&self) -> CorpusCandidateGenerator {
        match self {
            Self::PartitionRoute(_) => CorpusCandidateGenerator::PartitionRoute,
            Self::CompositionalRouteCut(_) => CorpusCandidateGenerator::CompositionalRouteCut,
            Self::CompositionalAbility(_) => CorpusCandidateGenerator::CompositionalAbility,
        }
    }

    #[must_use]
    pub const fn record_version(&self) -> u32 {
        match self {
            Self::PartitionRoute(record) => record.record_version,
            Self::CompositionalRouteCut(record) => record.record_version,
            Self::CompositionalAbility(record) => record.record_version,
        }
    }

    #[must_use]
    pub const fn generator_version(&self) -> u32 {
        match self {
            Self::PartitionRoute(record) => record.generator_version,
            Self::CompositionalRouteCut(record) => record.generator_version,
            Self::CompositionalAbility(record) => record.generation_version,
        }
    }

    /// Coordinate-free derivation version when the source separates grammar
    /// derivation from coordinate embedding.
    #[must_use]
    pub const fn derivation_version(&self) -> Option<u32> {
        match self {
            Self::PartitionRoute(_) => None,
            Self::CompositionalRouteCut(record) => Some(record.derivation_version),
            Self::CompositionalAbility(record) => Some(record.base_key.derivation_version),
        }
    }

    #[must_use]
    pub const fn socket_inventory_version(&self) -> Option<u32> {
        match self {
            Self::PartitionRoute(_) => None,
            Self::CompositionalRouteCut(record) => Some(record.socket_inventory_version),
            Self::CompositionalAbility(record) => Some(record.base_key.socket_inventory_version),
        }
    }

    /// Version of the nested baseline physical mapping when an ability source
    /// retains a complete compositional base key.
    #[must_use]
    pub const fn base_generator_version(&self) -> Option<u32> {
        match self {
            Self::PartitionRoute(_) | Self::CompositionalRouteCut(_) => None,
            Self::CompositionalAbility(record) => Some(record.base_key.generator_version),
        }
    }

    #[must_use]
    pub const fn rewrite_version(&self) -> Option<u32> {
        match self {
            Self::PartitionRoute(_) | Self::CompositionalRouteCut(_) => None,
            Self::CompositionalAbility(record) => Some(record.edge_rewrite_version),
        }
    }

    #[must_use]
    pub const fn gate_embedding_contract_version(&self) -> Option<u32> {
        match self {
            Self::PartitionRoute(_) | Self::CompositionalRouteCut(_) => None,
            Self::CompositionalAbility(record) => Some(record.gate_embedding_contract_version),
        }
    }

    #[must_use]
    pub const fn source_seed(&self) -> u64 {
        match self {
            Self::PartitionRoute(record) => record.source_seed,
            Self::CompositionalRouteCut(record) => record.source_seed,
            Self::CompositionalAbility(record) => record.base_key.source_seed,
        }
    }

    #[must_use]
    pub const fn construction_loadout(&self) -> EvaluationLoadout {
        match self {
            Self::PartitionRoute(record) => record.construction_loadout,
            Self::CompositionalRouteCut(record) => record.construction_loadout,
            Self::CompositionalAbility(record) => record.profile.construction_loadout(),
        }
    }

    #[must_use]
    pub const fn embedding_attempt(&self) -> u8 {
        match self {
            Self::PartitionRoute(record) => record.embedding_attempt,
            Self::CompositionalRouteCut(record) => record.embedding_attempt,
            Self::CompositionalAbility(record) => record.base_key.embedding_attempt,
        }
    }

    /// Exact graph-rewrite attempt for ability sources; ordinary sources have
    /// no rewrite coordinate.
    #[must_use]
    pub const fn rewrite_attempt(&self) -> Option<u16> {
        match self {
            Self::PartitionRoute(_) | Self::CompositionalRouteCut(_) => None,
            Self::CompositionalAbility(record) => Some(record.rewrite_attempt),
        }
    }

    /// Deterministic artifact/sort identity using only frozen explicit slugs.
    ///
    /// Every regeneration coordinate and relevant version is present. Rust
    /// `Debug` spelling is deliberately absent.
    #[must_use]
    pub fn stable_slug(&self) -> String {
        match self {
            Self::PartitionRoute(record) => format!(
                "candidate-v{}-partition-route-g{}-{}-{}-{}-a{:03}-s{:016x}",
                record.record_version,
                record.generator_version,
                record.profile.slug(),
                record.intent.slug(),
                record.construction_loadout.slug(),
                record.embedding_attempt,
                record.source_seed,
            ),
            Self::CompositionalRouteCut(record) => format!(
                "candidate-v{}-compositional-route-cut-d{}-g{}-k{}-{}-{}-{}-a{:03}-s{:016x}",
                record.record_version,
                record.derivation_version,
                record.generator_version,
                record.socket_inventory_version,
                record.grammar.slug(),
                record.intent.slug(),
                record.construction_loadout.slug(),
                record.embedding_attempt,
                record.source_seed,
            ),
            Self::CompositionalAbility(record) => format!(
                "candidate-v{}-compositional-ability-g{}-rewrite-v{}-contract-v{}-base-v{}-d{}-g{}-k{}-{}-{}-{}-{}-a{:03}-r{:05}-s{:016x}",
                record.record_version,
                record.generation_version,
                record.edge_rewrite_version,
                record.gate_embedding_contract_version,
                record.base_key.record_version,
                record.base_key.derivation_version,
                record.base_key.generator_version,
                record.base_key.socket_inventory_version,
                record.base_key.grammar.slug(),
                record.base_key.intent.slug(),
                record.base_key.construction_loadout.slug(),
                record.profile.slug(),
                record.base_key.embedding_attempt,
                record.rewrite_attempt,
                record.base_key.source_seed,
            ),
        }
    }

    /// Regenerate only this exact key. No retry or generator fallback occurs.
    pub fn regenerate(&self) -> Result<CorpusCandidate, CorpusCandidateRegenerationError> {
        match self {
            Self::PartitionRoute(record) => {
                let key = record
                    .to_native_key()
                    .map_err(CorpusCandidateRegenerationError::Key)?;
                key.regenerate()
                    .map(CorpusCandidate::from)
                    .map_err(|error| {
                        CorpusCandidateRegenerationError::PartitionRoute(Box::new(error))
                    })
            }
            Self::CompositionalRouteCut(record) => {
                let key = record
                    .to_native_key()
                    .map_err(CorpusCandidateRegenerationError::Key)?;
                key.regenerate()
                    .map(CorpusCandidate::from)
                    .map_err(|error| {
                        CorpusCandidateRegenerationError::CompositionalRouteCut(Box::new(error))
                    })
            }
            Self::CompositionalAbility(record) => {
                let key = record
                    .to_native_key()
                    .map_err(CorpusCandidateRegenerationError::Key)?;
                key.generate()
                    .map_err(|error| {
                        CorpusCandidateRegenerationError::CompositionalAbility(Box::new(error))
                    })
                    .and_then(|candidate| {
                        CorpusCandidate::try_from(candidate)
                            .map_err(CorpusCandidateRegenerationError::Key)
                    })
            }
        }
    }
}

/// A malformed or future exact-key record that this build cannot interpret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusCandidateKeyError {
    UnsupportedRecordVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnsupportedDerivationVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnsupportedGeneratorVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnsupportedSocketInventoryVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnsupportedRewriteVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnsupportedGateEmbeddingContractVersion {
        generator: CorpusCandidateGenerator,
        expected: u32,
        actual: u32,
    },
    UnpromotedAbilityProfile {
        profile: CompositionalAbilityGateProfile,
    },
    AbilityBaseKeyIsNotBaseline {
        actual: AbilitySet,
    },
}

impl fmt::Display for CorpusCandidateKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRecordVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} key-record version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnsupportedDerivationVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} derivation version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnsupportedGeneratorVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} generator version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnsupportedSocketInventoryVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} socket-inventory version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnsupportedRewriteVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} edge-rewrite version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnsupportedGateEmbeddingContractVersion {
                generator,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported {} gate-embedding-contract version {actual}; this build requires {expected}",
                generator.slug()
            ),
            Self::UnpromotedAbilityProfile { profile } => write!(
                formatter,
                "compositional ability profile {} has not been promoted into the final corpus",
                profile.slug()
            ),
            Self::AbilityBaseKeyIsNotBaseline { actual } => write!(
                formatter,
                "compositional ability base key must be baseline, found {actual:?}"
            ),
        }
    }
}

impl Error for CorpusCandidateKeyError {}

/// Failure to validate or regenerate a generator-neutral exact key.
#[derive(Debug)]
pub enum CorpusCandidateRegenerationError {
    Key(CorpusCandidateKeyError),
    PartitionRoute(Box<PartitionRouteGenerationError>),
    CompositionalRouteCut(Box<CompositionalRouteCutGenerationError>),
    CompositionalAbility(Box<CompositionalAbilityGenerationError>),
}

impl fmt::Display for CorpusCandidateRegenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(error) => error.fmt(formatter),
            Self::PartitionRoute(error) => error.fmt(formatter),
            Self::CompositionalRouteCut(error) => error.fmt(formatter),
            Self::CompositionalAbility(error) => error.fmt(formatter),
        }
    }
}

impl Error for CorpusCandidateRegenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Key(error) => Some(error),
            Self::PartitionRoute(error) => Some(error.as_ref()),
            Self::CompositionalRouteCut(error) => Some(error.as_ref()),
            Self::CompositionalAbility(error) => Some(error.as_ref()),
        }
    }
}

/// Full native candidate retained behind a generator-neutral dispatch point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusCandidate {
    PartitionRoute(Box<PartitionRouteCandidate>),
    CompositionalRouteCut(Box<CompositionalRouteCutCandidate>),
    CompositionalAbility(Box<CompositionalAbilityCandidate>),
}

impl From<PartitionRouteCandidate> for CorpusCandidate {
    fn from(candidate: PartitionRouteCandidate) -> Self {
        Self::PartitionRoute(Box::new(candidate))
    }
}

impl From<CompositionalRouteCutCandidate> for CorpusCandidate {
    fn from(candidate: CompositionalRouteCutCandidate) -> Self {
        Self::CompositionalRouteCut(Box::new(candidate))
    }
}

impl TryFrom<CompositionalAbilityCandidate> for CorpusCandidate {
    type Error = CorpusCandidateKeyError;

    fn try_from(candidate: CompositionalAbilityCandidate) -> Result<Self, Self::Error> {
        CompositionalAbilityGateProfileRecord::try_from(candidate.key.rewrite_key.profile)?;
        Ok(Self::CompositionalAbility(Box::new(candidate)))
    }
}

impl CorpusCandidate {
    #[must_use]
    pub const fn generator(&self) -> CorpusCandidateGenerator {
        match self {
            Self::PartitionRoute(_) => CorpusCandidateGenerator::PartitionRoute,
            Self::CompositionalRouteCut(_) => CorpusCandidateGenerator::CompositionalRouteCut,
            Self::CompositionalAbility(_) => CorpusCandidateGenerator::CompositionalAbility,
        }
    }

    #[must_use]
    pub fn generated(&self) -> &GeneratedLevel {
        match self {
            Self::PartitionRoute(candidate) => &candidate.generated,
            Self::CompositionalRouteCut(candidate) => &candidate.generated,
            Self::CompositionalAbility(candidate) => &candidate.generated,
        }
    }

    #[must_use]
    pub fn route_plan(&self) -> &RoutePlan {
        match self {
            Self::PartitionRoute(candidate) => &candidate.route_plan,
            Self::CompositionalRouteCut(candidate) => &candidate.route_plan,
            Self::CompositionalAbility(candidate) => &candidate.route_plan,
        }
    }

    #[must_use]
    pub fn route_plan_summary(&self) -> &RoutePlanSummary {
        match self {
            Self::PartitionRoute(candidate) => &candidate.route_summary,
            Self::CompositionalRouteCut(candidate) => &candidate.route_summary,
            Self::CompositionalAbility(candidate) => &candidate.route_summary,
        }
    }

    #[must_use]
    pub fn boundary_ports(&self) -> &[BoundaryPort] {
        match self {
            Self::PartitionRoute(candidate) => &candidate.boundary_ports,
            Self::CompositionalRouteCut(candidate) => &candidate.boundary_ports,
            Self::CompositionalAbility(candidate) => &candidate.boundary_ports,
        }
    }

    /// Construction abilities come from generated metadata, not an alias key
    /// or the order in which candidates were encountered.
    #[must_use]
    pub fn construction_abilities(&self) -> AbilitySet {
        self.generated().metadata.intended_abilities
    }

    /// Native ability-source facts needed by the variant-specific promotion
    /// validator. Baseline sources deliberately return `None`.
    #[must_use]
    pub const fn compositional_ability_candidate(&self) -> Option<&CompositionalAbilityCandidate> {
        match self {
            Self::PartitionRoute(_) | Self::CompositionalRouteCut(_) => None,
            Self::CompositionalAbility(candidate) => Some(candidate),
        }
    }

    #[must_use]
    pub fn exact_key(&self) -> CorpusCandidateKeyRecord {
        match self {
            Self::PartitionRoute(candidate) => {
                CorpusCandidateKeyRecord::PartitionRoute(candidate.key.into())
            }
            Self::CompositionalRouteCut(candidate) => {
                CorpusCandidateKeyRecord::CompositionalRouteCut(candidate.key.into())
            }
            Self::CompositionalAbility(candidate) => {
                CorpusCandidateKeyRecord::CompositionalAbility(
                    CompositionalAbilityKeyRecord::try_from(candidate.key)
                        .expect("CorpusCandidate excludes unpromoted ability profiles"),
                )
            }
        }
    }

    #[must_use]
    pub fn provenance(&self) -> CorpusCandidateProvenance<'_> {
        match self {
            Self::PartitionRoute(candidate) => CorpusCandidateProvenance::PartitionRoute {
                derivation: &candidate.derivation,
                generator_summary: &candidate.summary,
            },
            Self::CompositionalRouteCut(candidate) => {
                CorpusCandidateProvenance::CompositionalRouteCut {
                    mission: &candidate.mission,
                    mission_route_nodes: &candidate.mission_route_nodes,
                    embedding: &candidate.embedding,
                }
            }
            Self::CompositionalAbility(candidate) => {
                CorpusCandidateProvenance::CompositionalAbility {
                    rewritten_mission: &candidate.rewritten_mission,
                    mission_route_nodes: &candidate.mission_route_nodes,
                    embedding: &candidate.embedding,
                    evidence_state: &candidate.evidence_state,
                }
            }
        }
    }

    #[must_use]
    pub fn view(&self) -> CorpusCandidateView<'_> {
        CorpusCandidateView {
            generator: self.generator(),
            generated: self.generated(),
            route_plan: self.route_plan(),
            route_plan_summary: self.route_plan_summary(),
            boundary_ports: self.boundary_ports(),
            construction_abilities: self.construction_abilities(),
            exact_key: self.exact_key(),
            provenance: self.provenance(),
        }
    }

    #[must_use]
    pub fn physical_room_descriptor_v3(&self) -> CorpusPhysicalRoomDescriptorV3 {
        CorpusPhysicalRoomDescriptorV3::from_room(&self.generated().room)
    }
}

/// Native derivation evidence, borrowed without reducing it to common scalar
/// fields.
#[derive(Clone, Copy, Debug)]
pub enum CorpusCandidateProvenance<'a> {
    PartitionRoute {
        derivation: &'a PartitionDerivation,
        generator_summary: &'a PartitionRouteSummary,
    },
    CompositionalRouteCut {
        mission: &'a DerivedMission,
        mission_route_nodes: &'a [MissionRouteNodeMapping],
        embedding: &'a CompositionalRouteCutEmbeddingSummary,
    },
    CompositionalAbility {
        rewritten_mission: &'a AbilityRewrittenMission,
        mission_route_nodes: &'a [MissionRouteNodeMapping],
        embedding: &'a CompositionalAbilityEmbeddingSummary,
        evidence_state: &'a AbilityGateEmbeddingState,
    },
}

/// Shared borrow surface for downstream evaluation code.
#[derive(Clone, Debug)]
pub struct CorpusCandidateView<'a> {
    pub generator: CorpusCandidateGenerator,
    pub generated: &'a GeneratedLevel,
    pub route_plan: &'a RoutePlan,
    pub route_plan_summary: &'a RoutePlanSummary,
    pub boundary_ports: &'a [BoundaryPort],
    pub construction_abilities: AbilitySet,
    pub exact_key: CorpusCandidateKeyRecord,
    pub provenance: CorpusCandidateProvenance<'a>,
}

/// Exact geometry pair used as the physical-room grouping key in v3.
///
/// `room_id()` hashes both descriptors into a compact stable label. Hash/ID
/// equality is never sufficient for grouping; this structure's full `Eq`
/// comparison is authoritative.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CorpusPhysicalRoomDescriptorV3 {
    pub static_visual: StaticVisualDescriptor,
    pub simulation_geometry: SimulationGeometryDescriptor,
}

impl CorpusPhysicalRoomDescriptorV3 {
    #[must_use]
    pub fn from_room(room: &Room) -> Self {
        Self {
            static_visual: StaticVisualDescriptor::from_room(room),
            simulation_geometry: SimulationGeometryDescriptor::from_room(room),
        }
    }

    /// Compact deterministic label derived only from the two exact geometry
    /// descriptors. Use full descriptor equality to decide alias membership.
    #[must_use]
    pub fn room_id(&self) -> RoomId {
        RoomId(format!(
            "room-v{CORPUS_PHYSICAL_ROOM_ID_VERSION}-{:016x}-{:016x}",
            fingerprint_static_visual(&self.static_visual),
            self.simulation_geometry.stable_digest(),
        ))
    }
}

fn validate_record_version(
    generator: CorpusCandidateGenerator,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == CORPUS_CANDIDATE_KEY_RECORD_VERSION {
        Ok(())
    } else {
        Err(CorpusCandidateKeyError::UnsupportedRecordVersion {
            generator,
            expected: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            actual,
        })
    }
}

fn validate_derivation_version(
    generator: CorpusCandidateGenerator,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CorpusCandidateKeyError::UnsupportedDerivationVersion {
            generator,
            expected,
            actual,
        })
    }
}

fn validate_generator_version(
    generator: CorpusCandidateGenerator,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CorpusCandidateKeyError::UnsupportedGeneratorVersion {
            generator,
            expected,
            actual,
        })
    }
}

fn validate_socket_inventory_version(
    generator: CorpusCandidateGenerator,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CorpusCandidateKeyError::UnsupportedSocketInventoryVersion {
            generator,
            expected,
            actual,
        })
    }
}

fn validate_rewrite_version(
    generator: CorpusCandidateGenerator,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CorpusCandidateKeyError::UnsupportedRewriteVersion {
            generator,
            expected,
            actual,
        })
    }
}

fn validate_gate_embedding_contract_version(
    generator: CorpusCandidateGenerator,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusCandidateKeyError> {
    if actual == expected {
        Ok(())
    } else {
        Err(
            CorpusCandidateKeyError::UnsupportedGateEmbeddingContractVersion {
                generator,
                expected,
                actual,
            },
        )
    }
}

const fn loadout_from_abilities(abilities: AbilitySet) -> EvaluationLoadout {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => EvaluationLoadout::Baseline,
        (true, false) => EvaluationLoadout::WallJump,
        (false, true) => EvaluationLoadout::Dash,
        (true, true) => EvaluationLoadout::Both,
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{Exit, Point, Rect, Tile};
    use serde_json::Value;

    use super::*;

    #[test]
    fn partition_key_json_round_trip_regenerates_exact_native_candidate() {
        let native_key = PartitionRouteKey::new(
            7,
            AbilitySet::new(true, false),
            ChallengeIntent::Technical,
            PartitionRouteProfile::Branching,
        )
        .with_embedding_attempt(3);
        let original = CorpusCandidate::from(native_key.regenerate().unwrap());
        assert_eq!(
            original.exact_key().stable_slug(),
            format!(
                "candidate-v{}-partition-route-g{}-branching-technical-wall-jump-a003-s0000000000000007",
                CORPUS_CANDIDATE_KEY_RECORD_VERSION, PARTITION_ROUTE_GENERATION_VERSION
            )
        );
        assert_exact_round_trip(original, AbilitySet::new(true, false));
    }

    #[test]
    fn compositional_key_json_round_trip_regenerates_exact_native_candidate() {
        let native_key =
            CompositionalRouteCutKey::new(11, AbilitySet::ALL, ChallengeIntent::Standard)
                .with_embedding(CompositionalRouteCutGrammar::RecursiveMissionCutsV1, 0);
        let original = CorpusCandidate::from(native_key.regenerate().unwrap());
        assert_eq!(
            original.exact_key().stable_slug(),
            format!(
                "candidate-v{}-compositional-route-cut-d{}-g{}-k{}-recursive-mission-cuts-v1-standard-both-a000-s000000000000000b",
                CORPUS_CANDIDATE_KEY_RECORD_VERSION,
                COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
                COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
                COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            )
        );
        assert_exact_round_trip(original, AbilitySet::ALL);
    }

    #[test]
    fn promoted_wall_and_dash_keys_round_trip_and_regenerate_exact_native_candidates() {
        for (profile, expected_loadout) in [
            (
                CompositionalAbilityGateProfile::WallJump,
                EvaluationLoadout::WallJump,
            ),
            (
                CompositionalAbilityGateProfile::Dash,
                EvaluationLoadout::Dash,
            ),
        ] {
            let native_key =
                CompositionalAbilityGenerationKey::new(0, profile, ChallengeIntent::Standard)
                    .with_attempts(0, 0);
            let original = CorpusCandidate::try_from(native_key.generate().unwrap()).unwrap();
            let record = original.exact_key();
            assert_eq!(record.construction_loadout(), expected_loadout);
            assert_eq!(record.rewrite_attempt(), Some(0));
            assert_eq!(
                record.stable_slug(),
                format!(
                    "candidate-v{}-compositional-ability-g{}-rewrite-v{}-contract-v{}-base-v{}-d{}-g{}-k{}-recursive-mission-cuts-v1-standard-baseline-{}-a000-r00000-s0000000000000000",
                    CORPUS_CANDIDATE_KEY_RECORD_VERSION,
                    COMPOSITIONAL_ABILITY_GENERATION_VERSION,
                    COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
                    COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
                    CORPUS_CANDIDATE_KEY_RECORD_VERSION,
                    COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
                    COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
                    COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
                    profile.slug(),
                )
            );
            assert_exact_round_trip(original, expected_loadout.abilities());
        }
    }

    fn assert_exact_round_trip(original: CorpusCandidate, expected_abilities: AbilitySet) {
        let record = original.exact_key();
        let json = serde_json::to_string(&record).unwrap();
        let value = serde_json::from_str::<Value>(&json).unwrap();
        assert_eq!(
            value["key"]["record_version"],
            CORPUS_CANDIDATE_KEY_RECORD_VERSION
        );
        match &record {
            CorpusCandidateKeyRecord::PartitionRoute(record) => {
                assert_eq!(
                    value["key"]["generator_version"].as_u64(),
                    Some(u64::from(record.generator_version))
                );
                assert!(value["key"].get("derivation_version").is_none());
                assert!(value["key"].get("socket_inventory_version").is_none());
            }
            CorpusCandidateKeyRecord::CompositionalRouteCut(record) => {
                assert_eq!(
                    value["key"]["generator_version"].as_u64(),
                    Some(u64::from(record.generator_version))
                );
                assert_eq!(
                    value["key"]["derivation_version"].as_u64(),
                    Some(u64::from(record.derivation_version))
                );
                assert_eq!(
                    value["key"]["socket_inventory_version"].as_u64(),
                    Some(u64::from(record.socket_inventory_version))
                );
            }
            CorpusCandidateKeyRecord::CompositionalAbility(record) => {
                assert_eq!(
                    value["key"]["generation_version"].as_u64(),
                    Some(u64::from(record.generation_version))
                );
                assert_eq!(
                    value["key"]["edge_rewrite_version"].as_u64(),
                    Some(u64::from(record.edge_rewrite_version))
                );
                assert_eq!(
                    value["key"]["gate_embedding_contract_version"].as_u64(),
                    Some(u64::from(record.gate_embedding_contract_version))
                );
                assert_eq!(
                    value["key"]["base_key"]["derivation_version"].as_u64(),
                    Some(u64::from(record.base_key.derivation_version))
                );
                assert_eq!(
                    value["key"]["base_key"]["generator_version"].as_u64(),
                    Some(u64::from(record.base_key.generator_version))
                );
                assert_eq!(
                    value["key"]["base_key"]["socket_inventory_version"].as_u64(),
                    Some(u64::from(record.base_key.socket_inventory_version))
                );
            }
        }
        let decoded = serde_json::from_str::<CorpusCandidateKeyRecord>(&json).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(decoded.source_seed(), record.source_seed());
        assert_eq!(decoded.generator_version(), record.generator_version());
        assert_eq!(
            decoded.base_generator_version(),
            record.base_generator_version()
        );
        assert_eq!(decoded.derivation_version(), record.derivation_version());
        assert_eq!(
            decoded.socket_inventory_version(),
            record.socket_inventory_version()
        );
        assert_eq!(decoded.rewrite_version(), record.rewrite_version());
        assert_eq!(
            decoded.gate_embedding_contract_version(),
            record.gate_embedding_contract_version()
        );
        assert_eq!(
            decoded.construction_loadout(),
            record.construction_loadout()
        );
        assert_eq!(decoded.embedding_attempt(), record.embedding_attempt());
        assert_eq!(decoded.rewrite_attempt(), record.rewrite_attempt());
        assert_eq!(decoded.stable_slug(), record.stable_slug());
        let regenerated = decoded.regenerate().unwrap();
        assert_eq!(regenerated, original);

        let view = regenerated.view();
        assert_eq!(view.construction_abilities, expected_abilities);
        assert_eq!(
            view.construction_abilities,
            view.generated.metadata.intended_abilities
        );
        assert_eq!(view.route_plan_summary, &view.route_plan.summary());
        assert_eq!(view.boundary_ports.len(), view.generated.room.doors().len());
        assert_eq!(view.exact_key, decoded);
    }

    #[test]
    fn future_key_versions_are_not_silently_interpreted_as_current() {
        let mut record = PartitionRouteKeyRecord::from(PartitionRouteKey::new(
            0,
            AbilitySet::NONE,
            ChallengeIntent::Gentle,
            PartitionRouteProfile::MixedBsp,
        ));
        record.generator_version += 1;
        assert!(matches!(
            record.to_native_key(),
            Err(CorpusCandidateKeyError::UnsupportedGeneratorVersion { .. })
        ));

        let native_ability = CompositionalAbilityGenerationKey::new(
            0,
            CompositionalAbilityGateProfile::WallJump,
            ChallengeIntent::Standard,
        );
        let mut ability_record = CompositionalAbilityKeyRecord::try_from(native_ability).unwrap();
        ability_record.edge_rewrite_version += 1;
        assert!(matches!(
            ability_record.to_native_key(),
            Err(CorpusCandidateKeyError::UnsupportedRewriteVersion { .. })
        ));

        assert!(matches!(
            CompositionalAbilityKeyRecord::try_from(CompositionalAbilityGenerationKey::new(
                1,
                CompositionalAbilityGateProfile::Both,
                ChallengeIntent::Standard,
            )),
            Err(CorpusCandidateKeyError::UnpromotedAbilityProfile { .. })
        ));
    }

    #[test]
    fn room_v3_alias_identity_ignores_labels_but_groups_by_full_descriptors() {
        let first = labelled_room("first-room", "First room", "first-exit");
        let second = labelled_room("second-room", "Second room", "second-exit");
        let first_descriptor = CorpusPhysicalRoomDescriptorV3::from_room(&first);
        let second_descriptor = CorpusPhysicalRoomDescriptorV3::from_room(&second);

        assert_eq!(first_descriptor, second_descriptor);
        assert_eq!(first_descriptor.room_id(), second_descriptor.room_id());
        assert!(!first_descriptor.room_id().0.contains("first"));
        assert!(!second_descriptor.room_id().0.contains("second"));

        let mut changed_tiles = second.tiles().to_vec();
        changed_tiles[5] = Tile::Solid;
        let changed = Room::new(
            "second-room",
            "Second room",
            second.width(),
            second.height(),
            second.tile_size(),
            changed_tiles,
            second.spawn(),
            second.exits().to_vec(),
        )
        .unwrap();
        let changed_descriptor = CorpusPhysicalRoomDescriptorV3::from_room(&changed);
        assert_ne!(first_descriptor, changed_descriptor);
        assert_ne!(first_descriptor.room_id(), changed_descriptor.room_id());
    }

    fn labelled_room(id: &str, name: &str, exit_id: &str) -> Room {
        let width = 32;
        let height = 18;
        Room::new(
            id,
            name,
            width,
            height,
            10,
            vec![Tile::Empty; usize::from(width) * usize::from(height)],
            Point::new(10, 10),
            vec![Exit {
                id: exit_id.to_owned(),
                bounds: Rect::new(280, 140, 20, 20),
                destination: None,
                destination_entrance: None,
            }],
        )
        .unwrap()
    }
}
