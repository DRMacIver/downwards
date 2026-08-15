//! Production-facing facade for generation-v6 compositional rooms.
//!
//! Unlike the legacy whole-room families, a v6 room is identified by an exact
//! constructive strategy and challenge intent. [`CompositionalKey`] is the
//! complete regeneration key: selection never substitutes a different seed or
//! ability loadout when one composition fails to embed.

use std::{error::Error, fmt};

use downwards_core::{AbilitySet, Room};

use crate::{
    GeneratedLevel,
    experimental::{
        BoundaryPort, ChallengeIntent, ExperimentalCandidate, ExperimentalGenerationError,
        GenerationStrategy, RoutePlan, RoutePlanSummary, generate_candidate,
    },
};

/// Seed-to-room mapping version exposed by the compositional facade.
pub const COMPOSITIONAL_GENERATION_VERSION: u32 = 6;

/// Version of the deterministic feature-layer transform applied by
/// [`generate_staged_compositional`].
///
/// This version is deliberately independent from
/// [`COMPOSITIONAL_GENERATION_VERSION`]: staged candidates are derived from an
/// exact v6 candidate, while the legacy v6 seed mapping remains frozen.
pub const COMPOSITIONAL_FEATURE_STAGE_VERSION: u32 = 1;

const COMPOSITION_COUNT: usize = GenerationStrategy::ALL.len() * ChallengeIntent::ALL.len();

/// The exact generation controls independent of the room seed.
///
/// `abilities` is deliberately an [`AbilitySet`], rather than an ordered tier:
/// wall jump and dash are independent unlocks and validation must use exactly
/// the same loadout as generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalProfile {
    pub abilities: AbilitySet,
    pub strategy: GenerationStrategy,
    pub intent: ChallengeIntent,
}

impl CompositionalProfile {
    #[must_use]
    pub const fn new(
        abilities: AbilitySet,
        strategy: GenerationStrategy,
        intent: ChallengeIntent,
    ) -> Self {
        Self {
            abilities,
            strategy,
            intent,
        }
    }
}

/// Complete, stable regeneration identity for one compositional room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositionalKey {
    pub seed: u64,
    pub profile: CompositionalProfile,
}

impl CompositionalKey {
    #[must_use]
    pub const fn new(seed: u64, profile: CompositionalProfile) -> Self {
        Self { seed, profile }
    }

    /// Regenerate precisely this key without selection or fallback.
    pub fn regenerate(self) -> Result<CompositionalCandidate, CompositionalGenerationError> {
        generate_compositional(self)
    }
}

/// Generator features retained in a staged compositional candidate.
///
/// The variants are cumulative. Each stage starts from the exact same v6 room
/// and removes only feature layers introduced after that stage, so terrain and
/// route provenance remain directly comparable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CompositionalFeatureSet {
    /// Solids, one-way platforms, doors, and pickups, with no hazards.
    TerrainOnly,
    /// Terrain plus static [`downwards_core::Tile::Hazard`] tiles.
    StaticHazards,
    /// The complete current v6 output, including timed hazards.
    TimedHazards,
}

impl CompositionalFeatureSet {
    /// All feature sets in cumulative order.
    pub const ALL: [Self; 3] = [Self::TerrainOnly, Self::StaticHazards, Self::TimedHazards];

    /// The feature set corresponding to the complete current v6 generator.
    pub const CURRENT: Self = Self::TimedHazards;

    /// Stable identifier used in room identity and external provenance.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::TerrainOnly => "terrain-only",
            Self::StaticHazards => "static-hazards",
            Self::TimedHazards => "timed-hazards",
        }
    }

    #[must_use]
    const fn retains_static_hazards(self) -> bool {
        matches!(self, Self::StaticHazards | Self::TimedHazards)
    }

    #[must_use]
    const fn retains_timed_hazards(self) -> bool {
        matches!(self, Self::TimedHazards)
    }
}

