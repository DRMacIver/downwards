//! Human-calibrated WallJump-only room generation.
//!
//! This deliberately small grammar is independent of the corpus generators.
//! Its structural bounds come from the authored calibration gallery, while
//! authoritative solver and human playtests remain responsible for accepting
//! individual candidates. A generated room is never evidence of difficulty by
//! itself.

use downwards_core::{AbilitySet, Exit, Point, Rect, Room, Tile};

/// Version of the exact seed-to-room mapping in this module.
pub const CALIBRATED_WALL_JUMP_GENERATION_VERSION: u32 = 2;

/// The locked loadout for every candidate in this family.
pub const CALIBRATED_WALL_JUMP_ABILITIES: AbilitySet = AbilitySet::new(true, false);

/// Stable single-exit target used by every candidate.
pub const CALIBRATED_WALL_JUMP_TARGET: &str = "finish";

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;
const PLAYER_WIDTH: i32 = 8;

/// The isolated movement burden selected by a generated key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalibratedWallJumpCourse {
    /// Three readable transfers with a staging shelf.
    ShortTurns,
    /// A regular alternating ascent without intermediate recovery.
    EvenTempo,
    /// A longer ascent with one recovery shelf.
    RecoveryAscent,
    /// A readable climb followed by a short landing chain.
    Causeway,
    /// The same climb and chain with a generous, visible jump-cut bank.
    LowBridge,
}

impl CalibratedWallJumpCourse {
    /// Stable human-readable slug used in room identity.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::ShortTurns => "short-turns",
            Self::EvenTempo => "even-tempo",
            Self::RecoveryAscent => "recovery-ascent",
            Self::Causeway => "causeway",
            Self::LowBridge => "low-bridge",
        }
    }
}

/// Exact deterministic key for a calibrated candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CalibratedWallJumpKey {
    /// Seed within the current exact generation version.
    pub seed: u64,
}

impl CalibratedWallJumpKey {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Generate this exact candidate without search or fallback.
    #[must_use]
    pub fn generate(self) -> CalibratedWallJumpCandidate {
        generate(self)
    }
}

/// Structural facts retained with a generated candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CalibratedWallJumpParameters {
    pub course: CalibratedWallJumpCourse,
    /// Left backing-wall column before optional horizontal reflection.
    pub shaft_column: u16,
    /// Whether the complete authored course is horizontally reflected.
    pub reflected: bool,
    /// Intended alternating solid contact bands in the climb.
    pub contact_bands: u8,
    /// Narrowest intended contact band, measured in tiles.
    pub minimum_contact_tiles: u8,
    /// Number of intermediate one-way recovery surfaces.
    pub recovery_surfaces: u8,
}

/// A deterministic room and the structural parameters that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibratedWallJumpCandidate {
    pub key: CalibratedWallJumpKey,
    pub room: Room,
    pub parameters: CalibratedWallJumpParameters,
}

#[derive(Clone)]
struct CourseBuilder {
    tiles: Vec<Tile>,
    left_back: u16,
}

impl CourseBuilder {
    fn new(left_back: u16) -> Self {
        let mut result = Self {
            tiles: vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)],
            left_back,
        };
        for row in 0..HEIGHT {
            result.set(left_back, row, Tile::Solid);
            result.set(left_back + 1, row, Tile::HazardRight);
            result.set(left_back + 5, row, Tile::HazardLeft);
            result.set(left_back + 6, row, Tile::Solid);
        }
        result
    }

    fn set(&mut self, column: u16, row: u16, tile: Tile) {
        self.tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)] = tile;
    }

    fn fill(&mut self, start: u16, end_inclusive: u16, row: u16, tile: Tile) {
        for column in start..=end_inclusive {
            self.set(column, row, tile);
        }
    }

    fn safe_band(&mut self, left: bool, first_row: u16, rows: u16) {
        let column = if left {
            self.left_back + 1
        } else {
            self.left_back + 5
        };
        for row in first_row..first_row + rows {
            self.set(column, row, Tile::Solid);
        }
    }

    fn floor(&mut self, row: u16) {
        self.fill(self.left_back, self.left_back + 6, row, Tile::Solid);
    }

    fn shelf(&mut self, first_column: u16, width: u16, row: u16) {
        self.fill(first_column, first_column + width - 1, row, Tile::OneWay);
    }
}

