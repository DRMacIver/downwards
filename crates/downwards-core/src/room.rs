use std::{error::Error, fmt};

use crate::{
    Point, Rect,
    simulation::{PLAYER_HEIGHT, PLAYER_WIDTH},
};

pub const ROOM_WIDTH_PIXELS: i32 = 320;
pub const ROOM_HEIGHT_PIXELS: i32 = 180;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Tile {
    #[default]
    Empty = 0,
    Solid = 1,
    HazardUp = 2,
    /// A platform that collides only with a player crossing its top while
    /// moving down. It can be jumped through from below.
    OneWay = 3,
    HazardDown = 4,
    HazardLeft = 5,
    HazardRight = 6,
}

impl Tile {
    #[must_use]
    pub const fn is_hazard(self) -> bool {
        matches!(
            self,
            Self::HazardUp | Self::HazardDown | Self::HazardLeft | Self::HazardRight
        )
    }

    #[must_use]
    pub const fn hazard_direction(self) -> Option<HazardDirection> {
        match self {
            Self::HazardUp => Some(HazardDirection::Up),
            Self::HazardDown => Some(HazardDirection::Down),
            Self::HazardLeft => Some(HazardDirection::Left),
            Self::HazardRight => Some(HazardDirection::Right),
            Self::Empty | Self::Solid | Self::OneWay => None,
        }
    }
}

/// Direction in which a hazard tile's lethal points face.
///
/// Hazard tiles remain solid obstacles from their rear and side faces. The direction is authored
/// directly as part of the tile placement, so rendering and collision consume the same explicit
/// room data without guessing from neighbouring geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HazardDirection {
    Up,
    Down,
    Left,
    Right,
}

/// A rectangular hazard whose activation is derived solely from the room
/// clock. Fields are private so every instance satisfies its timing and
/// screen-bounds invariants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedHazard {
    bounds: Rect,
    period_ticks: u32,
    active_ticks: u32,
    phase_ticks: u32,
}

impl TimedHazard {
    pub fn new(
        bounds: Rect,
        period_ticks: u32,
        active_ticks: u32,
        phase_ticks: u32,
    ) -> Result<Self, RoomObjectError> {
        validate_object_bounds(bounds)?;
        if period_ticks == 0 {
            return Err(RoomObjectError::ZeroHazardPeriod);
        }
        if active_ticks == 0 || active_ticks > period_ticks {
            return Err(RoomObjectError::InvalidHazardActiveTicks {
                active_ticks,
                period_ticks,
            });
        }
        if phase_ticks >= period_ticks {
            return Err(RoomObjectError::InvalidHazardPhase {
                phase_ticks,
                period_ticks,
            });
        }
        Ok(Self {
            bounds,
            period_ticks,
            active_ticks,
            phase_ticks,
        })
    }

    #[must_use]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    #[must_use]
    pub const fn period_ticks(&self) -> u32 {
        self.period_ticks
    }

    #[must_use]
    pub const fn active_ticks(&self) -> u32 {
        self.active_ticks
    }

    /// Offset into the hazard's cycle at room tick zero.
    #[must_use]
    pub const fn phase_ticks(&self) -> u32 {
        self.phase_ticks
    }

    #[must_use]
    pub fn is_active_at(&self, room_tick: u64) -> bool {
        let period = u64::from(self.period_ticks);
        (room_tick % period + u64::from(self.phase_ticks)) % period < u64::from(self.active_ticks)
    }
}

/// A room-local collectible. Collection has no built-in gameplay effect;
/// scripts and higher-level run state interpret its stable id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pickup {
    id: String,
    bounds: Rect,
}

impl Pickup {
    pub fn new(id: impl Into<String>, bounds: Rect) -> Result<Self, RoomObjectError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(RoomObjectError::EmptyPickupId);
        }
        validate_object_bounds(bounds)?;
        Ok(Self { id, bounds })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomObjectError {
    InvalidBounds(Rect),
    ZeroHazardPeriod,
    InvalidHazardActiveTicks {
        active_ticks: u32,
        period_ticks: u32,
    },
    InvalidHazardPhase {
        phase_ticks: u32,
        period_ticks: u32,
    },
    EmptyPickupId,
}

