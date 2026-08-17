//! Deterministic generation of single-screen room candidates.
//!
//! This crate guarantees structural room invariants, not playability. Every
//! generated room must subsequently be accepted by the game-playing solver
//! using [`GeneratedMetadata::intended_abilities`]. The solver and the
//! authoritative simulation, rather than a geometric approximation here, are
//! the source of truth for solvability and difficulty.

#![forbid(unsafe_code)]

mod calibrated_wall_jump;
pub mod experimental;
mod room_grid;
pub mod v6;

pub use calibrated_wall_jump::{
    CALIBRATED_WALL_JUMP_ABILITIES, CALIBRATED_WALL_JUMP_GENERATION_VERSION,
    CALIBRATED_WALL_JUMP_TARGET, CalibratedWallJumpCandidate, CalibratedWallJumpCourse,
    CalibratedWallJumpKey, CalibratedWallJumpParameters,
};
pub use room_grid::{
    HAZARD_AMBER_WIND_UP_TICKS, HAZARD_MIN_OFF_TICKS, hazard_off_time_deficit, parse_room_grid,
    render_room_grid, useless_spikes,
};

pub use v6::{
    COMPOSITIONAL_FEATURE_STAGE_VERSION, COMPOSITIONAL_GENERATION_VERSION, CompositionFailure,
    CompositionalCandidate, CompositionalFeatureSet, CompositionalGenerationError,
    CompositionalKey, CompositionalProfile, StagedCompositionalCandidate,
    StagedCompositionalGenerationError, StagedCompositionalKey, UncuratedGenerationError,
    generate_compositional, generate_staged_compositional, generate_uncurated,
    uncurated_attempt_order,
};

use downwards_core::{AbilitySet, Exit, Pickup, Point, Rect, Room, RoomError, Tile, TimedHazard};

/// Version of the seed-to-room mapping implemented by this crate.
///
/// Bump this when a generation change intentionally alters existing seeds.
pub const GENERATION_VERSION: u32 = 5;

/// Width of every generated room in tiles.
pub const ROOM_WIDTH: u16 = 32;
/// Height of every generated room in tiles.
pub const ROOM_HEIGHT: u16 = 18;
/// Width and height of a generated tile in pixels.
pub const TILE_SIZE: i32 = 10;

const FLOOR_ROW: u16 = ROOM_HEIGHT - 1;
const WALKING_ROW: u16 = FLOOR_ROW - 1;

/// Named traversal loadouts used to select compatible generation families.
///
/// This is a classification of the exact [`AbilitySet`] stored in generated
/// metadata, not a claim that difficulty is totally ordered by these variants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AbilityTier {
    #[default]
    Baseline,
    WallJump,
    Dash,
    WallJumpAndDash,
}

impl AbilityTier {
    /// The exact simulation loadout represented by this tier.
    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        match self {
            Self::Baseline => AbilitySet::NONE,
            Self::WallJump => AbilitySet::new(true, false),
            Self::Dash => AbilitySet::new(false, true),
            Self::WallJumpAndDash => AbilitySet::ALL,
        }
    }

    #[must_use]
    pub const fn from_abilities(abilities: AbilitySet) -> Self {
        match (abilities.wall_jump, abilities.dash) {
            (false, false) => Self::Baseline,
            (true, false) => Self::WallJump,
            (false, true) => Self::Dash,
            (true, true) => Self::WallJumpAndDash,
        }
    }
}

/// Broad topology used to build a room candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutFamily {
    /// Ground-level hazards interspersed with optional elevated lines.
    HazardRun,
    /// A sequence of rising platforms ending at a high exit.
    TerracedAscent,
    /// A tall enclosed route built around wall interaction.
    Chimney,
    /// Long hazard fields and aerial platforms built around dashing.
    DashGallery,
}

impl LayoutFamily {
    const fn slug(self) -> &'static str {
        match self {
            Self::HazardRun => "hazard-run",
            Self::TerracedAscent => "terraced-ascent",
            Self::Chimney => "chimney",
            Self::DashGallery => "dash-gallery",
        }
    }
}

/// Structural measurements recorded during generation.
///
/// These counts are useful inputs to later analysis, but are not a difficulty
/// score. Difficulty comes from solver observations and playtesting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenerationStats {
    pub solid_tiles: u16,
    pub boundary_solid_tiles: u16,
    pub interior_solid_tiles: u16,
    pub one_way_tiles: u16,
    pub hazard_tiles: u16,
    pub hazard_clusters: u16,
    pub timed_hazards: u16,
    pub pickups: u16,
    pub route_waypoints: u16,
}

/// Provenance and generation facts accompanying a candidate room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedMetadata {
    pub generation_version: u32,
    pub seed: u64,
    pub layout_family: LayoutFamily,
    pub ability_tier: AbilityTier,
    pub intended_abilities: AbilitySet,
    pub stats: GenerationStats,
}

