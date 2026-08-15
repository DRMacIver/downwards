//! Small deterministic palette used by the first multi-room dungeon vertical slice.
//!
//! This deliberately stops short of assembling a dungeon graph. It generates a handful of
//! structurally distinct room shells with named boundary sockets; higher-level content assigns
//! exact destinations and run-state objects. The split keeps topology/persistence out of the
//! single-room generator while still exercising the same native [`Room`] and [`Door`] contracts.

use std::{error::Error, fmt};

use downwards_core::{
    BoundarySide, Door, DoorError, Exit, PLAYER_HEIGHT, Point, Rect, Room, RoomError, Tile,
};

pub const DUNGEON_PALETTE_GENERATION_VERSION: u32 = 6;

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DungeonPaletteCourse {
    HollowLanding,
    MossWalk,
    SplitRoot,
    RootCellar,
    BrokenAqueduct,
    OldLift,
    LanternGallery,
    WatchPost,
    Sluice,
    ClimberVault,
    WallAntechamber,
    BroadChimney,
    BellSwitchback,
    BellNiche,
    TempoHall,
    SplitSpire,
    RafterShrine,
    LandingChain,
    NeedleTurn,
    WallGate,
    Threshold,
    Crossroads,
    WallGallery,
    BootsVault,
    Underpass,
    DashChasm,
    GaleLanding,
    LowPassage,
    CurrentFork,
    CoinDuct,
    PulseGallery,
    StormSplit,
    StormCache,
    RelayChasm,
    BrakeTower,
    DashSeal,
    CoinLoft,
    NeedleRoom,
    Treasury,
    Gatehouse,
    CrownSanctum,
}