impl fmt::Display for RoomObjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds(bounds) => write!(
                formatter,
                "room object bounds ({}, {}, {}, {}) must be positive and fit within the screen",
                bounds.x, bounds.y, bounds.width, bounds.height
            ),
            Self::ZeroHazardPeriod => write!(formatter, "timed hazard period must be positive"),
            Self::InvalidHazardActiveTicks {
                active_ticks,
                period_ticks,
            } => write!(
                formatter,
                "timed hazard active duration must be in 1..={period_ticks}, got {active_ticks}"
            ),
            Self::InvalidHazardPhase {
                phase_ticks,
                period_ticks,
            } => write!(
                formatter,
                "timed hazard phase must be less than period {period_ticks}, got {phase_ticks}"
            ),
            Self::EmptyPickupId => write!(formatter, "pickup id must not be empty"),
        }
    }
}

impl Error for RoomObjectError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    pub id: String,
    pub bounds: Rect,
    pub destination: Option<String>,
    pub destination_entrance: Option<String>,
}

/// The screen boundary occupied by a room-to-room door.
///
/// `Ceiling` and `Floor` are named for their physical placement rather than
/// the direction of travel. Keeping this explicit lets dungeon assembly match
/// opposite sides without inferring topology from coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum BoundarySide {
    Left = 0,
    Right = 1,
    Ceiling = 2,
    Floor = 3,
}

impl BoundarySide {
    /// Boundary a directly adjacent room must expose for a connection.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Ceiling => Self::Floor,
            Self::Floor => Self::Ceiling,
        }
    }
}

/// Canonical boundary aperture used when assembling adjacent rooms.
///
/// Internal trigger depth and arrival placement are deliberately absent: two
/// rooms tile together when their opposite boundary openings have the same
/// offset and span, even if their safe landing geometry differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoorSocket {
    pub side: BoundarySide,
    pub offset: i32,
    pub span: i32,
}

impl DoorSocket {
    /// Socket an adjacent room must expose at this connection.
    #[must_use]
    pub const fn mate(self) -> Self {
        Self {
            side: self.side.opposite(),
            offset: self.offset,
            span: self.span,
        }
    }

    /// Whether these sockets are opposite halves of one aligned doorway.
    #[must_use]
    pub const fn matches(self, other: Self) -> bool {
        self.side.opposite() as u8 == other.side as u8
            && self.offset == other.offset
            && self.span == other.span
    }
}

/// A validated connection point on a room boundary.
///
/// The trigger is a half-open rectangle immediately inside `side`. `arrival`
/// is the top-left of the player after entering this room through this door;
/// it must be clear and must not overlap any door trigger. Destinations are
/// optional while rooms are generated in isolation, but when connected both
/// destination fields must be present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Door {
    pub id: String,
    pub side: BoundarySide,
    pub trigger_bounds: Rect,
    pub arrival: Point,
    pub destination_room: Option<String>,
    pub destination_door: Option<String>,
}

impl Door {
    /// Canonical aperture identity used by dungeon assembly and catalogue
    /// coverage checks.
    #[must_use]
    pub const fn socket(&self) -> DoorSocket {
        DoorSocket {
            side: self.side,
            offset: self.aperture_offset(),
            span: self.aperture_span(),
        }
    }

    /// Pixel offset of the opening along its boundary.
    #[must_use]
    pub const fn aperture_offset(&self) -> i32 {
        match self.side {
            BoundarySide::Left | BoundarySide::Right => self.trigger_bounds.y,
            BoundarySide::Ceiling | BoundarySide::Floor => self.trigger_bounds.x,
        }
    }

    /// Pixel span of the opening along its boundary.
    #[must_use]
    pub const fn aperture_span(&self) -> i32 {
        match self.side {
            BoundarySide::Left | BoundarySide::Right => self.trigger_bounds.height,
            BoundarySide::Ceiling | BoundarySide::Floor => self.trigger_bounds.width,
        }
    }

    /// Whether two doors can be aligned across adjacent equal-sized rooms.
    ///
    /// Destination IDs are deliberately ignored: dungeon assembly can use
    /// this geometric predicate before assigning room identities.
    #[must_use]
    pub const fn geometrically_matches(&self, other: &Self) -> bool {
        self.socket().matches(other.socket())
    }
}

/// Validation failures specific to attaching boundary doors to a room.
///
/// This is separate from [`RoomError`] so the long-standing exhaustive public
/// `RoomError` API remains source-compatible for content loaders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorError {
    EmptyId,
    DuplicateId(String),
    IdConflictsWithExit(String),
    InvalidTriggerBounds {
        id: String,
        bounds: Rect,
    },
    TriggerNotAdjacentToSide {
        id: String,
        side: BoundarySide,
        bounds: Rect,
    },
    IncompleteDestination {
        id: String,
    },
    EmptyDestinationRoom {
        id: String,
    },
    EmptyDestinationDoor {
        id: String,
    },
    ArrivalOutOfBounds {
        id: String,
        arrival: Point,
    },
    ArrivalBlocked {
        id: String,
        arrival: Point,
    },
    ArrivalBlockedByTimedHazard {
        id: String,
        hazard_index: usize,
    },
    ArrivalOverlapsTrigger {
        id: String,
        trigger_id: String,
    },
}