fn generate(key: CalibratedWallJumpKey) -> CalibratedWallJumpCandidate {
    let course = match key.seed % 5 {
        0 => CalibratedWallJumpCourse::ShortTurns,
        1 => CalibratedWallJumpCourse::EvenTempo,
        2 => CalibratedWallJumpCourse::RecoveryAscent,
        3 => CalibratedWallJumpCourse::Causeway,
        _ => CalibratedWallJumpCourse::LowBridge,
    };
    let reflection_requested = (key.seed / 5) % 2 == 1;
    // These two finish/recovery shapes are intentionally directional. The
    // reflected jump-cut bank produced a visibly thrashy retained route, while
    // the reflected recovery shelf admitted a bounded baseline bypass. V1
    // varies their shaft position instead.
    let shift_instead_of_reflect = matches!(
        course,
        CalibratedWallJumpCourse::RecoveryAscent | CalibratedWallJumpCourse::LowBridge
    );
    let reflected = reflection_requested && !shift_instead_of_reflect;
    let offset = if shift_instead_of_reflect {
        (key.seed / 5) % 3
    } else {
        (key.seed / 10) % 3
    };
    let base_column = if course == CalibratedWallJumpCourse::LowBridge {
        8
    } else {
        9
    };
    let shaft_column = base_column + u16::try_from(offset).expect("bounded shaft offset");
    let (mut builder, spawn, finish, contact_bands, minimum_contact_tiles, recovery_surfaces) =
        match course {
            CalibratedWallJumpCourse::ShortTurns => short_turns(shaft_column),
            CalibratedWallJumpCourse::EvenTempo => even_tempo(shaft_column, false),
            CalibratedWallJumpCourse::RecoveryAscent => even_tempo(shaft_column, true),
            CalibratedWallJumpCourse::Causeway => traverse_finish(shaft_column, false),
            CalibratedWallJumpCourse::LowBridge => traverse_finish(shaft_column, true),
        };

    if reflected {
        builder.tiles = reflect_tiles(&builder.tiles);
    }
    let spawn = if reflected {
        Point::new(
            i32::from(WIDTH) * TILE_SIZE - PLAYER_WIDTH - spawn.x,
            spawn.y,
        )
    } else {
        spawn
    };
    let finish = if reflected {
        Rect::new(
            i32::from(WIDTH) * TILE_SIZE - finish.x - finish.width,
            finish.y,
            finish.width,
            finish.height,
        )
    } else {
        finish
    };

    let id = format!(
        "generated.calibrated-wall-jump.v{CALIBRATED_WALL_JUMP_GENERATION_VERSION}.{}.{:016x}",
        course.slug(),
        key.seed
    );
    let title = format!("Generated {} {:02}", course.slug(), key.seed);
    let room = Room::new(
        id,
        title,
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        builder.tiles,
        spawn,
        vec![Exit {
            id: CALIBRATED_WALL_JUMP_TARGET.to_owned(),
            bounds: finish,
            destination: None,
            destination_entrance: None,
        }],
    )
    .expect("calibrated generator must preserve core room invariants");

    CalibratedWallJumpCandidate {
        key,
        room,
        parameters: CalibratedWallJumpParameters {
            course,
            shaft_column,
            reflected,
            contact_bands,
            minimum_contact_tiles,
            recovery_surfaces,
        },
    }
}

fn short_turns(left: u16) -> (CourseBuilder, Point, Rect, u8, u8, u8) {
    let mut builder = CourseBuilder::new(left);
    builder.floor(12);
    builder.safe_band(false, 9, 3);
    builder.safe_band(true, 6, 3);
    builder.safe_band(false, 3, 3);
    builder.shelf(left + 2, 3, 12);
    (
        builder,
        Point::new(i32::from(left + 2) * TILE_SIZE, 108),
        Rect::new(i32::from(left + 2) * TILE_SIZE, 0, 30, 30),
        3,
        3,
        1,
    )
}

fn even_tempo(left: u16, recovery: bool) -> (CourseBuilder, Point, Rect, u8, u8, u8) {
    let mut builder = CourseBuilder::new(left);
    builder.floor(16);
    for (left_side, first_row) in [(true, 13), (false, 10), (true, 7), (false, 4), (true, 1)] {
        builder.safe_band(left_side, first_row, 3);
    }
    if recovery {
        builder.shelf(left + 2, 3, 9);
    }
    (
        builder,
        Point::new(i32::from(left + 2) * TILE_SIZE, 148),
        Rect::new(i32::from(left + 2) * TILE_SIZE, 0, 30, 18),
        5,
        3,
        u8::from(recovery),
    )
}

