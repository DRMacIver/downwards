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

pub const DUNGEON_PALETTE_GENERATION_VERSION: u32 = 37;

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

/// One printable character per tile in the ASCII room-grid format.
const ROOM_GRID_LEGEND: [(char, Tile); 7] = [
    ('.', Tile::Empty),
    ('#', Tile::Solid),
    ('-', Tile::OneWay),
    ('^', Tile::HazardUp),
    ('v', Tile::HazardDown),
    ('<', Tile::HazardLeft),
    ('>', Tile::HazardRight),
];

/// Render a full room tile grid as newline-separated ASCII rows.
#[must_use]
pub fn render_room_grid(tiles: &[Tile]) -> String {
    assert_eq!(tiles.len(), usize::from(WIDTH) * usize::from(HEIGHT));
    let mut rendered = String::with_capacity(tiles.len() + usize::from(HEIGHT));
    for row in 0..usize::from(HEIGHT) {
        for column in 0..usize::from(WIDTH) {
            let tile = tiles[row * usize::from(WIDTH) + column];
            let (glyph, _) = ROOM_GRID_LEGEND
                .iter()
                .find(|(_, candidate)| *candidate == tile)
                .expect("every tile has a grid glyph");
            rendered.push(*glyph);
        }
        rendered.push('\n');
    }
    rendered
}

/// Parse an ASCII room grid rendered by [`render_room_grid`].
///
/// # Panics
///
/// Panics on wrong dimensions or an unknown glyph; grids are compiled-in
/// authored content, so a malformed grid is a build defect rather than a
/// runtime input error.
#[must_use]
pub fn parse_room_grid(source: &str) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity(usize::from(WIDTH) * usize::from(HEIGHT));
    let mut rows = 0;
    for (row_index, line) in source.lines().enumerate() {
        rows += 1;
        assert_eq!(
            line.chars().count(),
            usize::from(WIDTH),
            "room grid row {row_index} must have {WIDTH} columns"
        );
        for glyph in line.chars() {
            let (_, tile) = ROOM_GRID_LEGEND
                .iter()
                .find(|(candidate, _)| *candidate == glyph)
                .unwrap_or_else(|| panic!("unknown room grid glyph {glyph:?}"));
            tiles.push(*tile);
        }
    }
    assert_eq!(
        rows,
        usize::from(HEIGHT),
        "room grid must have {HEIGHT} rows"
    );
    tiles
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
        // Design-iteration override: when DOWNWARDS_ROOM_GRID_DIR is set,
        // grids are read from that directory at runtime so a tile edit can be
        // re-analysed with a prebuilt binary. Production and tests use the
        // compiled-in grids; never set the variable for persisted evidence.
        let disk_source = std::env::var("DOWNWARDS_ROOM_GRID_DIR")
            .ok()
            .and_then(|dir| std::fs::read_to_string(format!("{dir}/{}.txt", self.course.slug())).ok());
        let source = disk_source.as_deref().unwrap_or_else(|| {
            crate::room_grids::ROOM_GRIDS
                .iter()
                .find(|(slug, _)| *slug == self.course.slug())
                .unwrap_or_else(|| panic!("{:?} has no authored room grid", self.course))
                .1
        });
        let tiles = parse_room_grid(source);
        debug_assert!(
            one_way_surfaces_have_player_headroom(&tiles),
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

    #[must_use]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
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

fn one_way_surfaces_have_player_headroom(tiles: &[Tile]) -> bool {
    let clearance_rows = u16::try_from((PLAYER_HEIGHT + TILE_SIZE - 1) / TILE_SIZE)
        .expect("player clearance row count fits u16");
    (0..HEIGHT).all(|row| {
        (0..WIDTH).all(|column| {
            if tiles[usize::from(row) * usize::from(WIDTH) + usize::from(column)] != Tile::OneWay {
                return true;
            }
            row >= clearance_rows
                && (1..=clearance_rows).all(|offset| {
                    tiles[usize::from(row - offset) * usize::from(WIDTH) + usize::from(column)]
                        == Tile::Empty
                })
        })
    })
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
        // Wall-embedded up-spikes became solid when hazard sides turned
        // lethal; the walls' safe/spike rhythm lives in the side spikes.
        for row in [4, 10] {
            assert_eq!(tile(11, row), Tile::Solid);
        }
        for row in [7, 13] {
            assert_eq!(tile(16, row), Tile::Solid);
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
        // Wall-embedded up-spikes became solid when hazard sides turned
        // lethal; the walls' safe/spike rhythm lives in the side spikes.
        for row in [4, 10] {
            assert_eq!(tile(6, row), Tile::Solid);
        }
        for row in [7, 13] {
            assert_eq!(tile(11, row), Tile::Solid);
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
        // These wall segments were up-spikes when hazard sides were safe;
        // side lethality made wall-embedded spikes unslidable, so they are
        // solid now.
        for row in [4, 10] {
            assert_eq!(tile(13, row), Tile::Solid);
        }
        for row in [7, 13] {
            assert_eq!(tile(18, row), Tile::Solid);
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
        }
        for row in 1..13 {
            assert_eq!(tile(24, row), Tile::Solid, "last hanging pillar is open");
        }
        for row in 6..11 {
            assert_eq!(
                tile(13, row),
                Tile::Solid,
                "central climb lacks its opposing wall"
            );
        }
        for row in 7..17 {
            assert_eq!(tile(17, row), Tile::Solid, "central pillar is open");
        }
        for (row, range) in [(14, 10..14), (7, 17..22), (14, 19..22), (14, 28..30)] {
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
        // Wall-embedded up-spikes became solid when hazard sides turned
        // lethal; the walls' safe/spike rhythm lives in the side spikes.
        for row in [4, 10] {
            assert_eq!(tile(5, row), Tile::Solid);
        }
        for row in [7, 13] {
            assert_eq!(tile(9, row), Tile::Solid);
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
            assert!(
                one_way_surfaces_have_player_headroom(&candidate.tiles),
                "{course:?} contains a bridge with less than {PLAYER_HEIGHT}px headroom"
            );
        }
    }
}