impl fmt::Display for DoorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(formatter, "door id must not be empty"),
            Self::DuplicateId(id) => write!(formatter, "duplicate door id {id:?}"),
            Self::IdConflictsWithExit(id) => {
                write!(formatter, "door id {id:?} conflicts with an exit id")
            }
            Self::InvalidTriggerBounds { id, bounds } => write!(
                formatter,
                "door {id:?} has invalid trigger bounds ({}, {}, {}, {})",
                bounds.x, bounds.y, bounds.width, bounds.height
            ),
            Self::TriggerNotAdjacentToSide { id, side, bounds } => write!(
                formatter,
                "door {id:?} trigger ({}, {}, {}, {}) is not adjacent to its {side:?} boundary",
                bounds.x, bounds.y, bounds.width, bounds.height
            ),
            Self::IncompleteDestination { id } => write!(
                formatter,
                "door {id:?} must set both destination_room and destination_door, or neither"
            ),
            Self::EmptyDestinationRoom { id } => {
                write!(formatter, "door {id:?} destination room must not be empty")
            }
            Self::EmptyDestinationDoor { id } => {
                write!(formatter, "door {id:?} destination door must not be empty")
            }
            Self::ArrivalOutOfBounds { id, arrival } => write!(
                formatter,
                "door {id:?} arrival ({}, {}) does not fit in the room",
                arrival.x, arrival.y
            ),
            Self::ArrivalBlocked { id, arrival } => write!(
                formatter,
                "door {id:?} arrival ({}, {}) overlaps a non-empty tile",
                arrival.x, arrival.y
            ),
            Self::ArrivalBlockedByTimedHazard { id, hazard_index } => write!(
                formatter,
                "door {id:?} arrival overlaps timed hazard at index {hazard_index}"
            ),
            Self::ArrivalOverlapsTrigger { id, trigger_id } => write!(
                formatter,
                "door {id:?} arrival overlaps door trigger {trigger_id:?}"
            ),
        }
    }
}

impl Error for DoorError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) tile_size: i32,
    pub(crate) tiles: Vec<Tile>,
    pub(crate) spawn: Point,
    pub(crate) exits: Vec<Exit>,
    pub(crate) doors: Vec<Door>,
    pub(crate) timed_hazards: Vec<TimedHazard>,
    pub(crate) pickups: Vec<Pickup>,
    content_digest: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomError {
    EmptyId,
    InvalidTileSize(i32),
    WrongPixelSize { width: i64, height: i64 },
    WrongTileCount { expected: usize, actual: usize },
    SpawnOutOfBounds(Point),
    SpawnBlocked(Point),
    EmptyExitId,
    DuplicateExitId(String),
    InvalidExitBounds { id: String, bounds: Rect },
    IncompleteDestination { id: String },
    SpawnBlockedByTimedHazard { index: usize },
    DuplicatePickupId(String),
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "room id must not be empty"),
            Self::InvalidTileSize(size) => write!(f, "tile size must be positive, got {size}"),
            Self::WrongPixelSize { width, height } => write!(
                f,
                "room must be {ROOM_WIDTH_PIXELS}x{ROOM_HEIGHT_PIXELS} pixels, got {width}x{height}"
            ),
            Self::WrongTileCount { expected, actual } => {
                write!(f, "room needs {expected} tiles, got {actual}")
            }
            Self::SpawnOutOfBounds(point) => {
                write!(
                    f,
                    "spawn ({}, {}) does not fit in the room",
                    point.x, point.y
                )
            }
            Self::SpawnBlocked(point) => {
                write!(
                    f,
                    "spawn ({}, {}) overlaps a solid or hazard tile",
                    point.x, point.y
                )
            }
            Self::EmptyExitId => write!(f, "exit id must not be empty"),
            Self::DuplicateExitId(id) => write!(f, "duplicate exit id {id:?}"),
            Self::InvalidExitBounds { id, bounds } => write!(
                f,
                "exit {id:?} has invalid bounds ({}, {}, {}, {})",
                bounds.x, bounds.y, bounds.width, bounds.height
            ),
            Self::IncompleteDestination { id } => write!(
                f,
                "exit {id:?} must set both destination and destination_entrance, or neither"
            ),
            Self::SpawnBlockedByTimedHazard { index } => {
                write!(f, "spawn overlaps timed hazard at index {index}")
            }
            Self::DuplicatePickupId(id) => write!(f, "duplicate pickup id {id:?}"),
        }
    }
}

