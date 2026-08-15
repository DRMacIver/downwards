use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use super::FeatureStageRecord;

pub const CORPUS_SCHEMA_VERSION: u32 = 1;

/// Reproducible inputs for one bounded corpus-generation batch.
///
/// Evaluation policies will be added as the evidence pipeline lands. Keeping
/// this initial record narrow prevents an incomplete pilot from looking like
/// a complete corpus build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusBuildConfigV1 {
    pub schema_version: u32,
    pub start_seed: u64,
    pub seed_count: usize,
    pub feature_stage: FeatureStageRecord,
    pub target_min_rooms: usize,
    pub target_max_rooms: usize,
}

impl CorpusBuildConfigV1 {
    #[must_use]
    pub const fn terrain_only_pilot(start_seed: u64, seed_count: usize) -> Self {
        Self {
            schema_version: CORPUS_SCHEMA_VERSION,
            start_seed,
            seed_count,
            feature_stage: FeatureStageRecord::TerrainOnly,
            target_min_rooms: 500,
            target_max_rooms: 1_000,
        }
    }

    pub fn validate(&self) -> Result<(), CorpusConfigError> {
        if self.schema_version != CORPUS_SCHEMA_VERSION {
            return Err(CorpusConfigError::UnsupportedSchema(self.schema_version));
        }
        if self.seed_count == 0 {
            return Err(CorpusConfigError::EmptySeedBlock);
        }
        if self.target_min_rooms == 0 || self.target_min_rooms > self.target_max_rooms {
            return Err(CorpusConfigError::InvalidTargetRange {
                minimum: self.target_min_rooms,
                maximum: self.target_max_rooms,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusConfigError {
    UnsupportedSchema(u32),
    EmptySeedBlock,
    InvalidTargetRange { minimum: usize, maximum: usize },
}

impl fmt::Display for CorpusConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported corpus schema version {version}")
            }
            Self::EmptySeedBlock => write!(formatter, "corpus seed block must not be empty"),
            Self::InvalidTargetRange { minimum, maximum } => write!(
                formatter,
                "corpus target range must satisfy 0 < minimum <= maximum, got {minimum}..={maximum}"
            ),
        }
    }
}

impl Error for CorpusConfigError {}
