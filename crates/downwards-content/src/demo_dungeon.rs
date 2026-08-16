//! Hand-assembled multi-room vertical slice built from the deterministic dungeon palette.

use downwards_core::{AbilitySet, Exit, Pickup, Rect, Room, TimedHazard};
use downwards_gen::{DungeonPaletteConnection, DungeonPaletteCourse, DungeonPaletteKey};

use crate::{
    AUTHORED_DUNGEON_SCHEMA_VERSION, AuthoredConnection, AuthoredDoorRequirement,
    AuthoredDungeonDefinition, AuthoredDungeonInventory, AuthoredFloorDefinition, AuthoredFloorKey,
    TraversalMethod, TraversalMethods,
};

pub const DEMO_DUNGEON_START_ABILITIES: AbilitySet = AbilitySet::NONE;
pub const DEMO_DUNGEON_GLOVE_PICKUP: &str = "climbing-gloves";
pub const DEMO_DUNGEON_BOOT_PICKUP: &str = "winged-boots";
pub const DEMO_DUNGEON_CROWN_PICKUP: &str = "crown";
pub const DEMO_DUNGEON_GOAL_EXIT: &str = "crown-goal";
pub const DEMO_DUNGEON_TOTAL_COINS: u8 = 64;
pub const DEMO_DUNGEON_GLOVE_GATE_REQUIREMENT: u8 = 6;
pub const DEMO_DUNGEON_WALL_REGION_GATE_REQUIREMENT: u8 = 12;
/// Opens the lower route containing the last pre-Dash coin branches.
pub const DEMO_DUNGEON_LOWER_VAULT_REQUIREMENT: u8 = 18;
/// Opens the Treasury only after the Underpass coin has been collected.
pub const DEMO_DUNGEON_TREASURY_REQUIREMENT: u8 = 19;
/// Opens either entrance to the Winged Vault after every other pre-Dash coin.
pub const DEMO_DUNGEON_BOOT_GATE_REQUIREMENT: u8 = 21;
pub const DEMO_DUNGEON_DASH_REGION_GATE_REQUIREMENT: u8 = 28;
pub const DEMO_DUNGEON_FOUNDRY_GATE_REQUIREMENT: u8 = 40;
pub const DEMO_DUNGEON_GLASSWORKS_GATE_REQUIREMENT: u8 = 52;
pub const DEMO_DUNGEON_ASTRAL_GATE_REQUIREMENT: u8 = 64;
pub const DEMO_DUNGEON_CROWN_GATE_REQUIREMENT: u8 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DemoDungeonInventory {
    pub climbing_gloves: bool,
    pub winged_boots: bool,
    pub crown: bool,
    coin_mask: u128,
}