/// Complete regeneration identity for one feature-staged v6 room.
///
/// Staged keys are intentionally a different type from [`CompositionalKey`].
/// This prevents a terrain-only evaluation artifact from being mistaken for
/// the legacy/full v6 room with the same seed and profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StagedCompositionalKey {
    pub source: CompositionalKey,
    pub features: CompositionalFeatureSet,
}

impl StagedCompositionalKey {
    #[must_use]
    pub const fn new(source: CompositionalKey, features: CompositionalFeatureSet) -> Self {
        Self { source, features }
    }

    /// Regenerate precisely this staged key without selection or fallback.
    pub fn regenerate(
        self,
    ) -> Result<StagedCompositionalCandidate, StagedCompositionalGenerationError> {
        generate_staged_compositional(self)
    }
}

/// A structurally valid v6 room and its compositional provenance.
///
/// Solver acceptance is intentionally not implied. Offline curation remains
/// responsible for checking every ordered door pair and every pickup from
/// every entrance before placing a key in the playable catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalCandidate {
    pub key: CompositionalKey,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
}

/// A structurally valid feature-staged v6 room and its full provenance.
///
/// The route plan and boundary ports describe the common source geometry and
/// are preserved across all feature stages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedCompositionalCandidate {
    pub key: StagedCompositionalKey,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
}

/// Construction failure for one exact compositional key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionalGenerationError {
    pub key: CompositionalKey,
    pub cause: ExperimentalGenerationError,
}

impl fmt::Display for CompositionalGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v6 composition {} {} for seed {:016x} failed: {}",
            self.key.profile.strategy.slug(),
            self.key.profile.intent.slug(),
            self.key.seed,
            self.cause
        )
    }
}

impl Error for CompositionalGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Construction failure for one exact feature-staged compositional key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedCompositionalGenerationError {
    pub key: StagedCompositionalKey,
    pub cause: ExperimentalGenerationError,
}

impl fmt::Display for StagedCompositionalGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v6 feature stage v{} {} {} {} for seed {:016x} failed: {}",
            COMPOSITIONAL_FEATURE_STAGE_VERSION,
            self.key.features.slug(),
            self.key.source.profile.strategy.slug(),
            self.key.source.profile.intent.slug(),
            self.key.source.seed,
            self.cause
        )
    }
}

impl Error for StagedCompositionalGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// One failed composition considered by [`generate_uncurated`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionFailure {
    pub key: CompositionalKey,
    pub error: CompositionalGenerationError,
}

/// Every strategy/intent composition failed for one unchanged seed/profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncuratedGenerationError {
    pub seed: u64,
    pub abilities: AbilitySet,
    pub failures: Vec<CompositionFailure>,
}

impl fmt::Display for UncuratedGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "all {} v6 compositions failed for seed {:016x} with abilities {:?}",
            self.failures.len(),
            self.seed,
            self.abilities
        )
    }
}

impl Error for UncuratedGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.failures
            .first()
            .map(|failure| &failure.error as &(dyn Error + 'static))
    }
}

/// Generate exactly one v6 compositional key.
///
/// This is deterministic for both success and failure. The returned room is
/// labelled as `generated-v6-compositional`; the key, rather than the legacy
/// presentation-family field in [`crate::GeneratedMetadata`], is authoritative
/// provenance.
pub fn generate_compositional(
    key: CompositionalKey,
) -> Result<CompositionalCandidate, CompositionalGenerationError> {
    let CompositionalProfile {
        abilities,
        strategy,
        intent,
    } = key.profile;
    let candidate = generate_candidate(key.seed, abilities, strategy, intent)
        .map_err(|cause| CompositionalGenerationError { key, cause })?;
    promote_candidate(key, candidate).map_err(|cause| CompositionalGenerationError { key, cause })
}