/// A structurally valid room candidate and the information needed to validate
/// it with the game-playing AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedLevel {
    pub room: Room,
    pub metadata: GeneratedMetadata,
}

/// Generate a baseline room candidate from `seed`.
///
/// Use [`generate_for_abilities`] when validating unlock-specific content.
pub fn generate(seed: u64) -> Result<GeneratedLevel, RoomError> {
    generate_for_abilities(seed, AbilitySet::NONE)
}

/// Generate a room candidate intended for an exact simulation ability loadout.
///
/// The same seed and loadout always select the same family and geometry. A
/// successful return means that [`Room::new`] accepted the geometry; it does
/// not mean that the solver has accepted the room.
pub fn generate_for_abilities(
    seed: u64,
    intended_abilities: AbilitySet,
) -> Result<GeneratedLevel, RoomError> {
    let ability_tier = AbilityTier::from_abilities(intended_abilities);
    let mut rng = StableRng::new(seed);
    let layout_family = select_family(ability_tier, &mut rng);
    let mut draft = Draft::new();
    draft.add_boundary();

    let (spawn, exit) = match layout_family {
        LayoutFamily::HazardRun => build_hazard_run(&mut draft, &mut rng),
        LayoutFamily::TerracedAscent => build_terraced_ascent(&mut draft, &mut rng),
        LayoutFamily::Chimney => build_chimney(&mut draft, &mut rng),
        LayoutFamily::DashGallery => build_dash_gallery(&mut draft, &mut rng),
    };

    let stats = draft.stats();
    let id = format!(
        "generated-v{GENERATION_VERSION}-{seed:016x}-{}",
        layout_family.slug()
    );
    let name = format!(
        "Generated v{GENERATION_VERSION} {} {seed:016x}",
        layout_family.slug()
    );
    let Draft {
        tiles,
        timed_hazards,
        pickups,
        ..
    } = draft;
    let room = Room::new(
        id,
        name,
        ROOM_WIDTH,
        ROOM_HEIGHT,
        TILE_SIZE,
        tiles,
        spawn,
        vec![exit],
    )?
    .with_objects(timed_hazards, pickups)?;

    Ok(GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: GENERATION_VERSION,
            seed,
            layout_family,
            ability_tier,
            intended_abilities,
            stats,
        },
    })
}

fn select_family(tier: AbilityTier, rng: &mut StableRng) -> LayoutFamily {
    let alternate = rng.below(2) == 1;
    match (tier, alternate) {
        (AbilityTier::Baseline, false) => LayoutFamily::HazardRun,
        (AbilityTier::Baseline, true) => LayoutFamily::TerracedAscent,
        (AbilityTier::WallJump, false) => LayoutFamily::TerracedAscent,
        (AbilityTier::WallJump, true) => LayoutFamily::Chimney,
        (AbilityTier::Dash, false) => LayoutFamily::HazardRun,
        (AbilityTier::Dash, true) => LayoutFamily::DashGallery,
        (AbilityTier::WallJumpAndDash, false) => LayoutFamily::Chimney,
        (AbilityTier::WallJumpAndDash, true) => LayoutFamily::DashGallery,
    }
}

fn build_hazard_run(draft: &mut Draft, rng: &mut StableRng) -> (Point, Exit) {
    let first_start = 7 + rng.below(2);
    let second_start = 15 + rng.below(2);
    let third_start = 23 + rng.below(2);
    // Retain the old draws so the rest of a seed's HazardRun geometry does
    // not change along with this focused feasibility correction.
    let _legacy_length_rolls = [rng.below(2), rng.below(2), rng.below(2)];
    draft.hazard_run(first_start, 2);
    draft.hazard_run(second_start, 2);
    draft.hazard_run(third_start, 2);

    // Keep decorative solids above the measured baseline jump arc and leave a
    // full tile of horizontal air after the first two floor hazards. They can
    // still be reached through the optional one-way route.
    let low_y = 10 - rng.below(2);
    let high_y = 7 - rng.below(2);
    draft.solid_line(11, 15 + rng.below(2), low_y);
    draft.solid_line(19, 23 + rng.below(2), high_y);
    draft.route_waypoints = 5;

    // A readable optional staircase rises in 20-pixel steps. Earlier versions
    // started this branch 40 pixels above the floor, beyond a baseline jump,
    // so the visible cache could be impossible despite the exit being valid.
    draft.one_way_line(4, 8, 15);
    draft.one_way_line(6, 10, 13);
    draft.one_way_line(7, 11, 11);
    draft.pickup_above("optional-cache", 9, 11);
    draft.timed_hazard(Rect::new(103, 72, 6, 28), rng);

    (ground_spawn(2), ground_exit())
}

