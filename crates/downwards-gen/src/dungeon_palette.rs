//! Small deterministic palette used by the first multi-room dungeon vertical slice.
//!
//! This deliberately stops short of assembling a dungeon graph. It generates a handful of
//! structurally distinct room shells with named boundary sockets; higher-level content assigns
//! exact destinations and run-state objects. The split keeps topology/persistence out of the
//! single-room generator while still exercising the same native [`Room`] and [`Door`] contracts.

use std::{error::Error, fmt};

use downwards_core::{BoundarySide, Door, DoorError, Exit, Point, Rect, Room, RoomError, Tile};

pub const DUNGEON_PALETTE_GENERATION_VERSION: u32 = 1;

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DungeonPaletteCourse {
    Threshold,
    Crossroads,
    WallGallery,
    BootsVault,
    Underpass,
    DashChasm,
    CrownSanctum,
}

impl DungeonPaletteCourse {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Threshold => "threshold",
            Self::Crossroads => "crossroads",
            Self::WallGallery => "wall-gallery",
            Self::BootsVault => "boots-vault",
            Self::Underpass => "underpass",
            Self::DashChasm => "dash-chasm",
            Self::CrownSanctum => "crown-sanctum",
        }
    }

    const fn door_sides(self) -> &'static [(&'static str, BoundarySide)] {
        match self {
            Self::Threshold => &[("east", BoundarySide::Right)],
            Self::Crossroads => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::WallGallery => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::BootsVault => &[
                ("ceiling", BoundarySide::Ceiling),
                ("east", BoundarySide::Right),
            ],
            Self::Underpass => &[
                ("west", BoundarySide::Left),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::DashChasm => &[("west", BoundarySide::Left), ("east", BoundarySide::Right)],
            Self::CrownSanctum => &[("west", BoundarySide::Left)],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DungeonPaletteKey {
    pub seed: u64,
    pub course: DungeonPaletteCourse,
}

impl DungeonPaletteKey {
    #[must_use]
    pub const fn new(seed: u64, course: DungeonPaletteCourse) -> Self {
        Self { seed, course }
    }

    #[must_use]
    pub fn generate(self) -> DungeonPaletteCandidate {
        let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
        let mut draft = PaletteDraft { tiles: &mut tiles };
        draft.boundary();
        draft.open_doors(self.course);
        draft.course(self);
        DungeonPaletteCandidate { key: self, tiles }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DungeonPaletteConnection {
    pub door_id: String,
    pub destination_room: String,
    pub destination_door: String,
}

impl DungeonPaletteConnection {
    #[must_use]
    pub fn new(
        door_id: impl Into<String>,
        destination_room: impl Into<String>,
        destination_door: impl Into<String>,
    ) -> Self {
        Self {
            door_id: door_id.into(),
            destination_room: destination_room.into(),
            destination_door: destination_door.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DungeonPaletteCandidate {
    pub key: DungeonPaletteKey,
    tiles: Vec<Tile>,
}

impl DungeonPaletteCandidate {
    #[must_use]
    pub const fn door_count(&self) -> usize {
        self.key.course.door_sides().len()
    }

    pub fn materialize(
        &self,
        room_id: impl Into<String>,
        name: impl Into<String>,
        connections: &[DungeonPaletteConnection],
        exits: Vec<Exit>,
    ) -> Result<Room, DungeonPaletteError> {
        let room_id = room_id.into();
        for connection in connections {
            if !self
                .key
                .course
                .door_sides()
                .iter()
                .any(|(id, _)| *id == connection.door_id)
            {
                return Err(DungeonPaletteError::UnknownDoor(connection.door_id.clone()));
            }
        }
        for &(door_id, _) in self.key.course.door_sides() {
            let count = connections
                .iter()
                .filter(|connection| connection.door_id == door_id)
                .count();
            if count == 0 {
                return Err(DungeonPaletteError::MissingDoor(door_id.to_owned()));
            }
            if count > 1 {
                return Err(DungeonPaletteError::DuplicateDoor(door_id.to_owned()));
            }
        }

        let doors = self
            .key
            .course
            .door_sides()
            .iter()
            .map(|&(door_id, side)| {
                let connection = connections
                    .iter()
                    .find(|connection| connection.door_id == door_id)
                    .expect("connection cardinality checked above");
                let (trigger_bounds, arrival) = door_geometry(side);
                Door {
                    id: door_id.to_owned(),
                    side,
                    trigger_bounds,
                    arrival,
                    destination_room: Some(connection.destination_room.clone()),
                    destination_door: Some(connection.destination_door.clone()),
                }
            })
            .collect();

        Room::new(
            room_id,
            name,
            WIDTH,
            HEIGHT,
            TILE_SIZE,
            self.tiles.clone(),
            Point::new(20, 148),
            exits,
        )
        .map_err(DungeonPaletteError::Room)?
        .with_doors(doors)
        .map_err(DungeonPaletteError::Door)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DungeonPaletteError {
    MissingDoor(String),
    UnknownDoor(String),
    DuplicateDoor(String),
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for DungeonPaletteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDoor(id) => write!(formatter, "missing destination for door {id:?}"),
            Self::UnknownDoor(id) => write!(formatter, "unknown palette door {id:?}"),
            Self::DuplicateDoor(id) => write!(formatter, "duplicate destination for door {id:?}"),
            Self::Room(error) => write!(formatter, "palette room is invalid: {error}"),
            Self::Door(error) => write!(formatter, "palette door is invalid: {error}"),
        }
    }
}

impl Error for DungeonPaletteError {}

fn door_geometry(side: BoundarySide) -> (Rect, Point) {
    match side {
        BoundarySide::Left => (Rect::new(0, 130, 8, 40), Point::new(12, 148)),
        BoundarySide::Right => (Rect::new(312, 130, 8, 40), Point::new(300, 148)),
        BoundarySide::Ceiling => (Rect::new(140, 0, 40, 8), Point::new(150, 12)),
        BoundarySide::Floor => (Rect::new(140, 172, 40, 8), Point::new(150, 154)),
    }
}

struct PaletteDraft<'a> {
    tiles: &'a mut [Tile],
}

impl PaletteDraft<'_> {
    fn set(&mut self, column: u16, row: u16, tile: Tile) {
        self.tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)] = tile;
    }

    fn horizontal(&mut self, row: u16, start: u16, end: u16, tile: Tile) {
        for column in start..end {
            self.set(column, row, tile);
        }
    }

    fn vertical(&mut self, column: u16, start: u16, end: u16, tile: Tile) {
        for row in start..end {
            self.set(column, row, tile);
        }
    }

    fn boundary(&mut self) {
        self.horizontal(0, 0, WIDTH, Tile::Solid);
        self.horizontal(HEIGHT - 1, 0, WIDTH, Tile::Solid);
        self.vertical(0, 0, HEIGHT, Tile::Solid);
        self.vertical(WIDTH - 1, 0, HEIGHT, Tile::Solid);
    }

    fn open_doors(&mut self, course: DungeonPaletteCourse) {
        for &(_, side) in course.door_sides() {
            match side {
                BoundarySide::Left => self.vertical(0, 13, 17, Tile::Empty),
                BoundarySide::Right => self.vertical(WIDTH - 1, 13, 17, Tile::Empty),
                BoundarySide::Ceiling => self.horizontal(0, 14, 18, Tile::Empty),
                BoundarySide::Floor => self.horizontal(HEIGHT - 1, 14, 18, Tile::Empty),
            }
        }
    }

    fn course(&mut self, key: DungeonPaletteKey) {
        match key.course {
            DungeonPaletteCourse::Threshold => {
                self.horizontal(14, 5, 11, Tile::OneWay);
                self.horizontal(11, 14, 20, Tile::OneWay);
                self.horizontal(14, 23, 28, Tile::OneWay);
            }
            DungeonPaletteCourse::Crossroads => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 20, Tile::OneWay);
                self.horizontal(14, 23, 29, Tile::OneWay);
                // A visible safe lip around the downward branch.
                self.horizontal(16, 11, 14, Tile::Solid);
                self.horizontal(16, 18, 21, Tile::Solid);
            }
            DungeonPaletteCourse::WallGallery => {
                self.horizontal(15, 4, 9, Tile::OneWay);
                self.horizontal(12, 10, 15, Tile::OneWay);
                self.horizontal(9, 17, 22, Tile::OneWay);
                self.horizontal(6, 10, 15, Tile::OneWay);
                self.horizontal(3, 18, 25, Tile::OneWay);
                self.vertical(7, 7, 16, Tile::Solid);
                self.vertical(24, 4, 14, Tile::Solid);
            }
            DungeonPaletteCourse::BootsVault => {
                self.horizontal(15, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 18, Tile::OneWay);
                self.horizontal(9, 21, 28, Tile::OneWay);
                self.horizontal(6, 12, 19, Tile::OneWay);
                self.horizontal(3, 4, 11, Tile::OneWay);
            }
            DungeonPaletteCourse::Underpass => {
                self.horizontal(15, 4, 10, Tile::OneWay);
                self.horizontal(12, 12, 18, Tile::OneWay);
                self.horizontal(9, 20, 27, Tile::OneWay);
                self.horizontal(6, 12, 18, Tile::OneWay);
                self.horizontal(3, 4, 11, Tile::OneWay);
            }
            DungeonPaletteCourse::DashChasm => {
                self.horizontal(16, 12, 20, Tile::HazardUp);
                self.horizontal(17, 12, 20, Tile::Solid);
                // Keep the gap visually framed without offering wall-jump contacts.
                let ceiling_start = 9 + u16::try_from(key.seed & 1).expect("bit fits u16");
                self.horizontal(8, ceiling_start, 23, Tile::HazardDown);
            }
            DungeonPaletteCourse::CrownSanctum => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 29, Tile::OneWay);
                self.horizontal(5, 13, 20, Tile::OneWay);
                self.horizontal(2, 23, 29, Tile::OneWay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_course_materializes_with_exact_connected_sockets() {
        let courses = [
            DungeonPaletteCourse::Threshold,
            DungeonPaletteCourse::Crossroads,
            DungeonPaletteCourse::WallGallery,
            DungeonPaletteCourse::BootsVault,
            DungeonPaletteCourse::Underpass,
            DungeonPaletteCourse::DashChasm,
            DungeonPaletteCourse::CrownSanctum,
        ];
        for course in courses {
            let candidate = DungeonPaletteKey::new(7, course).generate();
            let connections = course
                .door_sides()
                .iter()
                .map(|&(id, side)| {
                    DungeonPaletteConnection::new(
                        id,
                        format!("destination-{id}"),
                        match side.opposite() {
                            BoundarySide::Left => "west",
                            BoundarySide::Right => "east",
                            BoundarySide::Ceiling => "ceiling",
                            BoundarySide::Floor => "floor",
                        },
                    )
                })
                .collect::<Vec<_>>();
            let room = candidate
                .materialize(
                    format!("test.{}", course.slug()),
                    course.slug(),
                    &connections,
                    vec![],
                )
                .unwrap();
            assert_eq!(room.doors().len(), candidate.door_count());
            assert!(
                room.doors()
                    .iter()
                    .all(|door| door.destination_room.is_some())
            );
        }
    }

    #[test]
    fn dash_chasm_has_an_outward_facing_hazard_bank() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::DashChasm).generate();
        assert!(
            (12..20).all(|column| {
                candidate.tiles[16 * usize::from(WIDTH) + column] == Tile::HazardUp
            })
        );
        assert!(
            (12..20)
                .all(|column| { candidate.tiles[17 * usize::from(WIDTH) + column] == Tile::Solid })
        );
    }
}