impl DemoDungeonInventory {
    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        AbilitySet::new(self.climbing_gloves, self.winged_boots)
    }

    #[must_use]
    pub const fn coin_count(self) -> u8 {
        self.coin_mask.count_ones() as u8
    }

    #[must_use]
    pub fn has_coin(self, id: &str) -> bool {
        coin_index(id).is_some_and(|index| self.coin_mask & (1_u128 << index) != 0)
    }

    pub fn collect_coin(&mut self, id: &str) -> bool {
        let Some(index) = coin_index(id) else {
            return false;
        };
        let bit = 1_u128 << index;
        let newly_collected = self.coin_mask & bit == 0;
        self.coin_mask |= bit;
        newly_collected
    }

    #[must_use]
    pub fn owns_persistent_pickup(self, id: &str) -> bool {
        (id == DEMO_DUNGEON_GLOVE_PICKUP && self.climbing_gloves)
            || (id == DEMO_DUNGEON_BOOT_PICKUP && self.winged_boots)
            || (id == DEMO_DUNGEON_CROWN_PICKUP && self.crown)
            || self.has_coin(id)
    }

    #[must_use]
    pub const fn with_coin_count_for_validation(count: u8) -> Self {
        let count = if count > DEMO_DUNGEON_TOTAL_COINS {
            DEMO_DUNGEON_TOTAL_COINS
        } else {
            count
        };
        Self {
            climbing_gloves: false,
            winged_boots: false,
            crown: false,
            coin_mask: if count == 128 {
                u128::MAX
            } else {
                (1_u128 << count) - 1
            },
        }
    }

    #[must_use]
    pub fn authored_progression_inventory(self) -> AuthoredDungeonInventory {
        let mut inventory = AuthoredDungeonInventory::new(TraversalMethods::NONE);
        for index in 0..DEMO_DUNGEON_TOTAL_COINS {
            if self.coin_mask & (1_u128 << index) != 0 {
                inventory.collect_coin(u16::from(index));
            }
        }
        if self.climbing_gloves {
            inventory.grant(TraversalMethod::WallJump);
        }
        if self.winged_boots {
            inventory.grant(TraversalMethod::Dash);
        }
        inventory
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DemoDungeonRoom {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoDungeonRouteTarget {
    Door(&'static str),
    Pickup(&'static str),
    GoalExit,
}

impl DemoDungeonRouteTarget {
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Door(_) => "door",
            Self::Pickup(_) => "pickup",
            Self::GoalExit => "exit",
        }
    }

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Door(id) | Self::Pickup(id) => id,
            Self::GoalExit => DEMO_DUNGEON_GOAL_EXIT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoDungeonRouteSpec {
    pub room: DemoDungeonRoom,
    pub entry_door: Option<&'static str>,
    pub target: DemoDungeonRouteTarget,
    pub inventory: DemoDungeonInventory,
}

impl DemoDungeonRouteSpec {
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.room.id()
    }
}

impl DemoDungeonRoom {
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
    pub const fn id(self) -> &'static str {
        match self {
            Self::HollowLanding => "demo-dungeon.hollow-landing",
            Self::MossWalk => "demo-dungeon.moss-walk",
            Self::SplitRoot => "demo-dungeon.split-root",
            Self::RootCellar => "demo-dungeon.root-cellar",
            Self::BrokenAqueduct => "demo-dungeon.broken-aqueduct",
            Self::OldLift => "demo-dungeon.old-lift",
            Self::LanternGallery => "demo-dungeon.lantern-gallery",
            Self::WatchPost => "demo-dungeon.watch-post",
            Self::Sluice => "demo-dungeon.sluice",
            Self::ClimberVault => "demo-dungeon.climber-vault",
            Self::WallAntechamber => "demo-dungeon.wall-antechamber",
            Self::BroadChimney => "demo-dungeon.broad-chimney",
            Self::BellSwitchback => "demo-dungeon.bell-switchback",
            Self::BellNiche => "demo-dungeon.bell-niche",
            Self::TempoHall => "demo-dungeon.tempo-hall",
            Self::SplitSpire => "demo-dungeon.split-spire",
            Self::RafterShrine => "demo-dungeon.rafter-shrine",
            Self::LandingChain => "demo-dungeon.landing-chain",
            Self::NeedleTurn => "demo-dungeon.needle-turn",
            Self::WallGate => "demo-dungeon.wall-gate",
            Self::Threshold => "demo-dungeon.threshold",
            Self::Crossroads => "demo-dungeon.crossroads",
            Self::WallGallery => "demo-dungeon.wall-gallery",
            Self::BootsVault => "demo-dungeon.boots-vault",
            Self::Underpass => "demo-dungeon.underpass",
            Self::DashChasm => "demo-dungeon.dash-chasm",
            Self::GaleLanding => "demo-dungeon.gale-landing",
            Self::LowPassage => "demo-dungeon.low-passage",
            Self::CurrentFork => "demo-dungeon.current-fork",
            Self::CoinDuct => "demo-dungeon.coin-duct",
            Self::PulseGallery => "demo-dungeon.pulse-gallery",
            Self::StormSplit => "demo-dungeon.storm-split",
            Self::StormCache => "demo-dungeon.storm-cache",
            Self::RelayChasm => "demo-dungeon.relay-chasm",
            Self::BrakeTower => "demo-dungeon.brake-tower",
            Self::DashSeal => "demo-dungeon.dash-seal",
            Self::AlloyThreshold => "demo-dungeon.alloy-threshold",
            Self::Windshaft => "demo-dungeon.windshaft",
            Self::SplitFurnace => "demo-dungeon.split-furnace",
            Self::EmberVault => "demo-dungeon.ember-vault",
            Self::GearGallery => "demo-dungeon.gear-gallery",
            Self::CrosswindChimney => "demo-dungeon.crosswind-chimney",
            Self::FoundryFork => "demo-dungeon.foundry-fork",
            Self::CoolingDuct => "demo-dungeon.cooling-duct",
            Self::HammerHall => "demo-dungeon.hammer-hall",
            Self::LiftShaft => "demo-dungeon.lift-shaft",
            Self::SparkNiche => "demo-dungeon.spark-niche",
            Self::RivetRun => "demo-dungeon.rivet-run",
            Self::BlastGallery => "demo-dungeon.blast-gallery",
            Self::PressureFork => "demo-dungeon.pressure-fork",
            Self::AshCache => "demo-dungeon.ash-cache",
            Self::VentSpire => "demo-dungeon.vent-spire",
            Self::PistonPass => "demo-dungeon.piston-pass",
            Self::CrucibleClimb => "demo-dungeon.crucible-climb",
            Self::CinderBridge => "demo-dungeon.cinder-bridge",
            Self::FoundrySeal => "demo-dungeon.foundry-seal",
            Self::GlassThreshold => "demo-dungeon.glass-threshold",
            Self::PrismRun => "demo-dungeon.prism-run",
            Self::SplitKiln => "demo-dungeon.split-kiln",
            Self::ShardVault => "demo-dungeon.shard-vault",
            Self::GlassGallery => "demo-dungeon.glass-gallery",
            Self::RefractionShaft => "demo-dungeon.refraction-shaft",
            Self::MirrorFork => "demo-dungeon.mirror-fork",
            Self::MirrorDuct => "demo-dungeon.mirror-duct",
            Self::TemperHall => "demo-dungeon.temper-hall",
            Self::FurnaceLift => "demo-dungeon.furnace-lift",
            Self::LensNiche => "demo-dungeon.lens-niche",
            Self::SliverRun => "demo-dungeon.sliver-run",
            Self::HotGlass => "demo-dungeon.hot-glass",
            Self::CulletFork => "demo-dungeon.cullet-fork",
            Self::CulletCache => "demo-dungeon.cullet-cache",
            Self::AnnealingSpire => "demo-dungeon.annealing-spire",
            Self::RazorPass => "demo-dungeon.razor-pass",
            Self::LatticeClimb => "demo-dungeon.lattice-climb",
            Self::CrystalBridge => "demo-dungeon.crystal-bridge",
            Self::GlassSeal => "demo-dungeon.glass-seal",
            Self::StarThreshold => "demo-dungeon.star-threshold",
            Self::CometRun => "demo-dungeon.comet-run",
            Self::OrbitFork => "demo-dungeon.orbit-fork",
            Self::MoonVault => "demo-dungeon.moon-vault",
            Self::ConstellationHall => "demo-dungeon.constellation-hall",
            Self::ZenithShaft => "demo-dungeon.zenith-shaft",
            Self::EclipseFork => "demo-dungeon.eclipse-fork",
            Self::ShadowDuct => "demo-dungeon.shadow-duct",
            Self::Observatory => "demo-dungeon.observatory",
            Self::GravityLift => "demo-dungeon.gravity-lift",
            Self::NovaNiche => "demo-dungeon.nova-niche",
            Self::MeteorRun => "demo-dungeon.meteor-run",
            Self::VacuumGallery => "demo-dungeon.vacuum-gallery",
            Self::TidalFork => "demo-dungeon.tidal-fork",
            Self::LunarCache => "demo-dungeon.lunar-cache",
            Self::AuroraSpire => "demo-dungeon.aurora-spire",
            Self::VoidPass => "demo-dungeon.void-pass",
            Self::StarwellClimb => "demo-dungeon.starwell-climb",
            Self::Skybridge => "demo-dungeon.skybridge",
            Self::AstralSeal => "demo-dungeon.astral-seal",
            Self::CoinLoft => "demo-dungeon.coin-loft",
            Self::NeedleRoom => "demo-dungeon.needle-room",
            Self::Treasury => "demo-dungeon.treasury",
            Self::Gatehouse => "demo-dungeon.gatehouse",
            Self::CrownSanctum => "demo-dungeon.crown-sanctum",
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::HollowLanding => "Hollow Landing",
            Self::MossWalk => "The Moss Walk",
            Self::SplitRoot => "The Split Root",
            Self::RootCellar => "Root Cellar",
            Self::BrokenAqueduct => "Broken Aqueduct",
            Self::OldLift => "The Old Lift",
            Self::LanternGallery => "Lantern Gallery",
            Self::WatchPost => "Sunken Watchpost",
            Self::Sluice => "The Dry Sluice",
            Self::ClimberVault => "Climber's Reliquary",
            Self::WallAntechamber => "Stone Lessons",
            Self::BroadChimney => "The Broad Chimney",
            Self::BellSwitchback => "Bell Switchback",
            Self::BellNiche => "The Bell Niche",
            Self::TempoHall => "Evening Measure",
            Self::SplitSpire => "The Split Spire",
            Self::RafterShrine => "Rafter Shrine",
            Self::LandingChain => "The Landing Chain",
            Self::NeedleTurn => "Needle Turn",
            Self::WallGate => "Climber's Gate",
            Self::Threshold => "Mosslit Threshold",
            Self::Crossroads => "Three-Way Hall",
            Self::WallGallery => "Climbers' Gallery",
            Self::BootsVault => "The Winged Vault",
            Self::Underpass => "Rootbound Underpass",
            Self::DashChasm => "Gale Chasm",
            Self::GaleLanding => "Gale Landing",
            Self::LowPassage => "The Low Passage",
            Self::CurrentFork => "The Forked Current",
            Self::CoinDuct => "The Coin Duct",
            Self::PulseGallery => "Pulse Gallery",
            Self::StormSplit => "The Storm Split",
            Self::StormCache => "Storm Cache",
            Self::RelayChasm => "Relay Chasm",
            Self::BrakeTower => "Brake Tower",
            Self::DashSeal => "The Dash Seal",
            Self::AlloyThreshold => "Alloy Threshold",
            Self::Windshaft => "The Windshaft",
            Self::SplitFurnace => "The Split Furnace",
            Self::EmberVault => "Ember Vault",
            Self::GearGallery => "Gear Gallery",
            Self::CrosswindChimney => "Crosswind Chimney",
            Self::FoundryFork => "The Foundry Fork",
            Self::CoolingDuct => "Cooling Duct",
            Self::HammerHall => "Hammer Hall",
            Self::LiftShaft => "The Lift Shaft",
            Self::SparkNiche => "Spark Niche",
            Self::RivetRun => "Rivet Run",
            Self::BlastGallery => "Blast Gallery",
            Self::PressureFork => "The Pressure Fork",
            Self::AshCache => "Ash Cache",
            Self::VentSpire => "Vent Spire",
            Self::PistonPass => "Piston Pass",
            Self::CrucibleClimb => "Crucible Climb",
            Self::CinderBridge => "Cinder Bridge",
            Self::FoundrySeal => "The Foundry Seal",
            Self::GlassThreshold => "Glass Threshold",
            Self::PrismRun => "Prism Run",
            Self::SplitKiln => "The Split Kiln",
            Self::ShardVault => "Shard Vault",
            Self::GlassGallery => "Glass Gallery",
            Self::RefractionShaft => "Refraction Shaft",
            Self::MirrorFork => "The Mirror Fork",
            Self::MirrorDuct => "Mirror Duct",
            Self::TemperHall => "Temper Hall",
            Self::FurnaceLift => "The Furnace Lift",
            Self::LensNiche => "Lens Niche",
            Self::SliverRun => "Sliver Run",
            Self::HotGlass => "Hot Glass",
            Self::CulletFork => "The Cullet Fork",
            Self::CulletCache => "Cullet Cache",
            Self::AnnealingSpire => "Annealing Spire",
            Self::RazorPass => "Razor Pass",
            Self::LatticeClimb => "Lattice Climb",
            Self::CrystalBridge => "Crystal Bridge",
            Self::GlassSeal => "The Glass Seal",
            Self::StarThreshold => "Star Threshold",
            Self::CometRun => "Comet Run",
            Self::OrbitFork => "The Orbit Fork",
            Self::MoonVault => "Moon Vault",
            Self::ConstellationHall => "Constellation Hall",
            Self::ZenithShaft => "Zenith Shaft",
            Self::EclipseFork => "The Eclipse Fork",
            Self::ShadowDuct => "Shadow Duct",
            Self::Observatory => "The Observatory",
            Self::GravityLift => "Gravity Lift",
            Self::NovaNiche => "Nova Niche",
            Self::MeteorRun => "Meteor Run",
            Self::VacuumGallery => "Vacuum Gallery",
            Self::TidalFork => "The Tidal Fork",
            Self::LunarCache => "Lunar Cache",
            Self::AuroraSpire => "Aurora Spire",
            Self::VoidPass => "Void Pass",
            Self::StarwellClimb => "Starwell Climb",
            Self::Skybridge => "The Skybridge",
            Self::AstralSeal => "The Astral Seal",
            Self::CoinLoft => "Rafter Mint",
            Self::NeedleRoom => "Needle Belfry",
            Self::Treasury => "The Deep Treasury",
            Self::Gatehouse => "The Crown Gate",
            Self::CrownSanctum => "The Empty Throne",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|room| room.id() == id)
    }

    #[must_use]
    pub const fn authored_key(self) -> AuthoredFloorKey {
        AuthoredFloorKey(match self {
            Self::HollowLanding => 0,
            Self::MossWalk => 1,
            Self::SplitRoot => 2,
            Self::RootCellar => 3,
            Self::BrokenAqueduct => 4,
            Self::OldLift => 5,
            Self::LanternGallery => 6,
            Self::WatchPost => 7,
            Self::Sluice => 8,
            Self::ClimberVault => 9,
            Self::WallAntechamber => 10,
            Self::BroadChimney => 11,
            Self::BellSwitchback => 12,
            Self::BellNiche => 13,
            Self::TempoHall => 14,
            Self::SplitSpire => 15,
            Self::RafterShrine => 16,
            Self::LandingChain => 17,
            Self::NeedleTurn => 18,
            Self::WallGate => 19,
            Self::Threshold => 20,
            Self::Crossroads => 21,
            Self::CoinLoft => 22,
            Self::WallGallery => 23,
            Self::NeedleRoom => 24,
            Self::BootsVault => 25,
            Self::Underpass => 26,
            Self::Treasury => 27,
            Self::DashChasm => 28,
            Self::GaleLanding => 29,
            Self::LowPassage => 30,
            Self::CurrentFork => 31,
            Self::CoinDuct => 32,
            Self::PulseGallery => 33,
            Self::StormSplit => 34,
            Self::StormCache => 35,
            Self::RelayChasm => 36,
            Self::BrakeTower => 37,
            Self::DashSeal => 38,
            Self::AlloyThreshold => 39,
            Self::Windshaft => 40,
            Self::SplitFurnace => 41,
            Self::EmberVault => 42,
            Self::GearGallery => 43,
            Self::CrosswindChimney => 44,
            Self::FoundryFork => 45,
            Self::CoolingDuct => 46,
            Self::HammerHall => 47,
            Self::LiftShaft => 48,
            Self::SparkNiche => 49,
            Self::RivetRun => 50,
            Self::BlastGallery => 51,
            Self::PressureFork => 52,
            Self::AshCache => 53,
            Self::VentSpire => 54,
            Self::PistonPass => 55,
            Self::CrucibleClimb => 56,
            Self::CinderBridge => 57,
            Self::FoundrySeal => 58,
            Self::GlassThreshold => 59,
            Self::PrismRun => 60,
            Self::SplitKiln => 61,
            Self::ShardVault => 62,
            Self::GlassGallery => 63,
            Self::RefractionShaft => 64,
            Self::MirrorFork => 65,
            Self::MirrorDuct => 66,
            Self::TemperHall => 67,
            Self::FurnaceLift => 68,
            Self::LensNiche => 69,
            Self::SliverRun => 70,
            Self::HotGlass => 71,
            Self::CulletFork => 72,
            Self::CulletCache => 73,
            Self::AnnealingSpire => 74,
            Self::RazorPass => 75,
            Self::LatticeClimb => 76,
            Self::CrystalBridge => 77,
            Self::GlassSeal => 78,
            Self::StarThreshold => 79,
            Self::CometRun => 80,
            Self::OrbitFork => 81,
            Self::MoonVault => 82,
            Self::ConstellationHall => 83,
            Self::ZenithShaft => 84,
            Self::EclipseFork => 85,
            Self::ShadowDuct => 86,
            Self::Observatory => 87,
            Self::GravityLift => 88,
            Self::NovaNiche => 89,
            Self::MeteorRun => 90,
            Self::VacuumGallery => 91,
            Self::TidalFork => 92,
            Self::LunarCache => 93,
            Self::AuroraSpire => 94,
            Self::VoidPass => 95,
            Self::StarwellClimb => 96,
            Self::Skybridge => 97,
            Self::AstralSeal => 98,
            Self::Gatehouse => 99,
            Self::CrownSanctum => 100,
        })
    }

    const fn course(self) -> DungeonPaletteCourse {
        match self {
            Self::HollowLanding => DungeonPaletteCourse::HollowLanding,
            Self::MossWalk => DungeonPaletteCourse::MossWalk,
            Self::SplitRoot => DungeonPaletteCourse::SplitRoot,
            Self::RootCellar => DungeonPaletteCourse::RootCellar,
            Self::BrokenAqueduct => DungeonPaletteCourse::BrokenAqueduct,
            Self::OldLift => DungeonPaletteCourse::OldLift,
            Self::LanternGallery => DungeonPaletteCourse::LanternGallery,
            Self::WatchPost => DungeonPaletteCourse::WatchPost,
            Self::Sluice => DungeonPaletteCourse::Sluice,
            Self::ClimberVault => DungeonPaletteCourse::ClimberVault,
            Self::WallAntechamber => DungeonPaletteCourse::WallAntechamber,
            Self::BroadChimney => DungeonPaletteCourse::BroadChimney,
            Self::BellSwitchback => DungeonPaletteCourse::BellSwitchback,
            Self::BellNiche => DungeonPaletteCourse::BellNiche,
            Self::TempoHall => DungeonPaletteCourse::TempoHall,
            Self::SplitSpire => DungeonPaletteCourse::SplitSpire,
            Self::RafterShrine => DungeonPaletteCourse::RafterShrine,
            Self::LandingChain => DungeonPaletteCourse::LandingChain,
            Self::NeedleTurn => DungeonPaletteCourse::NeedleTurn,
            Self::WallGate => DungeonPaletteCourse::WallGate,
            Self::Threshold => DungeonPaletteCourse::Threshold,
            Self::Crossroads => DungeonPaletteCourse::Crossroads,
            Self::WallGallery => DungeonPaletteCourse::WallGallery,
            Self::BootsVault => DungeonPaletteCourse::BootsVault,
            Self::Underpass => DungeonPaletteCourse::Underpass,
            Self::DashChasm => DungeonPaletteCourse::DashChasm,
            Self::GaleLanding => DungeonPaletteCourse::GaleLanding,
            Self::LowPassage => DungeonPaletteCourse::LowPassage,
            Self::CurrentFork => DungeonPaletteCourse::CurrentFork,
            Self::CoinDuct => DungeonPaletteCourse::CoinDuct,
            Self::PulseGallery => DungeonPaletteCourse::PulseGallery,
            Self::StormSplit => DungeonPaletteCourse::StormSplit,
            Self::StormCache => DungeonPaletteCourse::StormCache,
            Self::RelayChasm => DungeonPaletteCourse::RelayChasm,
            Self::BrakeTower => DungeonPaletteCourse::BrakeTower,
            Self::DashSeal => DungeonPaletteCourse::DashSeal,
            Self::AlloyThreshold => DungeonPaletteCourse::AlloyThreshold,
            Self::Windshaft => DungeonPaletteCourse::Windshaft,
            Self::SplitFurnace => DungeonPaletteCourse::SplitFurnace,
            Self::EmberVault => DungeonPaletteCourse::EmberVault,
            Self::GearGallery => DungeonPaletteCourse::GearGallery,
            Self::CrosswindChimney => DungeonPaletteCourse::CrosswindChimney,
            Self::FoundryFork => DungeonPaletteCourse::FoundryFork,
            Self::CoolingDuct => DungeonPaletteCourse::CoolingDuct,
            Self::HammerHall => DungeonPaletteCourse::HammerHall,
            Self::LiftShaft => DungeonPaletteCourse::LiftShaft,
            Self::SparkNiche => DungeonPaletteCourse::SparkNiche,
            Self::RivetRun => DungeonPaletteCourse::RivetRun,
            Self::BlastGallery => DungeonPaletteCourse::BlastGallery,
            Self::PressureFork => DungeonPaletteCourse::PressureFork,
            Self::AshCache => DungeonPaletteCourse::AshCache,
            Self::VentSpire => DungeonPaletteCourse::VentSpire,
            Self::PistonPass => DungeonPaletteCourse::PistonPass,
            Self::CrucibleClimb => DungeonPaletteCourse::CrucibleClimb,
            Self::CinderBridge => DungeonPaletteCourse::CinderBridge,
            Self::FoundrySeal => DungeonPaletteCourse::FoundrySeal,
            Self::GlassThreshold => DungeonPaletteCourse::GlassThreshold,
            Self::PrismRun => DungeonPaletteCourse::PrismRun,
            Self::SplitKiln => DungeonPaletteCourse::SplitKiln,
            Self::ShardVault => DungeonPaletteCourse::ShardVault,
            Self::GlassGallery => DungeonPaletteCourse::GlassGallery,
            Self::RefractionShaft => DungeonPaletteCourse::RefractionShaft,
            Self::MirrorFork => DungeonPaletteCourse::MirrorFork,
            Self::MirrorDuct => DungeonPaletteCourse::MirrorDuct,
            Self::TemperHall => DungeonPaletteCourse::TemperHall,
            Self::FurnaceLift => DungeonPaletteCourse::FurnaceLift,
            Self::LensNiche => DungeonPaletteCourse::LensNiche,
            Self::SliverRun => DungeonPaletteCourse::SliverRun,
            Self::HotGlass => DungeonPaletteCourse::HotGlass,
            Self::CulletFork => DungeonPaletteCourse::CulletFork,
            Self::CulletCache => DungeonPaletteCourse::CulletCache,
            Self::AnnealingSpire => DungeonPaletteCourse::AnnealingSpire,
            Self::RazorPass => DungeonPaletteCourse::RazorPass,
            Self::LatticeClimb => DungeonPaletteCourse::LatticeClimb,
            Self::CrystalBridge => DungeonPaletteCourse::CrystalBridge,
            Self::GlassSeal => DungeonPaletteCourse::GlassSeal,
            Self::StarThreshold => DungeonPaletteCourse::StarThreshold,
            Self::CometRun => DungeonPaletteCourse::CometRun,
            Self::OrbitFork => DungeonPaletteCourse::OrbitFork,
            Self::MoonVault => DungeonPaletteCourse::MoonVault,
            Self::ConstellationHall => DungeonPaletteCourse::ConstellationHall,
            Self::ZenithShaft => DungeonPaletteCourse::ZenithShaft,
            Self::EclipseFork => DungeonPaletteCourse::EclipseFork,
            Self::ShadowDuct => DungeonPaletteCourse::ShadowDuct,
            Self::Observatory => DungeonPaletteCourse::Observatory,
            Self::GravityLift => DungeonPaletteCourse::GravityLift,
            Self::NovaNiche => DungeonPaletteCourse::NovaNiche,
            Self::MeteorRun => DungeonPaletteCourse::MeteorRun,
            Self::VacuumGallery => DungeonPaletteCourse::VacuumGallery,
            Self::TidalFork => DungeonPaletteCourse::TidalFork,
            Self::LunarCache => DungeonPaletteCourse::LunarCache,
            Self::AuroraSpire => DungeonPaletteCourse::AuroraSpire,
            Self::VoidPass => DungeonPaletteCourse::VoidPass,
            Self::StarwellClimb => DungeonPaletteCourse::StarwellClimb,
            Self::Skybridge => DungeonPaletteCourse::Skybridge,
            Self::AstralSeal => DungeonPaletteCourse::AstralSeal,
            Self::CoinLoft => DungeonPaletteCourse::CoinLoft,
            Self::NeedleRoom => DungeonPaletteCourse::NeedleRoom,
            Self::Treasury => DungeonPaletteCourse::Treasury,
            Self::Gatehouse => DungeonPaletteCourse::Gatehouse,
            Self::CrownSanctum => DungeonPaletteCourse::CrownSanctum,
        }
    }

    fn connections(self) -> Vec<DungeonPaletteConnection> {
        let connection = |door: &str, room: Self, destination_door: &str| {
            DungeonPaletteConnection::new(door, room.id(), destination_door)
        };
        match self {
            Self::HollowLanding => vec![connection("east", Self::MossWalk, "west")],
            Self::MossWalk => vec![
                connection("west", Self::HollowLanding, "east"),
                connection("east", Self::SplitRoot, "west"),
            ],
            Self::SplitRoot => vec![
                connection("west", Self::MossWalk, "east"),
                connection("east", Self::BrokenAqueduct, "west"),
                connection("floor", Self::RootCellar, "ceiling"),
            ],
            Self::RootCellar => vec![connection("ceiling", Self::SplitRoot, "floor")],
            Self::BrokenAqueduct => vec![
                connection("west", Self::SplitRoot, "east"),
                connection("east", Self::OldLift, "west"),
            ],
            Self::OldLift => vec![
                connection("west", Self::BrokenAqueduct, "east"),
                connection("east", Self::LanternGallery, "west"),
            ],
            Self::LanternGallery => vec![
                connection("west", Self::OldLift, "east"),
                connection("east", Self::Sluice, "west"),
                connection("ceiling", Self::WatchPost, "floor"),
            ],
            Self::WatchPost => vec![connection("floor", Self::LanternGallery, "ceiling")],
            Self::Sluice => vec![
                connection("west", Self::LanternGallery, "east"),
                connection("east", Self::ClimberVault, "west"),
            ],
            Self::ClimberVault => vec![
                connection("west", Self::Sluice, "east"),
                connection("east", Self::WallAntechamber, "west"),
            ],
            Self::WallAntechamber => vec![
                connection("west", Self::ClimberVault, "east"),
                connection("east", Self::BroadChimney, "west"),
            ],
            Self::BroadChimney => vec![
                connection("west", Self::WallAntechamber, "east"),
                connection("east", Self::BellSwitchback, "west"),
            ],
            Self::BellSwitchback => vec![
                connection("west", Self::BroadChimney, "east"),
                connection("east", Self::TempoHall, "west"),
                connection("floor", Self::BellNiche, "ceiling"),
            ],
            Self::BellNiche => vec![connection("ceiling", Self::BellSwitchback, "floor")],
            Self::TempoHall => vec![
                connection("west", Self::BellSwitchback, "east"),
                connection("east", Self::SplitSpire, "west"),
            ],
            Self::SplitSpire => vec![
                connection("west", Self::TempoHall, "east"),
                connection("east", Self::LandingChain, "west"),
                connection("ceiling", Self::RafterShrine, "floor"),
            ],
            Self::RafterShrine => vec![connection("floor", Self::SplitSpire, "ceiling")],
            Self::LandingChain => vec![
                connection("west", Self::SplitSpire, "east"),
                connection("east", Self::NeedleTurn, "west"),
            ],
            Self::NeedleTurn => vec![
                connection("west", Self::LandingChain, "east"),
                connection("east", Self::WallGate, "west"),
            ],
            Self::WallGate => vec![
                connection("west", Self::NeedleTurn, "east"),
                connection("east", Self::Threshold, "west"),
            ],
            Self::Threshold => vec![
                connection("west", Self::WallGate, "east"),
                connection("east", Self::Crossroads, "west"),
            ],
            Self::Crossroads => vec![
                connection("west", Self::Threshold, "east"),
                connection("east", Self::WallGallery, "west"),
                connection("ceiling", Self::BootsVault, "floor"),
                connection("floor", Self::CoinLoft, "ceiling"),
            ],
            Self::WallGallery => vec![
                connection("west", Self::Crossroads, "east"),
                connection("east", Self::DashChasm, "west"),
                connection("ceiling", Self::NeedleRoom, "floor"),
                connection("floor", Self::Underpass, "ceiling"),
            ],
            Self::BootsVault => vec![
                connection("floor", Self::Crossroads, "ceiling"),
                connection("east", Self::Underpass, "west"),
            ],
            Self::Underpass => vec![
                connection("west", Self::BootsVault, "east"),
                connection("east", Self::Treasury, "west"),
                connection("ceiling", Self::WallGallery, "floor"),
            ],
            Self::DashChasm => vec![
                connection("west", Self::WallGallery, "east"),
                connection("east", Self::GaleLanding, "west"),
            ],
            Self::GaleLanding => vec![
                connection("west", Self::DashChasm, "east"),
                connection("east", Self::LowPassage, "west"),
            ],
            Self::LowPassage => vec![
                connection("west", Self::GaleLanding, "east"),
                connection("east", Self::CurrentFork, "west"),
            ],
            Self::CurrentFork => vec![
                connection("west", Self::LowPassage, "east"),
                connection("east", Self::PulseGallery, "west"),
                connection("floor", Self::CoinDuct, "ceiling"),
            ],
            Self::CoinDuct => vec![connection("ceiling", Self::CurrentFork, "floor")],
            Self::PulseGallery => vec![
                connection("west", Self::CurrentFork, "east"),
                connection("east", Self::StormSplit, "west"),
            ],
            Self::StormSplit => vec![
                connection("west", Self::PulseGallery, "east"),
                connection("east", Self::RelayChasm, "west"),
                connection("ceiling", Self::StormCache, "floor"),
            ],
            Self::StormCache => vec![connection("floor", Self::StormSplit, "ceiling")],
            Self::RelayChasm => vec![
                connection("west", Self::StormSplit, "east"),
                connection("east", Self::BrakeTower, "west"),
            ],
            Self::BrakeTower => vec![
                connection("west", Self::RelayChasm, "east"),
                connection("east", Self::DashSeal, "west"),
            ],
            Self::DashSeal => vec![
                connection("west", Self::BrakeTower, "east"),
                connection("east", Self::AlloyThreshold, "west"),
            ],
            Self::AlloyThreshold => vec![
                connection("west", Self::DashSeal, "east"),
                connection("east", Self::Windshaft, "west"),
            ],
            Self::Windshaft => vec![
                connection("west", Self::AlloyThreshold, "east"),
                connection("east", Self::SplitFurnace, "west"),
            ],
            Self::SplitFurnace => vec![
                connection("west", Self::Windshaft, "east"),
                connection("east", Self::GearGallery, "west"),
                connection("floor", Self::EmberVault, "ceiling"),
            ],
            Self::EmberVault => vec![connection("ceiling", Self::SplitFurnace, "floor")],
            Self::GearGallery => vec![
                connection("west", Self::SplitFurnace, "east"),
                connection("east", Self::CrosswindChimney, "west"),
            ],
            Self::CrosswindChimney => vec![
                connection("west", Self::GearGallery, "east"),
                connection("east", Self::FoundryFork, "west"),
            ],
            Self::FoundryFork => vec![
                connection("west", Self::CrosswindChimney, "east"),
                connection("east", Self::HammerHall, "west"),
                connection("ceiling", Self::CoolingDuct, "floor"),
            ],
            Self::CoolingDuct => vec![connection("floor", Self::FoundryFork, "ceiling")],
            Self::HammerHall => vec![
                connection("west", Self::FoundryFork, "east"),
                connection("east", Self::LiftShaft, "west"),
            ],
            Self::LiftShaft => vec![
                connection("west", Self::HammerHall, "east"),
                connection("east", Self::RivetRun, "west"),
                connection("ceiling", Self::SparkNiche, "floor"),
            ],
            Self::SparkNiche => vec![connection("floor", Self::LiftShaft, "ceiling")],
            Self::RivetRun => vec![
                connection("west", Self::LiftShaft, "east"),
                connection("east", Self::BlastGallery, "west"),
            ],
            Self::BlastGallery => vec![
                connection("west", Self::RivetRun, "east"),
                connection("east", Self::PressureFork, "west"),
            ],
            Self::PressureFork => vec![
                connection("west", Self::BlastGallery, "east"),
                connection("east", Self::VentSpire, "west"),
                connection("floor", Self::AshCache, "ceiling"),
            ],
            Self::AshCache => vec![connection("ceiling", Self::PressureFork, "floor")],
            Self::VentSpire => vec![
                connection("west", Self::PressureFork, "east"),
                connection("east", Self::PistonPass, "west"),
            ],
            Self::PistonPass => vec![
                connection("west", Self::VentSpire, "east"),
                connection("east", Self::CrucibleClimb, "west"),
            ],
            Self::CrucibleClimb => vec![
                connection("west", Self::PistonPass, "east"),
                connection("east", Self::CinderBridge, "west"),
            ],
            Self::CinderBridge => vec![
                connection("west", Self::CrucibleClimb, "east"),
                connection("east", Self::FoundrySeal, "west"),
            ],
            Self::FoundrySeal => vec![
                connection("west", Self::CinderBridge, "east"),
                connection("east", Self::GlassThreshold, "west"),
            ],
            Self::GlassThreshold => vec![
                connection("west", Self::FoundrySeal, "east"),
                connection("east", Self::PrismRun, "west"),
            ],
            Self::PrismRun => vec![
                connection("west", Self::GlassThreshold, "east"),
                connection("east", Self::SplitKiln, "west"),
            ],
            Self::SplitKiln => vec![
                connection("west", Self::PrismRun, "east"),
                connection("east", Self::GlassGallery, "west"),
                connection("floor", Self::ShardVault, "ceiling"),
            ],
            Self::ShardVault => vec![connection("ceiling", Self::SplitKiln, "floor")],
            Self::GlassGallery => vec![
                connection("west", Self::SplitKiln, "east"),
                connection("east", Self::RefractionShaft, "west"),
            ],
            Self::RefractionShaft => vec![
                connection("west", Self::GlassGallery, "east"),
                connection("east", Self::MirrorFork, "west"),
            ],
            Self::MirrorFork => vec![
                connection("west", Self::RefractionShaft, "east"),
                connection("east", Self::TemperHall, "west"),
                connection("ceiling", Self::MirrorDuct, "floor"),
            ],
            Self::MirrorDuct => vec![connection("floor", Self::MirrorFork, "ceiling")],
            Self::TemperHall => vec![
                connection("west", Self::MirrorFork, "east"),
                connection("east", Self::FurnaceLift, "west"),
            ],
            Self::FurnaceLift => vec![
                connection("west", Self::TemperHall, "east"),
                connection("east", Self::SliverRun, "west"),
                connection("ceiling", Self::LensNiche, "floor"),
            ],
            Self::LensNiche => vec![connection("floor", Self::FurnaceLift, "ceiling")],
            Self::SliverRun => vec![
                connection("west", Self::FurnaceLift, "east"),
                connection("east", Self::HotGlass, "west"),
            ],
            Self::HotGlass => vec![
                connection("west", Self::SliverRun, "east"),
                connection("east", Self::CulletFork, "west"),
            ],
            Self::CulletFork => vec![
                connection("west", Self::HotGlass, "east"),
                connection("east", Self::AnnealingSpire, "west"),
                connection("floor", Self::CulletCache, "ceiling"),
            ],
            Self::CulletCache => vec![connection("ceiling", Self::CulletFork, "floor")],
            Self::AnnealingSpire => vec![
                connection("west", Self::CulletFork, "east"),
                connection("east", Self::RazorPass, "west"),
            ],
            Self::RazorPass => vec![
                connection("west", Self::AnnealingSpire, "east"),
                connection("east", Self::LatticeClimb, "west"),
            ],
            Self::LatticeClimb => vec![
                connection("west", Self::RazorPass, "east"),
                connection("east", Self::CrystalBridge, "west"),
            ],
            Self::CrystalBridge => vec![
                connection("west", Self::LatticeClimb, "east"),
                connection("east", Self::GlassSeal, "west"),
            ],
            Self::GlassSeal => vec![
                connection("west", Self::CrystalBridge, "east"),
                connection("east", Self::StarThreshold, "west"),
            ],
            Self::StarThreshold => vec![
                connection("west", Self::GlassSeal, "east"),
                connection("east", Self::CometRun, "west"),
            ],
            Self::CometRun => vec![
                connection("west", Self::StarThreshold, "east"),
                connection("east", Self::OrbitFork, "west"),
            ],
            Self::OrbitFork => vec![
                connection("west", Self::CometRun, "east"),
                connection("east", Self::ConstellationHall, "west"),
                connection("floor", Self::MoonVault, "ceiling"),
            ],
            Self::MoonVault => vec![connection("ceiling", Self::OrbitFork, "floor")],
            Self::ConstellationHall => vec![
                connection("west", Self::OrbitFork, "east"),
                connection("east", Self::ZenithShaft, "west"),
            ],
            Self::ZenithShaft => vec![
                connection("west", Self::ConstellationHall, "east"),
                connection("east", Self::EclipseFork, "west"),
            ],
            Self::EclipseFork => vec![
                connection("west", Self::ZenithShaft, "east"),
                connection("east", Self::Observatory, "west"),
                connection("ceiling", Self::ShadowDuct, "floor"),
            ],
            Self::ShadowDuct => vec![connection("floor", Self::EclipseFork, "ceiling")],
            Self::Observatory => vec![
                connection("west", Self::EclipseFork, "east"),
                connection("east", Self::GravityLift, "west"),
            ],
            Self::GravityLift => vec![
                connection("west", Self::Observatory, "east"),
                connection("east", Self::MeteorRun, "west"),
                connection("ceiling", Self::NovaNiche, "floor"),
            ],
            Self::NovaNiche => vec![connection("floor", Self::GravityLift, "ceiling")],
            Self::MeteorRun => vec![
                connection("west", Self::GravityLift, "east"),
                connection("east", Self::VacuumGallery, "west"),
            ],
            Self::VacuumGallery => vec![
                connection("west", Self::MeteorRun, "east"),
                connection("east", Self::TidalFork, "west"),
            ],
            Self::TidalFork => vec![
                connection("west", Self::VacuumGallery, "east"),
                connection("east", Self::AuroraSpire, "west"),
                connection("floor", Self::LunarCache, "ceiling"),
            ],
            Self::LunarCache => vec![connection("ceiling", Self::TidalFork, "floor")],
            Self::AuroraSpire => vec![
                connection("west", Self::TidalFork, "east"),
                connection("east", Self::VoidPass, "west"),
            ],
            Self::VoidPass => vec![
                connection("west", Self::AuroraSpire, "east"),
                connection("east", Self::StarwellClimb, "west"),
            ],
            Self::StarwellClimb => vec![
                connection("west", Self::VoidPass, "east"),
                connection("east", Self::Skybridge, "west"),
            ],
            Self::Skybridge => vec![
                connection("west", Self::StarwellClimb, "east"),
                connection("east", Self::AstralSeal, "west"),
            ],
            Self::AstralSeal => vec![
                connection("west", Self::Skybridge, "east"),
                connection("east", Self::Gatehouse, "west"),
            ],
            Self::CoinLoft => vec![connection("ceiling", Self::Crossroads, "floor")],
            Self::NeedleRoom => vec![connection("floor", Self::WallGallery, "ceiling")],
            Self::Treasury => vec![connection("west", Self::Underpass, "east")],
            Self::Gatehouse => vec![
                connection("west", Self::AstralSeal, "east"),
                connection("east", Self::CrownSanctum, "west"),
            ],
            Self::CrownSanctum => vec![connection("west", Self::Gatehouse, "east")],
        }
    }
}

/// One mechanically regenerated representative route for every authored floor.
///
/// This is content metadata, not a difficulty ordering. The generated witness artifact binds an
/// exact replay to each coordinate and tests replay it under the current movement policy.
#[must_use]
pub fn demo_dungeon_route_specs() -> [DemoDungeonRouteSpec; 101] {
    const EMPTY: DemoDungeonInventory = DemoDungeonInventory {
        climbing_gloves: false,
        winged_boots: false,
        crown: false,
        coin_mask: 0,
    };
    const WALL: DemoDungeonInventory = DemoDungeonInventory {
        climbing_gloves: true,
        winged_boots: false,
        crown: false,
        coin_mask: 0,
    };
    const BOTH: DemoDungeonInventory = DemoDungeonInventory {
        climbing_gloves: true,
        winged_boots: true,
        crown: false,
        coin_mask: 0,
    };

    DemoDungeonRoom::ALL.map(|room| {
        let inventory = match room {
            DemoDungeonRoom::HollowLanding
            | DemoDungeonRoom::MossWalk
            | DemoDungeonRoom::SplitRoot
            | DemoDungeonRoom::RootCellar
            | DemoDungeonRoom::BrokenAqueduct
            | DemoDungeonRoom::OldLift
            | DemoDungeonRoom::LanternGallery
            | DemoDungeonRoom::WatchPost
            | DemoDungeonRoom::Sluice
            | DemoDungeonRoom::ClimberVault => EMPTY,
            DemoDungeonRoom::WallAntechamber
            | DemoDungeonRoom::BroadChimney
            | DemoDungeonRoom::BellSwitchback
            | DemoDungeonRoom::BellNiche
            | DemoDungeonRoom::TempoHall
            | DemoDungeonRoom::SplitSpire
            | DemoDungeonRoom::RafterShrine
            | DemoDungeonRoom::LandingChain
            | DemoDungeonRoom::NeedleTurn
            | DemoDungeonRoom::WallGate
            | DemoDungeonRoom::Threshold
            | DemoDungeonRoom::Crossroads
            | DemoDungeonRoom::WallGallery
            | DemoDungeonRoom::BootsVault
            | DemoDungeonRoom::CoinLoft
            | DemoDungeonRoom::NeedleRoom => WALL,
            DemoDungeonRoom::Underpass
            | DemoDungeonRoom::DashChasm
            | DemoDungeonRoom::GaleLanding
            | DemoDungeonRoom::LowPassage
            | DemoDungeonRoom::CurrentFork
            | DemoDungeonRoom::CoinDuct
            | DemoDungeonRoom::PulseGallery
            | DemoDungeonRoom::StormSplit
            | DemoDungeonRoom::StormCache
            | DemoDungeonRoom::RelayChasm
            | DemoDungeonRoom::BrakeTower
            | DemoDungeonRoom::DashSeal
            | DemoDungeonRoom::AlloyThreshold
            | DemoDungeonRoom::Windshaft
            | DemoDungeonRoom::SplitFurnace
            | DemoDungeonRoom::EmberVault
            | DemoDungeonRoom::GearGallery
            | DemoDungeonRoom::CrosswindChimney
            | DemoDungeonRoom::FoundryFork
            | DemoDungeonRoom::CoolingDuct
            | DemoDungeonRoom::HammerHall
            | DemoDungeonRoom::LiftShaft
            | DemoDungeonRoom::SparkNiche
            | DemoDungeonRoom::RivetRun
            | DemoDungeonRoom::BlastGallery
            | DemoDungeonRoom::PressureFork
            | DemoDungeonRoom::AshCache
            | DemoDungeonRoom::VentSpire
            | DemoDungeonRoom::PistonPass
            | DemoDungeonRoom::CrucibleClimb
            | DemoDungeonRoom::CinderBridge
            | DemoDungeonRoom::FoundrySeal
            | DemoDungeonRoom::GlassThreshold
            | DemoDungeonRoom::PrismRun
            | DemoDungeonRoom::SplitKiln
            | DemoDungeonRoom::ShardVault
            | DemoDungeonRoom::GlassGallery
            | DemoDungeonRoom::RefractionShaft
            | DemoDungeonRoom::MirrorFork
            | DemoDungeonRoom::MirrorDuct
            | DemoDungeonRoom::TemperHall
            | DemoDungeonRoom::FurnaceLift
            | DemoDungeonRoom::LensNiche
            | DemoDungeonRoom::SliverRun
            | DemoDungeonRoom::HotGlass
            | DemoDungeonRoom::CulletFork
            | DemoDungeonRoom::CulletCache
            | DemoDungeonRoom::AnnealingSpire
            | DemoDungeonRoom::RazorPass
            | DemoDungeonRoom::LatticeClimb
            | DemoDungeonRoom::CrystalBridge
            | DemoDungeonRoom::GlassSeal
            | DemoDungeonRoom::StarThreshold
            | DemoDungeonRoom::CometRun
            | DemoDungeonRoom::OrbitFork
            | DemoDungeonRoom::MoonVault
            | DemoDungeonRoom::ConstellationHall
            | DemoDungeonRoom::ZenithShaft
            | DemoDungeonRoom::EclipseFork
            | DemoDungeonRoom::ShadowDuct
            | DemoDungeonRoom::Observatory
            | DemoDungeonRoom::GravityLift
            | DemoDungeonRoom::NovaNiche
            | DemoDungeonRoom::MeteorRun
            | DemoDungeonRoom::VacuumGallery
            | DemoDungeonRoom::TidalFork
            | DemoDungeonRoom::LunarCache
            | DemoDungeonRoom::AuroraSpire
            | DemoDungeonRoom::VoidPass
            | DemoDungeonRoom::StarwellClimb
            | DemoDungeonRoom::Skybridge
            | DemoDungeonRoom::AstralSeal
            | DemoDungeonRoom::Treasury
            | DemoDungeonRoom::Gatehouse
            | DemoDungeonRoom::CrownSanctum => BOTH,
        };
        let entry_door = match room {
            DemoDungeonRoom::HollowLanding => None,
            DemoDungeonRoom::RootCellar
            | DemoDungeonRoom::BellNiche
            | DemoDungeonRoom::CoinLoft
            | DemoDungeonRoom::CoinDuct => Some("ceiling"),
            DemoDungeonRoom::WatchPost
            | DemoDungeonRoom::RafterShrine
            | DemoDungeonRoom::BootsVault
            | DemoDungeonRoom::NeedleRoom
            | DemoDungeonRoom::StormCache
            | DemoDungeonRoom::CoolingDuct
            | DemoDungeonRoom::SparkNiche => Some("floor"),
            DemoDungeonRoom::MirrorDuct
            | DemoDungeonRoom::LensNiche
            | DemoDungeonRoom::ShadowDuct
            | DemoDungeonRoom::NovaNiche => Some("floor"),
            DemoDungeonRoom::EmberVault
            | DemoDungeonRoom::AshCache
            | DemoDungeonRoom::ShardVault
            | DemoDungeonRoom::CulletCache
            | DemoDungeonRoom::MoonVault
            | DemoDungeonRoom::LunarCache => Some("ceiling"),
            _ => Some("west"),
        };
        let target = match room {
            DemoDungeonRoom::HollowLanding
            | DemoDungeonRoom::MossWalk
            | DemoDungeonRoom::BrokenAqueduct
            | DemoDungeonRoom::OldLift
            | DemoDungeonRoom::Sluice
            | DemoDungeonRoom::LandingChain
            | DemoDungeonRoom::WallGate
            | DemoDungeonRoom::Threshold
            | DemoDungeonRoom::DashChasm
            | DemoDungeonRoom::RelayChasm
            | DemoDungeonRoom::DashSeal
            | DemoDungeonRoom::CrosswindChimney
            | DemoDungeonRoom::RivetRun
            | DemoDungeonRoom::PistonPass
            | DemoDungeonRoom::CrucibleClimb
            | DemoDungeonRoom::RefractionShaft
            | DemoDungeonRoom::SliverRun
            | DemoDungeonRoom::RazorPass
            | DemoDungeonRoom::LatticeClimb
            | DemoDungeonRoom::ZenithShaft
            | DemoDungeonRoom::MeteorRun
            | DemoDungeonRoom::VoidPass
            | DemoDungeonRoom::StarwellClimb
            | DemoDungeonRoom::Gatehouse => DemoDungeonRouteTarget::Door("east"),
            DemoDungeonRoom::SplitRoot
            | DemoDungeonRoom::BellSwitchback
            | DemoDungeonRoom::CurrentFork
            | DemoDungeonRoom::SplitFurnace
            | DemoDungeonRoom::PressureFork
            | DemoDungeonRoom::SplitKiln
            | DemoDungeonRoom::CulletFork
            | DemoDungeonRoom::OrbitFork
            | DemoDungeonRoom::TidalFork => DemoDungeonRouteTarget::Door("floor"),
            DemoDungeonRoom::LanternGallery
            | DemoDungeonRoom::SplitSpire
            | DemoDungeonRoom::StormSplit
            | DemoDungeonRoom::FoundryFork
            | DemoDungeonRoom::LiftShaft
            | DemoDungeonRoom::MirrorFork
            | DemoDungeonRoom::FurnaceLift
            | DemoDungeonRoom::EclipseFork
            | DemoDungeonRoom::GravityLift => DemoDungeonRouteTarget::Door("ceiling"),
            DemoDungeonRoom::RootCellar => DemoDungeonRouteTarget::Pickup("dungeon-coin-03"),
            DemoDungeonRoom::WatchPost => DemoDungeonRouteTarget::Pickup("dungeon-coin-05"),
            DemoDungeonRoom::ClimberVault => {
                DemoDungeonRouteTarget::Pickup(DEMO_DUNGEON_GLOVE_PICKUP)
            }
            DemoDungeonRoom::WallAntechamber => DemoDungeonRouteTarget::Pickup("dungeon-coin-16"),
            DemoDungeonRoom::BroadChimney => DemoDungeonRouteTarget::Pickup("dungeon-coin-17"),
            DemoDungeonRoom::BellNiche => DemoDungeonRouteTarget::Pickup("dungeon-coin-18"),
            DemoDungeonRoom::TempoHall => DemoDungeonRouteTarget::Pickup("dungeon-coin-19"),
            DemoDungeonRoom::RafterShrine => DemoDungeonRouteTarget::Pickup("dungeon-coin-20"),
            DemoDungeonRoom::NeedleTurn => DemoDungeonRouteTarget::Pickup("dungeon-coin-21"),
            DemoDungeonRoom::Crossroads => DemoDungeonRouteTarget::Door("floor"),
            DemoDungeonRoom::WallGallery => DemoDungeonRouteTarget::Door("ceiling"),
            DemoDungeonRoom::BootsVault => DemoDungeonRouteTarget::Pickup(DEMO_DUNGEON_BOOT_PICKUP),
            DemoDungeonRoom::Underpass => DemoDungeonRouteTarget::Pickup("dungeon-coin-13"),
            DemoDungeonRoom::GaleLanding => DemoDungeonRouteTarget::Pickup("dungeon-coin-22"),
            DemoDungeonRoom::LowPassage => DemoDungeonRouteTarget::Pickup("dungeon-coin-23"),
            DemoDungeonRoom::CoinDuct => DemoDungeonRouteTarget::Pickup("dungeon-coin-24"),
            DemoDungeonRoom::PulseGallery => DemoDungeonRouteTarget::Pickup("dungeon-coin-25"),
            DemoDungeonRoom::StormCache => DemoDungeonRouteTarget::Pickup("dungeon-coin-26"),
            DemoDungeonRoom::BrakeTower => DemoDungeonRouteTarget::Pickup("dungeon-coin-27"),
            DemoDungeonRoom::AlloyThreshold => DemoDungeonRouteTarget::Pickup("dungeon-coin-28"),
            DemoDungeonRoom::Windshaft => DemoDungeonRouteTarget::Pickup("dungeon-coin-29"),
            DemoDungeonRoom::EmberVault => DemoDungeonRouteTarget::Pickup("dungeon-coin-30"),
            DemoDungeonRoom::GearGallery => DemoDungeonRouteTarget::Pickup("dungeon-coin-31"),
            DemoDungeonRoom::CoolingDuct => DemoDungeonRouteTarget::Pickup("dungeon-coin-32"),
            DemoDungeonRoom::HammerHall => DemoDungeonRouteTarget::Pickup("dungeon-coin-33"),
            DemoDungeonRoom::SparkNiche => DemoDungeonRouteTarget::Pickup("dungeon-coin-34"),
            DemoDungeonRoom::BlastGallery => DemoDungeonRouteTarget::Pickup("dungeon-coin-35"),
            DemoDungeonRoom::AshCache => DemoDungeonRouteTarget::Pickup("dungeon-coin-36"),
            DemoDungeonRoom::VentSpire => DemoDungeonRouteTarget::Pickup("dungeon-coin-37"),
            DemoDungeonRoom::CinderBridge => DemoDungeonRouteTarget::Pickup("dungeon-coin-38"),
            DemoDungeonRoom::FoundrySeal => DemoDungeonRouteTarget::Pickup("dungeon-coin-39"),
            DemoDungeonRoom::GlassThreshold => DemoDungeonRouteTarget::Pickup("dungeon-coin-40"),
            DemoDungeonRoom::PrismRun => DemoDungeonRouteTarget::Pickup("dungeon-coin-41"),
            DemoDungeonRoom::ShardVault => DemoDungeonRouteTarget::Pickup("dungeon-coin-42"),
            DemoDungeonRoom::GlassGallery => DemoDungeonRouteTarget::Pickup("dungeon-coin-43"),
            DemoDungeonRoom::MirrorDuct => DemoDungeonRouteTarget::Pickup("dungeon-coin-44"),
            DemoDungeonRoom::TemperHall => DemoDungeonRouteTarget::Pickup("dungeon-coin-45"),
            DemoDungeonRoom::LensNiche => DemoDungeonRouteTarget::Pickup("dungeon-coin-46"),
            DemoDungeonRoom::HotGlass => DemoDungeonRouteTarget::Pickup("dungeon-coin-47"),
            DemoDungeonRoom::CulletCache => DemoDungeonRouteTarget::Pickup("dungeon-coin-48"),
            DemoDungeonRoom::AnnealingSpire => DemoDungeonRouteTarget::Pickup("dungeon-coin-49"),
            DemoDungeonRoom::CrystalBridge => DemoDungeonRouteTarget::Pickup("dungeon-coin-50"),
            DemoDungeonRoom::GlassSeal => DemoDungeonRouteTarget::Pickup("dungeon-coin-51"),
            DemoDungeonRoom::StarThreshold => DemoDungeonRouteTarget::Pickup("dungeon-coin-52"),
            DemoDungeonRoom::CometRun => DemoDungeonRouteTarget::Pickup("dungeon-coin-53"),
            DemoDungeonRoom::MoonVault => DemoDungeonRouteTarget::Pickup("dungeon-coin-54"),
            DemoDungeonRoom::ConstellationHall => DemoDungeonRouteTarget::Pickup("dungeon-coin-55"),
            DemoDungeonRoom::ShadowDuct => DemoDungeonRouteTarget::Pickup("dungeon-coin-56"),
            DemoDungeonRoom::Observatory => DemoDungeonRouteTarget::Pickup("dungeon-coin-57"),
            DemoDungeonRoom::NovaNiche => DemoDungeonRouteTarget::Pickup("dungeon-coin-58"),
            DemoDungeonRoom::VacuumGallery => DemoDungeonRouteTarget::Pickup("dungeon-coin-59"),
            DemoDungeonRoom::LunarCache => DemoDungeonRouteTarget::Pickup("dungeon-coin-60"),
            DemoDungeonRoom::AuroraSpire => DemoDungeonRouteTarget::Pickup("dungeon-coin-61"),
            DemoDungeonRoom::Skybridge => DemoDungeonRouteTarget::Pickup("dungeon-coin-62"),
            DemoDungeonRoom::AstralSeal => DemoDungeonRouteTarget::Pickup("dungeon-coin-63"),
            DemoDungeonRoom::CoinLoft => DemoDungeonRouteTarget::Pickup("dungeon-coin-09"),
            DemoDungeonRoom::NeedleRoom => DemoDungeonRouteTarget::Pickup("dungeon-coin-11"),
            DemoDungeonRoom::Treasury => DemoDungeonRouteTarget::Pickup("dungeon-coin-15"),
            DemoDungeonRoom::CrownSanctum => DemoDungeonRouteTarget::GoalExit,
        };
        DemoDungeonRouteSpec {
            room,
            entry_door,
            target,
            inventory,
        }
    })
}

#[must_use]
pub const fn demo_dungeon_door_coin_requirement(
    room: DemoDungeonRoom,
    door_id: &str,
) -> Option<u8> {
    let requirement = demo_dungeon_door_requirement(room, door_id).coins;
    if requirement == 0 {
        None
    } else {
        Some(requirement as u8)
    }
}

#[must_use]
pub const fn demo_dungeon_door_requirement(
    room: DemoDungeonRoom,
    door_id: &str,
) -> AuthoredDoorRequirement {
    let coins = match (room, door_id.as_bytes()) {
        (DemoDungeonRoom::Sluice, b"east") => DEMO_DUNGEON_GLOVE_GATE_REQUIREMENT,
        (DemoDungeonRoom::WallGate, b"east") => DEMO_DUNGEON_WALL_REGION_GATE_REQUIREMENT,
        (DemoDungeonRoom::DashSeal, b"east") => DEMO_DUNGEON_DASH_REGION_GATE_REQUIREMENT,
        (DemoDungeonRoom::FoundrySeal, b"east") => DEMO_DUNGEON_FOUNDRY_GATE_REQUIREMENT,
        (DemoDungeonRoom::GlassSeal, b"east") => DEMO_DUNGEON_GLASSWORKS_GATE_REQUIREMENT,
        (DemoDungeonRoom::AstralSeal, b"east") => DEMO_DUNGEON_ASTRAL_GATE_REQUIREMENT,
        (DemoDungeonRoom::WallGallery, b"floor") => DEMO_DUNGEON_LOWER_VAULT_REQUIREMENT,
        (DemoDungeonRoom::Crossroads, b"ceiling") | (DemoDungeonRoom::Underpass, b"west") => {
            DEMO_DUNGEON_BOOT_GATE_REQUIREMENT
        }
        (DemoDungeonRoom::Underpass, b"east") => DEMO_DUNGEON_TREASURY_REQUIREMENT,
        (DemoDungeonRoom::Gatehouse, b"east") => DEMO_DUNGEON_CROWN_GATE_REQUIREMENT,
        _ => 0,
    };
    let traversal_methods = match (room, door_id.as_bytes()) {
        (DemoDungeonRoom::ClimberVault, b"east") => {
            TraversalMethods::one(TraversalMethod::WallJump)
        }
        (DemoDungeonRoom::WallGate, b"east") => TraversalMethods::one(TraversalMethod::WallJump),
        (DemoDungeonRoom::WallGallery, b"east") => TraversalMethods::one(TraversalMethod::Dash),
        (DemoDungeonRoom::DashSeal, b"east") => TraversalMethods::one(TraversalMethod::Dash),
        (DemoDungeonRoom::FoundrySeal, b"east") => TraversalMethods::ALL_CURRENT,
        (DemoDungeonRoom::GlassSeal, b"east") => TraversalMethods::ALL_CURRENT,
        (DemoDungeonRoom::AstralSeal, b"east") => TraversalMethods::ALL_CURRENT,
        (DemoDungeonRoom::Gatehouse, b"east") => TraversalMethods::ALL_CURRENT,
        _ => TraversalMethods::NONE,
    };
    AuthoredDoorRequirement::new(coins as u16, traversal_methods)
}

#[must_use]
pub fn demo_dungeon_definition() -> AuthoredDungeonDefinition {
    let floors = DemoDungeonRoom::ALL
        .into_iter()
        .map(|room| {
            let connections = room
                .connections()
                .into_iter()
                .map(|connection| {
                    let destination = DemoDungeonRoom::from_id(&connection.destination_room)
                        .expect("built-in demo graph only names built-in floors");
                    let requirement = demo_dungeon_door_requirement(room, &connection.door_id);
                    AuthoredConnection::new(
                        connection.door_id,
                        destination.authored_key(),
                        connection.destination_door,
                        requirement,
                    )
                })
                .collect();
            AuthoredFloorDefinition {
                key: room.authored_key(),
                id: room.id().to_owned(),
                title: room.title().to_owned(),
                geometry_key: format!(
                    "dungeon-palette-v{}:{}:{:016x}",
                    downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION,
                    room.course().slug(),
                    0xD06E_0A11_u64,
                ),
                connections,
                coin_indices: room_coin_specs(room)
                    .into_iter()
                    .map(|(index, _)| u16::from(index))
                    .collect(),
                traversal_unlock: match room {
                    DemoDungeonRoom::ClimberVault => Some(TraversalMethod::WallJump),
                    DemoDungeonRoom::BootsVault => Some(TraversalMethod::Dash),
                    _ => None,
                },
                contains_crown: room == DemoDungeonRoom::CrownSanctum,
            }
        })
        .collect();
    AuthoredDungeonDefinition {
        schema_version: AUTHORED_DUNGEON_SCHEMA_VERSION,
        id: "demo-dungeon-v31".to_owned(),
        start_floor: DemoDungeonRoom::HollowLanding.authored_key(),
        start_methods: TraversalMethods::NONE,
        crown_floor: DemoDungeonRoom::CrownSanctum.authored_key(),
        total_coins: u16::from(DEMO_DUNGEON_TOTAL_COINS),
        crown_requirement: AuthoredDoorRequirement::new(
            u16::from(DEMO_DUNGEON_CROWN_GATE_REQUIREMENT),
            TraversalMethods::ALL_CURRENT,
        ),
        required_floor_count: DemoDungeonRoom::ALL.len() as u16,
        floors,
    }
}

fn coin_id(index: u8) -> String {
    format!("dungeon-coin-{index:02}")
}

fn coin_index(id: &str) -> Option<u8> {
    let suffix = id.strip_prefix("dungeon-coin-")?;
    let index = suffix.parse::<u8>().ok()?;
    (index < DEMO_DUNGEON_TOTAL_COINS).then_some(index)
}

/// Build one room against the persistent run inventory.
///
/// Collected run items are deliberately omitted when a room is reconstructed, so returning
/// through the graph cannot respawn boots or the crown.
#[must_use]
pub fn demo_dungeon_room(room: DemoDungeonRoom, inventory: DemoDungeonInventory) -> Room {
    let exits = (room == DemoDungeonRoom::CrownSanctum && !inventory.crown)
        .then(|| Exit {
            id: DEMO_DUNGEON_GOAL_EXIT.to_owned(),
            bounds: Rect::new(302, 12, 8, 18),
            destination: None,
            destination_entrance: None,
        })
        .into_iter()
        .collect();
    let candidate = DungeonPaletteKey::new(0xD06E_0A11, room.course()).generate();
    let built = candidate
        .materialize(room.id(), room.title(), &room.connections(), exits)
        .expect("built-in dungeon palette and graph must satisfy room invariants");
    let mut pickups = room_coin_specs(room)
        .into_iter()
        .filter_map(|(index, bounds)| {
            let id = coin_id(index);
            (!inventory.has_coin(&id))
                .then(|| Pickup::new(id, bounds).expect("authored dungeon coin bounds are valid"))
        })
        .collect::<Vec<_>>();
    if room == DemoDungeonRoom::ClimberVault && !inventory.climbing_gloves {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_GLOVE_PICKUP, Rect::new(254, 44, 16, 16))
                .expect("glove bounds are valid"),
        );
    }
    if room == DemoDungeonRoom::BootsVault && !inventory.winged_boots {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_BOOT_PICKUP, Rect::new(214, 24, 16, 16))
                .expect("boots bounds are valid"),
        );
    }
    if room == DemoDungeonRoom::CrownSanctum && !inventory.crown {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_CROWN_PICKUP, Rect::new(285, 14, 16, 16))
                .expect("crown bounds are valid"),
        );
    }
    built
        .with_objects(room_timed_hazards(room), pickups)
        .expect("built-in dungeon pickups must satisfy room invariants")
}