/// Generate one explicitly feature-staged compositional key.
///
/// Generation first obtains the exact legacy v6 candidate identified by
/// [`StagedCompositionalKey::source`]. It then removes disallowed hazard
/// layers without rerunning or perturbing the constructive RNG stream. This
/// keeps solids, one-way platforms, doors, pickups, route plans, and boundary
/// ports identical across stages.
///
/// The returned room identity includes both
/// [`COMPOSITIONAL_FEATURE_STAGE_VERSION`] and the feature-set slug. It cannot
/// collide with the source room or with another feature stage.
pub fn generate_staged_compositional(
    key: StagedCompositionalKey,
) -> Result<StagedCompositionalCandidate, StagedCompositionalGenerationError> {
    let source =
        generate_compositional(key.source).map_err(|error| StagedCompositionalGenerationError {
            key,
            cause: error.cause,
        })?;
    stage_candidate(key, source).map_err(|cause| StagedCompositionalGenerationError { key, cause })
}

/// Generate an uncurated room by trying every composition in deterministic
/// seed-rotated order.
///
/// This is a development fallback, not the playable catalogue: it performs no
/// AI reachability or difficulty assessment. Every attempt retains `seed` and
/// `abilities`; only strategy and intent rotate.
pub fn generate_uncurated(
    seed: u64,
    abilities: AbilitySet,
) -> Result<CompositionalCandidate, UncuratedGenerationError> {
    let mut failures = Vec::new();
    for key in uncurated_attempt_order(seed, abilities) {
        match generate_compositional(key) {
            Ok(candidate) => return Ok(candidate),
            Err(error) => failures.push(CompositionFailure { key, error }),
        }
    }
    Err(UncuratedGenerationError {
        seed,
        abilities,
        failures,
    })
}

/// The complete deterministic fallback order used by [`generate_uncurated`].
///
/// Consecutive seed residues rotate the first preference across all nine
/// compositions. The order intentionally does not depend on abilities, which
/// makes the same seed comparable under different unlock test profiles.
#[must_use]
pub fn uncurated_attempt_order(
    seed: u64,
    abilities: AbilitySet,
) -> [CompositionalKey; COMPOSITION_COUNT] {
    let profiles = all_profiles(abilities);
    let offset = (seed % COMPOSITION_COUNT as u64) as usize;
    std::array::from_fn(|index| CompositionalKey {
        seed,
        profile: profiles[(offset + index) % COMPOSITION_COUNT],
    })
}

fn all_profiles(abilities: AbilitySet) -> [CompositionalProfile; COMPOSITION_COUNT] {
    std::array::from_fn(|index| {
        let strategy = GenerationStrategy::ALL[index / ChallengeIntent::ALL.len()];
        let intent = ChallengeIntent::ALL[index % ChallengeIntent::ALL.len()];
        CompositionalProfile::new(abilities, strategy, intent)
    })
}

fn promote_candidate(
    key: CompositionalKey,
    candidate: ExperimentalCandidate,
) -> Result<CompositionalCandidate, ExperimentalGenerationError> {
    let ExperimentalCandidate {
        mut generated,
        route_plan,
        route_summary,
        boundary_ports,
        ..
    } = candidate;

    // Rebuild only to give this production-facing artifact a stable v6
    // identity. Geometry and object order are preserved exactly.
    let original = &generated.room;
    let room = Room::new(
        format!(
            "generated-v{COMPOSITIONAL_GENERATION_VERSION}-compositional-{}-{}-{:016x}",
            key.profile.strategy.slug(),
            key.profile.intent.slug(),
            key.seed
        ),
        format!(
            "Generated v{COMPOSITIONAL_GENERATION_VERSION} compositional {} {} {:016x}",
            key.profile.strategy.slug(),
            key.profile.intent.slug(),
            key.seed
        ),
        original.width(),
        original.height(),
        original.tile_size(),
        original.tiles().to_vec(),
        original.spawn(),
        Vec::new(),
    )?
    .with_objects(
        original.timed_hazards().to_vec(),
        original.pickups().to_vec(),
    )?
    .with_doors(original.doors().to_vec())?;
    generated.room = room;
    generated.metadata.generation_version = COMPOSITIONAL_GENERATION_VERSION;

    Ok(CompositionalCandidate {
        key,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
    })
}

