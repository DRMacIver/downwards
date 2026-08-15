//! Versioned inputs for the final multi-generator corpus path.

use std::{error::Error, fmt};

use downwards_gen::experimental::{
    COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
    COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
    COMPOSITIONAL_ABILITY_GENERATION_VERSION, COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
    COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION, COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
    PARTITION_ROUTE_GENERATION_VERSION,
};
use serde::{Deserialize, Serialize};

use super::CORPUS_CANDIDATE_KEY_RECORD_VERSION;

/// Schema of [`CorpusBuildConfigV2`].
pub const CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION: u32 = 4;

/// The first final-path batch deliberately enumerates attempt zero only.
pub const CORPUS_BUILD_CONFIG_V2_EMBEDDING_ATTEMPT: u8 = 0;

/// The promoted one-ability sources likewise enumerate only exact rewrite
/// attempt zero.
pub const CORPUS_BUILD_CONFIG_V2_REWRITE_ATTEMPT: u16 = 0;

/// Version of the policy deciding which construction-ability coordinates are
/// honest claims for the current generator sources.
pub const CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION: u32 = 3;

/// Frozen source-capability coordinates admitted by a config schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusSourceCapabilityEnumerationPolicy {
    /// Historical final-path schema before physical ability sources were
    /// promoted. Retained only so old records deserialize and fail versioned
    /// validation explicitly.
    BaselineOnlyAuthoredEdges,
    /// Baseline PartitionRoute/CompositionalRouteCut plus promoted physical-v2
    /// WallJump and Dash sources. The unpromoted combined profile is excluded.
    BaselineAndPromotedSingleAbilitySources,
    /// Baseline PartitionRoute/CompositionalRouteCut plus the one currently
    /// certified physical-v2 Dash source. WallJump remains excluded after its
    /// missing-ability matrix vetoes and the rejected v3/v4 mapping pilots.
    BaselineAndCertifiedDashSource,
}

/// Frozen inputs for one bounded multi-generator construction batch.
///
/// Generator versions are repeated here as a cheap fail-fast guard. Every
/// enumerated candidate key also carries the same exact versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusBuildConfigV2 {
    pub schema_version: u32,
    pub candidate_key_record_version: u32,
    pub partition_route_generation_version: u32,
    pub compositional_route_cut_derivation_version: u32,
    pub compositional_route_cut_generation_version: u32,
    pub compositional_route_cut_socket_inventory_version: u32,
    pub compositional_ability_generation_version: u32,
    pub compositional_ability_edge_rewrite_version: u32,
    pub compositional_ability_gate_embedding_contract_version: u32,
    pub source_capability_policy_version: u32,
    pub source_capability_policy: CorpusSourceCapabilityEnumerationPolicy,
    pub embedding_attempt: u8,
    pub ability_rewrite_attempt: u16,
    pub start_seed: u64,
    pub seed_count: usize,
}

impl CorpusBuildConfigV2 {
    #[must_use]
    pub const fn attempt_zero(start_seed: u64, seed_count: usize) -> Self {
        Self {
            schema_version: CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION,
            candidate_key_record_version: CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            partition_route_generation_version: PARTITION_ROUTE_GENERATION_VERSION,
            compositional_route_cut_derivation_version: COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            compositional_route_cut_generation_version: COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            compositional_route_cut_socket_inventory_version:
                COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            compositional_ability_generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            compositional_ability_edge_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            compositional_ability_gate_embedding_contract_version:
                COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            source_capability_policy_version: CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION,
            source_capability_policy:
                CorpusSourceCapabilityEnumerationPolicy::BaselineAndCertifiedDashSource,
            embedding_attempt: CORPUS_BUILD_CONFIG_V2_EMBEDDING_ATTEMPT,
            ability_rewrite_attempt: CORPUS_BUILD_CONFIG_V2_REWRITE_ATTEMPT,
            start_seed,
            seed_count,
        }
    }