fn room_timed_hazards(room: DemoDungeonRoom) -> Vec<TimedHazard> {
    if room != DemoDungeonRoom::MeteorRun {
        return Vec::new();
    }

    // Three broad shutters share one period, with their twenty-six-tick inactive windows
    // advancing from west to east. A player can read each inactive sprite from the preceding safe
    // bay; the thirty-two-tick phase offset leaves enough time for a committed Dash and controlled
    // braking, but not for a blind uninterrupted run through the whole room.
    [(52, 45), (134, 13), (216, 77)]
        .into_iter()
        .map(|(x, phase)| {
            TimedHazard::new(Rect::new(x, 0, 52, 170), 96, 70, phase)
                .expect("authored Meteor shutters satisfy timed-hazard invariants")
        })
        .collect()
}

fn room_coin_specs(room: DemoDungeonRoom) -> Vec<(u8, Rect)> {
    match room {
        DemoDungeonRoom::HollowLanding => vec![(0, Rect::new(158, 100, 8, 10))],
        DemoDungeonRoom::MossWalk => vec![(1, Rect::new(222, 90, 8, 10))],
        DemoDungeonRoom::RootCellar => vec![
            (2, Rect::new(144, 110, 8, 10)),
            (3, Rect::new(244, 90, 8, 10)),
        ],
        DemoDungeonRoom::BrokenAqueduct => vec![(4, Rect::new(205, 130, 8, 10))],
        DemoDungeonRoom::WatchPost => vec![(5, Rect::new(238, 20, 8, 10))],
        DemoDungeonRoom::Threshold => vec![(6, Rect::new(158, 100, 8, 10))],
        DemoDungeonRoom::Crossroads => vec![(7, Rect::new(148, 100, 8, 10))],
        DemoDungeonRoom::CoinLoft => vec![
            (8, Rect::new(144, 110, 8, 10)),
            (9, Rect::new(244, 90, 8, 10)),
        ],
        DemoDungeonRoom::WallGallery => vec![(10, Rect::new(188, 80, 8, 10))],
        DemoDungeonRoom::NeedleRoom => vec![(11, Rect::new(188, 110, 8, 10))],
        DemoDungeonRoom::BootsVault => vec![(12, Rect::new(246, 30, 8, 10))],
        DemoDungeonRoom::Underpass => vec![(13, Rect::new(224, 80, 8, 10))],
        DemoDungeonRoom::Treasury => vec![
            (14, Rect::new(144, 100, 8, 10)),
            (15, Rect::new(244, 70, 8, 10)),
        ],
        DemoDungeonRoom::WallAntechamber => vec![(16, Rect::new(264, 120, 8, 10))],
        DemoDungeonRoom::BroadChimney => vec![(17, Rect::new(204, 30, 8, 10))],
        DemoDungeonRoom::BellNiche => vec![(18, Rect::new(144, 20, 8, 10))],
        DemoDungeonRoom::TempoHall => vec![(19, Rect::new(204, 20, 8, 10))],
        DemoDungeonRoom::RafterShrine => vec![(20, Rect::new(194, 10, 8, 10))],
        DemoDungeonRoom::NeedleTurn => vec![(21, Rect::new(204, 50, 8, 10))],
        DemoDungeonRoom::GaleLanding => vec![(22, Rect::new(264, 120, 8, 10))],
        DemoDungeonRoom::LowPassage => vec![(23, Rect::new(234, 94, 8, 10))],
        DemoDungeonRoom::CoinDuct => vec![(24, Rect::new(154, 14, 8, 10))],
        DemoDungeonRoom::PulseGallery => vec![(25, Rect::new(154, 114, 8, 10))],
        DemoDungeonRoom::StormCache => vec![(26, Rect::new(234, 14, 8, 10))],
        DemoDungeonRoom::BrakeTower => vec![(27, Rect::new(234, 14, 8, 10))],
        DemoDungeonRoom::AlloyThreshold => vec![(28, Rect::new(264, 120, 8, 10))],
        DemoDungeonRoom::Windshaft => vec![(29, Rect::new(214, 20, 8, 10))],
        DemoDungeonRoom::EmberVault => vec![(30, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::GearGallery => vec![(31, Rect::new(214, 20, 8, 10))],
        DemoDungeonRoom::CoolingDuct => vec![(32, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::HammerHall => vec![(33, Rect::new(264, 60, 8, 10))],
        DemoDungeonRoom::SparkNiche => vec![(34, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::BlastGallery => vec![(35, Rect::new(224, 20, 8, 10))],
        DemoDungeonRoom::AshCache => vec![(36, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::VentSpire => vec![(37, Rect::new(214, 20, 8, 10))],
        DemoDungeonRoom::CinderBridge => vec![(38, Rect::new(284, 30, 8, 10))],
        DemoDungeonRoom::FoundrySeal => vec![(39, Rect::new(214, 50, 8, 10))],
        DemoDungeonRoom::GlassThreshold => vec![(40, Rect::new(264, 120, 8, 10))],
        DemoDungeonRoom::PrismRun => vec![(41, Rect::new(274, 30, 8, 10))],
        DemoDungeonRoom::ShardVault => vec![(42, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::GlassGallery => vec![(43, Rect::new(214, 20, 8, 10))],
        DemoDungeonRoom::MirrorDuct => vec![(44, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::TemperHall => vec![(45, Rect::new(144, 90, 8, 10))],
        DemoDungeonRoom::LensNiche => vec![(46, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::HotGlass => vec![(47, Rect::new(224, 20, 8, 10))],
        DemoDungeonRoom::CulletCache => vec![(48, Rect::new(224, 30, 8, 10))],
        DemoDungeonRoom::AnnealingSpire => vec![(49, Rect::new(214, 20, 8, 10))],
        DemoDungeonRoom::CrystalBridge => vec![(50, Rect::new(274, 30, 8, 10))],
        DemoDungeonRoom::GlassSeal => vec![(51, Rect::new(214, 50, 8, 10))],
        DemoDungeonRoom::StarThreshold => vec![(52, Rect::new(144, 20, 8, 10))],
        DemoDungeonRoom::CometRun => vec![(53, Rect::new(294, 60, 8, 10))],
        DemoDungeonRoom::MoonVault => vec![(54, Rect::new(274, 40, 8, 10))],
        DemoDungeonRoom::ConstellationHall => vec![(55, Rect::new(274, 120, 8, 10))],
        DemoDungeonRoom::ShadowDuct => vec![(56, Rect::new(224, 10, 8, 10))],
        DemoDungeonRoom::Observatory => vec![(57, Rect::new(14, 10, 8, 10))],
        DemoDungeonRoom::NovaNiche => vec![(58, Rect::new(284, 20, 8, 10))],
        DemoDungeonRoom::VacuumGallery => vec![(59, Rect::new(294, 150, 8, 10))],
        DemoDungeonRoom::LunarCache => vec![(60, Rect::new(294, 20, 8, 10))],
        DemoDungeonRoom::AuroraSpire => vec![(61, Rect::new(294, 50, 8, 10))],
        DemoDungeonRoom::Skybridge => vec![(62, Rect::new(264, 110, 8, 10))],
        DemoDungeonRoom::AstralSeal => vec![(63, Rect::new(245, 50, 8, 10))],
        DemoDungeonRoom::SplitRoot
        | DemoDungeonRoom::OldLift
        | DemoDungeonRoom::LanternGallery
        | DemoDungeonRoom::Sluice
        | DemoDungeonRoom::ClimberVault
        | DemoDungeonRoom::BellSwitchback
        | DemoDungeonRoom::SplitSpire
        | DemoDungeonRoom::LandingChain
        | DemoDungeonRoom::WallGate
        | DemoDungeonRoom::CurrentFork
        | DemoDungeonRoom::StormSplit
        | DemoDungeonRoom::RelayChasm
        | DemoDungeonRoom::DashSeal
        | DemoDungeonRoom::SplitFurnace
        | DemoDungeonRoom::CrosswindChimney
        | DemoDungeonRoom::FoundryFork
        | DemoDungeonRoom::LiftShaft
        | DemoDungeonRoom::RivetRun
        | DemoDungeonRoom::PressureFork
        | DemoDungeonRoom::PistonPass
        | DemoDungeonRoom::CrucibleClimb
        | DemoDungeonRoom::SplitKiln
        | DemoDungeonRoom::RefractionShaft
        | DemoDungeonRoom::MirrorFork
        | DemoDungeonRoom::FurnaceLift
        | DemoDungeonRoom::SliverRun
        | DemoDungeonRoom::CulletFork
        | DemoDungeonRoom::RazorPass
        | DemoDungeonRoom::LatticeClimb
        | DemoDungeonRoom::OrbitFork
        | DemoDungeonRoom::ZenithShaft
        | DemoDungeonRoom::EclipseFork
        | DemoDungeonRoom::GravityLift
        | DemoDungeonRoom::MeteorRun
        | DemoDungeonRoom::TidalFork
        | DemoDungeonRoom::VoidPass
        | DemoDungeonRoom::StarwellClimb
        | DemoDungeonRoom::DashChasm
        | DemoDungeonRoom::Gatehouse
        | DemoDungeonRoom::CrownSanctum => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_ai::{
        NoiseFamily, SearchTarget, ShakyHandConfig, SolverConfig, TargetSolution,
        TargetSolveOutcome, evaluate_shaky_hand, solve_target,
    };
    use downwards_core::{JumpKind, Simulation, SimulationEvent};

    #[test]
    fn graph_connections_are_reciprocal_and_socket_matched() {
        let inventory = DemoDungeonInventory::default();
        for room_id in DemoDungeonRoom::ALL {
            let room = demo_dungeon_room(room_id, inventory);
            for door in room.doors() {
                let destination_id = DemoDungeonRoom::from_id(
                    door.destination_room
                        .as_deref()
                        .expect("every dungeon door is connected"),
                )
                .expect("destination room is in this dungeon");
                let destination = demo_dungeon_room(destination_id, inventory);
                let return_door = destination
                    .doors()
                    .iter()
                    .find(|candidate| {
                        candidate.id
                            == door
                                .destination_door
                                .as_deref()
                                .expect("every dungeon door names its mate")
                    })
                    .expect("destination door exists");
                assert_eq!(return_door.destination_room.as_deref(), Some(room_id.id()));
                assert_eq!(
                    return_door.destination_door.as_deref(),
                    Some(door.id.as_str())
                );
                assert!(door.geometrically_matches(return_door));
            }
        }
    }

    #[test]
    fn expanded_first_region_is_bound_to_the_scalable_authored_dungeon_contract() {
        let definition = demo_dungeon_definition();
        let audit = definition.validate().unwrap();
        assert_eq!(audit.reachable_floors, DemoDungeonRoom::ALL.len());
        assert_eq!(audit.collected_coins, u16::from(DEMO_DUNGEON_TOTAL_COINS));
        assert_eq!(audit.traversal_methods, TraversalMethods::ALL_CURRENT);
        assert_eq!(
            definition
                .floor(DemoDungeonRoom::ClimberVault.authored_key())
                .unwrap()
                .traversal_unlock,
            Some(TraversalMethod::WallJump)
        );
        assert_eq!(
            definition
                .floor(DemoDungeonRoom::BootsVault.authored_key())
                .unwrap()
                .traversal_unlock,
            Some(TraversalMethod::Dash)
        );
    }

    #[test]
    fn persistent_items_disappear_and_traversal_pickups_upgrade_the_loadout() {
        let empty = DemoDungeonInventory::default();
        assert!(!empty.abilities().wall_jump);
        assert!(!empty.abilities().dash);
        let gloves_room = demo_dungeon_room(DemoDungeonRoom::ClimberVault, empty);
        assert!(
            gloves_room
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_GLOVE_PICKUP)
        );
        let gloves = DemoDungeonInventory {
            climbing_gloves: true,
            ..empty
        };
        assert!(gloves.abilities().wall_jump);
        assert!(
            demo_dungeon_room(DemoDungeonRoom::ClimberVault, gloves)
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != DEMO_DUNGEON_GLOVE_PICKUP)
        );
        let uncollected = demo_dungeon_room(DemoDungeonRoom::BootsVault, empty);
        assert!(
            uncollected
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            uncollected
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == coin_id(12))
        );
        let acquired = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            crown: false,
            ..DemoDungeonInventory::default()
        };
        assert!(acquired.abilities().dash);
        let after_boots = demo_dungeon_room(DemoDungeonRoom::BootsVault, acquired);
        assert!(
            after_boots
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            after_boots
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == coin_id(12))
        );

        let mut collected = acquired;
        assert!(collected.collect_coin(&coin_id(12)));
        assert!(
            demo_dungeon_room(DemoDungeonRoom::BootsVault, collected)
                .pickups()
                .is_empty()
        );
    }

    #[test]
    fn coins_are_unique_persistent_and_bind_every_progression_gate() {
        let mut inventory = DemoDungeonInventory::default();
        for index in 0..DEMO_DUNGEON_TOTAL_COINS {
            let id = coin_id(index);
            assert!(
                inventory.collect_coin(&id),
                "first collection of {id} is new"
            );
            assert!(
                !inventory.collect_coin(&id),
                "second collection of {id} is not new"
            );
            assert!(inventory.has_coin(&id));
        }
        assert_eq!(inventory.coin_count(), DEMO_DUNGEON_TOTAL_COINS);
        let mut placed_indices = DemoDungeonRoom::ALL
            .into_iter()
            .flat_map(room_coin_specs)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        placed_indices.sort_unstable();
        assert_eq!(
            placed_indices,
            (0..DEMO_DUNGEON_TOTAL_COINS).collect::<Vec<_>>()
        );
        for (rooms, expected_indices) in [
            (
                vec![
                    DemoDungeonRoom::HollowLanding,
                    DemoDungeonRoom::MossWalk,
                    DemoDungeonRoom::RootCellar,
                    DemoDungeonRoom::BrokenAqueduct,
                    DemoDungeonRoom::WatchPost,
                ],
                (0..6).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::Threshold,
                    DemoDungeonRoom::Crossroads,
                    DemoDungeonRoom::CoinLoft,
                    DemoDungeonRoom::WallGallery,
                    DemoDungeonRoom::NeedleRoom,
                ],
                (6..12).collect::<Vec<_>>(),
            ),
            (
                vec![DemoDungeonRoom::BootsVault, DemoDungeonRoom::Underpass],
                (12..14).collect::<Vec<_>>(),
            ),
            (
                vec![DemoDungeonRoom::Treasury],
                (14..16).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::WallAntechamber,
                    DemoDungeonRoom::BroadChimney,
                    DemoDungeonRoom::BellNiche,
                    DemoDungeonRoom::TempoHall,
                    DemoDungeonRoom::RafterShrine,
                    DemoDungeonRoom::NeedleTurn,
                ],
                (16..22).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::GaleLanding,
                    DemoDungeonRoom::LowPassage,
                    DemoDungeonRoom::CoinDuct,
                    DemoDungeonRoom::PulseGallery,
                    DemoDungeonRoom::StormCache,
                    DemoDungeonRoom::BrakeTower,
                ],
                (22..28).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::AlloyThreshold,
                    DemoDungeonRoom::Windshaft,
                    DemoDungeonRoom::EmberVault,
                    DemoDungeonRoom::GearGallery,
                    DemoDungeonRoom::CoolingDuct,
                    DemoDungeonRoom::HammerHall,
                    DemoDungeonRoom::SparkNiche,
                    DemoDungeonRoom::BlastGallery,
                    DemoDungeonRoom::AshCache,
                    DemoDungeonRoom::VentSpire,
                    DemoDungeonRoom::CinderBridge,
                    DemoDungeonRoom::FoundrySeal,
                ],
                (28..40).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::GlassThreshold,
                    DemoDungeonRoom::PrismRun,
                    DemoDungeonRoom::ShardVault,
                    DemoDungeonRoom::GlassGallery,
                    DemoDungeonRoom::MirrorDuct,
                    DemoDungeonRoom::TemperHall,
                    DemoDungeonRoom::LensNiche,
                    DemoDungeonRoom::HotGlass,
                    DemoDungeonRoom::CulletCache,
                    DemoDungeonRoom::AnnealingSpire,
                    DemoDungeonRoom::CrystalBridge,
                    DemoDungeonRoom::GlassSeal,
                ],
                (40..52).collect::<Vec<_>>(),
            ),
            (
                vec![
                    DemoDungeonRoom::StarThreshold,
                    DemoDungeonRoom::CometRun,
                    DemoDungeonRoom::MoonVault,
                    DemoDungeonRoom::ConstellationHall,
                    DemoDungeonRoom::ShadowDuct,
                    DemoDungeonRoom::Observatory,
                    DemoDungeonRoom::NovaNiche,
                    DemoDungeonRoom::VacuumGallery,
                    DemoDungeonRoom::LunarCache,
                    DemoDungeonRoom::AuroraSpire,
                    DemoDungeonRoom::Skybridge,
                    DemoDungeonRoom::AstralSeal,
                ],
                (52..64).collect::<Vec<_>>(),
            ),
        ] {
            let mut actual = rooms
                .into_iter()
                .flat_map(room_coin_specs)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            actual.sort_unstable();
            assert_eq!(actual, expected_indices);
        }
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Sluice, "east"),
            Some(DEMO_DUNGEON_GLOVE_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::WallGate, "east"),
            Some(DEMO_DUNGEON_WALL_REGION_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Crossroads, "ceiling"),
            Some(DEMO_DUNGEON_BOOT_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::WallGallery, "floor"),
            Some(DEMO_DUNGEON_LOWER_VAULT_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Underpass, "west"),
            Some(DEMO_DUNGEON_BOOT_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Underpass, "east"),
            Some(DEMO_DUNGEON_TREASURY_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::DashSeal, "east"),
            Some(DEMO_DUNGEON_DASH_REGION_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::FoundrySeal, "east"),
            Some(DEMO_DUNGEON_FOUNDRY_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::GlassSeal, "east"),
            Some(DEMO_DUNGEON_GLASSWORKS_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::AstralSeal, "east"),
            Some(DEMO_DUNGEON_ASTRAL_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Gatehouse, "east"),
            Some(DEMO_DUNGEON_CROWN_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Gatehouse, "west"),
            None
        );
    }

    #[test]
    fn boots_and_crown_require_every_authored_floor() {
        let lower_ready = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_LOWER_VAULT_REQUIREMENT,
            )
        };
        assert!(
            demo_dungeon_door_requirement(DemoDungeonRoom::WallGallery, "floor")
                .is_satisfied_by(&lower_ready.authored_progression_inventory())
        );
        assert!(
            !demo_dungeon_door_requirement(DemoDungeonRoom::Underpass, "west")
                .is_satisfied_by(&lower_ready.authored_progression_inventory())
        );
        assert!(
            !demo_dungeon_door_requirement(DemoDungeonRoom::Underpass, "east")
                .is_satisfied_by(&lower_ready.authored_progression_inventory())
        );

        let underpass_ready = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_TREASURY_REQUIREMENT,
            )
        };
        assert!(
            demo_dungeon_door_requirement(DemoDungeonRoom::Underpass, "east")
                .is_satisfied_by(&underpass_ready.authored_progression_inventory())
        );
        assert!(
            !demo_dungeon_door_requirement(DemoDungeonRoom::Underpass, "west")
                .is_satisfied_by(&underpass_ready.authored_progression_inventory())
        );

        let vault_ready = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_BOOT_GATE_REQUIREMENT,
            )
        };
        for (room, door) in [
            (DemoDungeonRoom::Crossroads, "ceiling"),
            (DemoDungeonRoom::Underpass, "west"),
        ] {
            assert!(
                demo_dungeon_door_requirement(room, door)
                    .is_satisfied_by(&vault_ready.authored_progression_inventory()),
                "the Winged Vault entrance {room:?}/{door} should open only after the Treasury"
            );
        }

        // Mechanically remove each floor in turn and recompute progression. No floor may be
        // omitted while still obtaining both traversal methods, every coin, and the Crown floor.
        for blocked in DemoDungeonRoom::ALL {
            let mut reachable = if blocked == DemoDungeonRoom::HollowLanding {
                std::collections::BTreeSet::new()
            } else {
                std::collections::BTreeSet::from([DemoDungeonRoom::HollowLanding])
            };
            let mut inventory = DemoDungeonInventory::default();
            loop {
                let previous_reachable = reachable.clone();
                let previous_inventory = inventory;
                for &room in &previous_reachable {
                    for (coin, _) in room_coin_specs(room) {
                        assert!(
                            inventory.collect_coin(&coin_id(coin))
                                || inventory.has_coin(&coin_id(coin))
                        );
                    }
                    match room {
                        DemoDungeonRoom::ClimberVault => inventory.climbing_gloves = true,
                        DemoDungeonRoom::BootsVault => inventory.winged_boots = true,
                        _ => {}
                    }
                }
                for &room in &previous_reachable {
                    for connection in room.connections() {
                        let Some(destination) =
                            DemoDungeonRoom::from_id(&connection.destination_room)
                        else {
                            continue;
                        };
                        if destination != blocked
                            && demo_dungeon_door_requirement(room, &connection.door_id)
                                .is_satisfied_by(&inventory.authored_progression_inventory())
                        {
                            reachable.insert(destination);
                        }
                    }
                }
                if reachable == previous_reachable && inventory == previous_inventory {
                    break;
                }
            }
            assert!(
                !reachable.contains(&DemoDungeonRoom::CrownSanctum)
                    || inventory.coin_count() < DEMO_DUNGEON_TOTAL_COINS
                    || !inventory.climbing_gloves
                    || !inventory.winged_boots,
                "{blocked:?} can be skipped while still satisfying the final progression contract"
            );
        }
    }

    #[test]
    fn crown_is_the_only_terminal_goal() {
        let empty = DemoDungeonInventory::default();
        for room_id in DemoDungeonRoom::ALL {
            let room = demo_dungeon_room(room_id, empty);
            assert_eq!(
                room.exits()
                    .iter()
                    .any(|exit| exit.id == DEMO_DUNGEON_GOAL_EXIT),
                room_id == DemoDungeonRoom::CrownSanctum
            );
        }
        let collected = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            crown: true,
            ..DemoDungeonInventory::default()
        };
        let sanctum = demo_dungeon_room(DemoDungeonRoom::CrownSanctum, collected);
        assert!(sanctum.pickups().is_empty());
        assert!(sanctum.exits().is_empty());
    }

    fn assert_route(
        room_id: DemoDungeonRoom,
        entry_door: Option<&str>,
        inventory: DemoDungeonInventory,
        target: SearchTarget,
    ) {
        let room = demo_dungeon_room(room_id, inventory);
        let mut simulation = match entry_door {
            Some(door) => Simulation::enter_via_door(room, inventory.abilities(), door).unwrap(),
            None => Simulation::with_abilities(room, inventory.abilities()),
        };
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            target.clone(),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Solved(_)),
            "{room_id:?} from {entry_door:?} did not reach {target:?}: {outcome:?}"
        );
    }

    fn inventory_with_coin_indices(
        indices: impl IntoIterator<Item = u8>,
        climbing_gloves: bool,
        winged_boots: bool,
    ) -> DemoDungeonInventory {
        let mut inventory = DemoDungeonInventory {
            climbing_gloves,
            winged_boots,
            ..DemoDungeonInventory::default()
        };
        for index in indices {
            assert!(inventory.collect_coin(&coin_id(index)));
        }
        inventory
    }

    fn solve_route(
        room_id: DemoDungeonRoom,
        entry_door: Option<&str>,
        inventory: DemoDungeonInventory,
        target: SearchTarget,
    ) -> (Simulation, TargetSolution) {
        let room = demo_dungeon_room(room_id, inventory);
        let mut simulation = match entry_door {
            Some(door) => Simulation::enter_via_door(room, inventory.abilities(), door).unwrap(),
            None => Simulation::with_abilities(room, inventory.abilities()),
        };
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            target.clone(),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("{room_id:?} from {entry_door:?} lost {target:?}: {outcome:?}");
        };
        (simulation, solution)
    }

    fn solve_and_advance(mut simulation: Simulation, target: SearchTarget) -> Simulation {
        let abilities = simulation.abilities();
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            target.clone(),
            &SolverConfig::for_abilities(abilities),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("sequential dungeon route did not reach {target:?}: {outcome:?}");
        };
        for action in solution.replay.actions() {
            simulation.step(action);
        }
        simulation
    }

    #[test]
    fn authored_critical_route_is_solver_tractable_under_each_run_loadout() {
        let empty = DemoDungeonInventory::default();
        assert_route(
            DemoDungeonRoom::HollowLanding,
            None,
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::MossWalk,
            Some("west"),
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::SplitRoot,
            Some("west"),
            empty,
            SearchTarget::door("floor"),
        );
        assert_route(
            DemoDungeonRoom::RootCellar,
            Some("ceiling"),
            empty,
            SearchTarget::pickup(coin_id(3)),
        );
        assert_route(
            DemoDungeonRoom::BrokenAqueduct,
            Some("west"),
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::OldLift,
            Some("west"),
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::LanternGallery,
            Some("west"),
            empty,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::WatchPost,
            Some("floor"),
            empty,
            SearchTarget::pickup(coin_id(5)),
        );
        assert_route(
            DemoDungeonRoom::Sluice,
            Some("west"),
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::ClimberVault,
            Some("west"),
            empty,
            SearchTarget::pickup(DEMO_DUNGEON_GLOVE_PICKUP),
        );
        let gloves = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_GLOVE_GATE_REQUIREMENT,
            )
        };
        assert_route(
            DemoDungeonRoom::ClimberVault,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::WallAntechamber,
            Some("west"),
            gloves,
            SearchTarget::pickup(coin_id(16)),
        );
        assert_route(
            DemoDungeonRoom::WallAntechamber,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::BroadChimney,
            Some("west"),
            gloves,
            SearchTarget::pickup(coin_id(17)),
        );
        assert_route(
            DemoDungeonRoom::BroadChimney,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::BellSwitchback,
            Some("west"),
            gloves,
            SearchTarget::door("floor"),
        );
        assert_route(
            DemoDungeonRoom::BellNiche,
            Some("ceiling"),
            gloves,
            SearchTarget::pickup(coin_id(18)),
        );
        assert_route(
            DemoDungeonRoom::BellSwitchback,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::TempoHall,
            Some("west"),
            gloves,
            SearchTarget::pickup(coin_id(19)),
        );
        assert_route(
            DemoDungeonRoom::TempoHall,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::SplitSpire,
            Some("west"),
            gloves,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::RafterShrine,
            Some("floor"),
            gloves,
            SearchTarget::pickup(coin_id(20)),
        );
        assert_route(
            DemoDungeonRoom::SplitSpire,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::LandingChain,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::NeedleTurn,
            Some("west"),
            gloves,
            SearchTarget::pickup(coin_id(21)),
        );
        assert_route(
            DemoDungeonRoom::NeedleTurn,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        let wall_region_complete = inventory_with_coin_indices((0..6).chain(16..22), true, false);
        assert_route(
            DemoDungeonRoom::WallGate,
            Some("west"),
            wall_region_complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Threshold,
            Some("west"),
            wall_region_complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            gloves,
            SearchTarget::door("floor"),
        );
        assert_route(
            DemoDungeonRoom::CoinLoft,
            Some("ceiling"),
            gloves,
            SearchTarget::pickup(coin_id(8)),
        );
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            gloves,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::WallGallery,
            Some("west"),
            gloves,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::NeedleRoom,
            Some("floor"),
            gloves,
            SearchTarget::pickup(coin_id(11)),
        );
        let pre_boots = inventory_with_coin_indices((0..12).chain(16..22), true, false);
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            pre_boots,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::BootsVault,
            Some("floor"),
            pre_boots,
            SearchTarget::pickup(DEMO_DUNGEON_BOOT_PICKUP),
        );
        let boots = inventory_with_coin_indices((0..13).chain(16..22), true, true);
        assert_route(
            DemoDungeonRoom::BootsVault,
            None,
            boots,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            boots,
            SearchTarget::pickup(coin_id(13)),
        );
        let treasury_ready = inventory_with_coin_indices((0..14).chain(16..22), true, true);
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            treasury_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Treasury,
            Some("west"),
            treasury_ready,
            SearchTarget::pickup(coin_id(15)),
        );
        let complete = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_CROWN_GATE_REQUIREMENT,
            )
        };
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            complete,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::WallGallery,
            Some("floor"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::DashChasm,
            Some("west"),
            complete,
            SearchTarget::door("east"),
        );
        let dash_ready = inventory_with_coin_indices(0..22, true, true);
        assert_route(
            DemoDungeonRoom::GaleLanding,
            Some("west"),
            dash_ready,
            SearchTarget::pickup(coin_id(22)),
        );
        assert_route(
            DemoDungeonRoom::GaleLanding,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::LowPassage,
            Some("west"),
            dash_ready,
            SearchTarget::pickup(coin_id(23)),
        );
        assert_route(
            DemoDungeonRoom::LowPassage,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::CurrentFork,
            Some("west"),
            dash_ready,
            SearchTarget::door("floor"),
        );
        assert_route(
            DemoDungeonRoom::CoinDuct,
            Some("ceiling"),
            dash_ready,
            SearchTarget::pickup(coin_id(24)),
        );
        assert_route(
            DemoDungeonRoom::CurrentFork,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::PulseGallery,
            Some("west"),
            dash_ready,
            SearchTarget::pickup(coin_id(25)),
        );
        assert_route(
            DemoDungeonRoom::PulseGallery,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::StormSplit,
            Some("west"),
            dash_ready,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::StormCache,
            Some("floor"),
            dash_ready,
            SearchTarget::pickup(coin_id(26)),
        );
        assert_route(
            DemoDungeonRoom::StormSplit,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::RelayChasm,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::BrakeTower,
            Some("west"),
            dash_ready,
            SearchTarget::pickup(coin_id(27)),
        );
        assert_route(
            DemoDungeonRoom::BrakeTower,
            Some("west"),
            dash_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::DashSeal,
            Some("west"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Gatehouse,
            Some("west"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::CrownSanctum,
            Some("west"),
            complete,
            SearchTarget::exit(DEMO_DUNGEON_GOAL_EXIT),
        );
    }

    #[test]
    fn opening_region_routes_retain_observed_successes_under_small_input_perturbations() {
        let empty = DemoDungeonInventory::default();
        let routes = [
            (
                DemoDungeonRoom::HollowLanding,
                None,
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::MossWalk,
                Some("west"),
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::SplitRoot,
                Some("west"),
                SearchTarget::door("floor"),
            ),
            (
                DemoDungeonRoom::RootCellar,
                Some("ceiling"),
                SearchTarget::pickup(coin_id(3)),
            ),
            (
                DemoDungeonRoom::BrokenAqueduct,
                Some("west"),
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::OldLift,
                Some("west"),
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::LanternGallery,
                Some("west"),
                SearchTarget::door("ceiling"),
            ),
            (
                DemoDungeonRoom::WatchPost,
                Some("floor"),
                SearchTarget::pickup(coin_id(5)),
            ),
            (
                DemoDungeonRoom::Sluice,
                Some("west"),
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::ClimberVault,
                Some("west"),
                SearchTarget::pickup(DEMO_DUNGEON_GLOVE_PICKUP),
            ),
        ];
        for (index, (room, entry, target)) in routes.into_iter().enumerate() {
            let (initial, solution) = solve_route(room, entry, empty, target);
            let report = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: 0xD06E_5000 + index as u64,
                    trials_per_curve_point: 8,
                    grace_ticks: 18,
                    correlated_boundaries: 2,
                    convergence_confirmation_ticks: 2,
                },
            )
            .unwrap();
            assert!(report.exact_control_succeeded);
            for curve in report.curves.iter().filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            }) {
                assert!(
                    curve.successes > 0,
                    "{room:?} has no observed success for {:?} strength-1 perturbations; outcomes={:?}",
                    curve.family,
                    curve.trials_detail
                );
            }
        }
    }

    #[test]
    fn dash_region_routes_retain_observed_successes_under_small_input_perturbations() {
        let dash_ready = inventory_with_coin_indices(0..22, true, true);
        let routes = [
            (
                DemoDungeonRoom::GaleLanding,
                "west",
                SearchTarget::pickup(coin_id(22)),
            ),
            (
                DemoDungeonRoom::LowPassage,
                "west",
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::CurrentFork,
                "west",
                SearchTarget::door("floor"),
            ),
            (
                DemoDungeonRoom::CoinDuct,
                "ceiling",
                SearchTarget::pickup(coin_id(24)),
            ),
            (
                DemoDungeonRoom::PulseGallery,
                "west",
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::StormSplit,
                "west",
                SearchTarget::door("ceiling"),
            ),
            (
                DemoDungeonRoom::StormCache,
                "floor",
                SearchTarget::pickup(coin_id(26)),
            ),
            (
                DemoDungeonRoom::RelayChasm,
                "west",
                SearchTarget::door("east"),
            ),
            (
                DemoDungeonRoom::BrakeTower,
                "west",
                SearchTarget::pickup(coin_id(27)),
            ),
            (
                DemoDungeonRoom::DashSeal,
                "west",
                SearchTarget::door("east"),
            ),
        ];
        for (index, (room, entry, target)) in routes.into_iter().enumerate() {
            let (initial, solution) = solve_route(room, Some(entry), dash_ready, target);
            let report = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: 0xD06E_DA50 + index as u64,
                    trials_per_curve_point: 8,
                    grace_ticks: 18,
                    correlated_boundaries: 2,
                    convergence_confirmation_ticks: 2,
                },
            )
            .unwrap();
            assert!(report.exact_control_succeeded);
            for curve in report.curves.iter().filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            }) {
                assert!(
                    curve.successes > 0,
                    "{room:?} has no observed success for {:?} strength-1 perturbations; outcomes={:?}",
                    curve.family,
                    curve.trials_detail
                );
            }
        }
    }

    fn route_spec_target(target: DemoDungeonRouteTarget) -> SearchTarget {
        match target {
            DemoDungeonRouteTarget::Door(id) => SearchTarget::door(id),
            DemoDungeonRouteTarget::Pickup(id) => SearchTarget::pickup(id),
            DemoDungeonRouteTarget::GoalExit => SearchTarget::exit(DEMO_DUNGEON_GOAL_EXIT),
        }
    }

    fn stable_route_seed(id: &str) -> u64 {
        id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        }) ^ 0xD06E_7000
    }

    #[test]
    fn foundry_routes_are_exact_and_retain_observed_strength_one_successes() {
        for spec in demo_dungeon_route_specs()
            .into_iter()
            .filter(|spec| (39..=58).contains(&spec.room.authored_key().0))
        {
            let (initial, solution) = solve_route(
                spec.room,
                spec.entry_door,
                spec.inventory,
                route_spec_target(spec.target),
            );
            let report = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: stable_route_seed(spec.id()),
                    trials_per_curve_point: 64,
                    grace_ticks: 18,
                    correlated_boundaries: 2,
                    convergence_confirmation_ticks: 2,
                },
            )
            .unwrap();
            assert!(report.exact_control_succeeded, "{}", spec.id());
            for curve in report.curves.iter().filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            }) {
                assert!(
                    curve.successes > 0,
                    "{} has no observed success for {:?} strength-one perturbations: {:?}",
                    spec.id(),
                    curve.family,
                    curve.trials_detail
                );
            }
        }
    }

    #[test]
    fn foundry_seal_known_positive_uses_both_unlocked_traversal_methods() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_FOUNDRY_GATE_REQUIREMENT,
            )
        };
        let (initial, solution) = solve_route(
            DemoDungeonRoom::FoundrySeal,
            Some("west"),
            inventory,
            SearchTarget::door("east"),
        );
        let mut replayed = initial;
        let mut wall_jumps = 0;
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            wall_jumps > 0,
            "the Foundry Seal route bypassed its wall ascent"
        );
        assert!(
            dashes > 0,
            "the Foundry Seal route bypassed its low Dash partition"
        );

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..inventory
        };
        let room = demo_dungeon_room(DemoDungeonRoom::FoundrySeal, wall_only);
        let mut initial = Simulation::enter_via_door(room, wall_only.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Wall-Jump-only search unexpectedly crossed the mixed Foundry Seal: {outcome:?}"
        );
    }

    #[test]
    fn glassworks_routes_are_exact_and_retain_observed_strength_one_successes() {
        for spec in demo_dungeon_route_specs()
            .into_iter()
            .filter(|spec| (59..=78).contains(&spec.room.authored_key().0))
        {
            let (initial, solution) = solve_route(
                spec.room,
                spec.entry_door,
                spec.inventory,
                route_spec_target(spec.target),
            );
            let report = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: stable_route_seed(spec.id()),
                    trials_per_curve_point: 64,
                    grace_ticks: 18,
                    correlated_boundaries: 2,
                    convergence_confirmation_ticks: 2,
                },
            )
            .unwrap();
            assert!(report.exact_control_succeeded, "{}", spec.id());
            for curve in report.curves.iter().filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            }) {
                assert!(
                    curve.successes > 0,
                    "{} has no observed success for {:?} strength-one perturbations: {:?}",
                    spec.id(),
                    curve.family,
                    curve.trials_detail
                );
            }
        }
    }

    #[test]
    fn glass_seal_known_positive_uses_both_unlocked_traversal_methods() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_GLASSWORKS_GATE_REQUIREMENT,
            )
        };
        let (initial, solution) = solve_route(
            DemoDungeonRoom::GlassSeal,
            Some("west"),
            inventory,
            SearchTarget::door("east"),
        );
        let mut replayed = initial;
        let mut wall_jumps = 0;
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            wall_jumps > 0,
            "the Glass Seal route bypassed its wall ascent"
        );
        assert!(
            dashes > 0,
            "the Glass Seal route bypassed its low Dash partition"
        );

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..inventory
        };
        let room = demo_dungeon_room(DemoDungeonRoom::GlassSeal, wall_only);
        let mut initial = Simulation::enter_via_door(room, wall_only.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Wall-Jump-only search unexpectedly crossed the mixed Glass Seal: {outcome:?}"
        );
    }

    #[test]
    fn astral_routes_are_exact_and_retain_observed_strength_one_successes() {
        for spec in demo_dungeon_route_specs()
            .into_iter()
            .filter(|spec| (79..=98).contains(&spec.room.authored_key().0))
        {
            let (initial, solution) = solve_route(
                spec.room,
                spec.entry_door,
                spec.inventory,
                route_spec_target(spec.target),
            );
            let report = evaluate_shaky_hand(
                &initial,
                &solution,
                ShakyHandConfig {
                    seed: stable_route_seed(spec.id()),
                    trials_per_curve_point: 64,
                    grace_ticks: 18,
                    correlated_boundaries: 2,
                    convergence_confirmation_ticks: 2,
                },
            )
            .unwrap();
            assert!(report.exact_control_succeeded, "{}", spec.id());
            for curve in report.curves.iter().filter(|curve| {
                curve.family != NoiseFamily::Exact && curve.strength_ticks == 1 && curve.trials > 0
            }) {
                assert!(
                    curve.successes > 0,
                    "{} has no observed success for {:?} strength-one perturbations: {:?}",
                    spec.id(),
                    curve.family,
                    curve.trials_detail
                );
            }
        }
    }

    #[test]
    fn void_pass_is_a_distinct_late_wall_rhythm_with_no_known_dash_only_bypass() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_ASTRAL_GATE_REQUIREMENT,
            )
        };
        let (initial, solution) = solve_route(
            DemoDungeonRoom::VoidPass,
            Some("west"),
            inventory,
            SearchTarget::door("east"),
        );
        let mut replayed = initial;
        let mut wall_sides = Vec::new();
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                if let SimulationEvent::Jumped(JumpKind::Wall { side }) = event {
                    wall_sides.push(side);
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            wall_sides.len() >= 4,
            "Void Pass no longer demonstrates its alternating wall rhythm: {wall_sides:?}"
        );
        assert!(
            wall_sides
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .count()
                >= 2,
            "Void Pass known positive lost its rapid wall-side changes: {wall_sides:?}"
        );

        let dash_only = DemoDungeonInventory {
            climbing_gloves: false,
            ..inventory
        };
        let room = demo_dungeon_room(DemoDungeonRoom::VoidPass, dash_only);
        let mut initial = Simulation::enter_via_door(room, dash_only.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(dash_only.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Dash-only search unexpectedly bypassed the Void Pass wall rhythm: {outcome:?}"
        );

        let void = demo_dungeon_room(DemoDungeonRoom::VoidPass, inventory);
        let prism = demo_dungeon_room(DemoDungeonRoom::PrismRun, inventory);
        assert_ne!(
            void.tiles(),
            prism.tiles(),
            "Void Pass regressed to the copied horizontal bridge shell"
        );
    }

    #[test]
    fn starwell_climb_checked_route_alternates_then_commits_to_the_catch() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::StarwellClimb)
            .expect("Starwell Climb has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "west").unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(spec.room);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut dash_positions = Vec::new();
        let mut dash_ticks = Vec::new();
        let mut caught_star = false;
        for (tick, action) in actions.into_iter().enumerate() {
            jump_presses += usize::from(action.jump && !previous.jump);
            let dash_pressed = action.dash && !previous.dash;
            let pre_step_bounds = replayed.player().bounds();
            previous = action;
            let report = replayed.step(action);
            for event in report.events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Starwell route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_sides.push(side);
                            wall_jump_ticks.push(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        assert!(dash_pressed, "Starwell Dash event lacks an input edge");
                        dash_positions.push(pre_step_bounds);
                        dash_ticks.push(tick);
                    }
                    SimulationEvent::Landed => {
                        let bounds = replayed.player().bounds();
                        caught_star |= bounds.y == 78 && (242..=272).contains(&bounds.x);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert_eq!(
            jump_presses, accepted_jumps,
            "Starwell witness has jump spam"
        );
        assert_eq!(
            wall_sides.len(),
            4,
            "Starwell climb changed shape: {wall_sides:?}"
        );
        assert!(
            wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Starwell climb no longer alternates: {wall_sides:?}"
        );
        assert_eq!(
            dash_positions.len(),
            1,
            "Starwell relay should use one Dash"
        );
        assert!(
            dash_ticks[0] > *wall_jump_ticks.last().expect("wall ascent occurs"),
            "Starwell route used Dash to assist the climb"
        );
        assert!(
            dash_positions[0].x >= 170 && dash_positions[0].y == 28,
            "Starwell Dash no longer launches from its roof: {:?}",
            dash_positions[0]
        );
        assert!(
            caught_star,
            "Starwell route no longer lands on its catch platform"
        );
    }

    #[test]
    fn zenith_shaft_checked_route_climbs_dashes_then_climbs_again() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::ZenithShaft)
            .expect("Zenith Shaft has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "west").unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(spec.room);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut dash_ticks = Vec::new();
        let mut lower_recovery = false;
        let mut upper_floor = false;
        let mut upper_recovery = false;
        for (tick, action) in actions.into_iter().enumerate() {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            let report = replayed.step(action);
            for event in report.events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Zenith route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_sides.push(side);
                            wall_jump_ticks.push(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => dash_ticks.push(tick),
                    SimulationEvent::Landed => {
                        let bounds = replayed.player().bounds();
                        lower_recovery |= bounds.y == 48 && (82..=112).contains(&bounds.x);
                        upper_floor |= bounds.y == 88 && (182..=232).contains(&bounds.x);
                        upper_recovery |= bounds.y == 18 && (232..=272).contains(&bounds.x);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert_eq!(jump_presses, accepted_jumps, "Zenith witness has jump spam");
        assert_eq!(
            wall_sides.len(),
            6,
            "Zenith climb changed shape: {wall_sides:?}"
        );
        assert!(
            wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Zenith walls no longer alternate: {wall_sides:?}"
        );
        assert_eq!(dash_ticks.len(), 1, "Zenith transfer should use one Dash");
        assert!(
            wall_jump_ticks[3] < dash_ticks[0] && dash_ticks[0] < wall_jump_ticks[4],
            "Zenith Dash no longer separates its two climbs"
        );
        assert!(lower_recovery, "Zenith route skipped its lower roof");
        assert!(upper_floor, "Zenith route skipped its upper shaft floor");
        assert!(upper_recovery, "Zenith route skipped its upper roof");
    }

    #[test]
    fn zenith_shaft_refuses_each_incomplete_loadout() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::ZenithShaft)
            .expect("Zenith Shaft has route metadata");
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west")
                .expect("Zenith west entry is valid");
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                SearchTarget::door("east"),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly crossed Zenith Shaft: {outcome:?}"
            );
        }
    }

    #[test]
    fn zenith_shaft_retains_a_clean_full_loadout_return_route() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::ZenithShaft)
            .expect("Zenith Shaft has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut initial =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "east").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("west"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("full-loadout return route could not cross Zenith Shaft: {outcome:?}");
        };
        let mut replayed = initial;
        for action in solution.replay.actions() {
            let report = replayed.step(action);
            assert!(
                !report.events.iter().any(|event| matches!(
                    event,
                    SimulationEvent::Died(_) | SimulationEvent::Reset
                )),
                "Zenith return route is not clean: {:?}",
                report.events
            );
        }
        assert_eq!(replayed.reached_exit(), Some("west"));
    }

    #[test]
    fn starwell_climb_refuses_each_incomplete_loadout() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::StarwellClimb)
            .expect("Starwell Climb has route metadata");
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west")
                .expect("Starwell west entry is valid");
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                SearchTarget::door("east"),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly crossed Starwell Climb: {outcome:?}"
            );
        }
    }

    #[test]
    fn starwell_climb_retains_a_clean_full_loadout_return_route() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::StarwellClimb)
            .expect("Starwell Climb has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut initial =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "east").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("west"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("full-loadout return route could not cross Starwell Climb: {outcome:?}");
        };
        let mut replayed = initial;
        for action in solution.replay.actions() {
            let report = replayed.step(action);
            assert!(
                !report.events.iter().any(|event| matches!(
                    event,
                    SimulationEvent::Died(_) | SimulationEvent::Reset
                )),
                "Starwell return route is not clean: {:?}",
                report.events
            );
        }
        assert_eq!(replayed.reached_exit(), Some("west"));
    }

    #[test]
    fn astral_seal_known_positive_uses_both_unlocked_traversal_methods() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_ASTRAL_GATE_REQUIREMENT,
            )
        };
        let (initial, solution) = solve_route(
            DemoDungeonRoom::AstralSeal,
            Some("west"),
            inventory,
            SearchTarget::door("east"),
        );
        let mut replayed = initial;
        let mut wall_jumps = 0;
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            wall_jumps > 0,
            "the Astral Seal route bypassed its wall ascent"
        );
        assert!(
            dashes > 0,
            "the Astral Seal route bypassed its low Dash partition"
        );

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..inventory
        };
        let room = demo_dungeon_room(DemoDungeonRoom::AstralSeal, wall_only);
        let mut initial = Simulation::enter_via_door(room, wall_only.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Wall-Jump-only search unexpectedly crossed the mixed Astral Seal: {outcome:?}"
        );
    }

    #[test]
    fn eclipse_fork_checked_ceiling_route_alternates_then_uses_one_upward_dash() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::EclipseFork)
            .expect("Eclipse Fork has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "west").unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(spec.room);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut dash_positions = Vec::new();
        let mut dash_ticks = Vec::new();
        for (tick, action) in actions.into_iter().enumerate() {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            let report = replayed.step(action);
            for event in report.events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Eclipse route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_sides.push(side);
                            wall_jump_ticks.push(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        dash_positions.push(replayed.player().bounds());
                        dash_ticks.push(tick);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("ceiling"));
        assert_eq!(
            jump_presses, accepted_jumps,
            "Eclipse witness has jump spam"
        );
        assert!(
            wall_sides.len() >= 2 && wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Eclipse climb no longer alternates across the shaft: {wall_sides:?}"
        );
        assert_eq!(
            dash_positions.len(),
            1,
            "Eclipse ceiling relay should use one Dash: {dash_positions:?}"
        );
        assert!(
            dash_ticks[0] > *wall_jump_ticks.last().expect("wall ascent occurs"),
            "Eclipse route used Dash to assist its climb"
        );
        assert!(
            (160..=175).contains(&dash_positions[0].x) && dash_positions[0].y <= 45,
            "Eclipse Dash no longer launches from the upper balcony: {:?}",
            dash_positions[0]
        );
    }

    #[test]
    fn eclipse_fork_ceiling_branch_refuses_each_incomplete_loadout_but_corridor_stays_open() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::EclipseFork)
            .expect("Eclipse Fork has route metadata");
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west")
                .expect("Eclipse west entry is valid");
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                SearchTarget::door("ceiling"),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Eclipse ceiling: {outcome:?}"
            );
        }

        let room = demo_dungeon_room(DemoDungeonRoom::EclipseFork, spec.inventory);
        let mut corridor = Simulation::enter_via_door(room, AbilitySet::NONE, "west").unwrap();
        corridor.enable_current_player_movement();
        for _ in 0..220 {
            corridor.step(downwards_core::Action {
                move_x: 1,
                ..downwards_core::Action::default()
            });
            if corridor.reached_exit().is_some() {
                break;
            }
        }
        assert_eq!(corridor.reached_exit(), Some("east"));
    }

    #[test]
    fn astral_seal_checked_coin_route_alternates_then_commits_one_airborne_dash() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::AstralSeal)
            .expect("Astral Seal has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "west").unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(spec.room);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut dash_positions = Vec::new();
        let mut dash_ticks = Vec::new();
        for (tick, action) in actions.into_iter().enumerate() {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            let report = replayed.step(action);
            for event in report.events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Astral Seal route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_sides.push(side);
                            wall_jump_ticks.push(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        dash_positions.push(replayed.player().bounds());
                        dash_ticks.push(tick);
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == "dungeon-coin-63")
        );
        assert_eq!(
            jump_presses, accepted_jumps,
            "Astral Seal witness has jump spam"
        );
        assert!(
            wall_sides.len() >= 4 && wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Astral climb no longer alternates across the shaft: {wall_sides:?}"
        );
        assert_eq!(
            dash_positions.len(),
            1,
            "Astral relay should use one deliberate Dash: {dash_positions:?}"
        );
        assert!(
            dash_ticks[0] > *wall_jump_ticks.last().expect("wall ascent occurs"),
            "Astral relay used Dash to assist its climb"
        );
        assert!(
            (170..=185).contains(&dash_positions[0].x) && (25..=36).contains(&dash_positions[0].y),
            "Astral Dash no longer commits across the upper starwell: {:?}",
            dash_positions[0]
        );
    }

    #[test]
    fn astral_seal_coin_refuses_each_incomplete_traversal_loadout() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::AstralSeal)
            .expect("Astral Seal has route metadata");
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west")
                .expect("Astral Seal west entry is valid");
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                SearchTarget::pickup("dungeon-coin-63"),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly collected the Astral coin: {outcome:?}"
            );
        }
    }

    #[test]
    fn vacuum_gallery_known_positive_uses_both_and_missing_methods_have_no_known_route() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::VacuumGallery)
            .expect("Vacuum Gallery has route metadata");
        assert!(spec.inventory.climbing_gloves);
        assert!(spec.inventory.winged_boots);
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Vacuum Gallery has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut last_wall_jump_tick = None;
        let mut dashes = 0;
        let mut first_dash_tick = None;
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            for event in replayed.step(action).events {
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_jumps += 1;
                            wall_sides.push(side);
                            last_wall_jump_tick = Some(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        dashes += 1;
                        first_dash_tick.get_or_insert(tick);
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(jump_presses, accepted_jumps, "Vacuum witness has jump spam");
        assert_eq!(wall_jumps, 4, "Vacuum Gallery changed its wall rhythm");
        assert!(
            wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Vacuum Gallery repeats a wall instead of alternating: {wall_sides:?}"
        );
        assert_eq!(dashes, 3, "Vacuum Gallery changed its tunnel rhythm");
        assert!(
            last_wall_jump_tick < first_dash_tick,
            "Vacuum Gallery spent a Dash before completing its climb"
        );

        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(
                room,
                inventory.abilities(),
                spec.entry_door.expect("Vacuum Gallery has a west entry"),
            )
            .unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                route_spec_target(spec.target),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Vacuum Gallery coin: {outcome:?}"
            );
        }
    }

    #[test]
    fn lunar_cache_known_positive_uses_both_and_missing_methods_have_no_known_route() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::LunarCache)
            .expect("Lunar Cache has route metadata");
        assert!(spec.inventory.climbing_gloves);
        assert!(spec.inventory.winged_boots);
        let (initial, solution) = solve_route(
            spec.room,
            spec.entry_door,
            spec.inventory,
            route_spec_target(spec.target),
        );
        let mut replayed = initial;
        let mut wall_jumps = 0;
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(wall_jumps >= 3, "Lunar Cache bypassed its wall shaft");
        assert!(dashes > 0, "Lunar Cache bypassed its low tunnel");

        let return_outcome = solve_target(
            &replayed,
            SearchTarget::door("ceiling"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(return_solution) = return_outcome else {
            panic!("Lunar Cache coin route cannot return to its ceiling door: {return_outcome:?}");
        };
        for action in return_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("ceiling"));

        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(
                room,
                inventory.abilities(),
                spec.entry_door.expect("Lunar Cache has a ceiling entry"),
            )
            .unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                route_spec_target(spec.target),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Lunar Cache coin: {outcome:?}"
            );
        }
    }

    #[test]
    fn comet_run_requires_dash_and_can_continue_after_its_coin() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::CometRun)
            .expect("Comet Run has route metadata");
        assert!(spec.inventory.winged_boots);
        let (initial, solution) = solve_route(
            spec.room,
            spec.entry_door,
            spec.inventory,
            route_spec_target(spec.target),
        );
        let mut replayed = initial;
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(dashes >= 3, "Comet Run bypassed its Dash contour");

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Comet Run coin route cannot continue to its east door: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        let no_dash = DemoDungeonInventory {
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, no_dash);
        let mut initial = Simulation::enter_via_door(
            room,
            no_dash.abilities(),
            spec.entry_door.expect("Comet Run has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(no_dash.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "no-Dash search unexpectedly reached the Comet Run coin: {outcome:?}"
        );
    }

    #[test]
    fn star_threshold_demonstrates_dash_then_wall_jump_and_can_continue() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::StarThreshold)
            .expect("Star Threshold has route metadata");
        assert!(spec.inventory.climbing_gloves && spec.inventory.winged_boots);
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Star Threshold has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dashes = 0;
        let mut wall_jumps = 0;
        let mut first_wall_jump_tick = None;
        let mut dash_ticks = Vec::new();
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Star Threshold demonstration must remain clean: {event:?}"
                );
                if matches!(event, SimulationEvent::Dashed { .. }) {
                    dashes += 1;
                    dash_ticks.push(tick);
                }
                if matches!(event, SimulationEvent::Jumped(JumpKind::Wall { .. })) {
                    wall_jumps += 1;
                    first_wall_jump_tick.get_or_insert(tick);
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(
            dashes, 1,
            "the demonstration should commit one entrance Dash"
        );
        assert!(
            wall_jumps >= 4,
            "the demonstration bypassed the alternating wall rhythm"
        );
        assert!(
            dash_ticks[0] < first_wall_jump_tick.expect("wall rhythm is represented"),
            "Star Threshold no longer demonstrates Dash before Wall Jump"
        );

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Star Threshold cannot continue after its coin: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(
                room,
                inventory.abilities(),
                spec.entry_door.expect("Star Threshold has a west entry"),
            )
            .unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                route_spec_target(spec.target),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Star Threshold coin: {outcome:?}"
            );
        }
    }

    #[test]
    fn nova_niche_demonstrates_wall_core_then_dash_and_can_return() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::NovaNiche)
            .expect("Nova Niche has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Nova Niche has a floor entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut wall_jumps = 0;
        let mut dashes = 0;
        let mut first_dash_tick = None;
        let mut last_wall_jump_tick = None;
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Nova demonstration must remain clean: {event:?}"
                );
                if matches!(event, SimulationEvent::Jumped(JumpKind::Wall { .. })) {
                    wall_jumps += 1;
                    last_wall_jump_tick = Some(tick);
                }
                if matches!(event, SimulationEvent::Dashed { .. }) {
                    dashes += 1;
                    first_dash_tick.get_or_insert(tick);
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(wall_jumps >= 4, "Nova demonstration bypassed its wall core");
        assert_eq!(dashes, 1, "Nova demonstration should cross the corona once");
        assert!(
            last_wall_jump_tick.expect("wall core is represented")
                < first_dash_tick.expect("corona Dash is represented"),
            "Nova demonstration no longer climbs before its Dash"
        );

        let return_outcome = solve_target(
            &replayed,
            SearchTarget::door("floor"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(return_solution) = return_outcome else {
            panic!("Nova Niche cannot return after its coin: {return_outcome:?}");
        };
        for action in return_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("floor"));

        let dash_only = DemoDungeonInventory {
            climbing_gloves: false,
            winged_boots: true,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, dash_only);
        let mut initial = Simulation::enter_via_door(
            room,
            dash_only.abilities(),
            spec.entry_door.expect("Nova Niche has a floor entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(dash_only.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Nova's Dash-only wall-ascent-carry route should remain honestly recorded: {outcome:?}"
        );

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, wall_only);
        let mut initial = Simulation::enter_via_door(
            room,
            wall_only.abilities(),
            spec.entry_door.expect("Nova Niche has a floor entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Nova's difficult Wall-Jump-only corona route should remain honestly recorded: {outcome:?}"
        );
    }

    #[test]
    fn constellation_hall_checked_route_follows_the_under_over_under_slalom() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::ConstellationHall)
            .expect("Constellation Hall has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door
                .expect("Constellation Hall has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut wall_jumps = 0;
        let mut dashes = 0;
        let mut landings = Vec::new();
        for action in crate::demo_dungeon_witness_actions(spec.room) {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Constellation slalom must remain clean: {event:?}"
                );
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
                if matches!(event, SimulationEvent::Landed) {
                    landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(
            wall_jumps >= 2,
            "Constellation route bypassed its central climb"
        );
        assert!(
            dashes >= 4,
            "Constellation route bypassed its slalom transfers"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (90..=132).contains(&x) && y == 128)
                && landings
                    .iter()
                    .any(|&(x, y)| (160..=212).contains(&x) && y == 58)
                && landings
                    .iter()
                    .any(|&(x, y)| (200..=240).contains(&x) && y == 128),
            "Constellation route lost its under-over-under recoveries: {landings:?}"
        );

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Constellation Hall cannot continue after its coin: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        let baseline = DemoDungeonInventory {
            climbing_gloves: false,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, baseline);
        let mut initial = Simulation::enter_via_door(
            room,
            baseline.abilities(),
            spec.entry_door
                .expect("Constellation Hall has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(baseline.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "baseline search unexpectedly crossed the Constellation slalom: {outcome:?}"
        );
    }

    #[test]
    fn shadow_duct_checked_route_uses_entry_climb_and_reward_dash_and_can_return() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::ShadowDuct)
            .expect("Shadow Duct has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Shadow Duct has a floor entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dash_ticks = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut landings = Vec::new();
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Shadow route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Dashed { .. } => dash_ticks.push(tick),
                    SimulationEvent::Jumped(JumpKind::Wall { .. }) => {
                        wall_jump_ticks.push(tick);
                    }
                    SimulationEvent::Landed => {
                        landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(
            dash_ticks.len(),
            3,
            "Shadow route should have three deliberate Dashes"
        );
        assert!(
            wall_jump_ticks.len() >= 3,
            "Shadow route bypassed its sustained climb"
        );
        assert!(
            dash_ticks[0] < wall_jump_ticks[0]
                && wall_jump_ticks.last().expect("wall climb exists")
                    < dash_ticks.last().expect("reward Dash exists"),
            "Shadow route lost its entry-Dash, climb, reward-Dash ordering"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (90..=142).contains(&x) && y == 18)
                && landings
                    .iter()
                    .any(|&(x, y)| (212..=282).contains(&x) && y == 18),
            "Shadow route lost its two broad recovery shelves: {landings:?}"
        );

        let return_outcome = solve_target(
            &replayed,
            SearchTarget::door("floor"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(return_solution) = return_outcome else {
            panic!("Shadow Duct cannot return after its coin: {return_outcome:?}");
        };
        for action in return_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("floor"));

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, wall_only);
        let mut initial = Simulation::enter_via_door(
            room,
            wall_only.abilities(),
            spec.entry_door.expect("Shadow Duct has a floor entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Wall-Jump-only search unexpectedly crossed the low Shadow aperture: {outcome:?}"
        );

        let dash_only = DemoDungeonInventory {
            climbing_gloves: false,
            winged_boots: true,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, dash_only);
        let mut initial = Simulation::enter_via_door(
            room,
            dash_only.abilities(),
            spec.entry_door.expect("Shadow Duct has a floor entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(dash_only.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("the retained Dash-only wall-carry alternative disappeared: {outcome:?}");
        };
        let mut dashes = 0;
        for action in solution.replay.actions() {
            for event in initial.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "Dash-only Shadow alternative is not a clean replay: {event:?}"
                );
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
            }
        }
        assert!(
            initial
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(
            dashes >= dash_ticks.len() * 2,
            "Dash-only wall-carry alternate became comparable to the intended mixed route"
        );
    }

    #[test]
    fn observatory_checked_route_climbs_then_crosses_both_roof_gaps_and_can_continue() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::Observatory)
            .expect("Observatory has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Observatory has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dash_ticks = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut landings = Vec::new();
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Observatory route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Dashed { .. } => dash_ticks.push(tick),
                    SimulationEvent::Jumped(JumpKind::Wall { .. }) => {
                        wall_jump_ticks.push(tick);
                    }
                    SimulationEvent::Landed => {
                        landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(
            dash_ticks.len(),
            3,
            "Observatory should demonstrate one climb assist and two roof crossings"
        );
        assert!(
            wall_jump_ticks.len() >= 3,
            "Observatory demonstration bypassed its alternating tower"
        );
        assert!(
            dash_ticks[0] < wall_jump_ticks[0]
                && wall_jump_ticks.last().expect("tower climb exists") < &dash_ticks[1],
            "Observatory lost its climb-assist, wall-rhythm, roof-crossing order"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (190..=222).contains(&x) && y == 18)
                && landings
                    .iter()
                    .any(|&(x, y)| (100..=122).contains(&x) && y == 48)
                && landings
                    .iter()
                    .any(|&(x, y)| (10..=48).contains(&x) && y == 18),
            "Observatory route lost its three full roof recoveries: {landings:?}"
        );

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Observatory cannot continue after its coin: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(
                room,
                inventory.abilities(),
                spec.entry_door.expect("Observatory has a west entry"),
            )
            .unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                route_spec_target(spec.target),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Observatory coin: {outcome:?}"
            );
        }
    }

    #[test]
    fn gravity_lift_checked_route_uses_all_three_nonlethal_switchbacks_and_can_descend() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::GravityLift)
            .expect("Gravity Lift has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room.clone(),
            spec.inventory.abilities(),
            spec.entry_door.expect("Gravity Lift has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dashes = 0;
        let mut wall_jumps = 0;
        let mut landings = Vec::new();
        for action in crate::demo_dungeon_witness_actions(spec.room) {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Gravity Lift route must remain clean: {event:?}"
                );
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                if matches!(event, SimulationEvent::Landed) {
                    landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("ceiling"));
        assert_eq!(dashes, 3, "Gravity Lift should use one Dash per rise");
        assert!(wall_jumps >= 2, "Gravity Lift bypassed both end-wall kicks");
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (240..=272).contains(&x) && y == 118)
                && landings
                    .iter()
                    .any(|&(x, y)| (40..=72).contains(&x) && y == 78)
                && landings
                    .iter()
                    .any(|&(x, y)| (240..=272).contains(&x) && y == 38),
            "Gravity Lift route lost its right-left-right recoveries: {landings:?}"
        );

        let mut descending =
            Simulation::enter_via_door(room, spec.inventory.abilities(), "ceiling").unwrap();
        descending.enable_current_player_movement();
        let return_outcome = solve_target(
            &descending,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(return_solution) = return_outcome else {
            panic!("Gravity Lift ceiling branch cannot descend: {return_outcome:?}");
        };
        for action in return_solution.replay.actions() {
            descending.step(action);
        }
        assert_eq!(descending.reached_exit(), Some("east"));

        let baseline = DemoDungeonInventory {
            climbing_gloves: false,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, baseline);
        let mut initial = Simulation::enter_via_door(
            room,
            baseline.abilities(),
            spec.entry_door.expect("Gravity Lift has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(baseline.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "baseline search unexpectedly climbed the Gravity Lift: {outcome:?}"
        );
    }

    #[test]
    fn aurora_spire_checked_route_climbs_then_crosses_and_can_continue() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::AuroraSpire)
            .expect("Aurora Spire has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Aurora Spire has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dash_ticks = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut landings = Vec::new();
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Aurora route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Dashed { .. } => dash_ticks.push(tick),
                    SimulationEvent::Jumped(JumpKind::Wall { .. }) => wall_jump_ticks.push(tick),
                    SimulationEvent::Landed => {
                        landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(
            dash_ticks.len(),
            1,
            "Aurora should cross its light sheet once"
        );
        assert!(
            wall_jump_ticks.len() >= 3,
            "Aurora route bypassed its alternating core"
        );
        assert!(
            wall_jump_ticks.last().expect("Aurora climb exists")
                < dash_ticks.first().expect("Aurora crossing exists"),
            "Aurora route no longer climbs before crossing"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (276..=302).contains(&x) && y == 58),
            "Aurora route lost its full upper recovery: {landings:?}"
        );

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Aurora Spire cannot continue after its coin: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        let wall_only = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, wall_only);
        let mut initial = Simulation::enter_via_door(
            room,
            wall_only.abilities(),
            spec.entry_door.expect("Aurora Spire has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(wall_only.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Aurora's retained Wall-Jump-only alternate disappeared: {outcome:?}");
        };
        let mut alternate_wall_jumps = 0;
        for action in solution.replay.actions() {
            for event in initial.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "Aurora Wall-Jump-only alternate is not clean: {event:?}"
                );
                alternate_wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
            }
        }
        assert!(
            initial
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert!(
            alternate_wall_jumps >= wall_jump_ticks.len(),
            "Aurora's Dash-free crossing became simpler than its readable mixed route"
        );
    }

    #[test]
    fn skybridge_checked_route_goes_over_then_under_and_can_continue() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::Skybridge)
            .expect("Skybridge has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Skybridge has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dash_ticks = Vec::new();
        let mut wall_jump_ticks = Vec::new();
        let mut wall_sides = Vec::new();
        let mut landings = Vec::new();
        for (tick, action) in crate::demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Skybridge route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Dashed { .. } => dash_ticks.push(tick),
                    SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                        wall_jump_ticks.push(tick);
                        wall_sides.push(side);
                    }
                    SimulationEvent::Landed => {
                        landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                    }
                    _ => {}
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(
            dash_ticks.len(),
            1,
            "Skybridge should descend with one Dash"
        );
        assert_eq!(wall_jump_ticks.len(), 3, "Skybridge climb lost its rhythm");
        assert!(
            wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Skybridge climb stopped alternating sides: {wall_sides:?}"
        );
        assert!(
            wall_jump_ticks.last().expect("Skybridge climb exists")
                < dash_ticks.first().expect("Skybridge drop exists"),
            "Skybridge no longer climbs before dropping beneath the hanging mast"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (40..=102).contains(&x) && y == 118)
                && landings
                    .iter()
                    .any(|&(x, y)| (110..=172).contains(&x) && y == 38)
                && landings
                    .iter()
                    .any(|&(x, y)| (200..=272).contains(&x) && y == 118),
            "Skybridge route lost its lower-roof-lower recoveries: {landings:?}"
        );

        let exit_outcome = solve_target(
            &replayed,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(exit_solution) = exit_outcome else {
            panic!("Skybridge cannot continue after its coin: {exit_outcome:?}");
        };
        for action in exit_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("east"));

        let baseline = DemoDungeonInventory {
            climbing_gloves: false,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, baseline);
        let mut initial = Simulation::enter_via_door(
            room,
            baseline.abilities(),
            spec.entry_door.expect("Skybridge has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(baseline.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "baseline search unexpectedly crossed the broken Skybridge: {outcome:?}"
        );
    }

    #[test]
    fn crown_sanctum_checked_route_completes_both_climbs_and_the_low_passage() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::CrownSanctum)
            .expect("Crown Sanctum has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Crown Sanctum has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(spec.room);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_jump_ticks = Vec::new();
        let mut dash_ticks = Vec::new();
        let mut dash_positions = Vec::new();
        let mut landings = Vec::new();
        let mut crown_tick = None;
        let mut exit_tick = None;
        for (tick, action) in actions.into_iter().enumerate() {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Crown route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if matches!(kind, JumpKind::Wall { .. }) {
                            wall_jump_ticks.push(tick);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        dash_ticks.push(tick);
                        dash_positions.push(replayed.player().bounds());
                    }
                    SimulationEvent::Landed => {
                        landings.push((replayed.player().bounds().x, replayed.player().bounds().y));
                    }
                    SimulationEvent::PickupCollected { id } if id == DEMO_DUNGEON_CROWN_PICKUP => {
                        crown_tick = Some(tick);
                    }
                    SimulationEvent::ExitReached { id } if id == DEMO_DUNGEON_GOAL_EXIT => {
                        exit_tick = Some(tick);
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some(DEMO_DUNGEON_GOAL_EXIT));
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_CROWN_PICKUP)
        );
        assert_eq!(
            jump_presses, accepted_jumps,
            "Crown witness contains jump spam"
        );
        assert!(
            accepted_jumps >= 8,
            "Crown route no longer demonstrates the full capstone: {accepted_jumps} jumps"
        );
        assert!(
            wall_jump_ticks.len() >= 6,
            "Crown route bypassed a climb: {wall_jump_ticks:?}"
        );
        assert_eq!(
            dash_ticks.len(),
            1,
            "Crown route should cross the low passage once"
        );
        assert!(
            (170..=178).contains(&dash_positions[0].x)
                && (120..=122).contains(&dash_positions[0].y),
            "Crown Dash no longer enters the ten-pixel passage: {:?}",
            dash_positions[0]
        );
        let wall_jumps_before_dash = wall_jump_ticks
            .iter()
            .filter(|&&tick| tick < dash_ticks[0])
            .count();
        let wall_jumps_after_dash = wall_jump_ticks.len() - wall_jumps_before_dash;
        assert!(
            wall_jumps_before_dash >= 2 && wall_jumps_after_dash >= 3,
            "Crown route lost its climb-Dash-climb ordering: {wall_jump_ticks:?} / {dash_ticks:?}"
        );
        assert!(
            crown_tick.expect("Crown is collected") < exit_tick.expect("goal exit is reached"),
            "terminal trigger fired before the Crown pickup"
        );
        assert!(
            landings
                .iter()
                .any(|&(x, y)| (110..=162).contains(&x) && y == 38)
                && landings
                    .iter()
                    .any(|&(x, y)| (190..=272).contains(&x) && y == 118),
            "Crown route lost its full inter-act recoveries: {landings:?}"
        );
    }

    #[test]
    fn crown_sanctum_refuses_each_incomplete_traversal_loadout() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::CrownSanctum)
            .expect("Crown Sanctum has route metadata");
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..spec.inventory
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..spec.inventory
                },
            ),
        ] {
            let room = demo_dungeon_room(spec.room, inventory);
            let mut initial = Simulation::enter_via_door(
                room,
                inventory.abilities(),
                spec.entry_door.expect("Crown Sanctum has a west entry"),
            )
            .unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                route_spec_target(spec.target),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly reached the Crown: {outcome:?}"
            );
        }
    }

    #[test]
    fn meteor_run_is_a_readable_three_shutter_dash_timing_course() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::MeteorRun)
            .expect("Meteor Run has route metadata");
        assert!(spec.inventory.winged_boots);
        let room = demo_dungeon_room(spec.room, spec.inventory);
        assert_eq!(room.timed_hazards().len(), 3);
        assert!(room.timed_hazards().iter().all(|hazard| {
            hazard.period_ticks() == 96
                && hazard.active_ticks() == 70
                && hazard.bounds().height == 170
        }));

        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Meteor Run has a west entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dash_positions = Vec::new();
        for action in crate::demo_dungeon_witness_actions(spec.room) {
            let x_before = replayed.player().bounds().x;
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Meteor Run demonstration must remain clean: {event:?}"
                );
                assert!(
                    !matches!(event, SimulationEvent::Jumped(_)),
                    "Meteor Run should demonstrate shutter timing rather than jump spam"
                );
                if matches!(event, SimulationEvent::Dashed { .. }) {
                    dash_positions.push(x_before);
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert_eq!(
            dash_positions.len(),
            3,
            "the checked route should commit once through each shutter: {dash_positions:?}"
        );
        for (position, launch_range) in dash_positions.into_iter().zip([30..52, 105..134, 185..216])
        {
            assert!(
                launch_range.contains(&position),
                "Meteor Dash launches outside its readable safe bay: {position}"
            );
        }

        let no_dash = DemoDungeonInventory {
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, no_dash);
        let mut initial = Simulation::enter_via_door(
            room,
            no_dash.abilities(),
            spec.entry_door.expect("Meteor Run has a west entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(no_dash.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "no-Dash search unexpectedly crossed the Meteor shutters: {outcome:?}"
        );
    }

    #[test]
    fn moon_vault_checked_route_follows_its_orbit_and_can_return() {
        let spec = demo_dungeon_route_specs()
            .into_iter()
            .find(|spec| spec.room == DemoDungeonRoom::MoonVault)
            .expect("Moon Vault has route metadata");
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut replayed = Simulation::enter_via_door(
            room,
            spec.inventory.abilities(),
            spec.entry_door.expect("Moon Vault has a ceiling entry"),
        )
        .unwrap();
        replayed.enable_current_player_movement();
        let mut dashes = 0;
        let mut wall_jumps = 0;
        let mut landed = Vec::new();
        let mut previous_nonzero_x = 0;
        let mut reversals = 0;
        for action in crate::demo_dungeon_witness_actions(spec.room) {
            if action.move_x != 0 {
                reversals +=
                    usize::from(previous_nonzero_x != 0 && previous_nonzero_x != action.move_x);
                previous_nonzero_x = action.move_x;
            }
            for event in replayed.step(action).events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Moon orbit must remain clean: {event:?}"
                );
                dashes += usize::from(matches!(event, SimulationEvent::Dashed { .. }));
                wall_jumps += usize::from(matches!(
                    event,
                    SimulationEvent::Jumped(JumpKind::Wall { .. })
                ));
                if matches!(event, SimulationEvent::Landed) {
                    landed.push((replayed.player().bounds().x, replayed.player().bounds().y));
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == spec.target.id())
        );
        assert_eq!(dashes, 3, "Moon orbit should use one Dash per transfer");
        assert_eq!(
            wall_jumps, 1,
            "Moon orbit should finish with one visible boundary kick"
        );
        assert_eq!(
            reversals, 0,
            "Moon orbit should not contain controller thrash"
        );
        assert!(
            landed.iter().any(|&(_, y)| y == 128)
                && landed.iter().any(|&(x, y)| x >= 215 && y == 88),
            "Moon orbit lost its low and rising recovery landings: {landed:?}"
        );

        let return_outcome = solve_target(
            &replayed,
            SearchTarget::door("ceiling"),
            &SolverConfig::for_abilities(spec.inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(return_solution) = return_outcome else {
            panic!("Moon Vault cannot return after collecting its coin: {return_outcome:?}");
        };
        for action in return_solution.replay.actions() {
            replayed.step(action);
        }
        assert_eq!(replayed.reached_exit(), Some("ceiling"));

        let no_dash = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: false,
            ..spec.inventory
        };
        let room = demo_dungeon_room(spec.room, no_dash);
        let mut initial = Simulation::enter_via_door(
            room,
            no_dash.abilities(),
            spec.entry_door.expect("Moon Vault has a ceiling entry"),
        )
        .unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            route_spec_target(spec.target),
            &SolverConfig::for_abilities(no_dash.abilities()),
        )
        .unwrap();
        assert!(
            !matches!(outcome, TargetSolveOutcome::Solved(_)),
            "Wall-Jump-only search unexpectedly bypassed the Moon orbit: {outcome:?}"
        );
    }

    #[test]
    fn winged_boots_require_a_real_climb_in_the_known_positive_route() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_BOOT_GATE_REQUIREMENT,
            )
        };
        let room = demo_dungeon_room(DemoDungeonRoom::BootsVault, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "floor").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::pickup(DEMO_DUNGEON_BOOT_PICKUP),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Winged Boots climb lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                if let SimulationEvent::Jumped(kind) = event {
                    accepted_jumps += 1;
                    if let JumpKind::Wall { side } = kind {
                        wall_sides.push(side);
                    }
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            accepted_jumps >= 5 && wall_sides.len() >= 4,
            "the boots route must contain the alternating wall climb, got {accepted_jumps} jumps / {} wall jumps",
            wall_sides.len()
        );
        let side_changes = wall_sides
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count();
        assert!(
            side_changes >= 2,
            "the boots route must make multiple rapid wall-to-wall turns: {wall_sides:?}"
        );
    }

    #[test]
    fn climbing_gloves_require_a_baseline_platform_route_before_wall_jump_exists() {
        let inventory = DemoDungeonInventory::default();
        let room = demo_dungeon_room(DemoDungeonRoom::ClimberVault, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::pickup(DEMO_DUNGEON_GLOVE_PICKUP),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Climbing Gloves route lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut accepted_jumps = 0;
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                if let SimulationEvent::Jumped(kind) = event {
                    assert!(
                        !matches!(kind, JumpKind::Wall { .. }),
                        "the pre-unlock route cannot use Wall Jump"
                    );
                    accepted_jumps += 1;
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_GLOVE_PICKUP)
        );
        assert!(
            accepted_jumps >= 3,
            "the gloves should require a real baseline platform route, got {accepted_jumps} jumps"
        );
    }

    #[test]
    fn required_coin_branches_are_solver_tractable() {
        let gloves = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::default()
        };
        for (room, entry, targets) in [
            (
                DemoDungeonRoom::CoinLoft,
                "ceiling",
                vec![
                    SearchTarget::pickup(coin_id(8)),
                    SearchTarget::pickup(coin_id(9)),
                ],
            ),
            (
                DemoDungeonRoom::NeedleRoom,
                "floor",
                vec![SearchTarget::pickup(coin_id(11))],
            ),
        ] {
            for target in targets {
                assert_route(room, Some(entry), gloves, target);
            }
        }
        let boots = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::default()
        };
        for target in [
            SearchTarget::pickup(coin_id(14)),
            SearchTarget::pickup(coin_id(15)),
        ] {
            assert_route(DemoDungeonRoom::Treasury, Some("west"), boots, target);
        }
        assert_route(
            DemoDungeonRoom::CoinDuct,
            Some("ceiling"),
            boots,
            SearchTarget::pickup(coin_id(24)),
        );
        assert_route(
            DemoDungeonRoom::StormCache,
            Some("floor"),
            boots,
            SearchTarget::pickup(coin_id(26)),
        );
    }

    #[test]
    fn required_coin_branches_can_return_after_their_deepest_pickup() {
        let empty = DemoDungeonInventory::default();
        let root_cellar = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::RootCellar, empty),
            empty.abilities(),
            "ceiling",
        )
        .unwrap();
        let root_cellar = solve_and_advance(root_cellar, SearchTarget::pickup(coin_id(3)));
        let root_cellar = solve_and_advance(root_cellar, SearchTarget::door("ceiling"));
        assert_eq!(root_cellar.reached_exit(), Some("ceiling"));

        let watch_post = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::WatchPost, empty),
            empty.abilities(),
            "floor",
        )
        .unwrap();
        let watch_post = solve_and_advance(watch_post, SearchTarget::pickup(coin_id(5)));
        let watch_post = solve_and_advance(watch_post, SearchTarget::door("floor"));
        assert_eq!(watch_post.reached_exit(), Some("floor"));

        let gloves = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::default()
        };
        let bell_niche = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::BellNiche, gloves),
            gloves.abilities(),
            "ceiling",
        )
        .unwrap();
        let bell_niche = solve_and_advance(bell_niche, SearchTarget::pickup(coin_id(18)));
        let bell_niche = solve_and_advance(bell_niche, SearchTarget::door("ceiling"));
        assert_eq!(bell_niche.reached_exit(), Some("ceiling"));

        let rafter_shrine = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::RafterShrine, gloves),
            gloves.abilities(),
            "floor",
        )
        .unwrap();
        let rafter_shrine = solve_and_advance(rafter_shrine, SearchTarget::pickup(coin_id(20)));
        let rafter_shrine = solve_and_advance(rafter_shrine, SearchTarget::door("floor"));
        assert_eq!(rafter_shrine.reached_exit(), Some("floor"));

        let coin_loft = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::CoinLoft, gloves),
            gloves.abilities(),
            "ceiling",
        )
        .unwrap();
        let coin_loft = solve_and_advance(coin_loft, SearchTarget::pickup(coin_id(9)));
        let coin_loft = solve_and_advance(coin_loft, SearchTarget::door("ceiling"));
        assert_eq!(coin_loft.reached_exit(), Some("ceiling"));

        let needle = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::NeedleRoom, gloves),
            gloves.abilities(),
            "floor",
        )
        .unwrap();
        let needle = solve_and_advance(needle, SearchTarget::pickup(coin_id(11)));
        let needle = solve_and_advance(needle, SearchTarget::door("floor"));
        assert_eq!(needle.reached_exit(), Some("floor"));

        let treasury_ready = inventory_with_coin_indices((0..14).chain(16..22), true, true);
        let treasury = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::Treasury, treasury_ready),
            treasury_ready.abilities(),
            "west",
        )
        .unwrap();
        let treasury = solve_and_advance(treasury, SearchTarget::pickup(coin_id(15)));
        let treasury = solve_and_advance(treasury, SearchTarget::door("west"));
        assert_eq!(treasury.reached_exit(), Some("west"));

        let dash_ready = inventory_with_coin_indices(0..22, true, true);
        let coin_duct = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::CoinDuct, dash_ready),
            dash_ready.abilities(),
            "ceiling",
        )
        .unwrap();
        let coin_duct = solve_and_advance(coin_duct, SearchTarget::pickup(coin_id(24)));
        let coin_duct = solve_and_advance(coin_duct, SearchTarget::door("ceiling"));
        assert_eq!(coin_duct.reached_exit(), Some("ceiling"));

        let storm_cache = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::StormCache, dash_ready),
            dash_ready.abilities(),
            "floor",
        )
        .unwrap();
        let storm_cache = solve_and_advance(storm_cache, SearchTarget::pickup(coin_id(26)));
        let storm_cache = solve_and_advance(storm_cache, SearchTarget::door("floor"));
        assert_eq!(storm_cache.reached_exit(), Some("floor"));
    }

    #[test]
    fn dash_chasm_has_no_known_wall_jump_only_route_under_the_same_search_budget() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::DashChasm, inventory);
        let mut simulation =
            Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Inconclusive { .. }),
            "wall-jump-only search unexpectedly crossed the Dash gate: {outcome:?}"
        );
    }

    #[test]
    fn wall_region_gate_requires_an_observed_multi_wall_climb() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::WallGate, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Wall-region gate lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut wall_sides = Vec::new();
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                if let SimulationEvent::Jumped(JumpKind::Wall { side }) = event {
                    wall_sides.push(side);
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            wall_sides.len() >= 4
                && wall_sides.contains(&downwards_core::WallSide::Left)
                && wall_sides.contains(&downwards_core::WallSide::Right),
            "the mandatory wall gate must use both walls in a substantial climb: {wall_sides:?}"
        );

        let baseline = DemoDungeonInventory::default();
        let room = demo_dungeon_room(DemoDungeonRoom::WallGate, baseline);
        let mut initial = Simulation::enter_via_door(room, baseline.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(baseline.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Inconclusive { .. }),
            "baseline search unexpectedly crossed the mandatory Wall Jump gate: {outcome:?}"
        );
    }

    #[test]
    fn dash_region_seal_has_an_observed_dash_and_no_wall_jump_only_positive() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::DashSeal, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Dash-region seal lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut accepted_dashes = 0;
        for action in solution.replay.actions() {
            let report = replayed.step(action);
            accepted_dashes += report
                .events
                .iter()
                .filter(|event| matches!(event, SimulationEvent::Dashed { .. }))
                .count();
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            accepted_dashes >= 1,
            "the mandatory Dash seal route must actually use Dash"
        );

        let reduced = DemoDungeonInventory {
            climbing_gloves: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::DashSeal, reduced);
        let mut initial = Simulation::enter_via_door(room, reduced.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(reduced.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Inconclusive { .. }),
            "WallJump-only search unexpectedly crossed the mandatory Dash seal: {outcome:?}"
        );
    }

    #[test]
    fn gatehouse_checked_route_alternates_up_the_shaft_then_uses_one_low_dash() {
        let inventory = DemoDungeonInventory {
            climbing_gloves: true,
            winged_boots: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::Gatehouse, inventory);
        let mut replayed = Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        replayed.enable_current_player_movement();
        let actions = crate::demo_dungeon_witness_actions(DemoDungeonRoom::Gatehouse);
        let mut previous = downwards_core::Action::default();
        let mut jump_presses = 0;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        let mut dash_positions = Vec::new();
        let mut observed_low_posture = false;
        for action in actions {
            jump_presses += usize::from(action.jump && !previous.jump);
            previous = action;
            let report = replayed.step(action);
            observed_low_posture |= replayed.player().dash_compressed();
            for event in report.events {
                assert!(
                    !matches!(event, SimulationEvent::Died(_) | SimulationEvent::Reset),
                    "the checked Gatehouse route must remain clean: {event:?}"
                );
                match event {
                    SimulationEvent::Jumped(kind) => {
                        accepted_jumps += 1;
                        if let JumpKind::Wall { side } = kind {
                            wall_sides.push(side);
                        }
                    }
                    SimulationEvent::Dashed { .. } => {
                        dash_positions.push(replayed.player().bounds());
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert_eq!(
            jump_presses, accepted_jumps,
            "Gatehouse witness has jump spam"
        );
        assert!(
            wall_sides.len() >= 3 && wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "Gatehouse climb no longer alternates across the shaft: {wall_sides:?}"
        );
        assert_eq!(
            dash_positions.len(),
            1,
            "Gatehouse should use one deliberate Dash: {dash_positions:?}"
        );
        assert!(
            (130..=140).contains(&dash_positions[0].x) && (50..=54).contains(&dash_positions[0].y),
            "Gatehouse Dash no longer enters the upper keyhole: {:?}",
            dash_positions[0]
        );
        assert!(
            observed_low_posture,
            "the gatehouse route must traverse its one-tile passage in low Dash posture"
        );
    }

    #[test]
    fn gatehouse_refuses_each_incomplete_traversal_loadout() {
        for (label, inventory) in [
            (
                "Wall-Jump-only",
                DemoDungeonInventory {
                    climbing_gloves: true,
                    winged_boots: false,
                    ..DemoDungeonInventory::default()
                },
            ),
            (
                "Dash-only",
                DemoDungeonInventory {
                    climbing_gloves: false,
                    winged_boots: true,
                    ..DemoDungeonInventory::default()
                },
            ),
        ] {
            let room = demo_dungeon_room(DemoDungeonRoom::Gatehouse, inventory);
            let mut initial =
                Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
            initial.enable_current_player_movement();
            let outcome = solve_target(
                &initial,
                SearchTarget::door("east"),
                &SolverConfig::for_abilities(inventory.abilities()),
            )
            .unwrap();
            assert!(
                !matches!(outcome, TargetSolveOutcome::Solved(_)),
                "{label} search unexpectedly crossed the Gatehouse: {outcome:?}"
            );
        }
    }
}