fn stage_candidate(
    key: StagedCompositionalKey,
    candidate: CompositionalCandidate,
) -> Result<StagedCompositionalCandidate, ExperimentalGenerationError> {
    let CompositionalCandidate {
        mut generated,
        route_plan,
        route_summary,
        boundary_ports,
        ..
    } = candidate;
    let original = &generated.room;
    let tiles = original
        .tiles()
        .iter()
        .map(|tile| {
            if *tile == downwards_core::Tile::Hazard && !key.features.retains_static_hazards() {
                downwards_core::Tile::Empty
            } else {
                *tile
            }
        })
        .collect();
    let timed_hazards = if key.features.retains_timed_hazards() {
        original.timed_hazards().to_vec()
    } else {
        Vec::new()
    };
    let room = Room::new(
        format!(
            "generated-v{COMPOSITIONAL_GENERATION_VERSION}-compositional-features-v{}-{}-{}-{}-{:016x}",
            COMPOSITIONAL_FEATURE_STAGE_VERSION,
            key.features.slug(),
            key.source.profile.strategy.slug(),
            key.source.profile.intent.slug(),
            key.source.seed
        ),
        format!(
            "Generated v{COMPOSITIONAL_GENERATION_VERSION} compositional features v{} {} {} {} {:016x}",
            COMPOSITIONAL_FEATURE_STAGE_VERSION,
            key.features.slug(),
            key.source.profile.strategy.slug(),
            key.source.profile.intent.slug(),
            key.source.seed
        ),
        original.width(),
        original.height(),
        original.tile_size(),
        tiles,
        original.spawn(),
        original.exits().to_vec(),
    )?
    .with_objects(timed_hazards, original.pickups().to_vec())?
    .with_doors(original.doors().to_vec())?;
    generated.room = room;
    if !key.features.retains_static_hazards() {
        generated.metadata.stats.hazard_tiles = 0;
        generated.metadata.stats.hazard_clusters = 0;
    }
    if !key.features.retains_timed_hazards() {
        generated.metadata.stats.timed_hazards = 0;
    }

    Ok(StagedCompositionalCandidate {
        key,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use downwards_core::{DoorSocket, Tile};

    use super::*;

    const ALL_ABILITIES: [AbilitySet; 4] = [
        AbilitySet::NONE,
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::ALL,
    ];

    #[test]
    fn exact_keys_and_uncurated_selection_are_deterministic() {
        for abilities in ALL_ABILITIES {
            for seed in 0..64 {
                let first = generate_uncurated(seed, abilities);
                let second = generate_uncurated(seed, abilities);
                assert_eq!(first, second, "seed {seed}, abilities {abilities:?}");

                if let Ok(selected) = first {
                    assert_eq!(selected.key.seed, seed);
                    assert_eq!(selected.key.profile.abilities, abilities);
                    assert_eq!(selected.key.regenerate().unwrap(), selected);
                }
            }
        }
    }

    #[test]
    fn feature_stages_are_deterministic_and_preserve_source_geometry() {
        const SAMPLE: u64 = 128;

        let mut saw_static_hazard = false;
        let mut saw_timed_hazard = false;
        for abilities in ALL_ABILITIES {
            for seed in 0..SAMPLE {
                let source = generate_uncurated(seed, abilities).unwrap();
                let source_room = &source.generated.room;
                saw_static_hazard |= source_room.tiles().contains(&Tile::Hazard);
                saw_timed_hazard |= !source_room.timed_hazards().is_empty();

                let mut identities = HashSet::new();
                identities.insert(source_room.id().to_owned());
                for features in CompositionalFeatureSet::ALL {
                    let key = StagedCompositionalKey::new(source.key, features);
                    let staged = generate_staged_compositional(key).unwrap();
                    assert_eq!(key.regenerate().unwrap(), staged);
                    assert_eq!(staged.key, key);
                    assert!(identities.insert(staged.generated.room.id().to_owned()));

                    assert_eq!(staged.route_plan, source.route_plan);
                    assert_eq!(staged.route_summary, source.route_summary);
                    assert_eq!(staged.boundary_ports, source.boundary_ports);

                    let room = &staged.generated.room;
                    assert_eq!(room.width(), source_room.width());
                    assert_eq!(room.height(), source_room.height());
                    assert_eq!(room.tile_size(), source_room.tile_size());
                    assert_eq!(room.spawn(), source_room.spawn());
                    assert_eq!(room.exits(), source_room.exits());
                    assert_eq!(room.doors(), source_room.doors());
                    assert_eq!(room.pickups(), source_room.pickups());
                    if features == CompositionalFeatureSet::TimedHazards {
                        assert_eq!(room.timed_hazards(), source_room.timed_hazards());
                    } else {
                        assert!(room.timed_hazards().is_empty());
                    }
                    assert_eq!(room.tiles().len(), source_room.tiles().len());
                    for (actual, source_tile) in room.tiles().iter().zip(source_room.tiles()) {
                        let expected = if *source_tile == Tile::Hazard
                            && features == CompositionalFeatureSet::TerrainOnly
                        {
                            Tile::Empty
                        } else {
                            *source_tile
                        };
                        assert_eq!(*actual, expected);
                    }

                    let mut expected_metadata = source.generated.metadata.clone();
                    if features == CompositionalFeatureSet::TerrainOnly {
                        expected_metadata.stats.hazard_tiles = 0;
                        expected_metadata.stats.hazard_clusters = 0;
                    }
                    if features != CompositionalFeatureSet::TimedHazards {
                        expected_metadata.stats.timed_hazards = 0;
                    }
                    assert_eq!(staged.generated.metadata, expected_metadata);
                }
                assert_eq!(identities.len(), 4);
            }
        }
        assert!(saw_static_hazard);
        assert!(saw_timed_hazard);
    }

    #[test]
    fn feature_stages_exclude_forbidden_hazards_across_many_seeds() {
        const SAMPLE: u64 = 256;

        for abilities in ALL_ABILITIES {
            for seed in 0..SAMPLE {
                let source_key = generate_uncurated(seed, abilities).unwrap().key;
                let terrain =
                    StagedCompositionalKey::new(source_key, CompositionalFeatureSet::TerrainOnly)
                        .regenerate()
                        .unwrap();
                assert!(!terrain.generated.room.tiles().contains(&Tile::Hazard));
                assert!(terrain.generated.room.timed_hazards().is_empty());
                assert_eq!(terrain.generated.metadata.stats.hazard_tiles, 0);
                assert_eq!(terrain.generated.metadata.stats.hazard_clusters, 0);
                assert_eq!(terrain.generated.metadata.stats.timed_hazards, 0);

                let static_hazards =
                    StagedCompositionalKey::new(source_key, CompositionalFeatureSet::StaticHazards)
                        .regenerate()
                        .unwrap();
                assert!(static_hazards.generated.room.timed_hazards().is_empty());
                assert_eq!(static_hazards.generated.metadata.stats.timed_hazards, 0);

                let current =
                    StagedCompositionalKey::new(source_key, CompositionalFeatureSet::CURRENT)
                        .regenerate()
                        .unwrap();
                assert_eq!(
                    current.generated.room.timed_hazards().len(),
                    usize::from(current.generated.metadata.stats.timed_hazards)
                );
            }
        }
    }

    #[test]
    fn rotation_covers_every_strategy_and_intent_without_changing_seed() {
        let abilities = AbilitySet::new(false, true);
        let mut first_profiles = HashSet::new();
        for seed in 0..COMPOSITION_COUNT as u64 {
            let order = uncurated_attempt_order(seed, abilities);
            assert_eq!(order.len(), COMPOSITION_COUNT);
            assert!(order.iter().all(|key| key.seed == seed));
            assert!(order.iter().all(|key| key.profile.abilities == abilities));
            assert_eq!(
                order.iter().map(|key| key.profile).collect::<HashSet<_>>(),
                all_profiles(abilities).into_iter().collect()
            );
            first_profiles.insert(order[0].profile);
        }
        assert_eq!(first_profiles.len(), COMPOSITION_COUNT);
    }

    #[test]
    fn v6_rooms_use_only_two_to_four_boundary_doors() {
        for abilities in ALL_ABILITIES {
            for seed in 0..128 {
                let candidate = generate_uncurated(seed, abilities).unwrap();
                let room = &candidate.generated.room;
                assert!(room.id().starts_with("generated-v6-compositional-"));
                assert_eq!(candidate.generated.metadata.generation_version, 6);
                assert!(room.exits().is_empty());
                assert!((2..=4).contains(&room.doors().len()));
                assert_eq!(room.doors().len(), candidate.boundary_ports.len());
            }
        }
    }

    #[test]
    fn sockets_are_gridded_and_the_generated_inventory_contains_mates() {
        let mut inventory = HashSet::<DoorSocket>::new();
        for abilities in ALL_ABILITIES {
            for seed in 0..256 {
                let candidate = generate_uncurated(seed, abilities).unwrap();
                for door in candidate.generated.room.doors() {
                    let socket = door.socket();
                    assert_eq!(socket.span, 2 * crate::TILE_SIZE);
                    assert_eq!(socket.offset.rem_euclid(crate::TILE_SIZE), 0);
                    inventory.insert(socket);
                }
            }
        }

        assert!(!inventory.is_empty());
        for socket in &inventory {
            assert!(
                inventory.contains(&socket.mate()),
                "no generated mate for {socket:?}"
            );
        }
    }

    #[test]
    fn uncurated_selector_has_substantial_static_and_collision_diversity() {
        const SAMPLE: u64 = 256;
        const MIN_UNIQUE: usize = 230;

        for abilities in ALL_ABILITIES {
            let mut static_rooms = HashSet::new();
            let mut tile_fields = HashSet::new();
            let mut collision_fields = HashSet::new();
            let mut composition_counts = HashMap::new();
            for seed in 0..SAMPLE {
                let candidate = generate_uncurated(seed, abilities).unwrap();
                let room = &candidate.generated.room;
                let tiles = room
                    .tiles()
                    .iter()
                    .copied()
                    .map(tile_code)
                    .collect::<Vec<_>>();
                let collision = room
                    .tiles()
                    .iter()
                    .map(|tile| match tile {
                        Tile::Solid => 1,
                        Tile::OneWay => 2,
                        Tile::Empty | Tile::Hazard => 0,
                    })
                    .collect::<Vec<_>>();
                let mut doors = room
                    .doors()
                    .iter()
                    .map(|door| door.socket())
                    .collect::<Vec<_>>();
                doors.sort_unstable();
                let static_descriptor = (tiles.clone(), doors);

                static_rooms.insert(static_descriptor);
                tile_fields.insert(tiles);
                collision_fields.insert(collision);
                *composition_counts
                    .entry(candidate.key.profile)
                    .or_insert(0usize) += 1;
            }

            assert!(
                static_rooms.len() >= MIN_UNIQUE,
                "{abilities:?}: {} unique static rooms",
                static_rooms.len()
            );
            assert!(
                tile_fields.len() >= MIN_UNIQUE,
                "{abilities:?}: {} unique tile fields",
                tile_fields.len()
            );
            assert!(
                collision_fields.len() >= MIN_UNIQUE,
                "{abilities:?}: {} unique collision fields",
                collision_fields.len()
            );
            assert_eq!(composition_counts.len(), COMPOSITION_COUNT);
            eprintln!(
                "{abilities:?}: static={} tiles={} collision={} compositions={}",
                static_rooms.len(),
                tile_fields.len(),
                collision_fields.len(),
                composition_counts.len()
            );
        }
    }

    const fn tile_code(tile: Tile) -> u8 {
        match tile {
            Tile::Empty => 0,
            Tile::Solid => 1,
            Tile::Hazard => 2,
            Tile::OneWay => 3,
        }
    }
}