fn traverse_finish(left: u16, low_bridge: bool) -> (CourseBuilder, Point, Rect, u8, u8, u8) {
    let mut builder = CourseBuilder::new(left);
    builder.floor(16);
    for (left_side, first_row) in [(true, 13), (false, 10), (true, 7), (false, 4), (true, 1)] {
        builder.safe_band(left_side, first_row, 3);
    }

    // Open above the final right contact. The fifth (left) contact then sends
    // the player through this aperture without erasing the authored rhythm.
    for row in 0..4 {
        builder.set(left + 5, row, Tile::Empty);
        builder.set(left + 6, row, Tile::Empty);
    }
    let start = left + 8;
    builder.shelf(start, 3, 7);
    builder.shelf(start + 7, 2, 6);
    builder.shelf(start + 13, 32 - (start + 13), 5);
    let hazard_start = left + 7;
    // Paired banks meet at their solid bases and expose lethal tips on both
    // traversable sides. Reversing these rows would point both faces into the
    // inaccessible seam between the tiles.
    builder.fill(hazard_start, 31, 8, Tile::HazardUp);
    builder.fill(hazard_start, 31, 9, Tile::HazardDown);

    if low_bridge {
        // Six tiles of vertical clearance accept an ordinary short human tap
        // while a full-height jump remains meaningfully different.
        builder.fill(start + 2, start + 5, 1, Tile::HazardUp);
        builder.fill(start + 2, start + 5, 2, Tile::HazardDown);
    }

    (
        builder,
        Point::new(i32::from(left + 2) * TILE_SIZE, 148),
        Rect::new(304, 20, 16, 30),
        5,
        3,
        3,
    )
}

fn reflect_tiles(tiles: &[Tile]) -> Vec<Tile> {
    let mut reflected = vec![Tile::Empty; tiles.len()];
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let tile = tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)];
            let tile = match tile {
                Tile::HazardLeft => Tile::HazardRight,
                Tile::HazardRight => Tile::HazardLeft,
                other => other,
            };
            let reflected_column = WIDTH - 1 - column;
            reflected[usize::from(row) * usize::from(WIDTH) + usize::from(reflected_column)] = tile;
        }
    }
    reflected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_keys_are_deterministic_and_structurally_bounded() {
        let mut geometries = Vec::new();
        for seed in 0..15 {
            let key = CalibratedWallJumpKey::new(seed);
            let first = key.generate();
            let second = key.generate();
            assert_eq!(first, second);
            assert_eq!(first.key, key);
            assert!((3..=5).contains(&first.parameters.contact_bands));
            assert!(first.parameters.minimum_contact_tiles >= 3);
            assert!(first.parameters.recovery_surfaces <= 3);
            assert_eq!(first.room.exits().len(), 1);
            assert_eq!(first.room.exits()[0].id, CALIBRATED_WALL_JUMP_TARGET);
            let geometry = (
                first.room.tiles().to_vec(),
                first.room.spawn(),
                first.room.exits()[0].bounds,
            );
            assert!(!geometries.contains(&geometry));
            geometries.push(geometry);
        }
        assert_eq!(geometries.len(), 15);
    }

    #[test]
    fn reflection_preserves_directional_faces_and_changes_geometry() {
        let plain = CalibratedWallJumpKey::new(1).generate();
        let reflected = CalibratedWallJumpKey::new(6).generate();
        assert_eq!(plain.parameters.course, reflected.parameters.course);
        assert!(!plain.parameters.reflected);
        assert!(reflected.parameters.reflected);
        assert_ne!(plain.room, reflected.room);

        let width = usize::from(WIDTH);
        for row in 0..usize::from(HEIGHT) {
            for column in 0..width {
                let source = plain.room.tiles()[row * width + column];
                let expected = match source {
                    Tile::HazardLeft => Tile::HazardRight,
                    Tile::HazardRight => Tile::HazardLeft,
                    other => other,
                };
                assert_eq!(
                    reflected.room.tiles()[row * width + (width - 1 - column)],
                    expected
                );
            }
        }
    }

    #[test]
    fn traverse_spike_pairs_face_outward_instead_of_into_their_seam() {
        for seed in [3, 4, 8, 9, 13, 14] {
            let candidate = CalibratedWallJumpKey::new(seed).generate();
            let tiles = candidate.room.tiles();
            let width = usize::from(WIDTH);
            let mut outward_pairs = 0;
            let mut inward_pairs = 0;
            for row in 0..usize::from(HEIGHT - 1) {
                for column in 0..width {
                    let upper = tiles[row * width + column];
                    let lower = tiles[(row + 1) * width + column];
                    outward_pairs +=
                        usize::from(upper == Tile::HazardUp && lower == Tile::HazardDown);
                    inward_pairs +=
                        usize::from(upper == Tile::HazardDown && lower == Tile::HazardUp);
                }
            }
            assert!(outward_pairs > 0, "seed {seed} has no paired spike bank");
            assert_eq!(
                inward_pairs, 0,
                "seed {seed} points paired spikes into an inaccessible seam"
            );
        }
    }
}
