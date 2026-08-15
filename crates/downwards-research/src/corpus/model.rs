use downwards_core::AbilitySet;
use downwards_gen::{CompositionalFeatureSet, StagedCompositionalKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationLoadout {
    Baseline,
    WallJump,
    Dash,
    Both,
}

impl EvaluationLoadout {
    pub const ALL: [Self; 4] = [Self::Baseline, Self::WallJump, Self::Dash, Self::Both];

    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        match self {
            Self::Baseline => AbilitySet::NONE,
            Self::WallJump => AbilitySet::new(true, false),
            Self::Dash => AbilitySet::new(false, true),
            Self::Both => AbilitySet::ALL,
        }
    }

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
            Self::Both => "both",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureStageRecord {
    TerrainOnly,
    StaticHazards,
    TimedHazards,
}

impl FeatureStageRecord {
    #[must_use]
    pub const fn feature_set(self) -> CompositionalFeatureSet {
        match self {
            Self::TerrainOnly => CompositionalFeatureSet::TerrainOnly,
            Self::StaticHazards => CompositionalFeatureSet::StaticHazards,
            Self::TimedHazards => CompositionalFeatureSet::TimedHazards,
        }
    }

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::TerrainOnly => "terrain-only",
            Self::StaticHazards => "static-hazards",
            Self::TimedHazards => "timed-hazards",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateKeyRecord {
    pub seed: u64,
    pub construction_loadout: EvaluationLoadout,
    pub strategy: String,
    pub intent: String,
    pub feature_stage: FeatureStageRecord,
}

impl CandidateKeyRecord {
    #[must_use]
    pub fn from_staged_key(key: StagedCompositionalKey) -> Self {
        Self {
            seed: key.source.seed,
            construction_loadout: loadout_from_abilities(key.source.profile.abilities),
            strategy: key.source.profile.strategy.slug().to_owned(),
            intent: key.source.profile.intent.slug().to_owned(),
            feature_stage: match key.features {
                CompositionalFeatureSet::TerrainOnly => FeatureStageRecord::TerrainOnly,
                CompositionalFeatureSet::StaticHazards => FeatureStageRecord::StaticHazards,
                CompositionalFeatureSet::TimedHazards => FeatureStageRecord::TimedHazards,
            },
        }
    }

    #[must_use]
    pub fn stable_slug(&self) -> String {
        format!(
            "s{:016x}-{}-{}-{}-{}",
            self.seed,
            self.construction_loadout.slug(),
            self.strategy,
            self.intent,
            self.feature_stage.slug(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoomId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConstructionRecord {
    Constructed {
        room_id: RoomId,
        static_visual_fingerprint: String,
        simulation_geometry_fingerprint: String,
        port_count: usize,
        pickup_count: usize,
        route_signature: String,
        cycle_rank: u16,
        canonical: bool,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationAttemptRecord {
    pub key: CandidateKeyRecord,
    pub construction: ConstructionRecord,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationBatchSummary {
    pub attempted: usize,
    pub constructed: usize,
    pub rejected: usize,
    pub exact_static_rooms: usize,
    pub alias_candidates: usize,
}

pub(crate) const fn loadout_from_abilities(abilities: AbilitySet) -> EvaluationLoadout {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => EvaluationLoadout::Baseline,
        (true, false) => EvaluationLoadout::WallJump,
        (false, true) => EvaluationLoadout::Dash,
        (true, true) => EvaluationLoadout::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::{CandidateKeyRecord, EvaluationLoadout, FeatureStageRecord};

    #[test]
    fn candidate_slug_uses_explicit_canonical_feature_stage_names() {
        let mut key = CandidateKeyRecord {
            seed: 0x2a,
            construction_loadout: EvaluationLoadout::WallJump,
            strategy: "cyclic-graph".to_owned(),
            intent: "technical".to_owned(),
            feature_stage: FeatureStageRecord::TerrainOnly,
        };
        assert_eq!(
            key.stable_slug(),
            "s000000000000002a-wall-jump-cyclic-graph-technical-terrain-only"
        );

        key.feature_stage = FeatureStageRecord::StaticHazards;
        assert!(key.stable_slug().ends_with("-static-hazards"));
        key.feature_stage = FeatureStageRecord::TimedHazards;
        assert!(key.stable_slug().ends_with("-timed-hazards"));
    }
}