    pub fn validate(&self) -> Result<(), CorpusBuildConfigV2Error> {
        validate_version(
            CorpusBuildConfigV2VersionField::Schema,
            CORPUS_BUILD_CONFIG_V2_SCHEMA_VERSION,
            self.schema_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CandidateKeyRecord,
            CORPUS_CANDIDATE_KEY_RECORD_VERSION,
            self.candidate_key_record_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::PartitionRouteGeneration,
            PARTITION_ROUTE_GENERATION_VERSION,
            self.partition_route_generation_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalRouteCutDerivation,
            COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            self.compositional_route_cut_derivation_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalRouteCutGeneration,
            COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            self.compositional_route_cut_generation_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalRouteCutSocketInventory,
            COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            self.compositional_route_cut_socket_inventory_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalAbilityGeneration,
            COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            self.compositional_ability_generation_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalAbilityEdgeRewrite,
            COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            self.compositional_ability_edge_rewrite_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::CompositionalAbilityGateEmbeddingContract,
            COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            self.compositional_ability_gate_embedding_contract_version,
        )?;
        validate_version(
            CorpusBuildConfigV2VersionField::SourceCapabilityEnumerationPolicy,
            CORPUS_SOURCE_CAPABILITY_ENUMERATION_POLICY_VERSION,
            self.source_capability_policy_version,
        )?;
        if self.source_capability_policy
            != CorpusSourceCapabilityEnumerationPolicy::BaselineAndCertifiedDashSource
        {
            return Err(CorpusBuildConfigV2Error::UnsupportedSourceCapabilityPolicy);
        }
        if self.embedding_attempt != CORPUS_BUILD_CONFIG_V2_EMBEDDING_ATTEMPT {
            return Err(CorpusBuildConfigV2Error::UnsupportedEmbeddingAttempt {
                expected: CORPUS_BUILD_CONFIG_V2_EMBEDDING_ATTEMPT,
                actual: self.embedding_attempt,
            });
        }
        if self.ability_rewrite_attempt != CORPUS_BUILD_CONFIG_V2_REWRITE_ATTEMPT {
            return Err(CorpusBuildConfigV2Error::UnsupportedRewriteAttempt {
                expected: CORPUS_BUILD_CONFIG_V2_REWRITE_ATTEMPT,
                actual: self.ability_rewrite_attempt,
            });
        }
        if self.seed_count == 0 {
            return Err(CorpusBuildConfigV2Error::EmptySeedBlock);
        }
        let final_offset = u64::try_from(self.seed_count - 1).map_err(|_| {
            CorpusBuildConfigV2Error::SeedRangeOverflow {
                start_seed: self.start_seed,
                seed_count: self.seed_count,
            }
        })?;
        self.start_seed.checked_add(final_offset).ok_or(
            CorpusBuildConfigV2Error::SeedRangeOverflow {
                start_seed: self.start_seed,
                seed_count: self.seed_count,
            },
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusBuildConfigV2VersionField {
    Schema,
    CandidateKeyRecord,
    PartitionRouteGeneration,
    CompositionalRouteCutDerivation,
    CompositionalRouteCutGeneration,
    CompositionalRouteCutSocketInventory,
    CompositionalAbilityGeneration,
    CompositionalAbilityEdgeRewrite,
    CompositionalAbilityGateEmbeddingContract,
    SourceCapabilityEnumerationPolicy,
}

impl CorpusBuildConfigV2VersionField {
    const fn slug(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::CandidateKeyRecord => "candidate-key-record",
            Self::PartitionRouteGeneration => "partition-route-generation",
            Self::CompositionalRouteCutDerivation => "compositional-route-cut-derivation",
            Self::CompositionalRouteCutGeneration => "compositional-route-cut-generation",
            Self::CompositionalRouteCutSocketInventory => {
                "compositional-route-cut-socket-inventory"
            }
            Self::CompositionalAbilityGeneration => "compositional-ability-generation",
            Self::CompositionalAbilityEdgeRewrite => "compositional-ability-edge-rewrite",
            Self::CompositionalAbilityGateEmbeddingContract => {
                "compositional-ability-gate-embedding-contract"
            }
            Self::SourceCapabilityEnumerationPolicy => "source-capability-enumeration-policy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusBuildConfigV2Error {
    UnsupportedVersion {
        field: CorpusBuildConfigV2VersionField,
        expected: u32,
        actual: u32,
    },
    UnsupportedEmbeddingAttempt {
        expected: u8,
        actual: u8,
    },
    UnsupportedRewriteAttempt {
        expected: u16,
        actual: u16,
    },
    UnsupportedSourceCapabilityPolicy,
    EmptySeedBlock,
    SeedRangeOverflow {
        start_seed: u64,
        seed_count: usize,
    },
}

impl fmt::Display for CorpusBuildConfigV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "unsupported corpus-v2 {} version {actual}; this build requires {expected}",
                field.slug()
            ),
            Self::UnsupportedEmbeddingAttempt { expected, actual } => write!(
                formatter,
                "unsupported corpus-v2 embedding attempt {actual}; this slice enumerates exact attempt {expected} only"
            ),
            Self::UnsupportedRewriteAttempt { expected, actual } => write!(
                formatter,
                "unsupported corpus-v2 ability rewrite attempt {actual}; this slice enumerates exact attempt {expected} only"
            ),
            Self::UnsupportedSourceCapabilityPolicy => write!(
                formatter,
                "unsupported corpus-v2 source capability policy; this slice permits baseline sources plus the certified Dash physical source"
            ),
            Self::EmptySeedBlock => write!(formatter, "corpus-v2 seed block must not be empty"),
            Self::SeedRangeOverflow {
                start_seed,
                seed_count,
            } => write!(
                formatter,
                "corpus-v2 seed block starting at {start_seed} with {seed_count} seeds exceeds the u64 seed domain"
            ),
        }
    }
}