impl Error for RoomError {}

impl Room {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        width: u16,
        height: u16,
        tile_size: i32,
        tiles: Vec<Tile>,
        spawn: Point,
        exits: Vec<Exit>,
    ) -> Result<Self, RoomError> {
        let id = id.into();
        let name = name.into();
        if id.trim().is_empty() {
            return Err(RoomError::EmptyId);
        }
        if tile_size <= 0 {
            return Err(RoomError::InvalidTileSize(tile_size));
        }
        // Compute in a wider type because dimensions and tile size can arrive
        // directly from hostile content. Every u16-by-i32 product fits in i64,
        // so malformed rooms produce a useful validation error instead of an
        // overflow panic before they can be rejected.
        let pixel_width = i64::from(width) * i64::from(tile_size);
        let pixel_height = i64::from(height) * i64::from(tile_size);
        if pixel_width != i64::from(ROOM_WIDTH_PIXELS)
            || pixel_height != i64::from(ROOM_HEIGHT_PIXELS)
        {
            return Err(RoomError::WrongPixelSize {
                width: pixel_width,
                height: pixel_height,
            });
        }
        let expected = usize::from(width) * usize::from(height);
        if tiles.len() != expected {
            return Err(RoomError::WrongTileCount {
                expected,
                actual: tiles.len(),
            });
        }

        let room_bounds = Rect::new(0, 0, ROOM_WIDTH_PIXELS, ROOM_HEIGHT_PIXELS);
        let spawn_bounds = Rect::new(spawn.x, spawn.y, PLAYER_WIDTH, PLAYER_HEIGHT);
        if !room_bounds.contains(spawn_bounds) {
            return Err(RoomError::SpawnOutOfBounds(spawn));
        }
        if overlapping_tile(width, tile_size, &tiles, spawn_bounds, |tile| {
            tile == Tile::Solid || tile.is_hazard()
        })
        .is_some()
        {
            return Err(RoomError::SpawnBlocked(spawn));
        }

        for (index, exit) in exits.iter().enumerate() {
            if exit.id.trim().is_empty() {
                return Err(RoomError::EmptyExitId);
            }
            if exits[..index].iter().any(|other| other.id == exit.id) {
                return Err(RoomError::DuplicateExitId(exit.id.clone()));
            }
            if exit.bounds.width <= 0
                || exit.bounds.height <= 0
                || !room_bounds.contains(exit.bounds)
            {
                return Err(RoomError::InvalidExitBounds {
                    id: exit.id.clone(),
                    bounds: exit.bounds,
                });
            }
            if exit.destination.is_some() != exit.destination_entrance.is_some() {
                return Err(RoomError::IncompleteDestination {
                    id: exit.id.clone(),
                });
            }
        }

        let mut room = Self {
            id,
            name,
            width,
            height,
            tile_size,
            tiles,
            spawn,
            exits,
            doors: Vec::new(),
            timed_hazards: Vec::new(),
            pickups: Vec::new(),
            content_digest: 0,
        };
        room.content_digest = room.calculate_digest();
        Ok(room)
    }

    /// Add validated room objects without changing the compatible `Room::new`
    /// signature. This consumes the room so constructed rooms stay immutable.
    pub fn with_objects(
        mut self,
        timed_hazards: Vec<TimedHazard>,
        pickups: Vec<Pickup>,
    ) -> Result<Self, RoomError> {
        let spawn_bounds = Rect::new(self.spawn.x, self.spawn.y, PLAYER_WIDTH, PLAYER_HEIGHT);
        if let Some((index, _)) = timed_hazards
            .iter()
            .enumerate()
            .find(|(_, hazard)| spawn_bounds.intersects(hazard.bounds))
        {
            return Err(RoomError::SpawnBlockedByTimedHazard { index });
        }
        if let Some((index, _)) = timed_hazards.iter().enumerate().find(|(_, hazard)| {
            self.doors.iter().any(|door| {
                Rect::new(door.arrival.x, door.arrival.y, PLAYER_WIDTH, PLAYER_HEIGHT)
                    .intersects(hazard.bounds)
            })
        }) {
            // Keep the established RoomError API exhaustive for downstream
            // loaders. The variant now covers every room-entry spawn, not only
            // the canonical spawn.
            return Err(RoomError::SpawnBlockedByTimedHazard { index });
        }
        for (index, pickup) in pickups.iter().enumerate() {
            if pickups[..index].iter().any(|other| other.id == pickup.id) {
                return Err(RoomError::DuplicatePickupId(pickup.id.clone()));
            }
        }
        self.timed_hazards = timed_hazards;
        self.pickups = pickups;
        self.content_digest = self.calculate_digest();
        Ok(self)
    }

    /// Attach and validate boundary doors without changing the compatible
    /// [`Room::new`] signature.
    pub fn with_doors(mut self, doors: Vec<Door>) -> Result<Self, DoorError> {
        let room_bounds = Rect::new(0, 0, ROOM_WIDTH_PIXELS, ROOM_HEIGHT_PIXELS);
        for (index, door) in doors.iter().enumerate() {
            if door.id.trim().is_empty() {
                return Err(DoorError::EmptyId);
            }
            if doors[..index].iter().any(|other| other.id == door.id) {
                return Err(DoorError::DuplicateId(door.id.clone()));
            }
            if self.exits.iter().any(|exit| exit.id == door.id) {
                return Err(DoorError::IdConflictsWithExit(door.id.clone()));
            }
            if door.trigger_bounds.width <= 0
                || door.trigger_bounds.height <= 0
                || !room_bounds.contains(door.trigger_bounds)
            {
                return Err(DoorError::InvalidTriggerBounds {
                    id: door.id.clone(),
                    bounds: door.trigger_bounds,
                });
            }
            let adjacent = match door.side {
                BoundarySide::Left => door.trigger_bounds.x == 0,
                BoundarySide::Right => door.trigger_bounds.right() == ROOM_WIDTH_PIXELS,
                BoundarySide::Ceiling => door.trigger_bounds.y == 0,
                BoundarySide::Floor => door.trigger_bounds.bottom() == ROOM_HEIGHT_PIXELS,
            };
            if !adjacent {
                return Err(DoorError::TriggerNotAdjacentToSide {
                    id: door.id.clone(),
                    side: door.side,
                    bounds: door.trigger_bounds,
                });
            }
            if door.destination_room.is_some() != door.destination_door.is_some() {
                return Err(DoorError::IncompleteDestination {
                    id: door.id.clone(),
                });
            }
            if door
                .destination_room
                .as_deref()
                .is_some_and(|destination| destination.trim().is_empty())
            {
                return Err(DoorError::EmptyDestinationRoom {
                    id: door.id.clone(),
                });
            }
            if door
                .destination_door
                .as_deref()
                .is_some_and(|destination| destination.trim().is_empty())
            {
                return Err(DoorError::EmptyDestinationDoor {
                    id: door.id.clone(),
                });
            }

            let arrival_bounds =
                Rect::new(door.arrival.x, door.arrival.y, PLAYER_WIDTH, PLAYER_HEIGHT);
            if !room_bounds.contains(arrival_bounds) {
                return Err(DoorError::ArrivalOutOfBounds {
                    id: door.id.clone(),
                    arrival: door.arrival,
                });
            }
            if overlapping_tile(
                self.width,
                self.tile_size,
                &self.tiles,
                arrival_bounds,
                |tile| tile != Tile::Empty,
            )
            .is_some()
            {
                return Err(DoorError::ArrivalBlocked {
                    id: door.id.clone(),
                    arrival: door.arrival,
                });
            }
            if let Some((hazard_index, _)) = self
                .timed_hazards
                .iter()
                .enumerate()
                .find(|(_, hazard)| arrival_bounds.intersects(hazard.bounds))
            {
                return Err(DoorError::ArrivalBlockedByTimedHazard {
                    id: door.id.clone(),
                    hazard_index,
                });
            }
            if let Some(trigger) = doors
                .iter()
                .find(|trigger| arrival_bounds.intersects(trigger.trigger_bounds))
            {
                return Err(DoorError::ArrivalOverlapsTrigger {
                    id: door.id.clone(),
                    trigger_id: trigger.id.clone(),
                });
            }
        }
        self.doors = doors;
        self.content_digest = self.calculate_digest();
        Ok(self)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    #[must_use]
    pub const fn tile_size(&self) -> i32 {
        self.tile_size
    }

    #[must_use]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    #[must_use]
    pub const fn spawn(&self) -> Point {
        self.spawn
    }

    #[must_use]
    pub fn exits(&self) -> &[Exit] {
        &self.exits
    }

    #[must_use]
    pub fn doors(&self) -> &[Door] {
        &self.doors
    }

    #[must_use]
    pub fn timed_hazards(&self) -> &[TimedHazard] {
        &self.timed_hazards
    }

    #[must_use]
    pub fn pickups(&self) -> &[Pickup] {
        &self.pickups
    }

    #[must_use]
    pub fn tile(&self, x: u16, y: u16) -> Option<Tile> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.tiles[usize::from(y) * usize::from(self.width) + usize::from(x)])
    }

    #[must_use]
    pub fn tile_bounds(&self, x: u16, y: u16) -> Rect {
        Rect::new(
            i32::from(x) * self.tile_size,
            i32::from(y) * self.tile_size,
            self.tile_size,
            self.tile_size,
        )
    }

    /// Return the direction explicitly authored into a spike tile.
    #[must_use]
    pub fn hazard_direction(&self, x: u16, y: u16) -> Option<HazardDirection> {
        self.tile(x, y).and_then(Tile::hazard_direction)
    }

    pub(crate) fn first_tile_matching(
        &self,
        bounds: Rect,
        predicate: impl Fn(Tile) -> bool,
    ) -> Option<(u16, u16)> {
        overlapping_tile(self.width, self.tile_size, &self.tiles, bounds, predicate)
    }

    pub(crate) const fn content_digest(&self) -> u64 {
        self.content_digest
    }

    fn calculate_digest(&self) -> u64 {
        let mut hash = StableHash::new();
        hash.bytes(self.id.as_bytes());
        hash.bytes(self.name.as_bytes());
        hash.u16(self.width);
        hash.u16(self.height);
        hash.i32(self.tile_size);
        hash.i32(self.spawn.x);
        hash.i32(self.spawn.y);
        for tile in &self.tiles {
            hash.byte(*tile as u8);
        }
        for exit in &self.exits {
            hash.bytes(exit.id.as_bytes());
            hash.rect(exit.bounds);
            hash.option_string(exit.destination.as_deref());
            hash.option_string(exit.destination_entrance.as_deref());
        }
        // Preserve the historical content identity of rooms without doors,
        // while domain-separating and hashing every field when doors exist.
        if !self.doors.is_empty() {
            hash.bytes(b"boundary-doors-v1");
            hash.u64(self.doors.len() as u64);
            for door in &self.doors {
                hash.bytes(door.id.as_bytes());
                hash.byte(door.side as u8);
                hash.rect(door.trigger_bounds);
                hash.i32(door.arrival.x);
                hash.i32(door.arrival.y);
                hash.option_string(door.destination_room.as_deref());
                hash.option_string(door.destination_door.as_deref());
            }
        }
        hash.u64(self.timed_hazards.len() as u64);
        for hazard in &self.timed_hazards {
            hash.rect(hazard.bounds);
            hash.u32(hazard.period_ticks);
            hash.u32(hazard.active_ticks);
            hash.u32(hazard.phase_ticks);
        }
        hash.u64(self.pickups.len() as u64);
        for pickup in &self.pickups {
            hash.bytes(pickup.id.as_bytes());
            hash.rect(pickup.bounds);
        }
        hash.finish()
    }
}

