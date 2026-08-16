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

pub const DUNGEON_PALETTE_GENERATION_VERSION: u32 = 31;

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
    AlloyThreshold,
    Windshaft,
    SplitFurnace,
    EmberVault,
    GearGallery,
    CrosswindChimney,
    FoundryFork,
    CoolingDuct,
    HammerHall,
    LiftShaft,
    SparkNiche,
    RivetRun,
    BlastGallery,
    PressureFork,
    AshCache,
    VentSpire,
    PistonPass,
    CrucibleClimb,
    CinderBridge,
    FoundrySeal,
    GlassThreshold,
    PrismRun,
    SplitKiln,
    ShardVault,
    GlassGallery,
    RefractionShaft,
    MirrorFork,
    MirrorDuct,
    TemperHall,
    FurnaceLift,
    LensNiche,
    SliverRun,
    HotGlass,
    CulletFork,
    CulletCache,
    AnnealingSpire,
    RazorPass,
    LatticeClimb,
    CrystalBridge,
    GlassSeal,
    StarThreshold,
    CometRun,
    OrbitFork,
    MoonVault,
    ConstellationHall,
    ZenithShaft,
    EclipseFork,
    ShadowDuct,
    Observatory,
    GravityLift,
    NovaNiche,
    MeteorRun,
    VacuumGallery,
    TidalFork,
    LunarCache,
    AuroraSpire,
    VoidPass,
    StarwellClimb,
    Skybridge,
    AstralSeal,
    CoinLoft,
    NeedleRoom,
    Treasury,
    Gatehouse,
    CrownSanctum,
}