fn build_terraced_ascent(draft: &mut Draft, rng: &mut StableRng) -> (Point, Exit) {
    let shift = rng.below(2);
    let first_end = 12 + rng.below(2);
    let second_end = 20 + rng.below(2);
    let third_end = 27 + rng.below(2);

    draft.solid_line(6 + shift, first_end, 14);
    draft.solid_line(13 + shift, second_end, 11);
    draft.solid_line(20 + shift, third_end, 8);
    draft.solid_line(25, 30, 5);

    draft.hazard_run(9, 2 + rng.below(2));
    draft.hazard_run(16, 2 + rng.below(2));
    draft.hazard_run(23, 2 + rng.below(2));
    draft.route_waypoints = 6;

    // Interleave an optional left-hand branch with the mandatory terraces.
    // Its 10- and 20-pixel rises leave deliberate margin below the measured
    // baseline full-jump rise instead of relying on exact 30-pixel landings.
    draft.one_way_line(3, 6, 15);
    draft.one_way_line(9, 13, 12);
    draft.one_way_line(7, 11, 9);
    draft.pickup_above("optional-cache", 9, 9);
    draft.timed_hazard(Rect::new(95, 42, 6, 28), rng);

    let exit_x = 27 * TILE_SIZE;
    let exit = exit_at(Rect::new(exit_x, 25, 20, 25));
    (ground_spawn(2), exit)
}

fn build_chimney(draft: &mut Draft, rng: &mut StableRng) -> (Point, Exit) {
    let left_wall = 12 + rng.below(2);
    let right_wall = left_wall + 6;
    draft.solid_column(left_wall, 3, FLOOR_ROW);
    draft.solid_column(right_wall, 3, FLOOR_ROW);

    // Alternating ledges create both landing opportunities and routing choices
    // without closing the shaft.
    draft.solid_line(left_wall + 1, left_wall + 3, 12);
    draft.solid_line(right_wall - 2, right_wall, 9);
    draft.solid_line(left_wall + 1, left_wall + 3, 6);

    draft.hazard_run(4, 3 + rng.below(2));
    draft.hazard_run(24, 3 + rng.below(2));
    draft.route_waypoints = 7;

    // Pass-through ledges add optional recovery and collection lines without
    // removing either wall used by the central ascent.
    draft.one_way_line(left_wall + 1, left_wall + 4, 14);
    draft.one_way_line(right_wall - 3, right_wall, 10);
    draft.pickup_above("optional-cache", left_wall + 1, 6);
    draft.timed_hazard(
        Rect::new(i32::from(right_wall - 1) * TILE_SIZE + 2, 65, 6, 25),
        rng,
    );

    let shaft_left = (left_wall + 1) * TILE_SIZE as u16;
    let spawn = Point::new(i32::from(shaft_left + 16), ground_spawn_y());
    let exit = exit_at(Rect::new(i32::from(shaft_left + 10), 10, 20, 25));
    (spawn, exit)
}

fn build_dash_gallery(draft: &mut Draft, rng: &mut StableRng) -> (Point, Exit) {
    let first_start = 6 + rng.below(2);
    let first_length = 8 + rng.below(2);
    let second_start = 19 + rng.below(2);
    let second_length = 8 + rng.below(2);
    draft.hazard_run(first_start, first_length);
    draft.hazard_run(second_start, second_length.min(29 - second_start));

    // Preserve the original mandatory dash landing at row 13: lowering this
    // solid changed the collision timing of otherwise valid exit routes. Its
    // 40-pixel rise is a jump-into-up-dash step, with substantially more
    // vertical envelope than either baseline jump or cardinal dash alone.
    draft.solid_line(10, 14, 13);
    draft.solid_line(16, 19, 12);
    draft.solid_line(21, 25, 10);
    draft.route_waypoints = 6;

    // The high line asks for an extra upward commitment while leaving the
    // established dash route and its landing platforms unchanged.
    draft.one_way_line(15, 19, 9);
    draft.one_way_line(24, 28, 7);
    draft.pickup_above("optional-cache", 26, 7);
    draft.timed_hazard(Rect::new(285, 35, 6, 35), rng);

    (ground_spawn(2), ground_exit())
}

fn ground_spawn(tile_x: u16) -> Point {
    Point::new(i32::from(tile_x) * TILE_SIZE, ground_spawn_y())
}

const fn ground_spawn_y() -> i32 {
    FLOOR_ROW as i32 * TILE_SIZE - downwards_core::PLAYER_HEIGHT
}

fn ground_exit() -> Exit {
    exit_at(Rect::new(30 * TILE_SIZE, 140, TILE_SIZE, 30))
}

fn exit_at(bounds: Rect) -> Exit {
    Exit {
        id: "down".to_owned(),
        bounds,
        destination: None,
        destination_entrance: None,
    }
}

struct Draft {
    tiles: Vec<Tile>,
    timed_hazards: Vec<TimedHazard>,
    pickups: Vec<Pickup>,
    hazard_clusters: u16,
    route_waypoints: u16,
}

impl Draft {
    fn new() -> Self {
        Self {
            tiles: vec![Tile::Empty; usize::from(ROOM_WIDTH) * usize::from(ROOM_HEIGHT)],
            timed_hazards: Vec::new(),
            pickups: Vec::new(),
            hazard_clusters: 0,
            route_waypoints: 0,
        }
    }