fn validate_object_bounds(bounds: Rect) -> Result<(), RoomObjectError> {
    let screen = Rect::new(0, 0, ROOM_WIDTH_PIXELS, ROOM_HEIGHT_PIXELS);
    if bounds.width <= 0 || bounds.height <= 0 || !screen.contains(bounds) {
        return Err(RoomObjectError::InvalidBounds(bounds));
    }
    Ok(())
}

fn overlapping_tile(
    width: u16,
    tile_size: i32,
    tiles: &[Tile],
    bounds: Rect,
    predicate: impl Fn(Tile) -> bool,
) -> Option<(u16, u16)> {
    if bounds.width <= 0 || bounds.height <= 0 || width == 0 {
        return None;
    }
    let height = (tiles.len() / usize::from(width)) as i32;
    let first_x = bounds
        .x
        .div_euclid(tile_size)
        .clamp(0, i32::from(width) - 1);
    let last_x = (bounds.right() - 1)
        .div_euclid(tile_size)
        .clamp(0, i32::from(width) - 1);
    let first_y = bounds.y.div_euclid(tile_size).clamp(0, height - 1);
    let last_y = (bounds.bottom() - 1)
        .div_euclid(tile_size)
        .clamp(0, height - 1);
    for y in first_y..=last_y {
        for x in first_x..=last_x {
            let x = x as u16;
            let y = y as u16;
            let tile = tiles[usize::from(y) * usize::from(width) + usize::from(x)];
            if predicate(tile) {
                let tile_bounds = Rect::new(
                    i32::from(x) * tile_size,
                    i32::from(y) * tile_size,
                    tile_size,
                    tile_size,
                );
                if bounds.intersects(tile_bounds) {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

struct StableHash(u64);

impl StableHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn byte(&mut self, byte: u8) {
        self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    fn bytes(&mut self, bytes: &[u8]) {
        self.u64(bytes.len() as u64);
        for &byte in bytes {
            self.byte(byte);
        }
    }
    fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn i32(&mut self, value: i32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn rect(&mut self, value: Rect) {
        self.i32(value.x);
        self.i32(value.y);
        self.i32(value.width);
        self.i32(value.height);
    }
    fn option_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.byte(1);
                self.bytes(value.as_bytes());
            }
            None => self.byte(0),
        }
    }
    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_room(exits: Vec<Exit>) -> Room {
        Room::new(
            "room",
            "Room",
            32,
            18,
            10,
            vec![Tile::Empty; 32 * 18],
            Point::new(150, 80),
            exits,
        )
        .unwrap()
    }

    fn door(id: &str, side: BoundarySide, trigger_bounds: Rect, arrival: Point) -> Door {
        Door {
            id: id.to_owned(),
            side,
            trigger_bounds,
            arrival,
            destination_room: None,
            destination_door: None,
        }
    }

    #[test]
    fn hazard_direction_is_explicit_tile_data() {
        let mut room = empty_room(vec![]);
        room.tiles[8 * 32 + 5] = Tile::HazardUp;
        room.tiles[4 * 32 + 5] = Tile::HazardDown;
        room.tiles[7 * 32 + 13] = Tile::HazardRight;
        room.tiles[7 * 32 + 20] = Tile::HazardLeft;

        assert_eq!(room.hazard_direction(5, 8), Some(HazardDirection::Up));
        assert_eq!(room.hazard_direction(5, 4), Some(HazardDirection::Down));
        assert_eq!(room.hazard_direction(13, 7), Some(HazardDirection::Right));
        assert_eq!(room.hazard_direction(20, 7), Some(HazardDirection::Left));
        assert_eq!(room.hazard_direction(0, 0), None);
    }

    #[test]
    fn rejects_non_screen_sized_rooms() {
        let error = Room::new(
            "room",
            "Room",
            31,
            18,
            10,
            vec![Tile::Empty; 31 * 18],
            Point::new(1, 1),
            vec![],
        )
        .unwrap_err();
        assert!(matches!(error, RoomError::WrongPixelSize { .. }));
    }

    #[test]
    fn extreme_tile_size_returns_a_dimension_error_without_overflowing() {
        let error = Room::new(
            "room",
            "Room",
            32,
            18,
            i32::MAX,
            vec![Tile::Empty; 32 * 18],
            Point::new(1, 1),
            vec![],
        )
        .unwrap_err();

        assert_eq!(
            error,
            RoomError::WrongPixelSize {
                width: i64::from(32) * i64::from(i32::MAX),
                height: i64::from(18) * i64::from(i32::MAX),
            }
        );
    }

    #[test]
    fn accepts_clear_doors_on_each_boundary_side() {
        let doors = vec![
            door(
                "west",
                BoundarySide::Left,
                Rect::new(0, 40, 4, 20),
                Point::new(8, 44),
            ),
            door(
                "east",
                BoundarySide::Right,
                Rect::new(316, 40, 4, 20),
                Point::new(304, 44),
            ),
            door(
                "north",
                BoundarySide::Ceiling,
                Rect::new(100, 0, 20, 4),
                Point::new(106, 8),
            ),
            door(
                "south",
                BoundarySide::Floor,
                Rect::new(100, 176, 20, 4),
                Point::new(106, 160),
            ),
        ];
        let room = empty_room(vec![]).with_doors(doors.clone()).unwrap();
        assert_eq!(room.doors(), doors);
    }

    #[test]
    fn opposite_doors_match_only_at_the_same_boundary_aperture() {
        let west = door(
            "west",
            BoundarySide::Left,
            Rect::new(0, 40, 4, 20),
            Point::new(8, 44),
        );
        let east = door(
            "east",
            BoundarySide::Right,
            Rect::new(316, 40, 4, 20),
            Point::new(304, 44),
        );
        let shifted = door(
            "shifted",
            BoundarySide::Right,
            Rect::new(316, 50, 4, 20),
            Point::new(304, 54),
        );
        let floor = door(
            "floor",
            BoundarySide::Floor,
            Rect::new(40, 176, 20, 4),
            Point::new(46, 160),
        );

        assert_eq!(BoundarySide::Left.opposite(), BoundarySide::Right);
        assert_eq!(BoundarySide::Ceiling.opposite(), BoundarySide::Floor);
        assert_eq!(west.aperture_offset(), 40);
        assert_eq!(west.aperture_span(), 20);
        assert_eq!(
            west.socket(),
            DoorSocket {
                side: BoundarySide::Left,
                offset: 40,
                span: 20,
            }
        );
        assert_eq!(west.socket().mate(), east.socket());
        assert!(west.socket().matches(east.socket()));
        assert!(west.geometrically_matches(&east));
        assert!(east.geometrically_matches(&west));
        assert!(!west.geometrically_matches(&shifted));
        assert!(!west.geometrically_matches(&floor));
    }

    #[test]
    fn rejects_invalid_door_ids_sides_bounds_and_pairings() {
        let valid = door(
            "west",
            BoundarySide::Left,
            Rect::new(0, 40, 4, 20),
            Point::new(8, 44),
        );
        let mut invalid = valid.clone();
        invalid.id = "  ".into();
        assert_eq!(
            empty_room(vec![]).with_doors(vec![invalid]),
            Err(DoorError::EmptyId)
        );

        assert!(matches!(
            empty_room(vec![]).with_doors(vec![valid.clone(), valid.clone()]),
            Err(DoorError::DuplicateId(id)) if id == "west"
        ));

        let exit = Exit {
            id: "west".into(),
            bounds: Rect::new(10, 10, 10, 10),
            destination: None,
            destination_entrance: None,
        };
        assert!(matches!(
            empty_room(vec![exit]).with_doors(vec![valid.clone()]),
            Err(DoorError::IdConflictsWithExit(id)) if id == "west"
        ));

        let mut out_of_bounds = valid.clone();
        out_of_bounds.trigger_bounds = Rect::new(-1, 40, 4, 20);
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![out_of_bounds]),
            Err(DoorError::InvalidTriggerBounds { .. })
        ));

        let mut detached = valid.clone();
        detached.trigger_bounds = Rect::new(1, 40, 4, 20);
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![detached]),
            Err(DoorError::TriggerNotAdjacentToSide { .. })
        ));

        let mut incomplete = valid.clone();
        incomplete.destination_room = Some("next".into());
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![incomplete]),
            Err(DoorError::IncompleteDestination { .. })
        ));

        let mut blank_room = valid.clone();
        blank_room.destination_room = Some(" ".into());
        blank_room.destination_door = Some("east".into());
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![blank_room]),
            Err(DoorError::EmptyDestinationRoom { .. })
        ));

        let mut blank_door = valid;
        blank_door.destination_room = Some("next".into());
        blank_door.destination_door = Some(String::new());
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![blank_door]),
            Err(DoorError::EmptyDestinationDoor { .. })
        ));
    }

    #[test]
    fn rejects_unsafe_door_arrivals_and_both_timed_hazard_attachment_orders() {
        let valid = door(
            "west",
            BoundarySide::Left,
            Rect::new(0, 40, 4, 20),
            Point::new(20, 50),
        );
        let mut outside = valid.clone();
        outside.arrival = Point::new(313, 50);
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![outside]),
            Err(DoorError::ArrivalOutOfBounds { .. })
        ));

        for tile in [Tile::Solid, Tile::HazardUp, Tile::OneWay] {
            let mut tiled = empty_room(vec![]);
            tiled.tiles[5 * 32 + 2] = tile;
            assert!(matches!(
                tiled.with_doors(vec![valid.clone()]),
                Err(DoorError::ArrivalBlocked { .. })
            ));
        }

        let mut trigger_overlap = valid.clone();
        trigger_overlap.arrival = Point::new(0, 44);
        assert!(matches!(
            empty_room(vec![]).with_doors(vec![trigger_overlap]),
            Err(DoorError::ArrivalOverlapsTrigger { .. })
        ));

        let hazard = TimedHazard::new(Rect::new(20, 50, 8, 12), 10, 1, 0).unwrap();
        let objects_first = empty_room(vec![])
            .with_objects(vec![hazard.clone()], vec![])
            .unwrap();
        assert!(matches!(
            objects_first.with_doors(vec![valid.clone()]),
            Err(DoorError::ArrivalBlockedByTimedHazard {
                hazard_index: 0,
                ..
            })
        ));

        let doors_first = empty_room(vec![]).with_doors(vec![valid]).unwrap();
        assert_eq!(
            doors_first.with_objects(vec![hazard], vec![]),
            Err(RoomError::SpawnBlockedByTimedHazard { index: 0 })
        );
    }
}