impl DungeonPaletteCourse {
    pub const ALL: [Self; 101] = [
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
        Self::AlloyThreshold,
        Self::Windshaft,
        Self::SplitFurnace,
        Self::EmberVault,
        Self::GearGallery,
        Self::CrosswindChimney,
        Self::FoundryFork,
        Self::CoolingDuct,
        Self::HammerHall,
        Self::LiftShaft,
        Self::SparkNiche,
        Self::RivetRun,
        Self::BlastGallery,
        Self::PressureFork,
        Self::AshCache,
        Self::VentSpire,
        Self::PistonPass,
        Self::CrucibleClimb,
        Self::CinderBridge,
        Self::FoundrySeal,
        Self::GlassThreshold,
        Self::PrismRun,
        Self::SplitKiln,
        Self::ShardVault,
        Self::GlassGallery,
        Self::RefractionShaft,
        Self::MirrorFork,
        Self::MirrorDuct,
        Self::TemperHall,
        Self::FurnaceLift,
        Self::LensNiche,
        Self::SliverRun,
        Self::HotGlass,
        Self::CulletFork,
        Self::CulletCache,
        Self::AnnealingSpire,
        Self::RazorPass,
        Self::LatticeClimb,
        Self::CrystalBridge,
        Self::GlassSeal,
        Self::StarThreshold,
        Self::CometRun,
        Self::OrbitFork,
        Self::MoonVault,
        Self::ConstellationHall,
        Self::ZenithShaft,
        Self::EclipseFork,
        Self::ShadowDuct,
        Self::Observatory,
        Self::GravityLift,
        Self::NovaNiche,
        Self::MeteorRun,
        Self::VacuumGallery,
        Self::TidalFork,
        Self::LunarCache,
        Self::AuroraSpire,
        Self::VoidPass,
        Self::StarwellClimb,
        Self::Skybridge,
        Self::AstralSeal,
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
            Self::AlloyThreshold => "alloy-threshold",
            Self::Windshaft => "windshaft",
            Self::SplitFurnace => "split-furnace",
            Self::EmberVault => "ember-vault",
            Self::GearGallery => "gear-gallery",
            Self::CrosswindChimney => "crosswind-chimney",
            Self::FoundryFork => "foundry-fork",
            Self::CoolingDuct => "cooling-duct",
            Self::HammerHall => "hammer-hall",
            Self::LiftShaft => "lift-shaft",
            Self::SparkNiche => "spark-niche",
            Self::RivetRun => "rivet-run",
            Self::BlastGallery => "blast-gallery",
            Self::PressureFork => "pressure-fork",
            Self::AshCache => "ash-cache",
            Self::VentSpire => "vent-spire",
            Self::PistonPass => "piston-pass",
            Self::CrucibleClimb => "crucible-climb",
            Self::CinderBridge => "cinder-bridge",
            Self::FoundrySeal => "foundry-seal",
            Self::GlassThreshold => "glass-threshold",
            Self::PrismRun => "prism-run",
            Self::SplitKiln => "split-kiln",
            Self::ShardVault => "shard-vault",
            Self::GlassGallery => "glass-gallery",
            Self::RefractionShaft => "refraction-shaft",
            Self::MirrorFork => "mirror-fork",
            Self::MirrorDuct => "mirror-duct",
            Self::TemperHall => "temper-hall",
            Self::FurnaceLift => "furnace-lift",
            Self::LensNiche => "lens-niche",
            Self::SliverRun => "sliver-run",
            Self::HotGlass => "hot-glass",
            Self::CulletFork => "cullet-fork",
            Self::CulletCache => "cullet-cache",
            Self::AnnealingSpire => "annealing-spire",
            Self::RazorPass => "razor-pass",
            Self::LatticeClimb => "lattice-climb",
            Self::CrystalBridge => "crystal-bridge",
            Self::GlassSeal => "glass-seal",
            Self::StarThreshold => "star-threshold",
            Self::CometRun => "comet-run",
            Self::OrbitFork => "orbit-fork",
            Self::MoonVault => "moon-vault",
            Self::ConstellationHall => "constellation-hall",
            Self::ZenithShaft => "zenith-shaft",
            Self::EclipseFork => "eclipse-fork",
            Self::ShadowDuct => "shadow-duct",
            Self::Observatory => "observatory",
            Self::GravityLift => "gravity-lift",
            Self::NovaNiche => "nova-niche",
            Self::MeteorRun => "meteor-run",
            Self::VacuumGallery => "vacuum-gallery",
            Self::TidalFork => "tidal-fork",
            Self::LunarCache => "lunar-cache",
            Self::AuroraSpire => "aurora-spire",
            Self::VoidPass => "void-pass",
            Self::StarwellClimb => "starwell-climb",
            Self::Skybridge => "skybridge",
            Self::AstralSeal => "astral-seal",
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
            | Self::AlloyThreshold
            | Self::Windshaft
            | Self::GearGallery
            | Self::CrosswindChimney
            | Self::HammerHall
            | Self::RivetRun
            | Self::BlastGallery
            | Self::VentSpire
            | Self::PistonPass
            | Self::CrucibleClimb
            | Self::CinderBridge
            | Self::FoundrySeal
            | Self::GlassThreshold
            | Self::PrismRun
            | Self::GlassGallery
            | Self::RefractionShaft
            | Self::TemperHall
            | Self::SliverRun
            | Self::HotGlass
            | Self::AnnealingSpire
            | Self::RazorPass
            | Self::LatticeClimb
            | Self::CrystalBridge
            | Self::GlassSeal
            | Self::StarThreshold
            | Self::CometRun
            | Self::ConstellationHall
            | Self::ZenithShaft
            | Self::Observatory
            | Self::MeteorRun
            | Self::VacuumGallery
            | Self::AuroraSpire
            | Self::VoidPass
            | Self::StarwellClimb
            | Self::Skybridge
            | Self::AstralSeal
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
            Self::SplitFurnace
            | Self::PressureFork
            | Self::SplitKiln
            | Self::CulletFork
            | Self::OrbitFork
            | Self::TidalFork => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("floor", BoundarySide::Floor),
            ],
            Self::EmberVault
            | Self::AshCache
            | Self::ShardVault
            | Self::CulletCache
            | Self::MoonVault
            | Self::LunarCache => &[("ceiling", BoundarySide::Ceiling)],
            Self::FoundryFork
            | Self::LiftShaft
            | Self::MirrorFork
            | Self::FurnaceLift
            | Self::EclipseFork
            | Self::GravityLift => &[
                ("west", BoundarySide::Left),
                ("east", BoundarySide::Right),
                ("ceiling", BoundarySide::Ceiling),
            ],
            Self::CoolingDuct
            | Self::SparkNiche
            | Self::MirrorDuct
            | Self::LensNiche
            | Self::ShadowDuct
            | Self::NovaNiche => &[("floor", BoundarySide::Floor)],
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
                self.vertical(11, 12, 14, Tile::Solid);
                self.vertical(15, 10, 12, Tile::HazardLeft);
                self.vertical(15, 14, 16, Tile::Solid);
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
                self.horizontal(5, 17, 30, Tile::OneWay);
                self.horizontal(9, 17, 30, Tile::OneWay);
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
                // A five-beat alternating contact pattern using two-tile safe windows. The floor
                // door places the player inside the bottom of the shaft; hazard-faced bands make
                // riding one wall lethal and require the same rapid direction changes as the
                // calibrated Two-Tile Turn / Even Tempo exercises before the reward shelf.
                self.vertical(13, 0, 16, Tile::Solid);
                self.set(14, 0, Tile::HazardRight);
                self.vertical(14, 1, 5, Tile::HazardRight);
                self.vertical(14, 5, 7, Tile::Solid);
                self.vertical(14, 7, 11, Tile::HazardRight);
                self.vertical(14, 11, 13, Tile::Solid);
                self.vertical(14, 13, 16, Tile::HazardRight);
                self.vertical(19, 4, 16, Tile::Solid);
                self.vertical(18, 4, 5, Tile::HazardLeft);
                self.vertical(18, 5, 7, Tile::Solid);
                self.vertical(18, 7, 11, Tile::HazardLeft);
                self.vertical(18, 11, 13, Tile::Solid);
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
                // The first post-Boots room teaches the horizontal commitment twice rather than
                // demanding one opaque long crossing. Each sixty-pixel bank is beyond the
                // ordinary no-Dash jump envelope; the three-tile centre island is a full recovery
                // point from which the player can read and repeat the same action.
                for (start, end) in [(9, 15), (18, 24)] {
                    self.horizontal(16, start, end, Tile::HazardUp);
                    self.horizontal(17, start, end, Tile::Solid);
                }
                self.horizontal(16, 15, 18, Tile::OneWay);
                // Frame the course high enough that it cannot turn an imperfect diagonal Dash
                // into a ceiling collision.
                self.horizontal(6, 9, 24, Tile::HazardDown);
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
            DungeonPaletteCourse::AlloyThreshold => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 12, 17, Tile::OneWay);
                self.horizontal(8, 20, 25, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 9, 12, Tile::HazardUp);
                self.horizontal(17, 9, 12, Tile::Solid);
                self.horizontal(16, 17, 20, Tile::HazardUp);
                self.horizontal(17, 17, 20, Tile::Solid);
                self.horizontal(16, 25, 27, Tile::HazardUp);
                self.horizontal(17, 25, 27, Tile::Solid);
            }
            DungeonPaletteCourse::Windshaft => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(12, 11, 16, Tile::OneWay);
                self.horizontal(9, 18, 23, Tile::OneWay);
                self.horizontal(6, 11, 16, Tile::OneWay);
                self.horizontal(3, 19, 25, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 16, 18, Tile::HazardUp);
                self.horizontal(17, 16, 18, Tile::Solid);
            }
            DungeonPaletteCourse::SplitFurnace => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 12, 18, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 9, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::Solid);
                self.horizontal(7, 12, 16, Tile::HazardDown);
            }
            DungeonPaletteCourse::EmberVault => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 11, 16, Tile::OneWay);
                self.horizontal(8, 20, 26, Tile::OneWay);
                self.horizontal(5, 12, 18, Tile::OneWay);
                self.horizontal(3, 21, 27, Tile::OneWay);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
            }
            DungeonPaletteCourse::GearGallery => {
                self.vertical(12, 2, 16, Tile::Solid);
                self.vertical(18, 5, 17, Tile::Solid);
                self.horizontal(16, 12, 19, Tile::Solid);
                self.horizontal(9, 19, 26, Tile::OneWay);
                self.horizontal(6, 13, 17, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(12, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::CrosswindChimney => {
                self.vertical(10, 3, 12, Tile::Solid);
                self.vertical(16, 3, 17, Tile::Solid);
                self.horizontal(16, 10, 17, Tile::Solid);
                self.horizontal(5, 17, 25, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 19, 23, Tile::HazardUp);
                self.horizontal(17, 19, 23, Tile::Solid);
            }
            DungeonPaletteCourse::FoundryFork => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 12, 18, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 9, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::Solid);
                self.horizontal(5, 13, 19, Tile::OneWay);
            }
            DungeonPaletteCourse::CoolingDuct => {
                self.vertical(10, 5, 16, Tile::Solid);
                self.vertical(17, 3, 14, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::Solid);
                self.horizontal(12, 18, 23, Tile::OneWay);
                self.horizontal(8, 11, 16, Tile::OneWay);
                self.horizontal(4, 18, 24, Tile::OneWay);
            }
            DungeonPaletteCourse::HammerHall => {
                self.horizontal(14, 3, 10, Tile::OneWay);
                self.horizontal(11, 13, 18, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
                self.horizontal(16, 10, 23, Tile::HazardUp);
                self.horizontal(17, 10, 23, Tile::Solid);
            }
            DungeonPaletteCourse::LiftShaft => {
                self.horizontal(14, 4, 10, Tile::OneWay);
                self.horizontal(11, 12, 19, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(6, 14, 19, Tile::OneWay);
                self.horizontal(3, 14, 19, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 10, 13, Tile::HazardUp);
                self.horizontal(17, 10, 13, Tile::Solid);
                self.horizontal(16, 20, 23, Tile::HazardUp);
                self.horizontal(17, 20, 23, Tile::Solid);
            }
            DungeonPaletteCourse::SparkNiche => {
                self.vertical(9, 5, 16, Tile::Solid);
                self.vertical(16, 2, 13, Tile::Solid);
                self.horizontal(16, 9, 13, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
                self.horizontal(11, 17, 22, Tile::OneWay);
                self.horizontal(7, 10, 15, Tile::OneWay);
                self.horizontal(3, 17, 23, Tile::OneWay);
                self.horizontal(16, 22, 25, Tile::HazardUp);
            }
            DungeonPaletteCourse::RivetRun => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 15, Tile::OneWay);
                self.horizontal(8, 18, 22, Tile::OneWay);
                self.horizontal(5, 25, 29, Tile::OneWay);
                self.horizontal(14, 27, 30, Tile::OneWay);
                self.horizontal(16, 8, 11, Tile::HazardUp);
                self.horizontal(17, 8, 11, Tile::Solid);
                self.horizontal(16, 15, 18, Tile::HazardUp);
                self.horizontal(17, 15, 18, Tile::Solid);
                self.horizontal(16, 22, 25, Tile::HazardUp);
                self.horizontal(17, 22, 25, Tile::Solid);
            }
            DungeonPaletteCourse::BlastGallery => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(12, 11, 15, Tile::OneWay);
                self.horizontal(9, 19, 23, Tile::OneWay);
                self.horizontal(6, 12, 16, Tile::OneWay);
                self.horizontal(3, 20, 26, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 12, Tile::HazardUp);
                self.horizontal(17, 8, 12, Tile::Solid);
                self.horizontal(16, 16, 20, Tile::HazardUp);
                self.horizontal(17, 16, 20, Tile::Solid);
                self.horizontal(6, 17, 21, Tile::HazardDown);
            }
            DungeonPaletteCourse::PressureFork => {
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 12, 18, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(16, 9, 14, Tile::Solid);
                self.horizontal(16, 18, 23, Tile::Solid);
                self.horizontal(6, 14, 19, Tile::OneWay);
                self.horizontal(5, 19, 23, Tile::HazardDown);
            }
            DungeonPaletteCourse::AshCache => {
                self.vertical(10, 4, 16, Tile::Solid);
                self.vertical(17, 2, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::Solid);
                self.horizontal(12, 18, 23, Tile::OneWay);
                self.horizontal(8, 11, 16, Tile::OneWay);
                self.horizontal(4, 18, 24, Tile::OneWay);
                self.horizontal(16, 23, 26, Tile::HazardUp);
            }
            DungeonPaletteCourse::VentSpire => {
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
            DungeonPaletteCourse::PistonPass => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(10, 11, 15, Tile::OneWay);
                self.horizontal(13, 18, 22, Tile::OneWay);
                self.horizontal(8, 25, 29, Tile::OneWay);
                self.horizontal(16, 8, 11, Tile::HazardUp);
                self.horizontal(17, 8, 11, Tile::Solid);
                self.horizontal(16, 15, 18, Tile::HazardUp);
                self.horizontal(17, 15, 18, Tile::Solid);
                self.horizontal(16, 22, 25, Tile::HazardUp);
                self.horizontal(17, 22, 25, Tile::Solid);
                self.horizontal(6, 12, 21, Tile::HazardDown);
            }
            DungeonPaletteCourse::CrucibleClimb => {
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
            DungeonPaletteCourse::CinderBridge => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 12, 16, Tile::OneWay);
                self.horizontal(8, 20, 24, Tile::OneWay);
                self.horizontal(5, 27, 30, Tile::OneWay);
                self.horizontal(16, 8, 12, Tile::HazardUp);
                self.horizontal(17, 8, 12, Tile::Solid);
                self.horizontal(16, 16, 20, Tile::HazardUp);
                self.horizontal(17, 16, 20, Tile::Solid);
                self.horizontal(16, 24, 27, Tile::HazardUp);
                self.horizontal(17, 24, 27, Tile::Solid);
                self.horizontal(3, 17, 26, Tile::HazardDown);
            }
            DungeonPaletteCourse::FoundrySeal => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.vertical(10, 4, 14, Tile::Solid);
                self.vertical(16, 4, 17, Tile::Solid);
                self.horizontal(16, 10, 17, Tile::Solid);
                self.horizontal(6, 17, 23, Tile::OneWay);
                self.horizontal(14, 24, 30, Tile::OneWay);
                // The exit partition combines a wall ascent with the low Dash posture.
                self.horizontal(15, 18, 25, Tile::Solid);
                self.vertical(25, 1, 16, Tile::Solid);
                self.horizontal(9, 18, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::GlassThreshold => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 16, Tile::OneWay);
                self.horizontal(8, 20, 25, Tile::OneWay);
                self.horizontal(14, 27, 30, Tile::OneWay);
                self.horizontal(16, 8, 11, Tile::HazardUp);
                self.horizontal(17, 8, 11, Tile::Solid);
                self.horizontal(16, 16, 20, Tile::HazardUp);
                self.horizontal(17, 16, 20, Tile::Solid);
                self.horizontal(16, 25, 28, Tile::HazardUp);
                self.horizontal(17, 25, 28, Tile::Solid);
            }
            DungeonPaletteCourse::PrismRun => {
                self.horizontal(14, 3, 7, Tile::OneWay);
                self.horizontal(11, 11, 14, Tile::OneWay);
                self.horizontal(8, 18, 21, Tile::OneWay);
                self.horizontal(5, 25, 28, Tile::OneWay);
                self.horizontal(14, 28, 30, Tile::OneWay);
                for (start, end) in [(7, 11), (14, 18), (21, 25)] {
                    self.horizontal(16, start, end, Tile::HazardUp);
                    self.horizontal(17, start, end, Tile::Solid);
                }
                self.horizontal(3, 15, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::SplitKiln => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 17, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 14, Tile::Solid);
                self.horizontal(16, 18, 24, Tile::Solid);
                self.horizontal(6, 13, 18, Tile::OneWay);
                self.horizontal(5, 18, 23, Tile::HazardDown);
            }
            DungeonPaletteCourse::ShardVault => {
                self.vertical(10, 4, 16, Tile::Solid);
                self.vertical(17, 2, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::Solid);
                self.horizontal(12, 18, 23, Tile::OneWay);
                self.horizontal(8, 11, 16, Tile::OneWay);
                self.horizontal(4, 18, 24, Tile::OneWay);
                self.vertical(16, 8, 11, Tile::HazardLeft);
            }
            DungeonPaletteCourse::GlassGallery => {
                self.vertical(12, 2, 16, Tile::Solid);
                self.vertical(18, 5, 17, Tile::Solid);
                self.horizontal(16, 12, 19, Tile::Solid);
                self.horizontal(9, 19, 25, Tile::OneWay);
                self.horizontal(6, 13, 17, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(11, 19, 23, Tile::HazardDown);
                for row in 13..16 {
                    self.set(12, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::RefractionShaft => {
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
                self.horizontal(4, 15, 24, Tile::Solid);
                self.horizontal(14, 24, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(10, row, Tile::Empty);
                    self.set(11, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::MirrorFork => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 17, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 14, Tile::Solid);
                self.horizontal(16, 18, 24, Tile::Solid);
                self.horizontal(5, 14, 19, Tile::OneWay);
                self.horizontal(16, 14, 18, Tile::HazardUp);
            }
            DungeonPaletteCourse::MirrorDuct => {
                self.vertical(9, 4, 16, Tile::Solid);
                self.vertical(16, 2, 13, Tile::Solid);
                self.horizontal(16, 9, 13, Tile::Solid);
                self.horizontal(16, 18, 22, Tile::Solid);
                self.horizontal(11, 17, 22, Tile::OneWay);
                self.horizontal(7, 10, 15, Tile::OneWay);
                self.horizontal(3, 17, 23, Tile::OneWay);
                self.horizontal(16, 22, 26, Tile::HazardUp);
            }
            DungeonPaletteCourse::TemperHall => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 12, 17, Tile::OneWay);
                self.horizontal(14, 23, 30, Tile::OneWay);
                self.horizontal(16, 8, 12, Tile::HazardUp);
                self.horizontal(17, 8, 12, Tile::Solid);
                self.horizontal(16, 17, 23, Tile::HazardUp);
                self.horizontal(17, 17, 23, Tile::Solid);
                self.horizontal(7, 14, 22, Tile::HazardDown);
            }
            DungeonPaletteCourse::FurnaceLift => {
                self.horizontal(14, 4, 9, Tile::OneWay);
                self.horizontal(11, 12, 18, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(6, 14, 19, Tile::OneWay);
                self.horizontal(3, 14, 19, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 9, 13, Tile::HazardUp);
                self.horizontal(17, 9, 13, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::HazardUp);
                self.horizontal(17, 19, 23, Tile::Solid);
            }
            DungeonPaletteCourse::LensNiche => {
                self.vertical(10, 4, 16, Tile::Solid);
                self.vertical(17, 2, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::Solid);
                self.horizontal(12, 18, 23, Tile::OneWay);
                self.horizontal(8, 11, 16, Tile::OneWay);
                self.horizontal(4, 18, 24, Tile::OneWay);
                self.horizontal(16, 23, 27, Tile::HazardUp);
            }
            DungeonPaletteCourse::SliverRun => {
                self.horizontal(14, 3, 7, Tile::OneWay);
                self.horizontal(11, 11, 14, Tile::OneWay);
                self.horizontal(8, 18, 21, Tile::OneWay);
                self.horizontal(5, 25, 28, Tile::OneWay);
                self.horizontal(14, 28, 30, Tile::OneWay);
                for (start, end) in [(7, 11), (14, 18), (21, 25)] {
                    self.horizontal(16, start, end, Tile::HazardUp);
                    self.horizontal(17, start, end, Tile::Solid);
                }
            }
            DungeonPaletteCourse::HotGlass => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(12, 11, 15, Tile::OneWay);
                self.horizontal(9, 19, 23, Tile::OneWay);
                self.horizontal(6, 12, 16, Tile::OneWay);
                self.horizontal(3, 20, 26, Tile::OneWay);
                self.horizontal(14, 27, 30, Tile::OneWay);
                self.horizontal(16, 8, 12, Tile::HazardUp);
                self.horizontal(17, 8, 12, Tile::Solid);
                self.horizontal(16, 16, 20, Tile::HazardUp);
                self.horizontal(17, 16, 20, Tile::Solid);
                self.horizontal(6, 17, 22, Tile::HazardDown);
            }
            DungeonPaletteCourse::CulletFork => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 17, Tile::OneWay);
                self.horizontal(8, 21, 27, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 14, Tile::Solid);
                self.horizontal(16, 18, 24, Tile::Solid);
                self.horizontal(6, 13, 19, Tile::OneWay);
                self.horizontal(5, 19, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::CulletCache => {
                self.vertical(10, 4, 16, Tile::Solid);
                self.vertical(17, 2, 13, Tile::Solid);
                self.horizontal(16, 10, 14, Tile::Solid);
                self.horizontal(16, 19, 23, Tile::Solid);
                self.horizontal(12, 18, 23, Tile::OneWay);
                self.horizontal(8, 11, 16, Tile::OneWay);
                self.horizontal(4, 18, 24, Tile::OneWay);
                self.vertical(16, 7, 10, Tile::HazardLeft);
            }
            DungeonPaletteCourse::AnnealingSpire => {
                self.vertical(12, 2, 16, Tile::Solid);
                self.vertical(18, 5, 17, Tile::Solid);
                self.horizontal(16, 12, 19, Tile::Solid);
                self.horizontal(9, 19, 25, Tile::OneWay);
                self.horizontal(6, 13, 17, Tile::OneWay);
                self.horizontal(3, 14, 18, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(12, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::RazorPass => {
                self.horizontal(14, 3, 7, Tile::OneWay);
                self.horizontal(10, 11, 14, Tile::OneWay);
                self.horizontal(13, 18, 21, Tile::OneWay);
                self.horizontal(8, 25, 28, Tile::OneWay);
                self.horizontal(16, 7, 11, Tile::HazardUp);
                self.horizontal(17, 7, 11, Tile::Solid);
                self.horizontal(16, 14, 18, Tile::HazardUp);
                self.horizontal(17, 14, 18, Tile::Solid);
                self.horizontal(16, 21, 25, Tile::HazardUp);
                self.horizontal(17, 21, 25, Tile::Solid);
                self.horizontal(6, 12, 22, Tile::HazardDown);
            }
            DungeonPaletteCourse::LatticeClimb => {
                self.vertical(11, 1, 17, Tile::Solid);
                self.vertical(17, 4, 17, Tile::Solid);
                self.vertical(12, 1, 4, Tile::Solid);
                self.vertical(12, 4, 7, Tile::HazardRight);
                self.vertical(12, 7, 10, Tile::Solid);
                self.vertical(12, 10, 13, Tile::HazardRight);
                self.vertical(12, 13, 16, Tile::Solid);
                self.vertical(16, 4, 7, Tile::Solid);
                self.vertical(16, 7, 10, Tile::HazardLeft);
                self.vertical(16, 10, 13, Tile::Solid);
                self.vertical(16, 13, 16, Tile::HazardLeft);
                self.horizontal(4, 16, 25, Tile::Solid);
                self.horizontal(14, 25, 30, Tile::OneWay);
                for row in 13..16 {
                    self.set(11, row, Tile::Empty);
                    self.set(12, row, Tile::Empty);
                }
            }
            DungeonPaletteCourse::CrystalBridge => {
                self.horizontal(14, 3, 7, Tile::OneWay);
                self.horizontal(11, 11, 14, Tile::OneWay);
                self.horizontal(8, 18, 21, Tile::OneWay);
                self.horizontal(5, 25, 28, Tile::OneWay);
                self.horizontal(14, 28, 30, Tile::OneWay);
                for (start, end) in [(7, 11), (14, 18), (21, 25)] {
                    self.horizontal(16, start, end, Tile::HazardUp);
                    self.horizontal(17, start, end, Tile::Solid);
                }
                self.horizontal(3, 16, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::GlassSeal => {
                self.horizontal(14, 3, 7, Tile::OneWay);
                self.vertical(10, 3, 14, Tile::Solid);
                self.vertical(16, 4, 17, Tile::Solid);
                self.horizontal(16, 10, 17, Tile::Solid);
                self.horizontal(6, 17, 23, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
                self.horizontal(15, 18, 26, Tile::Solid);
                self.vertical(26, 1, 16, Tile::Solid);
                self.horizontal(9, 18, 25, Tile::HazardDown);
            }
            DungeonPaletteCourse::StarThreshold => {
                // The late-game threshold introduces its two-method contract in a visible
                // sequence. A one-tile passage through the sealed left backing first requires
                // Dash posture. It opens directly into a broad alternating Wall-Jump shaft;
                // upward-facing caps keep its safe bands from becoming Dash-refill ledges. The
                // reward shelf ends before the east wall, leaving a forgiving descent after the
                // coin instead of making the return journey part of the precision test.
                self.horizontal(15, 3, 6, Tile::Solid);

                self.vertical(5, 1, 17, Tile::Solid);
                self.vertical(6, 1, 4, Tile::HazardRight);
                self.set(6, 4, Tile::HazardUp);
                self.vertical(6, 5, 7, Tile::Solid);
                self.vertical(6, 7, 10, Tile::HazardRight);
                self.set(6, 10, Tile::HazardUp);
                self.vertical(6, 11, 13, Tile::Solid);
                self.vertical(6, 13, 15, Tile::HazardRight);

                self.vertical(12, 3, 17, Tile::Solid);
                self.vertical(11, 3, 5, Tile::Solid);
                self.vertical(11, 5, 7, Tile::HazardLeft);
                self.set(11, 7, Tile::HazardUp);
                self.vertical(11, 8, 10, Tile::Solid);
                self.vertical(11, 10, 13, Tile::HazardLeft);
                self.set(11, 13, Tile::HazardUp);
                self.vertical(11, 14, 17, Tile::Solid);

                self.set(5, 16, Tile::Empty);
                for row in 15..17 {
                    self.set(6, row, Tile::Empty);
                }
                self.horizontal(3, 11, 18, Tile::Solid);
            }
            DungeonPaletteCourse::CometRun => {
                // A late precision-Dash contour rather than another monotone bridge staircase.
                // The route rises twice, descends beneath the ceiling bank, then rises once more
                // to the coin. Thirty-pixel recovery platforms leave useful braking margin while
                // remaining materially smaller than the early game's broad shelves.
                self.horizontal(16, 1, 6, Tile::OneWay);
                self.horizontal(16, 6, 28, Tile::HazardUp);
                self.horizontal(11, 11, 13, Tile::OneWay);
                self.horizontal(7, 18, 20, Tile::OneWay);
                self.horizontal(12, 24, 26, Tile::OneWay);
                self.horizontal(7, 28, 31, Tile::OneWay);
                self.horizontal(4, 21, 28, Tile::HazardDown);
            }
            DungeonPaletteCourse::OrbitFork => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(10, 11, 17, Tile::OneWay);
                self.horizontal(7, 21, 27, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 14, Tile::Solid);
                self.horizontal(16, 18, 24, Tile::Solid);
                self.horizontal(4, 14, 19, Tile::OneWay);
                self.horizontal(4, 19, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::MoonVault => {
                // A readable clockwise orbit beneath the ceiling entrance: settle on the broad
                // central shelf, descend left and inward, then climb through the right-hand
                // recovery shelves to the coin. The ceiling bank prevents a single horizontal
                // Dash from skipping directly from the entrance shelf to the prize.
                self.horizontal(6, 13, 18, Tile::OneWay);
                self.horizontal(10, 5, 8, Tile::OneWay);
                self.horizontal(14, 13, 16, Tile::OneWay);
                self.horizontal(10, 22, 25, Tile::OneWay);
                self.horizontal(14, 22, 25, Tile::OneWay);
                self.horizontal(6, 26, 30, Tile::OneWay);
                self.vertical(20, 4, 12, Tile::Solid);
                self.horizontal(16, 1, 31, Tile::HazardUp);
                self.horizontal(3, 18, 26, Tile::HazardUp);
                self.horizontal(4, 18, 26, Tile::HazardDown);
            }
            DungeonPaletteCourse::ConstellationHall => {
                // A late under-over-under slalom rather than another diagonal Dash staircase.
                // Broad recovery shelves make the intended silhouette readable: pass beneath the
                // first ceiling pillar, climb over the floor-anchored centre, then descend and
                // commit under the final ceiling pillar. The lethal floor closes the low bypass
                // while leaving each authored recovery large enough for deliberate braking.
                self.horizontal(16, 1, 7, Tile::OneWay);
                self.horizontal(16, 7, 31, Tile::HazardUp);

                self.vertical(9, 1, 11, Tile::Solid);
                self.horizontal(14, 10, 14, Tile::OneWay);

                self.vertical(17, 7, 17, Tile::Solid);
                self.horizontal(7, 17, 22, Tile::Solid);

                self.vertical(24, 1, 11, Tile::Solid);
                self.horizontal(14, 20, 24, Tile::OneWay);
                self.horizontal(14, 25, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::ZenithShaft => {
                // Two offset chambers make the late mainline read as a deliberate sequence:
                // climb the lower shaft, recover on its roof, Dash to the upper shaft's floor,
                // then climb again and descend through the east return bay. Backed hazards close
                // the ground route while every transfer retains a visible recovery surface.
                self.horizontal(16, 1, 10, Tile::Solid);
                self.horizontal(16, 10, 28, Tile::HazardUp);
                self.horizontal(16, 28, 31, Tile::Solid);
                self.horizontal(17, 1, 31, Tile::Solid);

                self.vertical(4, 1, 14, Tile::Solid);
                self.vertical(9, 6, 14, Tile::Solid);
                self.horizontal(6, 9, 12, Tile::Solid);

                self.vertical(20, 3, 8, Tile::Solid);
                self.vertical(24, 3, 17, Tile::Solid);
                self.horizontal(10, 20, 25, Tile::Solid);
                self.horizontal(3, 24, 28, Tile::Solid);

                self.vertical(30, 3, 14, Tile::Solid);
            }
            DungeonPaletteCourse::EclipseFork => {
                // The three-way junction remains safe to traverse east-west, while its ceiling
                // branch is a compact two-method movement checkpoint. Both walls leave a
                // standing-height opening above the corridor; their broad alternating shaft
                // reaches a full launch balcony, from which one upward Dash enters the eclipse.
                self.horizontal(16, 1, 31, Tile::Solid);
                self.horizontal(17, 1, 31, Tile::Solid);
                self.vertical(11, 7, 14, Tile::Solid);
                self.vertical(16, 7, 14, Tile::Solid);
                self.horizontal(7, 16, 20, Tile::Solid);
            }
            DungeonPaletteCourse::ShadowDuct => {
                // A compact three-act branch instead of three broad staircase shelves. The floor
                // door opens into a safe starting bay, but the left shaft is reachable only
                // through a ten-pixel Dash aperture. Its alternating contact bands are capped so
                // they cannot become Dash-refill ledges. The upper exit faces a seventy-pixel
                // aerial commitment to the coin shelf, while the lethal lower bay closes the
                // original walk-and-jump shortcut. Both shelves remain broad enough to brake and
                // make the return route legible.
                self.horizontal(16, 4, 12, Tile::Solid);
                self.horizontal(16, 12, 20, Tile::OneWay);
                self.horizontal(16, 20, 31, Tile::HazardUp);

                self.vertical(4, 1, 17, Tile::Solid);
                self.vertical(5, 1, 4, Tile::HazardRight);
                self.set(5, 4, Tile::HazardUp);
                self.vertical(5, 5, 7, Tile::Solid);
                self.vertical(5, 7, 10, Tile::HazardRight);
                self.set(5, 10, Tile::HazardUp);
                self.vertical(5, 11, 13, Tile::Solid);
                self.vertical(5, 13, 15, Tile::HazardRight);
                self.vertical(5, 15, 17, Tile::Solid);

                self.vertical(10, 1, 17, Tile::Solid);
                self.vertical(9, 3, 5, Tile::Solid);
                self.vertical(9, 5, 7, Tile::HazardLeft);
                self.set(9, 7, Tile::HazardUp);
                self.vertical(9, 8, 10, Tile::Solid);
                self.vertical(9, 10, 13, Tile::HazardLeft);
                self.set(9, 13, Tile::HazardUp);
                self.vertical(9, 14, 17, Tile::Solid);

                // Make the shaft backing lethal on its starting-bay side, so the player cannot
                // single-wall-climb around the low entrance. These two authored openings are the
                // only route through it: low Dash entry, then broad upper exit.
                self.vertical(11, 1, 16, Tile::HazardRight);
                for row in [1, 2, 15] {
                    for column in 9..12 {
                        self.set(column, row, Tile::Empty);
                    }
                }
                self.horizontal(3, 9, 15, Tile::Solid);
                self.horizontal(3, 22, 29, Tile::Solid);
            }
            DungeonPaletteCourse::Observatory => {
                // Enter beneath a sealed observatory roof, run to the far tower, climb its broad
                // cap-safe alternating core, then reverse direction across two separated roof
                // instruments to the coin. The lethal canopy closes the direct west-side ascent
                // without making the lower approach dangerous; the three roof shelves retain
                // full braking space and make the return route visible.
                self.horizontal(16, 1, 31, Tile::Solid);
                self.horizontal(8, 1, 21, Tile::Solid);
                self.horizontal(7, 1, 21, Tile::HazardUp);

                self.vertical(21, 1, 17, Tile::Solid);
                self.vertical(22, 1, 3, Tile::HazardRight);
                self.set(22, 3, Tile::HazardUp);
                self.vertical(22, 4, 9, Tile::Solid);
                self.vertical(22, 9, 13, Tile::HazardRight);
                self.set(22, 13, Tile::HazardUp);
                self.vertical(22, 14, 17, Tile::Solid);

                self.vertical(27, 1, 17, Tile::Solid);
                self.vertical(26, 2, 6, Tile::Solid);
                self.set(26, 6, Tile::HazardLeft);
                self.set(26, 7, Tile::HazardUp);
                self.vertical(26, 8, 12, Tile::Solid);
                self.vertical(26, 12, 14, Tile::HazardLeft);
                self.set(26, 14, Tile::HazardUp);
                self.vertical(26, 15, 17, Tile::Solid);

                for row in 14..16 {
                    for column in [21, 22, 26, 27] {
                        self.set(column, row, Tile::Empty);
                    }
                }
                for row in 1..3 {
                    for column in [21, 22] {
                        self.set(column, row, Tile::Empty);
                    }
                }
                self.horizontal(3, 19, 23, Tile::Solid);
                self.horizontal(6, 10, 13, Tile::Solid);
                self.horizontal(3, 1, 5, Tile::Solid);
            }
            DungeonPaletteCourse::GravityLift => {
                // A non-lethal vertical switchback. Three thick lift baffles alternate their
                // opening from right to left to right, so holding an upward diagonal cannot reach
                // the ceiling branch. Each narrow end bay gives a wall to kick from and each
                // baffle is a full recovery floor; a miss costs height rather than a life.
                self.horizontal(16, 1, 31, Tile::Solid);

                self.horizontal(13, 1, 28, Tile::Solid);
                self.vertical(27, 12, 14, Tile::Solid);

                self.horizontal(9, 4, 31, Tile::Solid);
                self.vertical(4, 8, 10, Tile::Solid);

                self.horizontal(5, 1, 28, Tile::Solid);
                self.vertical(27, 4, 6, Tile::Solid);
            }
            DungeonPaletteCourse::NovaNiche => {
                // A compact late-game branch with two visible acts. The floor door lands inside
                // a cap-safe alternating Wall-Jump core. Its upper recovery shelf faces a
                // sixty-pixel gap above lethal floor; the coin shelf and only return route are on
                // the far side, making Dash the readable aerial commitment rather than incidental
                // speed. Current wall-ascent carry also permits a harder Dash-only climb; this
                // room intentionally explores that interaction instead of claiming a method gate.
                self.vertical(12, 1, 17, Tile::Solid);
                self.vertical(13, 1, 4, Tile::HazardRight);
                self.set(13, 4, Tile::HazardUp);
                self.vertical(13, 5, 7, Tile::Solid);
                self.vertical(13, 7, 10, Tile::HazardRight);
                self.set(13, 10, Tile::HazardUp);
                self.vertical(13, 11, 13, Tile::Solid);
                self.vertical(13, 13, 16, Tile::HazardRight);

                self.vertical(19, 3, 17, Tile::Solid);
                self.vertical(18, 3, 5, Tile::Solid);
                self.vertical(18, 5, 7, Tile::HazardLeft);
                self.set(18, 7, Tile::HazardUp);
                self.vertical(18, 8, 10, Tile::Solid);
                self.vertical(18, 10, 13, Tile::HazardLeft);
                self.set(18, 13, Tile::HazardUp);
                self.vertical(18, 14, 17, Tile::Solid);

                self.horizontal(3, 18, 22, Tile::Solid);
                self.horizontal(3, 28, 31, Tile::Solid);
                self.horizontal(16, 20, 31, Tile::HazardUp);
            }
            DungeonPaletteCourse::MeteorRun => {
                // The collision layer is intentionally a clear runway: Meteor Run's obstacle
                // course is supplied by three room-clock shutters in authored dungeon content.
                // Keeping broad safe bays between them makes their advancing timing legible and
                // gives the player somewhere to brake rather than hiding difficulty in landings.
            }
            DungeonPaletteCourse::VacuumGallery => {
                // A late mixed-method route: enter the narrow alternating wall shaft from the
                // lower left, climb over its sealed backing, then descend into a low tunnel whose
                // far wall is open only at Dash posture. The solid backing prevents side-entry
                // shortcuts. Upward-facing caps on every intermediate contact band prevent Dash
                // from landing, refreshing, and substituting for the authored wall rhythm.
                self.vertical(10, 1, 17, Tile::Solid);
                self.vertical(11, 1, 4, Tile::HazardRight);
                self.set(11, 4, Tile::HazardUp);
                self.vertical(11, 5, 7, Tile::Solid);
                self.vertical(11, 7, 10, Tile::HazardRight);
                self.set(11, 10, Tile::HazardUp);
                self.vertical(11, 11, 13, Tile::Solid);
                self.vertical(11, 13, 15, Tile::HazardRight);
                self.vertical(17, 3, 17, Tile::Solid);
                self.vertical(16, 3, 7, Tile::HazardLeft);
                self.set(16, 7, Tile::HazardUp);
                self.vertical(16, 8, 10, Tile::Solid);
                self.vertical(16, 10, 13, Tile::HazardLeft);
                self.set(16, 13, Tile::HazardUp);
                self.vertical(16, 14, 17, Tile::Solid);
                self.horizontal(15, 21, 29, Tile::Solid);
                self.vertical(28, 1, 16, Tile::Solid);
                for row in 15..17 {
                    self.set(10, row, Tile::Empty);
                    self.set(11, row, Tile::Empty);
                }
                self.horizontal(3, 17, 21, Tile::Solid);
            }
            DungeonPaletteCourse::TidalFork => {
                self.horizontal(14, 3, 8, Tile::OneWay);
                self.horizontal(11, 11, 17, Tile::OneWay);
                self.horizontal(7, 21, 27, Tile::OneWay);
                self.horizontal(14, 26, 30, Tile::OneWay);
                self.horizontal(16, 8, 14, Tile::Solid);
                self.horizontal(16, 18, 24, Tile::Solid);
                self.horizontal(5, 13, 19, Tile::OneWay);
                self.horizontal(4, 19, 24, Tile::HazardDown);
            }
            DungeonPaletteCourse::LunarCache => {
                // The ceiling entrance drops into a broad, non-lethal return stair. The coin
                // chamber on the right is sealed from the ceiling by column 21: its only ingress
                // is the ten-pixel passage under row 15. Dash posture fits; the standing player
                // does not. Beyond it, the same cap-safe alternating-wall primitive used by the
                // late physical gates forces a Wall-Jump ascent instead of a chain of Dash-refill
                // landings. The upper shelf is deliberately outside the separator so the coin
                // cannot be reached directly from the entrance.
                self.horizontal(14, 3, 9, Tile::OneWay);
                self.horizontal(11, 11, 17, Tile::OneWay);
                self.horizontal(8, 3, 9, Tile::OneWay);
                self.horizontal(5, 11, 17, Tile::OneWay);

                self.vertical(21, 1, 15, Tile::Solid);
                self.vertical(22, 1, 4, Tile::HazardRight);
                self.set(22, 4, Tile::HazardUp);
                self.vertical(22, 5, 7, Tile::Solid);
                self.vertical(22, 7, 10, Tile::HazardRight);
                self.set(22, 10, Tile::HazardUp);
                self.vertical(22, 11, 13, Tile::Solid);
                self.vertical(22, 13, 15, Tile::HazardRight);

                self.vertical(28, 3, 17, Tile::Solid);
                // The topmost contact is the authored endpoint rather than an intermediate Dash
                // refill. Keeping it safe gives the final Wall Jump a visible opposing face;
                // the lethal band immediately below still prevents climbing to it by contact.
                self.vertical(27, 3, 5, Tile::Solid);
                self.vertical(27, 5, 7, Tile::HazardLeft);
                self.set(27, 7, Tile::HazardUp);
                self.vertical(27, 8, 10, Tile::Solid);
                self.vertical(27, 10, 13, Tile::HazardLeft);
                self.set(27, 13, Tile::HazardUp);
                self.vertical(27, 14, 17, Tile::Solid);

                self.horizontal(15, 18, 24, Tile::Solid);
                self.horizontal(3, 27, 31, Tile::Solid);
            }
            DungeonPaletteCourse::AuroraSpire => {
                // Rise inside the left cap-safe core, cross the open crown of the spire, then
                // descend a visible right-hand cascade to the coin and east corridor. The full
                // backing wall prevents the old monotone diagonal shortcut; the descent turns a
                // familiar climb into an out-and-over route with safe recovery after the Dash.
                self.horizontal(16, 1, 31, Tile::Solid);
                self.vertical(12, 1, 17, Tile::Solid);
                self.vertical(13, 1, 4, Tile::HazardRight);
                self.set(13, 4, Tile::HazardUp);
                self.vertical(13, 5, 7, Tile::Solid);
                self.vertical(13, 7, 10, Tile::HazardRight);
                self.set(13, 10, Tile::HazardUp);
                self.vertical(13, 11, 13, Tile::Solid);
                self.vertical(13, 13, 16, Tile::HazardRight);

                self.vertical(19, 1, 17, Tile::Solid);
                self.vertical(18, 3, 5, Tile::Solid);
                self.vertical(18, 5, 7, Tile::HazardLeft);
                self.set(18, 7, Tile::HazardUp);
                self.vertical(18, 8, 10, Tile::Solid);
                self.vertical(18, 10, 13, Tile::HazardLeft);
                self.set(18, 13, Tile::HazardUp);
                self.vertical(18, 14, 17, Tile::Solid);

                for row in 14..16 {
                    self.set(12, row, Tile::Empty);
                    self.set(13, row, Tile::Empty);
                }
                for row in 1..3 {
                    self.set(18, row, Tile::Empty);
                    self.set(19, row, Tile::Empty);
                }
                self.horizontal(3, 18, 22, Tile::Solid);
                self.horizontal(7, 19, 28, Tile::HazardUp);
                self.horizontal(7, 28, 31, Tile::OneWay);
                self.horizontal(10, 21, 30, Tile::OneWay);
                self.set(30, 10, Tile::HazardUp);
                self.horizontal(13, 24, 29, Tile::Solid);
            }
            DungeonPaletteCourse::VoidPass => {
                // A late-game combination course rather than another copy of the horizontal
                // bridge template. Enter the shaft through its low left aperture, alternate
                // through four broad contact bands, then leave over the right wall's cap. The
                // uppermost tile of every safe band is HazardUp: it is safe from the shaft-facing
                // side but cannot become a tiny landing that refills Dash between bands.
                self.vertical(10, 1, 17, Tile::Solid);
                self.vertical(11, 1, 4, Tile::HazardRight);
                self.set(11, 4, Tile::HazardUp);
                self.vertical(11, 5, 7, Tile::Solid);
                self.vertical(11, 7, 10, Tile::HazardRight);
                self.set(11, 10, Tile::HazardUp);
                self.vertical(11, 11, 13, Tile::Solid);
                self.vertical(11, 13, 15, Tile::HazardRight);
                self.vertical(17, 3, 17, Tile::Solid);
                self.vertical(16, 3, 7, Tile::HazardLeft);
                self.set(16, 7, Tile::HazardUp);
                self.vertical(16, 8, 10, Tile::Solid);
                self.vertical(16, 10, 13, Tile::HazardLeft);
                self.set(16, 13, Tile::HazardUp);
                self.vertical(16, 14, 17, Tile::Solid);
                for row in 15..17 {
                    self.set(10, row, Tile::Empty);
                    self.set(11, row, Tile::Empty);
                }
                self.horizontal(3, 17, 24, Tile::Solid);
                self.horizontal(14, 25, 30, Tile::OneWay);
            }
            DungeonPaletteCourse::StarwellClimb => {
                // A late two-method relay rather than another generic wall shell. The west bay
                // opens beneath a broad paired-wall climb. Its roof is a full recovery and launch
                // shelf; the east door is reachable only after committing across the backed
                // starwell to a separate catch platform, then dropping through the safe east bay.
                self.horizontal(16, 1, 13, Tile::Solid);
                self.horizontal(16, 13, 28, Tile::HazardUp);
                self.horizontal(16, 28, 31, Tile::Solid);
                self.horizontal(17, 1, 31, Tile::Solid);

                self.vertical(7, 1, 14, Tile::Solid);
                self.vertical(12, 4, 14, Tile::Solid);
                self.horizontal(4, 12, 18, Tile::Solid);

                // The catch doubles as the roof of a return shaft. Its two-tile bottom opening
                // makes the mainline reversible without providing another west-to-east route.
                self.vertical(25, 9, 17, Tile::Solid);
                self.vertical(30, 9, 14, Tile::Solid);
                self.horizontal(9, 25, 28, Tile::Solid);
            }
            DungeonPaletteCourse::Skybridge => {
                // The broken bridge first goes over a floor-anchored mast, then beneath a
                // ceiling-hung one. Broad recovery decks make the two readings explicit: climb
                // the left obstruction, then drop and Dash through the thirty-pixel aperture.
                self.horizontal(16, 1, 6, Tile::Solid);
                self.horizontal(16, 6, 28, Tile::HazardUp);
                self.horizontal(17, 1, 31, Tile::Solid);
                self.horizontal(16, 28, 31, Tile::Solid);
                self.horizontal(13, 5, 11, Tile::OneWay);
                self.vertical(7, 5, 11, Tile::Solid);
                self.vertical(11, 5, 17, Tile::Solid);
                self.horizontal(5, 11, 18, Tile::Solid);
                self.vertical(20, 1, 10, Tile::Solid);
                self.horizontal(13, 21, 28, Tile::Solid);
            }
            DungeonPaletteCourse::AstralSeal => {
                // The last regional coin is an exposed two-method relay rather than a tangled
                // shelf maze. Enter the broad shaft through its standing-height opening, climb
                // to the small launch deck, then commit one airborne Dash across the starwell.
                self.horizontal(16, 1, 15, Tile::Solid);
                self.horizontal(16, 15, 29, Tile::HazardUp);
                self.horizontal(16, 29, 31, Tile::Solid);
                self.horizontal(17, 1, 31, Tile::Solid);

                self.vertical(9, 6, 14, Tile::Solid);
                self.vertical(14, 6, 17, Tile::Solid);
                self.horizontal(6, 14, 17, Tile::Solid);

                self.horizontal(6, 24, 29, Tile::Solid);
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
                // The final lock is physical as well as inventory-gated. The west bay is sealed
                // by a tall chimney, whose cap is a full recovery before a ten-pixel horizontal
                // keyhole. Only low Dash posture fits beneath the lintel; the far side opens into
                // a safe descent to the Crown door rather than asking for a blind landing.
                self.horizontal(16, 1, 8, Tile::Solid);
                self.vertical(7, 6, 14, Tile::Solid);
                self.vertical(12, 6, 17, Tile::Solid);

                self.horizontal(6, 12, 24, Tile::Solid);
                for row in 1..5 {
                    self.horizontal(row, 14, 24, Tile::Solid);
                }

                self.horizontal(16, 24, 31, Tile::Solid);
                self.horizontal(17, 1, 31, Tile::Solid);
            }
            DungeonPaletteCourse::CrownSanctum => {
                // The Crown sits beyond a final W-shaped movement proof: climb the west core,
                // descend beneath the hanging centre mast, then climb the taller east core. Each
                // act has a full recovery, but the abyss prevents the old monotone shelf route.
                self.horizontal(16, 1, 13, Tile::Solid);
                self.horizontal(16, 13, 31, Tile::HazardUp);
                self.horizontal(17, 1, 31, Tile::Solid);

                self.vertical(7, 5, 11, Tile::Solid);
                self.vertical(12, 5, 17, Tile::Solid);
                self.horizontal(5, 12, 17, Tile::Solid);

                self.vertical(18, 1, 12, Tile::Solid);
                self.horizontal(13, 14, 18, Tile::Solid);
                self.horizontal(13, 19, 28, Tile::Solid);

                self.vertical(23, 4, 11, Tile::Solid);
                self.vertical(28, 3, 14, Tile::Solid);
                self.horizontal(3, 28, 31, Tile::Solid);
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
    fn dash_chasm_has_two_outward_facing_banks_with_a_recovery_between_them() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::DashChasm).generate();
        let course_row = &candidate.tiles[16 * usize::from(WIDTH)..17 * usize::from(WIDTH)];
        let mut hazard_runs = Vec::new();
        let mut run_start = None;
        for (column, &tile) in course_row
            .iter()
            .chain(std::iter::once(&Tile::Empty))
            .enumerate()
        {
            if tile == Tile::HazardUp {
                run_start.get_or_insert(column);
            } else if let Some(start) = run_start.take() {
                hazard_runs.push(start..column);
            }
        }
        assert_eq!(hazard_runs.len(), 2, "Dash Chasm must have two hazard gaps");
        assert!(hazard_runs.iter().all(|run| run.len() >= 2));
        for run in &hazard_runs {
            assert!(run.clone().all(|column| {
                candidate.tiles[17 * usize::from(WIDTH) + column] == Tile::Solid
            }));
        }
        let recovery = hazard_runs[0].end..hazard_runs[1].start;
        assert!(!recovery.is_empty(), "hazard gaps need a recovery interval");
        assert!(
            recovery
                .clone()
                .all(|column| course_row[column] == Tile::OneWay)
        );
    }

    #[test]
    fn comet_run_serializes_an_up_then_down_dash_slalom() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::CometRun).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 6..28 {
            assert_eq!(tile(column, 16), Tile::HazardUp);
            assert_eq!(tile(column, 17), Tile::Solid);
        }
        for (row, range) in [(11, 11..13), (7, 18..20), (12, 24..26), (7, 28..31)] {
            for column in range {
                assert_eq!(tile(column, row), Tile::OneWay);
            }
        }
        for column in 21..28 {
            assert_eq!(tile(column, 4), Tile::HazardDown);
        }
    }

    #[test]
    fn vacuum_gallery_serializes_an_alternating_climb_and_low_dash_tunnel() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::VacuumGallery).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for row in 15..17 {
            assert_eq!(tile(10, row), Tile::Empty, "lower shaft entry is sealed");
            assert_eq!(tile(11, row), Tile::Empty, "lower shaft face is sealed");
        }
        for row in [4, 10] {
            assert_eq!(tile(11, row), Tile::HazardUp);
        }
        for row in [7, 13] {
            assert_eq!(tile(16, row), Tile::HazardUp);
        }
        for (column, rows) in [(11, [5..7, 11..13]), (16, [8..10, 14..17])] {
            for row in rows.into_iter().flatten() {
                assert_eq!(tile(column, row), Tile::Solid);
            }
        }
        for column in 21..29 {
            assert_eq!(tile(column, 15), Tile::Solid, "low tunnel lacks a ceiling");
            assert_eq!(tile(column, 16), Tile::Empty, "Dash aperture is obstructed");
            assert_eq!(tile(column, 17), Tile::Solid, "low tunnel lacks a floor");
        }
        for row in 1..16 {
            assert_eq!(
                tile(28, row),
                Tile::Solid,
                "tunnel barrier has an upper bypass"
            );
        }
        assert_eq!(
            tile(28, 16),
            Tile::Empty,
            "tunnel barrier lacks its Dash aperture"
        );
    }

    #[test]
    fn star_threshold_serializes_dash_then_cap_safe_wall_climb() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::StarThreshold).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 3..6 {
            assert_eq!(tile(column, 15), Tile::Solid, "low tunnel lacks a ceiling");
            assert_eq!(tile(column, 16), Tile::Empty, "Dash aperture is obstructed");
            assert_eq!(tile(column, 17), Tile::Solid, "low tunnel lacks a floor");
        }
        for row in 1..15 {
            assert_eq!(tile(5, row), Tile::Solid, "threshold backing has a bypass");
        }
        assert_eq!(tile(5, 15), Tile::Solid, "threshold tunnel ceiling is open");
        assert_eq!(
            tile(5, 16),
            Tile::Empty,
            "threshold Dash aperture is sealed"
        );
        for row in 15..17 {
            assert_eq!(tile(6, row), Tile::Empty, "shaft entry is sealed");
        }
        for row in [4, 10] {
            assert_eq!(tile(6, row), Tile::HazardUp);
        }
        for row in [7, 13] {
            assert_eq!(tile(11, row), Tile::HazardUp);
        }
        for (column, rows) in [(6, [5..7, 11..13]), (11, [8..10, 14..17])] {
            for row in rows.into_iter().flatten() {
                assert_eq!(tile(column, row), Tile::Solid);
            }
        }
        for column in 11..18 {
            assert_eq!(tile(column, 3), Tile::Solid, "reward shelf is incomplete");
        }
        assert_eq!(
            tile(18, 3),
            Tile::Empty,
            "reward shelf blocks the safe descent"
        );
    }

    #[test]
    fn nova_niche_serializes_a_cap_safe_core_and_corona_gap() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::NovaNiche).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for row in 1..17 {
            assert_eq!(tile(12, row), Tile::Solid, "Nova core backing is open");
        }
        for row in [4, 10] {
            assert_eq!(tile(13, row), Tile::HazardUp);
        }
        for row in [7, 13] {
            assert_eq!(tile(18, row), Tile::HazardUp);
        }
        for (column, rows) in [(13, [5..7, 11..13]), (18, [8..10, 14..17])] {
            for row in rows.into_iter().flatten() {
                assert_eq!(tile(column, row), Tile::Solid);
            }
        }
        for column in 18..22 {
            assert_eq!(
                tile(column, 3),
                Tile::Solid,
                "Nova launch shelf is incomplete"
            );
        }
        for column in 22..28 {
            assert_eq!(tile(column, 3), Tile::Empty, "Nova corona gap is bridged");
        }
        for column in 28..31 {
            assert_eq!(
                tile(column, 3),
                Tile::Solid,
                "Nova coin shelf is incomplete"
            );
        }
        for column in 20..31 {
            assert_eq!(tile(column, 16), Tile::HazardUp, "Nova fall is non-lethal");
        }
    }

    #[test]
    fn constellation_hall_serializes_an_under_over_under_slalom() {
        let candidate =
            DungeonPaletteKey::new(0, DungeonPaletteCourse::ConstellationHall).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..7 {
            assert_eq!(
                tile(column, 16),
                Tile::OneWay,
                "start recovery is incomplete"
            );
        }
        for column in (7..17).chain(18..31) {
            assert_eq!(
                tile(column, 16),
                Tile::HazardUp,
                "slalom floor has a bypass"
            );
        }
        assert_eq!(
            tile(17, 16),
            Tile::Solid,
            "central slalom pillar does not meet the floor"
        );
        for row in 1..11 {
            assert_eq!(tile(9, row), Tile::Solid, "first hanging pillar is open");
            assert_eq!(tile(24, row), Tile::Solid, "last hanging pillar is open");
        }
        for row in 7..17 {
            assert_eq!(tile(17, row), Tile::Solid, "central pillar is open");
        }
        for (row, range) in [(14, 10..14), (7, 17..22), (14, 20..24), (14, 25..30)] {
            for column in range {
                assert!(
                    matches!(tile(column, row), Tile::OneWay | Tile::Solid),
                    "slalom recovery is incomplete at ({column}, {row})"
                );
            }
        }
    }

    #[test]
    fn shadow_duct_serializes_low_entry_cap_safe_climb_and_reward_gap() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::ShadowDuct).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 4..12 {
            assert_eq!(tile(column, 16), Tile::Solid, "Shadow shaft floor is open");
        }
        for column in 12..20 {
            assert_eq!(
                tile(column, 16),
                Tile::OneWay,
                "Shadow start bay cannot return through its floor door"
            );
        }
        for column in 20..31 {
            assert_eq!(
                tile(column, 16),
                Tile::HazardUp,
                "Shadow fall is non-lethal"
            );
        }
        for row in [1, 2, 15] {
            for column in 9..12 {
                assert_eq!(tile(column, row), Tile::Empty, "Shadow aperture is sealed");
            }
        }
        for row in 3..15 {
            assert!(
                !matches!(tile(10, row), Tile::Empty | Tile::OneWay),
                "Shadow backing has a bypass at row {row}"
            );
        }
        for row in [4, 10] {
            assert_eq!(tile(5, row), Tile::HazardUp);
        }
        for row in [7, 13] {
            assert_eq!(tile(9, row), Tile::HazardUp);
        }
        for (column, range) in [(9, 9..15), (22, 22..29)] {
            for shelf_column in range {
                assert_eq!(
                    tile(shelf_column, 3),
                    Tile::Solid,
                    "Shadow shelf from column {column} is incomplete"
                );
            }
        }
        for column in 15..22 {
            assert_eq!(tile(column, 3), Tile::Empty, "Shadow reward gap is bridged");
        }
    }

    #[test]
    fn observatory_serializes_a_broad_tower_and_two_roof_crossings() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::Observatory).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..31 {
            assert_eq!(tile(column, 16), Tile::Solid, "Observatory floor is open");
        }
        for column in 1..21 {
            assert_eq!(tile(column, 8), Tile::Solid, "Observatory canopy is open");
            assert_eq!(
                tile(column, 7),
                Tile::HazardUp,
                "Observatory canopy admits a direct ascent"
            );
        }
        for row in 3..14 {
            assert_eq!(tile(21, row), Tile::Solid, "left tower backing is open");
            assert_eq!(tile(27, row), Tile::Solid, "right tower backing is open");
        }
        for row in [1, 2, 14, 15] {
            assert_eq!(tile(21, row), Tile::Empty, "tower opening is sealed");
        }
        for row in [14, 15] {
            assert_eq!(tile(22, row), Tile::Empty, "tower entrance is sealed");
            assert_eq!(tile(26, row), Tile::Empty, "tower entrance is sealed");
            assert_eq!(tile(27, row), Tile::Empty, "tower entrance is sealed");
        }
        for row in 4..9 {
            assert_eq!(
                tile(22, row),
                Tile::Solid,
                "left contact band is too narrow"
            );
        }
        for row in 8..12 {
            assert_eq!(
                tile(26, row),
                Tile::Solid,
                "right contact band is too narrow"
            );
        }
        for (row, range) in [(3, 19..23), (6, 10..13), (3, 1..5)] {
            for column in range {
                assert_eq!(
                    tile(column, row),
                    Tile::Solid,
                    "roof shelf is incomplete at ({column}, {row})"
                );
            }
        }
        for column in 13..19 {
            assert_eq!(tile(column, 3), Tile::Empty, "first roof gap is bridged");
        }
        for column in 5..10 {
            assert_eq!(tile(column, 3), Tile::Empty, "second roof gap is bridged");
        }
    }

    #[test]
    fn gravity_lift_serializes_three_alternating_nonlethal_baffles() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::GravityLift).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..31 {
            assert_eq!(tile(column, 16), Tile::Solid, "Gravity Lift floor is open");
        }
        for column in 1..28 {
            assert_eq!(tile(column, 13), Tile::Solid, "lower lift floor is open");
            assert_eq!(tile(column, 5), Tile::Solid, "upper lift floor is open");
        }
        for column in 4..31 {
            assert_eq!(tile(column, 9), Tile::Solid, "middle lift floor is open");
        }
        assert_eq!(tile(27, 12), Tile::Solid, "lower right turn lacks a wall");
        assert_eq!(tile(4, 8), Tile::Solid, "middle left turn lacks a wall");
        assert_eq!(tile(27, 4), Tile::Solid, "upper right turn lacks a wall");
        for row in [5, 9, 13] {
            for column in 1..31 {
                assert_ne!(
                    tile(column, row),
                    Tile::HazardUp,
                    "Gravity Lift recovery floor became lethal"
                );
            }
        }
        for (row, columns) in [(13, 28..31), (9, 1..4), (5, 28..31)] {
            for column in columns {
                assert_eq!(
                    tile(column, row),
                    Tile::Empty,
                    "Gravity Lift switchback opening is obstructed"
                );
            }
        }
    }

    #[test]
    fn aurora_spire_serializes_a_cap_safe_core_and_recovery_cascade() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::AuroraSpire).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..31 {
            assert_eq!(tile(column, 16), Tile::Solid, "Aurora floor is open");
        }
        for row in 1..17 {
            if !(14..16).contains(&row) {
                assert_eq!(tile(12, row), Tile::Solid, "left shaft backing is open");
            }
            if !(1..3).contains(&row) {
                let expected = if row == 7 {
                    Tile::HazardUp
                } else {
                    Tile::Solid
                };
                assert_eq!(tile(19, row), expected, "right shaft backing is open");
            }
        }
        for row in 14..16 {
            assert_eq!(tile(12, row), Tile::Empty, "shaft entrance is sealed");
            assert_eq!(tile(13, row), Tile::Empty, "shaft entrance is sealed");
        }
        for row in 1..3 {
            assert_eq!(tile(18, row), Tile::Empty, "shaft crown is sealed");
            assert_eq!(tile(19, row), Tile::Empty, "shaft crown is sealed");
        }
        for column in 19..28 {
            assert_eq!(
                tile(column, 7),
                Tile::HazardUp,
                "Aurora light sheet has a safe gap"
            );
        }
        for column in 28..31 {
            assert_eq!(
                tile(column, 7),
                Tile::OneWay,
                "upper recovery is incomplete"
            );
        }
        for column in 21..30 {
            assert_eq!(
                tile(column, 10),
                Tile::OneWay,
                "middle recovery is incomplete"
            );
        }
        assert_eq!(
            tile(30, 10),
            Tile::HazardUp,
            "middle recovery lacks its stop"
        );
        for column in 24..29 {
            assert_eq!(
                tile(column, 13),
                Tile::Solid,
                "lower recovery is incomplete"
            );
        }
    }

    #[test]
    fn skybridge_serializes_an_over_then_under_broken_bridge() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::Skybridge).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..6 {
            assert_eq!(tile(column, 16), Tile::Solid, "west sill is incomplete");
        }
        for column in 6..28 {
            let expected = if column == 11 {
                Tile::Solid
            } else {
                Tile::HazardUp
            };
            assert_eq!(tile(column, 16), expected, "bridge abyss has a safe gap");
        }
        for column in 28..31 {
            assert_eq!(tile(column, 16), Tile::Solid, "east sill is incomplete");
        }
        for column in 1..31 {
            assert_eq!(tile(column, 17), Tile::Solid, "bridge abyss lacks backing");
        }
        for column in 5..11 {
            assert_eq!(tile(column, 13), Tile::OneWay, "lower island is incomplete");
        }
        for row in 5..11 {
            assert_eq!(tile(7, row), Tile::Solid, "left climb face is open");
        }
        for row in 5..17 {
            assert_eq!(tile(11, row), Tile::Solid, "floor mast is open");
        }
        for column in 11..18 {
            assert_eq!(tile(column, 5), Tile::Solid, "mast roof is incomplete");
        }
        for row in 1..10 {
            assert_eq!(tile(20, row), Tile::Solid, "hanging mast is open");
        }
        for row in 10..13 {
            assert_eq!(tile(20, row), Tile::Empty, "low aperture is obstructed");
        }
        for column in 21..28 {
            assert_eq!(tile(column, 13), Tile::Solid, "far recovery is incomplete");
        }
    }

    #[test]
    fn astral_seal_serializes_a_broad_climb_and_exposed_dash_relay() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::AstralSeal).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..15 {
            assert_eq!(tile(column, 16), Tile::Solid, "west sill is incomplete");
        }
        for column in 15..29 {
            assert_eq!(
                tile(column, 16),
                Tile::HazardUp,
                "starwell has a floor bypass"
            );
        }
        for row in 6..14 {
            assert_eq!(tile(9, row), Tile::Solid, "west climb face is open");
        }
        for row in 14..16 {
            assert_eq!(tile(9, row), Tile::Empty, "shaft entrance is obstructed");
        }
        for row in 6..17 {
            assert_eq!(tile(14, row), Tile::Solid, "east climb face is open");
        }
        for column in 14..17 {
            assert_eq!(tile(column, 6), Tile::Solid, "launch deck is incomplete");
        }
        for column in 17..24 {
            assert_eq!(
                tile(column, 6),
                Tile::Empty,
                "airborne relay gap is obstructed"
            );
        }
        for column in 24..29 {
            assert_eq!(tile(column, 6), Tile::Solid, "coin deck is incomplete");
        }
        for column in 1..31 {
            assert_eq!(
                tile(column, 17),
                Tile::Solid,
                "starwell lacks floor backing"
            );
        }
    }

    #[test]
    fn starwell_climb_serializes_a_backed_ascent_and_separate_catch() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::StarwellClimb).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..13 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "west launch floor is incomplete"
            );
        }
        for column in 13..28 {
            let expected = if column == 25 {
                Tile::Solid
            } else {
                Tile::HazardUp
            };
            assert_eq!(tile(column, 16), expected, "starwell has a floor bypass");
        }
        for column in 28..31 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "east arrival floor is incomplete"
            );
        }
        for column in 1..31 {
            assert_eq!(tile(column, 17), Tile::Solid, "starwell lacks backing");
        }
        for row in 1..14 {
            assert_eq!(
                tile(7, row),
                Tile::Solid,
                "ceiling-backed west wall is open"
            );
        }
        for row in 4..14 {
            assert_eq!(tile(12, row), Tile::Solid, "east climb face is open");
        }
        for row in 14..16 {
            assert_eq!(tile(7, row), Tile::Empty, "shaft entrance is obstructed");
            assert_eq!(tile(12, row), Tile::Empty, "shaft entrance is obstructed");
        }
        for column in 12..18 {
            assert_eq!(tile(column, 4), Tile::Solid, "launch shelf is incomplete");
        }
        for column in 18..25 {
            assert_eq!(tile(column, 9), Tile::Empty, "starwell relay is obstructed");
        }
        for column in 25..28 {
            assert_eq!(tile(column, 9), Tile::Solid, "catch platform is incomplete");
        }
        for row in 9..17 {
            assert_eq!(tile(25, row), Tile::Solid, "return shaft backing is open");
        }
        for row in 9..14 {
            assert_eq!(tile(30, row), Tile::Solid, "return shaft face is open");
        }
        for row in 14..16 {
            assert_eq!(
                tile(30, row),
                Tile::Empty,
                "return shaft entrance is obstructed"
            );
        }
    }

    #[test]
    fn zenith_shaft_serializes_two_offset_climbs_and_a_return_bay() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::ZenithShaft).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..10 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "lower launch floor is incomplete"
            );
        }
        for column in 10..28 {
            let expected = if column == 24 {
                Tile::Solid
            } else {
                Tile::HazardUp
            };
            assert_eq!(tile(column, 16), expected, "Zenith floor has a bypass");
        }
        for column in 28..31 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "east arrival floor is incomplete"
            );
        }
        for column in 1..31 {
            assert_eq!(tile(column, 17), Tile::Solid, "Zenith floor lacks backing");
        }

        for row in 1..14 {
            assert_eq!(tile(4, row), Tile::Solid, "lower west wall is open");
        }
        for row in 6..14 {
            assert_eq!(tile(9, row), Tile::Solid, "lower east wall is open");
        }
        for column in 9..12 {
            assert_eq!(
                tile(column, 6),
                Tile::Solid,
                "lower recovery roof is incomplete"
            );
        }

        for row in 3..8 {
            assert_eq!(tile(20, row), Tile::Solid, "upper west wall is open");
        }
        for row in 8..10 {
            assert_eq!(
                tile(20, row),
                Tile::Empty,
                "upper entry aperture is obstructed"
            );
        }
        for row in 3..17 {
            assert_eq!(tile(24, row), Tile::Solid, "upper east wall is open");
        }
        for column in 20..24 {
            assert_eq!(
                tile(column, 10),
                Tile::Solid,
                "upper shaft floor is incomplete"
            );
        }
        for column in 24..28 {
            assert_eq!(
                tile(column, 3),
                Tile::Solid,
                "upper recovery roof is incomplete"
            );
        }
        for row in 3..14 {
            assert_eq!(tile(30, row), Tile::Solid, "return wall is open");
        }
        for row in 14..16 {
            assert_eq!(
                tile(30, row),
                Tile::Empty,
                "return-bay entrance is obstructed"
            );
        }
    }

    #[test]
    fn eclipse_fork_serializes_a_safe_alternating_shaft_and_launch_balcony() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::EclipseFork).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..31 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "fork corridor floor is incomplete"
            );
            assert_eq!(tile(column, 17), Tile::Solid, "fork corridor lacks backing");
        }
        for row in 7..14 {
            assert_eq!(tile(11, row), Tile::Solid, "west shaft face is open");
            assert_eq!(tile(16, row), Tile::Solid, "east shaft face is open");
        }
        for row in 14..16 {
            assert_eq!(
                tile(11, row),
                Tile::Empty,
                "west shaft entrance is obstructed"
            );
            assert_eq!(
                tile(16, row),
                Tile::Empty,
                "east shaft entrance is obstructed"
            );
        }
        for column in 16..20 {
            assert_eq!(tile(column, 7), Tile::Solid, "launch balcony is incomplete");
        }
        for row in 1..7 {
            for column in 14..18 {
                assert_eq!(
                    tile(column, row),
                    Tile::Empty,
                    "ceiling Dash lane is obstructed"
                );
            }
        }
    }

    #[test]
    fn gatehouse_serializes_a_wall_climb_and_low_dash_keyhole() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::Gatehouse).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..8 {
            assert_eq!(tile(column, 16), Tile::Solid, "west sill is incomplete");
        }
        for row in 6..14 {
            assert_eq!(tile(7, row), Tile::Solid, "west climb face is open");
        }
        for row in 14..16 {
            assert_eq!(tile(7, row), Tile::Empty, "shaft entrance is obstructed");
        }
        for row in 6..17 {
            assert_eq!(tile(12, row), Tile::Solid, "east climb face is open");
        }
        for column in 12..24 {
            assert_eq!(tile(column, 6), Tile::Solid, "keyhole floor is open");
        }
        for column in 14..24 {
            for row in 1..5 {
                assert_eq!(tile(column, row), Tile::Solid, "keyhole lintel is open");
            }
            assert_eq!(
                tile(column, 5),
                Tile::Empty,
                "keyhole passage is obstructed"
            );
        }
        for column in 24..31 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "east landing bay is incomplete"
            );
        }
        for column in 1..31 {
            assert_eq!(
                tile(column, 17),
                Tile::Solid,
                "gatehouse lacks floor backing"
            );
        }
    }

    #[test]
    fn crown_sanctum_serializes_two_climbs_around_a_low_dash_passage() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::CrownSanctum).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for column in 1..13 {
            assert_eq!(
                tile(column, 16),
                Tile::Solid,
                "Crown approach floor is open"
            );
        }
        for column in 13..31 {
            assert_eq!(
                tile(column, 16),
                Tile::HazardUp,
                "Crown abyss has a safe bypass"
            );
        }
        for row in 5..11 {
            assert_eq!(tile(7, row), Tile::Solid, "west climb face is open");
        }
        for row in 5..17 {
            assert_eq!(tile(12, row), Tile::Solid, "west climb backing is open");
        }
        for column in 12..17 {
            assert_eq!(
                tile(column, 5),
                Tile::Solid,
                "west recovery roof is incomplete"
            );
        }
        for row in 1..12 {
            assert_eq!(tile(18, row), Tile::Solid, "centre hanging mast is open");
        }
        assert_eq!(
            tile(18, 12),
            Tile::Empty,
            "ten-pixel Dash passage is sealed"
        );
        for column in 14..18 {
            assert_eq!(
                tile(column, 13),
                Tile::Solid,
                "west Dash recovery is incomplete"
            );
        }
        for column in 19..28 {
            assert_eq!(
                tile(column, 13),
                Tile::Solid,
                "east Dash recovery is incomplete"
            );
        }
        for row in 4..11 {
            assert_eq!(tile(23, row), Tile::Solid, "east climb face is open");
        }
        for row in 3..14 {
            assert_eq!(tile(28, row), Tile::Solid, "east climb backing is open");
        }
        for column in 28..31 {
            assert_eq!(tile(column, 3), Tile::Solid, "Crown dais is incomplete");
        }
    }

    #[test]
    fn lunar_cache_seals_its_coin_behind_a_low_tunnel_and_cap_safe_shaft() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::LunarCache).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for row in 1..15 {
            assert_eq!(
                tile(21, row),
                Tile::Solid,
                "coin chamber has an upper separator bypass"
            );
        }
        for column in 18..24 {
            assert_eq!(tile(column, 15), Tile::Solid, "low tunnel lacks a ceiling");
            assert_eq!(tile(column, 16), Tile::Empty, "Dash aperture is obstructed");
            assert_eq!(tile(column, 17), Tile::Solid, "low tunnel lacks a floor");
        }
        for row in [4, 10] {
            assert_eq!(tile(22, row), Tile::HazardUp);
        }
        for row in [7, 13] {
            assert_eq!(tile(27, row), Tile::HazardUp);
        }
        for (column, rows) in [(22, [5..7, 11..13]), (27, [8..10, 14..17])] {
            for row in rows.into_iter().flatten() {
                assert_eq!(tile(column, row), Tile::Solid);
            }
        }
        for column in 27..31 {
            assert_eq!(tile(column, 3), Tile::Solid, "coin shelf is incomplete");
        }
    }

    #[test]
    fn moon_vault_has_a_blocked_shortcut_and_broad_orbit_recoveries() {
        let candidate = DungeonPaletteKey::new(0, DungeonPaletteCourse::MoonVault).generate();
        let tile = |column: usize, row: usize| candidate.tiles[row * usize::from(WIDTH) + column];

        for row in 5..12 {
            assert_eq!(
                tile(20, row),
                Tile::Solid,
                "Moon shortcut separator has a high bypass"
            );
        }
        for column in 18..26 {
            assert_eq!(tile(column, 3), Tile::HazardUp);
            assert_eq!(tile(column, 4), Tile::HazardDown);
        }
        for (row, range) in [(6, 13..18), (14, 13..16), (14, 22..25), (10, 22..25)] {
            for column in range {
                assert_eq!(
                    tile(column, row),
                    Tile::OneWay,
                    "Moon orbit recovery is incomplete"
                );
            }
        }
        for column in 1..31 {
            assert_eq!(tile(column, 16), Tile::HazardUp);
        }
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