impl Error for CorpusBuildConfigV2Error {}

fn validate_version(
    field: CorpusBuildConfigV2VersionField,
    expected: u32,
    actual: u32,
) -> Result<(), CorpusBuildConfigV2Error> {
    if expected == actual {
        Ok(())
    } else {
        Err(CorpusBuildConfigV2Error::UnsupportedVersion {
            field,
            expected,
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_retains_every_explicit_version() {
        let config = CorpusBuildConfigV2::attempt_zero(19, 3);
        config.validate().unwrap();
        let encoded = serde_json::to_string(&config).unwrap();
        assert_eq!(
            serde_json::from_str::<CorpusBuildConfigV2>(&encoded).unwrap(),
            config
        );
    }

    #[test]
    fn nonzero_attempt_and_future_versions_are_rejected() {
        let mut config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.embedding_attempt = 1;
        assert!(matches!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedEmbeddingAttempt { .. })
        ));

        config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.ability_rewrite_attempt = 1;
        assert!(matches!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedRewriteAttempt { .. })
        ));

        config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.partition_route_generation_version += 1;
        assert!(matches!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedVersion {
                field: CorpusBuildConfigV2VersionField::PartitionRouteGeneration,
                ..
            })
        ));

        config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.source_capability_policy_version += 1;
        assert!(matches!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedVersion {
                field: CorpusBuildConfigV2VersionField::SourceCapabilityEnumerationPolicy,
                ..
            })
        ));

        config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.source_capability_policy =
            CorpusSourceCapabilityEnumerationPolicy::BaselineOnlyAuthoredEdges;
        assert_eq!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedSourceCapabilityPolicy)
        );

        config = CorpusBuildConfigV2::attempt_zero(0, 1);
        config.source_capability_policy =
            CorpusSourceCapabilityEnumerationPolicy::BaselineAndPromotedSingleAbilitySources;
        assert_eq!(
            config.validate(),
            Err(CorpusBuildConfigV2Error::UnsupportedSourceCapabilityPolicy)
        );
    }

    #[test]
    fn seed_blocks_must_not_wrap_the_seed_domain() {
        CorpusBuildConfigV2::attempt_zero(u64::MAX, 1)
            .validate()
            .unwrap();
        assert!(matches!(
            CorpusBuildConfigV2::attempt_zero(u64::MAX, 2).validate(),
            Err(CorpusBuildConfigV2Error::SeedRangeOverflow {
                start_seed: u64::MAX,
                seed_count: 2,
            })
        ));
    }
}
