use std::collections::{HashMap, HashSet};

use downwards_catalogue::{
    CatalogueEntry, CatalogueManifest, CorpusPlaytestEntry, CorpusPlaytestManifest,
    CorpusPlaytestPublicationState, RepresentativeActions, RepresentativePickup,
};
use downwards_core::Simulation;
use downwards_gen::AbilityTier;

const ABILITY_TIERS: [AbilityTier; 4] = [
    AbilityTier::Baseline,
    AbilityTier::WallJump,
    AbilityTier::Dash,
    AbilityTier::WallJumpAndDash,
];

const NAME_FIRST_WORDS: [&str; 16] = [
    "AMBER", "BLUE", "BRISK", "CALM", "COLD", "DARK", "FAINT", "GOLD", "GREEN", "PALE", "QUICK",
    "RED", "SHARP", "STILL", "WARM", "WHITE",
];
const NAME_SECOND_WORDS: [&str; 16] = [
    "BAT", "BELL", "BIRD", "BONE", "COIN", "CROW", "DUSK", "FLAME", "FROG", "GLASS", "MOSS",
    "MOTH", "PEARL", "RAIN", "STAR", "WOLF",
];
const NAME_THIRD_WORDS: [&str; 16] = [
    "ARCH", "CAVE", "CLIFF", "GATE", "HALL", "KEEP", "LAKE", "PATH", "PIT", "SHAFT", "SPIRE",
    "STAIR", "VAULT", "WELL", "WOOD", "YARD",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlayableEntry {
    Legacy(CatalogueEntry),
    Corpus(CorpusPlaytestEntry),
}

impl PlayableEntry {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Legacy(entry) => entry.id(),
            Self::Corpus(entry) => entry.id(),
        }
    }
    pub(crate) fn index(&self) -> usize {
        match self {
            Self::Legacy(entry) => entry.index(),
            Self::Corpus(entry) => entry.index(),
        }
    }
    pub(crate) fn source_door_id(&self) -> &str {
        match self {
            Self::Legacy(entry) => entry.source_door_id(),
            Self::Corpus(entry) => entry.source_door_id(),
        }
    }
    pub(crate) fn target_door_id(&self) -> &str {
        match self {
            Self::Legacy(entry) => entry.target_door_id(),
            Self::Corpus(entry) => entry.target_door_id(),
        }
    }
    pub(crate) fn representative_actions(&self) -> Option<&RepresentativeActions> {
        match self {
            Self::Legacy(entry) => entry.representative_actions(),
            Self::Corpus(entry) => Some(entry.representative_actions()),
        }
    }
    pub(crate) fn representative_pickup(&self) -> Option<&RepresentativePickup> {
        match self {
            Self::Legacy(entry) => entry.representative_pickup(),
            Self::Corpus(_) => None,
        }
    }
    pub(crate) const fn legacy(&self) -> Option<&CatalogueEntry> {
        match self {
            Self::Legacy(entry) => Some(entry),
            Self::Corpus(_) => None,
        }
    }
    pub(crate) const fn corpus(&self) -> Option<&CorpusPlaytestEntry> {
        match self {
            Self::Legacy(_) => None,
            Self::Corpus(entry) => Some(entry),
        }
    }
    pub(crate) fn load_corpus(&self) -> Result<Option<Simulation>, String> {
        match self {
            Self::Legacy(_) => Ok(None),
            Self::Corpus(entry) => entry
                .load_verified()
                .map(Some)
                .map_err(|error| error.to_string()),
        }
    }
    pub(crate) fn identity(&self) -> String {
        match self {
            Self::Legacy(entry) => legacy_entry_identity(entry),
            Self::Corpus(entry) => format!(
                "{}|{}|{:016x}",
                entry.id(),
                entry.key().generator_slug(),
                entry.key().source_seed()
            ),
        }
    }
}