    fn add_boundary(&mut self) {
        self.solid_line(0, ROOM_WIDTH, 0);
        self.solid_line(0, ROOM_WIDTH, FLOOR_ROW);
        self.solid_column(0, 1, FLOOR_ROW);
        self.solid_column(ROOM_WIDTH - 1, 1, FLOOR_ROW);
    }

    fn solid_line(&mut self, start_x: u16, end_x: u16, y: u16) {
        debug_assert!(start_x < end_x && end_x <= ROOM_WIDTH && y < ROOM_HEIGHT);
        for x in start_x..end_x {
            self.set(x, y, Tile::Solid);
        }
    }

    fn solid_column(&mut self, x: u16, start_y: u16, end_y: u16) {
        debug_assert!(start_y < end_y && end_y <= ROOM_HEIGHT && x < ROOM_WIDTH);
        for y in start_y..end_y {
            self.set(x, y, Tile::Solid);
        }
    }

    fn one_way_line(&mut self, start_x: u16, end_x: u16, y: u16) {
        debug_assert!(start_x < end_x && end_x <= ROOM_WIDTH && y < ROOM_HEIGHT);
        for x in start_x..end_x {
            self.set(x, y, Tile::OneWay);
        }
    }

    fn hazard_run(&mut self, start_x: u16, length: u16) {
        let end_x = start_x + length;
        debug_assert!(start_x > 0 && start_x < end_x && end_x < ROOM_WIDTH);
        for x in start_x..end_x {
            self.set(x, WALKING_ROW, Tile::HazardUp);
        }
        self.hazard_clusters += 1;
    }

    fn pickup_above(&mut self, id: &str, tile_x: u16, support_y: u16) {
        let bounds = Rect::new(
            i32::from(tile_x) * TILE_SIZE + 2,
            i32::from(support_y) * TILE_SIZE - 18,
            6,
            6,
        );
        self.pickups.push(
            Pickup::new(id, bounds)
                .expect("generator pickup bounds and identifiers must remain valid"),
        );
    }

    fn timed_hazard(&mut self, bounds: Rect, rng: &mut StableRng) {
        let period_ticks = 180 + u32::from(rng.below(3)) * 30;
        let active_ticks = 30 + u32::from(rng.below(3)) * 9;
        // Every generated pulse begins inactive, giving a player entering the
        // optional line time to read it before the first active window.
        let inactive_ticks = period_ticks - active_ticks;
        let phase_ticks = active_ticks + u32::from(rng.below(inactive_ticks as u16));
        self.timed_hazards.push(
            TimedHazard::new(bounds, period_ticks, active_ticks, phase_ticks)
                .expect("generator timed-hazard constants must remain valid"),
        );
    }

    fn set(&mut self, x: u16, y: u16, tile: Tile) {
        let index = usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x);
        self.tiles[index] = tile;
    }

    fn stats(&self) -> GenerationStats {
        let mut solid_tiles = 0;
        let mut boundary_solid_tiles = 0;
        let mut one_way_tiles = 0;
        let mut hazard_tiles = 0;
        for y in 0..ROOM_HEIGHT {
            for x in 0..ROOM_WIDTH {
                let tile = self.tiles[usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x)];
                if tile == Tile::Solid {
                    solid_tiles += 1;
                    if x == 0 || x == ROOM_WIDTH - 1 || y == 0 || y == FLOOR_ROW {
                        boundary_solid_tiles += 1;
                    }
                } else if tile.is_hazard() {
                    hazard_tiles += 1;
                } else if tile == Tile::OneWay {
                    one_way_tiles += 1;
                }
            }
        }
        GenerationStats {
            solid_tiles,
            boundary_solid_tiles,
            interior_solid_tiles: solid_tiles - boundary_solid_tiles,
            one_way_tiles,
            hazard_tiles,
            hazard_clusters: self.hazard_clusters,
            timed_hazards: self.timed_hazards.len() as u16,
            pickups: self.pickups.len() as u16,
            route_waypoints: self.route_waypoints,
        }
    }
}

/// SplitMix64 with a deliberately small, stable surface for content generation.
struct StableRng {
    state: u64,
}

impl StableRng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, upper_exclusive: u16) -> u16 {
        debug_assert!(upper_exclusive > 0);
        (self.next_u64() % u64::from(upper_exclusive)) as u16
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_core::{Action, PLAYER_HEIGHT, PLAYER_WIDTH, Simulation, SimulationEvent};

    use super::*;

    const ALL_TIERS: [AbilityTier; 4] = [
        AbilityTier::Baseline,
        AbilityTier::WallJump,
        AbilityTier::Dash,
        AbilityTier::WallJumpAndDash,
    ];

    #[test]
    fn same_seed_and_loadout_are_reproducible() {
        for tier in ALL_TIERS {
            let first = generate_for_abilities(0x5eed_cafe, tier.abilities()).unwrap();
            let second = generate_for_abilities(0x5eed_cafe, tier.abilities()).unwrap();
            assert_eq!(first, second);
        }
    }