impl DungeonPaletteCourse {
    pub const ALL: [Self; 41] = [
        Self::HollowLanding,
        Self::MossWalk,
        Self::SplitRoot,
        Self::RootCellar,
        Self::BrokenAqueduct,
        Self::OldLift,
        Self::LanternGallery,
        Self::WatchPost,
        Self::Sluice,
        Self::ClimberVault,
        Self::WallAntechamber,
        Self::BroadChimney,
        Self::BellSwitchback,
        Self::BellNiche,
        Self::TempoHall,
        Self::SplitSpire,
        Self::RafterShrine,
        Self::LandingChain,
        Self::NeedleTurn,
        Self::WallGate,
        Self::Threshold,
        Self::Crossroads,
        Self::WallGallery,
        Self::BootsVault,
        Self::Underpass,
        Self::DashChasm,
        Self::GaleLanding,
        Self::LowPassage,
        Self::CurrentFork,
        Self::CoinDuct,
        Self::PulseGallery,
        Self::StormSplit,
        Self::StormCache,
        Self::RelayChasm,
        Self::BrakeTower,
        Self::DashSeal,
        Self::CoinLoft,
        Self::NeedleRoom,
        Self::Treasury,
        Self::Gatehouse,
        Self::CrownSanctum,
    ];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::HollowLanding => "hollow-landing",
            Self::MossWalk => "moss-walk",
            Self::SplitRoot => "split-root",
            Self::RootCellar => "root-cellar",
            Self::BrokenAqueduct => "broken-aqueduct",
            Self::OldLift => "old-lift",
            Self::LanternGallery => "lantern-gallery",
            Self::WatchPost => "watch-post",
            Self::Sluice => "sluice",
            Self::ClimberVault => "climber-vault",
            Self::WallAntechamber => "wall-antechamber",
            Self::BroadChimney => "broad-chimney",
            Self::BellSwitchback => "bell-switchback",
            Self::BellNiche => "bell-niche",
            Self::TempoHall => "tempo-hall",
            Self::SplitSpire => "split-spire",
            Self::RafterShrine => "rafter-shrine",
            Self::LandingChain => "landing-chain",
            Self::NeedleTurn => "needle-turn",
            Self::WallGate => "wall-gate",
            Self::Threshold => "threshold",
            Self::Crossroads => "crossroads",
            Self::WallGallery => "wall-gallery",
            Self::BootsVault => "boots-vault",
            Self::Underpass => "underpass",
            Self::DashChasm => "dash-chasm",
            Self::GaleLanding => "gale-landing",
            Self::LowPassage => "low-passage",
            Self::CurrentFork => "current-fork",
            Self::CoinDuct => "coin-duct",
            Self::PulseGallery => "pulse-gallery",
            Self::StormSplit => "storm-split",
            Self::StormCache => "storm-cache",
            Self::RelayChasm => "relay-chasm",
            Self::BrakeTower => "brake-tower",
            Self::DashSeal => "dash-seal",
            Self::CoinLoft => "coin-loft",
            Self::NeedleRoom => "needle-room",
            Self::Treasury => "treasury",
            Self::Gatehouse => "gatehouse",
            Self::CrownSanctum => "crown-sanctum",
        }
    }

    const fn door_sides(self) -> &'static [(&'static str, BoundarySide)] {
        match self {
            Self::HollowLanding => &[("east", BoundarySide::Right)],
            Self::MossWalk
            | Self::BrokenAqueduct
            | Self::OldLift
            | Self::Sluice
            | Self::ClimberVault
            | Self::WallAntechamber
            | Self::BroadChimney
            | Self::TempoHall
            | Self::LandingChain
            | Self::NeedleTurn
            | Self::WallGate
            | Self::GaleLanding
            | Self::LowPassage
            | Self::PulseGallery
            | Self::RelayChasm
            | Self::BrakeTower
            | Self::DashSeal
            | Self::Threshold => &[("west", BoundarySide::Left), ("east", BoundarySide::Right)],
            Self::SplitRoot => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::RootCellar => &[("ceiling", BoundarySide::Ceiling)],
            Self::LanternGallery => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::WatchPost => &[("floor", BoundarySide::Floor)],
            Self::BellSwitchback => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::BellNiche => &[("ceiling", BoundarySide::Ceiling)],
            Self::SplitSpire => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::RafterShrine => &[("floor", BoundarySide::Floor)],
            Self::Crossroads => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
                ("floor", BoundarySide::Floor),
            ],
            Self::WallGallery => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
                ("floor", BoundarySide::Floor),
            ],
            Self::BootsVault => &[
                ("floor", BoundarySide::Floor),
                ("east", BoundarySide::Right),
            ],
            Self::Underpass => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::DashChasm => &[("west", BoundarySide::Left), ("east", BoundarySide::Right)],
            Self::CurrentFork => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::CoinDuct => &[("ceiling", BoundarySide::Ceiling)],
            Self::StormSplit => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::StormCache => &[("floor", BoundarySide::Floor)],
            Self::CoinLoft => &[("ceiling", BoundarySide::Ceiling)],
            Self::NeedleRoom => &[("floor", BoundarySide::Floor)],
            Self::Treasury => &[("west", BoundarySide::Left)],
            Self::Gatehouse => &[("west", BoundarySide::Left), ("east", BoundarySide::Right)],
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
        debug_assert!(
            draft.one_way_surfaces_have_player_headroom(),
            "{:?} contains a one-way surface without standing headroom",
            self.course
        );
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
        // A floor entrance lands on the drop-through sill authored by `course` below. This
        // leaves a full-height standing place beside the door instead of spawning the player
        // into a four-tile aperture with nothing underneath them.
        BoundarySide::Floor => (Rect::new(140, 172, 40, 8), Point::new(150, 148)),
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
        if key
            .course
            .door_sides()
            .iter()
            .any(|(_, side)| *side == BoundarySide::Floor)
        {
            // Drop through this sill to leave by the floor door; arrivals stand safely on top.
            self.horizontal(16, 14, 18, Tile::OneWay);
        }
        match key.course {
            DungeonPaletteCourse::HollowLanding => {
                self.horizontal(14, 5, 12, Tile::OneWay);
                self.horizontal(12, 15, 21, Tile::OneWay);
                self.horizontal(14, 24, 29, Tile::OneWay);
            }
            DungeonPaletteCourse::MossWalk => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(12, 12, 18, Tile::OneWay);
                self.horizontal(10, 20, 26, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::SplitRoot => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(11, 12, 20, Tile::OneWay);
                self.horizontal(14, 23, 29, Tile::OneWay);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
            }
            DungeonPaletteCourse::RootCellar => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(12, 12, 19, Tile::OneWay);
                self.horizontal(10, 21, 28, Tile::OneWay);
                self.horizontal(7, 13, 19, Tile::OneWay);
                self.horizontal(4, 14, 18, Tile::OneWay);
            }
            DungeonPaletteCourse::BrokenAqueduct => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(14, 19, 24, Tile::OneWay);
                self.horizontal(11, 25, 30, Tile::OneWay);
                self.horizontal(16, 15, 18, Tile::HazardUp);
                self.horizontal(17, 15, 18, Tile::Solid);
            }
            DungeonPaletteCourse::OldLift => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(9, 18, 23, Tile::OneWay);
                self.horizontal(6, 11, 16, Tile::OneWay);
                self.horizontal(3, 19, 25, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::LanternGallery => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(6, 14, 19, Tile::OneWay);
                self.horizontal(3, 14, 19, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::WatchPost => {
                self.horizontal(14, 4, 11, Tile::OneWay);
                self.horizontal(11, 13, 19, Tile::OneWay);
                self.horizontal(8, 21, 28, Tile::OneWay);
                self.horizontal(5, 13, 19, Tile::OneWay);
                self.horizontal(3, 22, 28, Tile::OneWay);
            }
            DungeonPaletteCourse::Sluice => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 17, Tile::OneWay);
                self.horizontal(14, 19, 25, Tile::OneWay);
                self.horizontal(11, 24, 30, Tile::OneWay);
                self.horizontal(16, 10, 12, Tile::HazardUp);
                self.horizontal(17, 10, 12, Tile::Solid);
            }
            DungeonPaletteCourse::ClimberVault => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(9, 18, 23, Tile::OneWay);
                self.horizontal(6, 24, 29, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                self.vertical(17, 10, 15, Tile::Solid);
            }
            DungeonPaletteCourse::WallAntechamber => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(9, 18, 23, Tile::OneWay);
                self.horizontal(6, 24, 29, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                self.vertical(16, 7, 15, Tile::Solid);
            }
            DungeonPaletteCourse::BroadChimney => {
                // The west entry opens directly into the bottom of the shaft; the opposing
                // walls begin above it so the player must climb rather than detour around it.
                self.vertical(10, 3, 12, Tile::Solid);
                self.vertical(16, 3, 17, Tile::Solid);
                self.horizontal(16, 10, 17, Tile::Solid);
                self.horizontal(5, 17, 25, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::BellSwitchback => {
                self.vertical(10, 4, 12, Tile::Solid);
                self.vertical(16, 3, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::Solid);
                self.horizontal(7, 17, 25, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::BellNiche => {
                self.vertical(11, 2, 16, Tile::Solid);
                self.vertical(17, 4, 17, Tile::Solid);
                self.horizontal(16, 11, 18, Tile::Solid);
                self.horizontal(8, 18, 25, Tile::OneWay);
                self.horizontal(4, 13, 16, Tile::OneWay);
            }
            DungeonPaletteCourse::TempoHall => {
                self.vertical(10, 1, 17, Tile::Solid);
                self.vertical(16, 4, 17, Tile::Solid);
                self.vertical(11, 1, 4, Tile::Solid);
                self.vertical(11, 4, 7, Tile::HazardRight);
                self.vertical(11, 7, 10, Tile::Solid);
                self.vertical(11, 10, 13, Tile::HazardRight);
                self.vertical(11, 13, 16, Tile::Solid);
                self.vertical(15, 4, 7, Tile::Solid);
                self.vertical(15, 7, 10, Tile::HazardLeft);
                self.vertical(15, 10, 13, Tile::Solid);
                self.vertical(15, 13, 16, Tile::HazardLeft);
                self.horizontal(4, 15, 25, Tile::Solid);
                self.horizontal(14, 23, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(10, row, Tile::Empty);
                    self.set(11, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::SplitSpire => {
                self.vertical(12, 2, 16, Tile::Solid);
                self.vertical(18, 5, 17, Tile::Solid);
                self.horizontal(16, 12, 19, Tile::Solid);
                self.horizontal(9, 19, 26, Tile::OneWay);
                self.horizontal(6, 13, 17, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(12, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::RafterShrine => {
                self.vertical(10, 4, 16, Tile::Solid);
                self.vertical(16, 2, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::Solid);
                self.horizontal(10, 17, 24, Tile::OneWay);
                self.horizontal(6, 11, 15, Tile::OneWay);
                self.horizontal(3, 17, 24, Tile::OneWay);
            }
            DungeonPaletteCourse::LandingChain => {
                self.vertical(9, 6, 16, Tile::Solid);
                self.vertical(15, 3, 13, Tile::Solid);
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 16, 20, Tile::OneWay);
                self.horizontal(8, 21, 24, Tile::OneWay);
                self.horizontal(5, 25, 28, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(9, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::NeedleTurn => {
                self.vertical(10, 8, 17, Tile::Solid);
                self.vertical(16, 7, 17, Tile::Solid);
                self.vertical(11, 8, 10, Tile::HazardRight);
                self.vertical(11, 12, 14, Tile::HazardRight);
                self.vertical(15, 10, 12, Tile::HazardLeft);
                self.vertical(15, 14, 16, Tile::HazardLeft);
                self.horizontal(7, 16, 25, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                for row in 14..16 {
                    self.set(10, row, Tile::Empty);
                    self.set(11, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::WallGate => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.vertical(10, 4, 12, Tile::Solid);
                self.vertical(16, 4, 17, Tile::Solid);
                self.horizontal(5, 17, 24, Tile::OneWay);
                self.horizontal(9, 23, 28, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::Threshold => {
                self.horizontal(14, 5, 11, Tile::OneWay);
                self.horizontal(11, 14, 20, Tile::OneWay);
                self.horizontal(14, 23, 28, Tile::OneWay);
            }
            DungeonPaletteCourse::Crossroads => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 20, Tile::OneWay);
                self.horizontal(14, 23, 29, Tile::OneWay);
                self.vertical(13, 1, 9, Tile::Solid);
                self.vertical(18, 1, 9, Tile::Solid);
                // A visible safe lip around the downward branch.
                self.horizontal(16, 11, 14, Tile::Solid);
                self.horizontal(16, 18, 21, Tile::Solid);
            }
            DungeonPaletteCourse::WallGallery => {
                self.horizontal(14, 4, 9, Tile::OneWay);
                self.horizontal(12, 10, 15, Tile::OneWay);
                self.horizontal(9, 17, 22, Tile::OneWay);
                self.horizontal(6, 10, 15, Tile::OneWay);
                self.horizontal(3, 18, 25, Tile::OneWay);
                self.vertical(7, 7, 16, Tile::Solid);
                self.vertical(24, 4, 14, Tile::Solid);
            }
            DungeonPaletteCourse::BootsVault => {
                // This is the five-beat alternating contact pattern calibrated by the gallery's
                // Even Tempo room. The floor door places the player inside the bottom of the
                // shaft; hazard-faced bands prevent riding one wall and force rapid direction
                // changes before the reward shelf.
                self.vertical(13, 0, 16, Tile::Solid);
                self.set(14, 0, Tile::HazardRight);
                self.vertical(14, 1, 4, Tile::Solid);
                self.vertical(14, 4, 7, Tile::HazardRight);
                self.vertical(14, 7, 10, Tile::Solid);
                self.vertical(14, 10, 13, Tile::HazardRight);
                self.vertical(14, 13, 16, Tile::Solid);
                self.vertical(19, 4, 16, Tile::Solid);
                self.vertical(18, 4, 7, Tile::Solid);
                self.vertical(18, 7, 10, Tile::HazardLeft);
                self.vertical(18, 10, 13, Tile::Solid);
                self.vertical(18, 13, 16, Tile::HazardLeft);
                self.horizontal(4, 18, 26, Tile::Solid);
                self.set(14, 16, Tile::Empty);
            }
            DungeonPaletteCourse::Underpass => {
                self.horizontal(14, 4, 10, Tile::OneWay);
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
            DungeonPaletteCourse::GaleLanding => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(11, 13, 18, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
                self.horizontal(16, 10, 23, Tile::HazardUp);
                self.horizontal(17, 10, 23, Tile::Solid);
            }
            DungeonPaletteCourse::LowPassage => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 21, 27, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                // A standing body cannot enter this ten-pixel passage. Horizontal Dash uses the
                // low posture and is the only advertised traversal through the partition.
                self.horizontal(15, 7, 20, Tile::Solid);
                self.vertical(20, 1, 16, Tile::Solid);
            }
            DungeonPaletteCourse::CurrentFork => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
                self.horizontal(7, 20, 27, Tile::OneWay);
            }
            DungeonPaletteCourse::CoinDuct => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(9, 19, 24, Tile::OneWay);
                self.horizontal(6, 11, 16, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(16, 17, 21, Tile::HazardUp);
                self.horizontal(17, 17, 21, Tile::Solid);
            }
            DungeonPaletteCourse::PulseGallery => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(13, 14, 18, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
                self.horizontal(16, 9, 14, Tile::HazardUp);
                self.horizontal(17, 9, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::HazardUp);
                self.horizontal(17, 18, 23, Tile::Solid);
                self.horizontal(8, 11, 21, Tile::HazardDown);
            }
            DungeonPaletteCourse::StormSplit => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 17, Tile::OneWay);
                self.horizontal(9, 20, 27, Tile::OneWay);
                self.horizontal(6, 13, 19, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 17, 21, Tile::HazardUp);
                self.horizontal(17, 17, 21, Tile::Solid);
            }
            DungeonPaletteCourse::StormCache => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 11, 16, Tile::OneWay);
                self.horizontal(8, 20, 26, Tile::OneWay);
                self.horizontal(5, 12, 18, Tile::OneWay);
                self.horizontal(3, 21, 27, Tile::OneWay);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
            }
            DungeonPaletteCourse::RelayChasm => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(14, 16, 20, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 10, 16, Tile::HazardUp);
                self.horizontal(17, 10, 16, Tile::Solid);
                self.horizontal(16, 20, 26, Tile::HazardUp);
                self.horizontal(17, 20, 26, Tile::Solid);
            }
            DungeonPaletteCourse::BrakeTower => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 12, 16, Tile::OneWay);
                self.horizontal(9, 20, 24, Tile::OneWay);
                self.horizontal(6, 12, 16, Tile::OneWay);
                self.horizontal(3, 21, 27, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.vertical(18, 10, 17, Tile::Solid);
            }
            DungeonPaletteCourse::DashSeal => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 22, 28, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                self.horizontal(15, 7, 21, Tile::Solid);
                self.vertical(21, 1, 16, Tile::Solid);
                self.horizontal(8, 11, 18, Tile::HazardDown);
            }
            DungeonPaletteCourse::CoinLoft => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(12, 12, 19, Tile::OneWay);
                self.horizontal(10, 21, 29, Tile::OneWay);
                self.horizontal(8, 17, 21, Tile::OneWay);
                self.horizontal(6, 14, 17, Tile::OneWay);
                self.horizontal(3, 14, 17, Tile::OneWay);
                self.horizontal(4, 3, 10, Tile::Solid);
                // The branch is entered from above. Its drop-through landing is also the base of
                // a short return chimney, so collecting its coins cannot strand the player.
                self.vertical(13, 1, 6, Tile::Solid);
                self.vertical(17, 1, 6, Tile::Solid);
            }
            DungeonPaletteCourse::NeedleRoom => {
                self.vertical(8, 4, 15, Tile::HazardRight);
                self.vertical(23, 3, 13, Tile::HazardLeft);
                self.horizontal(14, 9, 14, Tile::OneWay);
                self.horizontal(12, 17, 23, Tile::OneWay);
                self.horizontal(9, 10, 16, Tile::OneWay);
                self.horizontal(6, 17, 23, Tile::OneWay);
                self.horizontal(2, 17, 23, Tile::Solid);
            }
            DungeonPaletteCourse::Treasury => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 28, Tile::OneWay);
                self.horizontal(5, 13, 20, Tile::OneWay);
                self.horizontal(16, 10, 13, Tile::HazardUp);
                self.horizontal(16, 20, 23, Tile::HazardUp);
            }
            DungeonPaletteCourse::Gatehouse => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 28, Tile::OneWay);
                self.horizontal(14, 23, 29, Tile::OneWay);
                // The partition seals ceiling to within one tile of the floor. The standing
                // player cannot enter the approach under row 15; a horizontal Dash adopts the
                // low posture and carries through the ten-pixel passage.
                self.horizontal(15, 8, 20, Tile::Solid);
                self.vertical(20, 1, 16, Tile::Solid);
            }
            DungeonPaletteCourse::CrownSanctum => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 29, Tile::OneWay);
                self.horizontal(5, 13, 20, Tile::OneWay);
                self.horizontal(3, 23, 29, Tile::OneWay);
            }
        }
    }

    fn one_way_surfaces_have_player_headroom(&self) -> bool {
        let clearance_rows = u16::try_from((PLAYER_HEIGHT + TILE_SIZE - 1) / TILE_SIZE)
            .expect("player clearance row count fits u16");
        (0..HEIGHT).all(|row| {
            (0..WIDTH).all(|column| {
                if self.tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)]
                    != Tile::OneWay
                {
                    return true;
                }
                row >= clearance_rows
                    && (1..=clearance_rows).all(|offset| {
                        self.tiles
                            [usize::from(row - offset) * usize::from(WIDTH) + usize::from(column)]
                            == Tile::Empty
                    })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_course_materializes_with_exact_connected_sockets() {
        for course in DungeonPaletteCourse::ALL {
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

    #[test]
    fn every_authored_bridge_has_full_player_headroom() {
        for course in DungeonPaletteCourse::ALL {
            let candidate = DungeonPaletteKey::new(0xD06E_0A11, course).generate();
            let mut tiles = candidate.tiles.clone();
            let draft = PaletteDraft { tiles: &mut tiles };
            assert!(
                draft.one_way_surfaces_have_player_headroom(),
                "{course:?} contains a bridge with less than {PLAYER_HEIGHT}px headroom"
            );
        }
    }
}