pub(crate) struct PlayableCatalogue {
    entries: HashMap<AbilityTier, Vec<PlayableEntry>>,
    names: HashMap<String, String>,
    corpus: bool,
    provisional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PolicyIdentity {
    manifest_version: u32,
    compositional_generation_version: u32,
    experimental_backing_version: u32,
    selection_version: u32,
    solver_policy_version: u32,
    difficulty_heuristic_version: u32,
    difficulty_config_id: String,
}

impl PolicyIdentity {
    fn of(manifest: &CatalogueManifest) -> Self {
        let identity = manifest.identity();
        Self {
            manifest_version: identity.manifest_version,
            compositional_generation_version: identity.compositional_generation_version,
            experimental_backing_version: identity.experimental_backing_version,
            selection_version: identity.selection_version,
            solver_policy_version: identity.solver_policy_version,
            difficulty_heuristic_version: identity.difficulty_heuristic_version,
            difficulty_config_id: identity.difficulty_config_id.clone(),
        }
    }
}

impl PlayableCatalogue {
    pub(crate) fn parse(sources: &[(&str, &str)]) -> Result<Self, String> {
        let mut manifests = HashMap::new();
        let mut ids = HashSet::new();
        let mut shared_policy = None;
        for &(path, source) in sources {
            let manifest = CatalogueManifest::parse(source)
                .map_err(|error| format!("could not load {path}: {error}"))?;
            let policy = PolicyIdentity::of(&manifest);
            if let Some(expected) = &shared_policy {
                if expected != &policy {
                    return Err(format!(
                        "curated manifest {path} uses a different generation/selection/AI policy"
                    ));
                }
            } else {
                shared_policy = Some(policy);
            }
            let tier = manifest.tier();
            if manifests.insert(tier, manifest).is_some() {
                return Err(format!("multiple curated manifests supplied for {tier:?}"));
            }
        }
        let mut entries = HashMap::new();
        for tier in ABILITY_TIERS {
            let manifest = manifests
                .remove(&tier)
                .ok_or_else(|| format!("no curated manifest supplied for {tier:?}"))?;
            if manifest.entries().is_empty() {
                return Err(format!("curated manifest for {tier:?} contains no rooms"));
            }
            let tier_entries = manifest
                .entries()
                .iter()
                .cloned()
                .map(PlayableEntry::Legacy)
                .collect::<Vec<_>>();
            for entry in &tier_entries {
                if !ids.insert(entry.id().to_owned()) {
                    return Err(format!(
                        "curated room id {:?} appears in multiple manifests",
                        entry.id()
                    ));
                }
            }
            entries.insert(tier, tier_entries);
        }
        Self::finish(entries, false, false)
    }

    pub(crate) fn parse_corpus(source: &str, allow_provisional: bool) -> Result<Self, String> {
        let manifest = CorpusPlaytestManifest::parse(source, allow_provisional)
            .map_err(|error| format!("could not load corpus playtest manifest: {error}"))?;
        let mut entries = HashMap::new();
        for tier in ABILITY_TIERS {
            entries.insert(tier, Vec::new());
        }
        for entry in manifest.entries() {
            let tier = AbilityTier::from_abilities(entry.key().construction_loadout().abilities());
            entries
                .get_mut(&tier)
                .expect("all tiers initialized")
                .push(PlayableEntry::Corpus(entry.clone()));
        }
        let provisional = manifest.publication_state()
            == CorpusPlaytestPublicationState::ProvisionalOperationalCache;
        Self::finish(entries, true, provisional)
    }

    fn finish(
        entries: HashMap<AbilityTier, Vec<PlayableEntry>>,
        corpus: bool,
        provisional: bool,
    ) -> Result<Self, String> {
        let names = assign_unique_names(entries.values().flatten())?;
        Ok(Self {
            entries,
            names,
            corpus,
            provisional,
        })
    }

    pub(crate) fn entries(&self, tier: AbilityTier) -> &[PlayableEntry] {
        self.entries.get(&tier).expect("all tiers loaded")
    }
    pub(crate) fn entry(&self, tier: AbilityTier, index: usize) -> Option<&PlayableEntry> {
        self.entries
            .get(&tier)
            .and_then(|entries| entries.get(index))
    }
    pub(crate) fn name(&self, entry: &PlayableEntry) -> &str {
        self.names
            .get(entry.id())
            .map(String::as_str)
            .expect("every entry named")
    }
    pub(crate) const fn is_corpus(&self) -> bool {
        self.corpus
    }
    pub(crate) const fn is_provisional(&self) -> bool {
        self.provisional
    }
}

fn assign_unique_names<'a>(
    entries: impl Iterator<Item = &'a PlayableEntry>,
) -> Result<HashMap<String, String>, String> {
    let mut entries = entries.collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| entry.identity());
    if entries.len() > 4_096 {
        return Err("the three-word name space holds at most 4096 rooms".to_owned());
    }
    let mut used_slots = HashSet::new();
    let mut names = HashMap::new();
    for entry in entries {
        let identity = entry.identity();
        let mut slot = stable_hash(identity.as_bytes()) as usize & 0x0fff;
        while !used_slots.insert(slot) {
            slot = (slot + 1) & 0x0fff;
        }
        let first = NAME_FIRST_WORDS[slot & 0x0f];
        let second = NAME_SECOND_WORDS[(slot >> 4) & 0x0f];
        let third = NAME_THIRD_WORDS[(slot >> 8) & 0x0f];
        names.insert(entry.id().to_owned(), format!("{first} {second} {third}"));
    }
    Ok(names)
}

fn legacy_entry_identity(entry: &CatalogueEntry) -> String {
    let key = entry.key();
    format!(
        "{}|{:016x}|{}|{}|{}|{}|{}|{}|{:016x}",
        entry.id(),
        key.seed,
        u8::from(key.profile.abilities.wall_jump),
        u8::from(key.profile.abilities.dash),
        key.profile.strategy.slug(),
        key.profile.intent.slug(),
        entry.band().slug(),
        format_args!("{}>{}", entry.source_door_id(), entry.target_door_id()),
        entry.visual_fingerprint()
    )
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in b"downwards-catalogue-name-v2\0".iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_hash_is_repeatable_and_sensitive_to_profile_fields() {
        assert_eq!(stable_hash(b"room-a"), stable_hash(b"room-a"));
        assert_ne!(stable_hash(b"room-a"), stable_hash(b"room-b"));
    }
}