    #[test]
    fn seeds_vary_geometry_and_cover_multiple_families() {
        let mut geometries = HashSet::new();
        let mut families = HashSet::new();
        for seed in 0..64 {
            let level = generate(seed).unwrap();
            geometries.insert(
                level
                    .room
                    .tiles()
                    .iter()
                    .map(|tile| *tile as u8)
                    .collect::<Vec<_>>(),
            );
            families.insert(level.metadata.layout_family);
        }
        assert!(geometries.len() >= 12, "only {} layouts", geometries.len());
        assert!(families.len() >= 2, "only {families:?}");
    }

    #[test]
    fn spawn_is_clear_grounded_and_safe_when_idle() {
        for seed in 0..128 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                let room = &level.room;
                let spawn = room.spawn();
                let spawn_bounds = Rect::new(spawn.x, spawn.y, PLAYER_WIDTH, PLAYER_HEIGHT);

                for y in 0..ROOM_HEIGHT {
                    for x in 0..ROOM_WIDTH {
                        if spawn_bounds.intersects(room.tile_bounds(x, y)) {
                            assert_eq!(room.tile(x, y), Some(Tile::Empty));
                        }
                    }
                }

                let below_y = ((spawn.y + PLAYER_HEIGHT) / TILE_SIZE) as u16;
                let first_x = (spawn.x / TILE_SIZE) as u16;
                let last_x = ((spawn.x + PLAYER_WIDTH - 1) / TILE_SIZE) as u16;
                for x in first_x..=last_x {
                    assert_eq!(room.tile(x, below_y), Some(Tile::Solid));
                }

                let mut simulation = Simulation::with_abilities(level.room, tier.abilities());
                // This spans at least two complete cycles of every generated
                // timed hazard, rather than only checking its initial phase.
                for _ in 0..480 {
                    let report = simulation.step(Action::default());
                    assert!(
                        !report
                            .events
                            .iter()
                            .any(|event| matches!(event, SimulationEvent::Died(_)))
                    );
                }
            }
        }
    }

    #[test]
    fn every_room_has_fixed_bounds_boundary_hazards_platforms_and_one_exit() {
        let pixel_bounds = Rect::new(
            0,
            0,
            i32::from(ROOM_WIDTH) * TILE_SIZE,
            i32::from(ROOM_HEIGHT) * TILE_SIZE,
        );
        for seed in 0..256 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                let room = &level.room;
                assert_eq!(room.width(), ROOM_WIDTH);
                assert_eq!(room.height(), ROOM_HEIGHT);
                assert_eq!(room.tile_size(), TILE_SIZE);
                assert_eq!(room.exits().len(), 1);
                assert!(pixel_bounds.contains(room.exits()[0].bounds));
                assert!(level.metadata.stats.hazard_tiles > 0);
                assert!(level.metadata.stats.interior_solid_tiles > 0);
                assert!(level.metadata.stats.one_way_tiles > 0);
                assert_eq!(level.metadata.stats.timed_hazards, 1);
                assert_eq!(level.metadata.stats.pickups, 1);

                for x in 0..ROOM_WIDTH {
                    assert_eq!(room.tile(x, 0), Some(Tile::Solid));
                    assert_eq!(room.tile(x, FLOOR_ROW), Some(Tile::Solid));
                }
                for y in 0..ROOM_HEIGHT {
                    assert_eq!(room.tile(0, y), Some(Tile::Solid));
                    assert_eq!(room.tile(ROOM_WIDTH - 1, y), Some(Tile::Solid));
                }
            }
        }
    }

    #[test]
    fn stats_match_constructed_tiles_and_loadout() {
        for seed in 0..128 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                let solid_count = level
                    .room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile == Tile::Solid)
                    .count() as u16;
                let hazard_count = level
                    .room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile.is_hazard())
                    .count() as u16;
                let one_way_count = level
                    .room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile == Tile::OneWay)
                    .count() as u16;
                assert_eq!(level.metadata.generation_version, GENERATION_VERSION);
                assert_eq!(level.metadata.seed, seed);
                assert_eq!(level.metadata.ability_tier, tier);
                assert_eq!(level.metadata.intended_abilities, tier.abilities());
                assert_eq!(level.metadata.stats.solid_tiles, solid_count);
                assert_eq!(level.metadata.stats.hazard_tiles, hazard_count);
                assert_eq!(level.metadata.stats.one_way_tiles, one_way_count);
                assert_eq!(
                    usize::from(level.metadata.stats.timed_hazards),
                    level.room.timed_hazards().len()
                );
                assert_eq!(
                    usize::from(level.metadata.stats.pickups),
                    level.room.pickups().len()
                );
                assert_eq!(
                    level.metadata.stats.solid_tiles,
                    level.metadata.stats.boundary_solid_tiles
                        + level.metadata.stats.interior_solid_tiles
                );
            }
        }
    }

    #[test]
    fn optional_objects_are_supported_and_clear_of_protected_geometry() {
        let floor_path = Rect::new(
            TILE_SIZE,
            ground_spawn_y(),
            i32::from(ROOM_WIDTH - 2) * TILE_SIZE,
            PLAYER_HEIGHT,
        );
        for seed in 0..256 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                let room = &level.room;
                let spawn = room.spawn();
                let spawn_bounds = Rect::new(spawn.x, spawn.y, PLAYER_WIDTH, PLAYER_HEIGHT);
                let exit_bounds = room.exits()[0].bounds;

                for hazard in room.timed_hazards() {
                    assert!(!hazard.is_active_at(0));
                    assert!(!hazard.bounds().intersects(spawn_bounds));
                    assert!(!hazard.bounds().intersects(exit_bounds));
                    assert!(!hazard.bounds().intersects(floor_path));
                    assert!(
                        room.pickups()
                            .iter()
                            .all(|pickup| { !hazard.bounds().intersects(pickup.bounds()) })
                    );
                    for y in 0..ROOM_HEIGHT {
                        for x in 0..ROOM_WIDTH {
                            if room.tile(x, y) != Some(Tile::Empty) {
                                assert!(!hazard.bounds().intersects(room.tile_bounds(x, y)));
                            }
                        }
                    }
                }

                for pickup in room.pickups() {
                    let bounds = pickup.bounds();
                    assert!(!bounds.intersects(spawn_bounds));
                    assert!(!bounds.intersects(exit_bounds));
                    assert!(!bounds.intersects(floor_path));
                    assert!(has_nearby_support(room, bounds));
                }
            }
        }
    }

    #[test]
    fn every_loadout_deterministically_selects_two_candidate_families() {
        for tier in ALL_TIERS {
            let mut families = HashSet::new();
            for seed in 0..64 {
                let first = generate_for_abilities(seed, tier.abilities()).unwrap();
                let second = generate_for_abilities(seed, tier.abilities()).unwrap();
                assert_eq!(first, second);
                families.insert(first.metadata.layout_family);
            }
            assert_eq!(families.len(), 2, "{tier:?} selected {families:?}");
        }
    }

    #[test]
    fn baseline_hazard_run_never_has_a_floor_cluster_wider_than_two_tiles() {
        let mut checked = 0;
        for seed in 0..512 {
            let level = generate(seed).unwrap();
            if level.metadata.layout_family != LayoutFamily::HazardRun {
                continue;
            }
            checked += 1;
            assert!(
                widest_hazard_run(level.room.tiles(), WALKING_ROW) <= 2,
                "baseline HazardRun seed {seed} exceeds its floor-gap promise"
            );
        }
        assert!(checked > 0, "sample did not select a HazardRun candidate");
    }

    #[test]
    fn baseline_hazard_run_keeps_a_clear_lower_arc_after_every_cluster() {
        const ARC_CEILING_ROW: u16 = 11;

        let mut checked = 0;
        for seed in 0..512 {
            let level = generate(seed).unwrap();
            if level.metadata.layout_family != LayoutFamily::HazardRun {
                continue;
            }
            checked += 1;
            for (start, end) in hazard_runs(level.room.tiles(), WALKING_ROW) {
                let clearance_start = start.saturating_sub(1);
                let clearance_end = (end + 1).min(ROOM_WIDTH - 2);
                for y in ARC_CEILING_ROW..WALKING_ROW {
                    for x in clearance_start..=clearance_end {
                        assert_ne!(
                            level.room.tile(x, y),
                            Some(Tile::Solid),
                            "baseline HazardRun seed {seed} blocks the lower arc at ({x}, {y})"
                        );
                    }
                }
                assert_ne!(
                    level.room.tile(end, WALKING_ROW),
                    Some(Tile::Solid),
                    "baseline HazardRun seed {seed} has a side wall after a floor cluster"
                );
            }
        }
        assert!(checked > 0, "sample did not select a HazardRun candidate");
    }

    #[test]
    fn baseline_optional_cache_routes_keep_a_full_tile_of_jump_margin() {
        const CONSERVATIVE_FULL_JUMP_RISE_PIXELS: i32 = 30;
        const REQUIRED_MARGIN_PIXELS: i32 = TILE_SIZE;

        let mut hazard_runs_checked = 0;
        let mut terraces_checked = 0;
        for seed in 0..512 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                let route = match level.metadata.layout_family {
                    LayoutFamily::HazardRun => {
                        hazard_runs_checked += 1;
                        vec![
                            Support::new(1, ROOM_WIDTH - 1, FLOOR_ROW, Tile::Solid),
                            Support::new(4, 8, 15, Tile::OneWay),
                            Support::new(6, 10, 13, Tile::OneWay),
                            Support::new(7, 11, 11, Tile::OneWay),
                        ]
                    }
                    LayoutFamily::TerracedAscent => {
                        terraces_checked += 1;
                        vec![
                            Support::new(1, ROOM_WIDTH - 1, FLOOR_ROW, Tile::Solid),
                            Support::new(3, 6, 15, Tile::OneWay),
                            // Common subsets of the slightly randomized terraces.
                            Support::new(7, 12, 14, Tile::Solid),
                            Support::new(9, 13, 12, Tile::OneWay),
                            Support::new(14, 20, 11, Tile::Solid),
                            Support::new(7, 11, 9, Tile::OneWay),
                        ]
                    }
                    LayoutFamily::Chimney | LayoutFamily::DashGallery => continue,
                };

                assert_support_route(&level.room, &route);
                for pair in route.windows(2) {
                    let rise = pair[0].rise_to(pair[1]);
                    assert!(
                        CONSERVATIVE_FULL_JUMP_RISE_PIXELS - rise >= REQUIRED_MARGIN_PIXELS,
                        "{:?} seed {seed} has only {}px of vertical margin at rows {} -> {}",
                        level.metadata.layout_family,
                        CONSERVATIVE_FULL_JUMP_RISE_PIXELS - rise,
                        pair[0].row,
                        pair[1].row
                    );
                }
                assert_pickup_above_support(&level.room, *route.last().unwrap());
            }
        }
        assert!(hazard_runs_checked > 0);
        assert!(terraces_checked > 0);
    }

    #[test]
    fn baseline_seed_one_optional_cache_uses_comfortable_rises() {
        let level = generate(1).unwrap();
        assert_eq!(level.metadata.generation_version, 5);
        assert_eq!(level.metadata.layout_family, LayoutFamily::TerracedAscent);

        let route = [
            Support::new(1, ROOM_WIDTH - 1, FLOOR_ROW, Tile::Solid),
            Support::new(3, 6, 15, Tile::OneWay),
            Support::new(7, 12, 14, Tile::Solid),
            Support::new(9, 13, 12, Tile::OneWay),
            Support::new(14, 20, 11, Tile::Solid),
            Support::new(7, 11, 9, Tile::OneWay),
        ];
        assert_support_route(&level.room, &route);
        assert!(route.windows(2).all(|pair| pair[0].rise_to(pair[1]) <= 20));
        assert_pickup_above_support(&level.room, route[route.len() - 1]);
    }

    #[test]
    fn dash_gallery_cache_route_has_comfortable_jump_dash_height_margin() {
        const CONSERVATIVE_FULL_JUMP_RISE_PIXELS: i32 = 30;
        const CARDINAL_DASH_RANGE_PIXELS: i32 = 40;
        const REQUIRED_MARGIN_PIXELS: i32 = 2 * TILE_SIZE;

        let mut checked = 0;
        for seed in 0..512 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                if level.metadata.layout_family != LayoutFamily::DashGallery {
                    continue;
                }
                checked += 1;
                assert!(level.metadata.intended_abilities.dash);
                let route = [
                    Support::new(1, ROOM_WIDTH - 1, FLOOR_ROW, Tile::Solid),
                    Support::new(10, 14, 13, Tile::Solid),
                    Support::new(16, 19, 12, Tile::Solid),
                    Support::new(21, 25, 10, Tile::Solid),
                    Support::new(24, 28, 7, Tile::OneWay),
                ];
                assert_support_route(&level.room, &route);
                assert!(
                    route.windows(2).any(|pair| {
                        pair[0].rise_to(pair[1]) > CONSERVATIVE_FULL_JUMP_RISE_PIXELS
                    }),
                    "dash-gallery seed {seed} has no support rise that asks for dash"
                );
                for pair in route.windows(2) {
                    let rise = pair[0].rise_to(pair[1]);
                    let combined_range =
                        CONSERVATIVE_FULL_JUMP_RISE_PIXELS + CARDINAL_DASH_RANGE_PIXELS;
                    assert!(
                        combined_range - rise >= REQUIRED_MARGIN_PIXELS,
                        "dash-gallery seed {seed} has only {}px of jump-dash height margin",
                        combined_range - rise
                    );
                }
                assert_pickup_above_support(&level.room, route[route.len() - 1]);
            }
        }
        assert!(checked > 0, "sample did not select a DashGallery candidate");
    }

    #[test]
    fn chimney_cache_route_retains_continuous_wall_jump_geometry() {
        let mut checked = 0;
        for seed in 0..512 {
            for tier in ALL_TIERS {
                let level = generate_for_abilities(seed, tier.abilities()).unwrap();
                if level.metadata.layout_family != LayoutFamily::Chimney {
                    continue;
                }
                checked += 1;
                assert!(level.metadata.intended_abilities.wall_jump);

                let wall_columns = (1..ROOM_WIDTH - 1)
                    .filter(|&x| (3..FLOOR_ROW).all(|y| level.room.tile(x, y) == Some(Tile::Solid)))
                    .collect::<Vec<_>>();
                assert_eq!(wall_columns.len(), 2, "chimney seed {seed}");
                let left_wall = wall_columns[0];
                let right_wall = wall_columns[1];
                assert_eq!(right_wall - left_wall, 6, "chimney seed {seed}");
                assert!(
                    i32::from(right_wall - left_wall - 1) * TILE_SIZE
                        >= PLAYER_WIDTH + 4 * TILE_SIZE,
                    "chimney seed {seed} has a cramped wall-jump shaft"
                );

                let target = Support::new(left_wall + 1, left_wall + 3, 6, Tile::Solid);
                assert_support_route(&level.room, &[target]);
                assert_pickup_above_support(&level.room, target);
                let spawn = level.room.spawn();
                assert!(spawn.x >= i32::from(left_wall + 1) * TILE_SIZE);
                assert!(spawn.x + PLAYER_WIDTH <= i32::from(right_wall) * TILE_SIZE);
            }
        }
        assert!(checked > 0, "sample did not select a Chimney candidate");
    }

    #[test]
    fn many_seeds_construct_through_room_validation() {
        for seed in 0..4_096 {
            for tier in ALL_TIERS {
                generate_for_abilities(seed, tier.abilities()).unwrap();
            }
        }
    }

    fn has_nearby_support(room: &Room, pickup: Rect) -> bool {
        (0..ROOM_HEIGHT).any(|y| {
            (0..ROOM_WIDTH).any(|x| {
                let tile = room.tile(x, y);
                if !matches!(tile, Some(Tile::Solid | Tile::OneWay)) {
                    return false;
                }
                let support = room.tile_bounds(x, y);
                let horizontal_overlap = pickup.x < support.right() && pickup.right() > support.x;
                let distance_below = support.y - pickup.bottom();
                horizontal_overlap && (0..=20).contains(&distance_below)
            })
        })
    }

    #[derive(Clone, Copy)]
    struct Support {
        start_x: u16,
        end_x: u16,
        row: u16,
        tile: Tile,
    }

    impl Support {
        const fn new(start_x: u16, end_x: u16, row: u16, tile: Tile) -> Self {
            Self {
                start_x,
                end_x,
                row,
                tile,
            }
        }

        fn rise_to(self, next: Self) -> i32 {
            assert!(next.row < self.row);
            i32::from(self.row - next.row) * TILE_SIZE
        }

        fn bounds(self) -> Rect {
            Rect::new(
                i32::from(self.start_x) * TILE_SIZE,
                i32::from(self.row) * TILE_SIZE,
                i32::from(self.end_x - self.start_x) * TILE_SIZE,
                TILE_SIZE,
            )
        }
    }

    fn assert_support_route(room: &Room, route: &[Support]) {
        for support in route {
            for x in support.start_x..support.end_x {
                assert_eq!(
                    room.tile(x, support.row),
                    Some(support.tile),
                    "missing {:?} route support at ({x}, {})",
                    support.tile,
                    support.row
                );
            }
        }
        for pair in route.windows(2) {
            let from = pair[0].bounds();
            let to = pair[1].bounds();
            let horizontal_gap = if from.right() < to.x {
                to.x - from.right()
            } else if to.right() < from.x {
                from.x - to.right()
            } else {
                0
            };
            assert!(
                horizontal_gap <= 3 * TILE_SIZE,
                "route supports at rows {} -> {} have a {horizontal_gap}px horizontal gap",
                pair[0].row,
                pair[1].row
            );
        }
    }

    fn assert_pickup_above_support(room: &Room, support: Support) {
        let [pickup] = room.pickups() else {
            panic!("generated room must have exactly one optional cache")
        };
        let pickup = pickup.bounds();
        let support = support.bounds();
        assert!(pickup.x >= support.x && pickup.right() <= support.right());
        assert_eq!(support.y - pickup.bottom(), PLAYER_HEIGHT);
    }

    fn widest_hazard_run(tiles: &[Tile], row: u16) -> u16 {
        let start = usize::from(row) * usize::from(ROOM_WIDTH);
        let end = start + usize::from(ROOM_WIDTH);
        let mut widest = 0;
        let mut current = 0;
        for tile in &tiles[start..end] {
            if *tile == Tile::HazardUp {
                current += 1;
                widest = widest.max(current);
            } else {
                current = 0;
            }
        }
        widest
    }

    fn hazard_runs(tiles: &[Tile], row: u16) -> Vec<(u16, u16)> {
        let start = usize::from(row) * usize::from(ROOM_WIDTH);
        let mut runs = Vec::new();
        let mut run_start = None;
        for x in 0..ROOM_WIDTH {
            let hazard = tiles[start + usize::from(x)] == Tile::HazardUp;
            match (run_start, hazard) {
                (None, true) => run_start = Some(x),
                (Some(first), false) => {
                    runs.push((first, x));
                    run_start = None;
                }
                _ => {}
            }
        }
        if let Some(first) = run_start {
            runs.push((first, ROOM_WIDTH));
        }
        runs
    }
}
