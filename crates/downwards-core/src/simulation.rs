use std::{fmt, sync::Arc};

use crate::{
    HazardDirection, Pickup, Point, Rect, Room, Tile, TimedHazard,
    room::{ROOM_HEIGHT_PIXELS, ROOM_WIDTH_PIXELS},
};

pub const TICKS_PER_SECOND: u32 = 60;
pub const SUBPIXELS_PER_PIXEL: i32 = 256;
pub const PLAYER_WIDTH: i32 = 8;
pub const PLAYER_HEIGHT: i32 = 12;
/// Collision height while a player-facing horizontal Dash is squeezing through a low passage.
pub const DASH_PLAYER_HEIGHT: i32 = 8;
pub const COYOTE_TICKS: u8 = 5;
pub const JUMP_BUFFER_TICKS: u8 = 5;
pub const JUMP_HOLD_TICKS: u8 = 10;
/// Number of simulation ticks for which a dash has its fixed velocity.
pub const DASH_TICKS: u8 = 10;
/// Brief collision-ignore window used to make down+jump pass through a
/// one-way platform deterministically even at subpixel boundaries.
pub const ONE_WAY_DROP_TICKS: u8 = 3;

/// Version of the player-facing movement policy used by live play and new AI solves.
///
/// Historical corpus replays deliberately keep constructing an unconfigured `Simulation`; their
/// digests therefore remain in the legacy policy domain until that corpus is regenerated.
pub const PLAYER_MOVEMENT_POLICY_VERSION: u32 = 10;

const GRAVITY: i32 = 96;
const HELD_JUMP_GRAVITY: i32 = 48;
const MAX_FALL_SPEED: i32 = 1_536;
/// Clamp for wall-slide-adjacent horizontal motion; matches the tuned top speed scale.
const RUN_SPEED: i32 = 384;
const JUMP_SPEED: i32 = -960;
const JUMP_CUT_SPEED: i32 = -448;
const WALL_JUMP_HORIZONTAL_SPEED: i32 = 768;
const WALL_SLIDE_SPEED: i32 = 384;
const WALL_ASCENT_MAX_UPWARD_SPEED: i32 = -1_536;
const HUMAN_WALL_FORCE_TICKS: u8 = 9;
const HUMAN_WALL_MINIMUM_ASCENT_TICKS: u8 = JUMP_HOLD_TICKS;
const DASH_SPEED: i32 = 1_024;
// `DASH_SPEED / sqrt(2)`, rounded once and kept as an integer constant so
// diagonal dashes have deterministic, near-equal magnitude.
const DASH_DIAGONAL_SPEED: i32 = 724;

/// Human-readable horizontal-control tuning used by an explicitly opted-in live simulation.
/// Stored replays, solver searches, and generated-room evidence use legacy physics unless they
/// explicitly install a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MovementTuning {
    pub top_speed_pixels_per_second: u16,
    pub acceleration_milliseconds: u16,
    pub braking_milliseconds: u16,
    pub wall_ascent_carry_percent: u16,
    pub wall_carry_percent: u16,
    pub wall_momentum_milliseconds: u16,
}

impl MovementTuning {
    /// Movement values selected through the human playtest menu on 2026-08-15. This is the default
    /// for live play and for every new player-facing AI solve.
    pub const GAMEPLAY_DEFAULT: Self = Self {
        top_speed_pixels_per_second: 110,
        acceleration_milliseconds: 72,
        braking_milliseconds: 203,
        wall_ascent_carry_percent: 50,
        wall_carry_percent: 50,
        wall_momentum_milliseconds: 250,
    };

    /// Compatibility alias for the movement-lab tests and callers written before the tuning was
    /// promoted to the game-wide policy.
    pub const LAB_DEFAULT: Self = Self::GAMEPLAY_DEFAULT;
}

#[derive(Clone, Copy)]
struct HorizontalMotionTuning {
    maximum_speed: i32,
    acceleration: i32,
    release_deceleration: i32,
    reversal_acceleration: i32,
    wall_retention_ticks: u8,
}

fn horizontal_motion_tuning(tuning: MovementTuning, grounded: bool) -> HorizontalMotionTuning {
    {
        {
            let maximum_speed =
                pixels_per_second_to_subpixels_per_tick(tuning.top_speed_pixels_per_second);
            let acceleration_ticks = milliseconds_to_ticks(if grounded {
                u32::from(tuning.acceleration_milliseconds)
            } else {
                u32::from(tuning.acceleration_milliseconds) * 3 / 2
            });
            let braking_ticks = milliseconds_to_ticks(if grounded {
                u32::from(tuning.braking_milliseconds)
            } else {
                u32::from(tuning.braking_milliseconds) * 2
            });
            let release_deceleration = rate_for_ticks(maximum_speed, braking_ticks);
            HorizontalMotionTuning {
                maximum_speed,
                acceleration: rate_for_ticks(maximum_speed, acceleration_ticks),
                release_deceleration,
                // A full +speed to -speed reversal takes the displayed braking time.
                reversal_acceleration: release_deceleration * 2,
                wall_retention_ticks: milliseconds_to_ticks(u32::from(
                    tuning.wall_momentum_milliseconds,
                )),
            }
        }
    }
}

fn pixels_per_second_to_subpixels_per_tick(pixels_per_second: u16) -> i32 {
    (i32::from(pixels_per_second) * SUBPIXELS_PER_PIXEL + TICKS_PER_SECOND as i32 / 2)
        / TICKS_PER_SECOND as i32
}

fn milliseconds_to_ticks(milliseconds: u32) -> u8 {
    if milliseconds == 0 {
        return 0;
    }
    let ticks = (milliseconds * TICKS_PER_SECOND + 500) / 1_000;
    u8::try_from(ticks.max(1)).unwrap_or(u8::MAX)
}

fn rate_for_ticks(maximum_speed: i32, ticks: u8) -> i32 {
    (maximum_speed + i32::from(ticks.max(1)) - 1) / i32::from(ticks.max(1))
}

/// Traversal abilities owned for this run. Running and jumping are baseline
/// mechanics and therefore deliberately do not have flags here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilitySet {
    pub wall_jump: bool,
    pub dash: bool,
}

impl AbilitySet {
    pub const NONE: Self = Self {
        wall_jump: false,
        dash: false,
    };
    pub const ALL: Self = Self {
        wall_jump: true,
        dash: true,
    };

    #[must_use]
    pub const fn new(wall_jump: bool, dash: bool) -> Self {
        Self { wall_jump, dash }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Action {
    /// Held horizontal movement/dash intent. Clamped to -1, 0, or 1.
    pub move_x: i8,
    /// Held vertical dash intent. Clamped to -1, 0, or 1. Positive is down.
    pub move_y: i8,
    /// Whether jump is held. Press edges are derived by the simulation.
    pub jump: bool,
    /// Whether dash is held. Press edges are derived by the simulation.
    pub dash: bool,
    /// Reset the room on this tick.
    pub restart: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallSide {
    Left,
    Right,
}

impl WallSide {
    const fn direction(self) -> i8 {
        match self {
            Self::Left => -1,
            Self::Right => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DashDirection {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl DashDirection {
    #[must_use]
    pub const fn intent(self) -> Point {
        match self {
            Self::Up => Point::new(0, -1),
            Self::UpRight => Point::new(1, -1),
            Self::Right => Point::new(1, 0),
            Self::DownRight => Point::new(1, 1),
            Self::Down => Point::new(0, 1),
            Self::DownLeft => Point::new(-1, 1),
            Self::Left => Point::new(-1, 0),
            Self::UpLeft => Point::new(-1, -1),
        }
    }

    const fn velocity(self) -> Point {
        let intent = self.intent();
        let speed = if intent.x != 0 && intent.y != 0 {
            DASH_DIAGONAL_SPEED
        } else {
            DASH_SPEED
        };
        Point::new(intent.x * speed, intent.y * speed)
    }

    const fn from_intent(move_x: i8, move_y: i8, facing: i8) -> Self {
        match (move_x, move_y) {
            (0, -1) => Self::Up,
            (1, -1) => Self::UpRight,
            (1, 0) => Self::Right,
            (1, 1) => Self::DownRight,
            (0, 1) => Self::Down,
            (-1, 1) => Self::DownLeft,
            (-1, 0) => Self::Left,
            (-1, -1) => Self::UpLeft,
            // A neutral dash follows the last horizontal facing direction.
            _ if facing < 0 => Self::Left,
            _ => Self::Right,
        }
    }

    const fn digest_tag(self) -> u8 {
        match self {
            Self::Up => 0,
            Self::UpRight => 1,
            Self::Right => 2,
            Self::DownRight => 3,
            Self::Down => 4,
            Self::DownLeft => 5,
            Self::Left => 6,
            Self::UpLeft => 7,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerState {
    position_subpixels: Point,
    velocity_subpixels: Point,
    grounded: bool,
    coyote_ticks: u8,
    jump_buffer_ticks: u8,
    jump_hold_ticks: u8,
    wall_contact: Option<WallSide>,
    wall_sliding: bool,
    facing: i8,
    dash_available: bool,
    dash_ticks: u8,
    dash_direction: Option<DashDirection>,
    dash_compressed: bool,
    one_way_drop_ticks: u8,
}

impl PlayerState {
    #[must_use]
    pub const fn position_subpixels(&self) -> Point {
        self.position_subpixels
    }
    #[must_use]
    pub const fn velocity_subpixels(&self) -> Point {
        self.velocity_subpixels
    }
    #[must_use]
    pub const fn grounded(&self) -> bool {
        self.grounded
    }
    #[must_use]
    pub const fn coyote_ticks_remaining(&self) -> u8 {
        self.coyote_ticks
    }
    #[must_use]
    pub const fn jump_buffer_ticks_remaining(&self) -> u8 {
        self.jump_buffer_ticks
    }
    /// Remaining variable-height jump ticks. This is future-relevant state
    /// for deterministic search and replay diagnostics.
    #[must_use]
    pub const fn jump_hold_ticks_remaining(&self) -> u8 {
        self.jump_hold_ticks
    }
    #[must_use]
    pub const fn wall_contact(&self) -> Option<WallSide> {
        self.wall_contact
    }
    #[must_use]
    pub const fn wall_sliding(&self) -> bool {
        self.wall_sliding
    }
    #[must_use]
    pub const fn facing(&self) -> i8 {
        self.facing
    }
    #[must_use]
    pub const fn dash_available(&self) -> bool {
        self.dash_available
    }
    #[must_use]
    pub const fn dash_ticks_remaining(&self) -> u8 {
        self.dash_ticks
    }
    #[must_use]
    pub const fn dash_direction(&self) -> Option<DashDirection> {
        self.dash_direction
    }
    /// Whether the player is using the low-profile horizontal-Dash collider.
    #[must_use]
    pub const fn dash_compressed(&self) -> bool {
        self.dash_compressed
    }
    #[must_use]
    pub const fn one_way_drop_ticks_remaining(&self) -> u8 {
        self.one_way_drop_ticks
    }

    #[must_use]
    pub const fn bounds(&self) -> Rect {
        Rect::new(
            self.position_subpixels.x.div_euclid(SUBPIXELS_PER_PIXEL),
            self.position_subpixels.y.div_euclid(SUBPIXELS_PER_PIXEL),
            PLAYER_WIDTH,
            if self.dash_compressed {
                DASH_PLAYER_HEIGHT
            } else {
                PLAYER_HEIGHT
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpKind {
    Grounded,
    Coyote,
    Buffered,
    Wall { side: WallSide },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathReason {
    Hazard { tile_x: u16, tile_y: u16 },
    TimedHazard { hazard_index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimulationEvent {
    Jumped(JumpKind),
    Dashed {
        direction: DashDirection,
    },
    Landed,
    Died(DeathReason),
    /// The pickup left the world, but it only banks (PickupCollected) once
    /// the player next lands somewhere safe or leaves through an exit.
    PickupTouched {
        id: String,
    },
    PickupCollected {
        id: String,
    },
    Reset,
    ExitReached {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateDigest(pub u64);

impl fmt::Display for StateDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Failure to select a room entry door when constructing a simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoorEntryError {
    UnknownDoor { room_id: String, door_id: String },
}

impl fmt::Display for DoorEntryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDoor { room_id, door_id } => {
                write!(formatter, "room {room_id:?} has no door {door_id:?}")
            }
        }
    }
}

impl std::error::Error for DoorEntryError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepReport {
    pub tick: u64,
    pub events: Vec<SimulationEvent>,
    pub digest: StateDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SimulationState {
    tick: u64,
    room_tick: u64,
    player: PlayerState,
    previous_action: Action,
    deaths: u32,
    reached_exit: Option<String>,
    collected_pickups: Vec<bool>,
    /// Touched but not yet banked; cleared by reset, banked on safe landing.
    pending_pickups: Vec<bool>,
}

/// Cloneable, renderer-independent authoritative simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulation {
    room: Arc<Room>,
    abilities: AbilitySet,
    entry_door: Option<String>,
    state: SimulationState,
    human_wall_assists: bool,
    recent_wall: Option<WallSide>,
    wall_grace_ticks: u8,
    wall_force_direction: i8,
    wall_force_ticks: u8,
    wall_minimum_ascent_ticks: u8,
    movement_tuning: MovementTuning,
    retained_wall_velocity_x: i32,
    wall_velocity_retention_ticks: u8,
    wall_ascent_carry_wall: Option<WallSide>,
}

impl Simulation {
    /// Construct a simulation with only the baseline run and jump mechanics.
    #[must_use]
    pub fn new(room: Room) -> Self {
        Self::with_abilities(room, AbilitySet::NONE)
    }

    /// Construct a simulation with an explicit per-run traversal loadout.
    #[must_use]
    pub fn with_abilities(room: Room, abilities: AbilitySet) -> Self {
        Self::from_entry(room, abilities, None)
    }

    /// Construct a simulation as though the player arrived through `door_id`.
    /// Manual restarts and deaths return to that door's safe arrival point.
    pub fn enter_via_door(
        room: Room,
        abilities: AbilitySet,
        door_id: impl AsRef<str>,
    ) -> Result<Self, DoorEntryError> {
        let requested = door_id.as_ref();
        let Some(door) = room.doors.iter().find(|door| door.id == requested) else {
            return Err(DoorEntryError::UnknownDoor {
                room_id: room.id.clone(),
                door_id: requested.to_owned(),
            });
        };
        let entry_door = door.id.clone();
        Ok(Self::from_entry(room, abilities, Some(entry_door)))
    }

    fn from_entry(room: Room, abilities: AbilitySet, entry_door: Option<String>) -> Self {
        let spawn = entry_door
            .as_deref()
            .and_then(|id| room.doors.iter().find(|door| door.id == id))
            .map_or(room.spawn, |door| door.arrival);
        let player = spawn_player(spawn, abilities);
        let collected_pickups = vec![false; room.pickups.len()];
        let pending_pickups = vec![false; room.pickups.len()];
        Self {
            room: Arc::new(room),
            abilities,
            entry_door,
            state: SimulationState {
                tick: 0,
                room_tick: 0,
                player,
                previous_action: Action::default(),
                deaths: 0,
                reached_exit: None,
                collected_pickups,
                pending_pickups,
            },
            human_wall_assists: false,
            recent_wall: None,
            wall_grace_ticks: 0,
            wall_force_direction: 0,
            wall_force_ticks: 0,
            wall_minimum_ascent_ticks: 0,
            movement_tuning: MovementTuning::GAMEPLAY_DEFAULT,
            retained_wall_velocity_x: 0,
            wall_velocity_retention_ticks: 0,
            wall_ascent_carry_wall: None,
        }
    }

    #[must_use]
    pub fn room(&self) -> &Room {
        &self.room
    }
    #[must_use]
    pub const fn abilities(&self) -> AbilitySet {
        self.abilities
    }
    /// Permanently add traversal abilities to the current run without moving or resetting the
    /// player. Newly granted Dash starts charged, matching a fresh spawn with that loadout.
    ///
    /// This API is intentionally additive: persistent run-state code may unlock abilities, but
    /// cannot silently revoke mechanics from an in-flight authoritative simulation.
    pub fn grant_abilities(&mut self, abilities: AbilitySet) {
        let gained_dash = abilities.dash && !self.abilities.dash;
        self.abilities.wall_jump |= abilities.wall_jump;
        self.abilities.dash |= abilities.dash;
        if gained_dash {
            self.state.player.dash_available = true;
        }
    }
    /// The door used to enter this room, or `None` for its canonical spawn.
    #[must_use]
    pub fn entry_door(&self) -> Option<&str> {
        self.entry_door.as_deref()
    }
    #[must_use]
    pub const fn player(&self) -> &PlayerState {
        &self.state.player
    }
    /// Enable the forgiving live-control wall-jump contract. Historical replay and solver
    /// simulations remain on the exact legacy policy unless they opt in explicitly.
    pub fn enable_human_wall_assists(&mut self) {
        self.human_wall_assists = true;
    }

    /// Apply the current player-facing movement policy. Tuned movement is the
    /// only physics; this additionally enables the live wall-jump assists that
    /// human-modelling searches should share.
    pub fn enable_current_player_movement(&mut self) {
        self.enable_human_wall_assists();
        self.set_movement_tuning(MovementTuning::GAMEPLAY_DEFAULT);
    }
    #[must_use]
    pub const fn human_wall_assists_enabled(&self) -> bool {
        self.human_wall_assists
    }
    /// Opt into experimental horizontal-control tuning. Changing tuning clears any short-lived
    /// wall-momentum state; callers should start a new recorded attempt boundary.
    pub fn set_movement_tuning(&mut self, tuning: MovementTuning) {
        self.movement_tuning = tuning;
        self.retained_wall_velocity_x = 0;
        self.wall_velocity_retention_ticks = 0;
        self.wall_ascent_carry_wall = None;
    }
    pub fn clear_movement_tuning(&mut self) {
        self.set_movement_tuning(MovementTuning::GAMEPLAY_DEFAULT);
    }
    #[must_use]
    pub const fn movement_tuning(&self) -> MovementTuning {
        self.movement_tuning
    }
    #[must_use]
    pub const fn human_wall_jump_available(&self) -> bool {
        self.abilities.wall_jump
            && (self.state.player.wall_contact.is_some() || self.wall_grace_ticks > 0)
    }
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.state.tick
    }
    /// Tick of the current room attempt. Unlike the global replay tick, this
    /// returns to zero on restart or death and drives timed hazards.
    #[must_use]
    pub const fn room_tick(&self) -> u64 {
        self.state.room_tick
    }
    #[must_use]
    pub const fn deaths(&self) -> u32 {
        self.state.deaths
    }
    #[must_use]
    pub fn reached_exit(&self) -> Option<&str> {
        self.state.reached_exit.as_deref()
    }

    /// Reject a just-triggered boundary door and return the player to that door's safe interior
    /// arrival point without resetting room-local pickups or the room clock.
    ///
    /// Higher-level dungeon state uses this for visible locked doors. It succeeds only when the
    /// named door is the currently reached exit, so callers cannot use it as an arbitrary warp.
    pub fn reject_reached_door(&mut self, door_id: &str) -> bool {
        if self.state.reached_exit.as_deref() != Some(door_id) {
            return false;
        }
        let Some(arrival) = self
            .room
            .doors
            .iter()
            .find(|door| door.id == door_id)
            .map(|door| door.arrival)
        else {
            return false;
        };
        self.state.player = spawn_player(arrival, self.abilities);
        self.state.previous_action = Action::default();
        self.state.reached_exit = None;
        self.recent_wall = None;
        self.wall_grace_ticks = 0;
        self.wall_force_direction = 0;
        self.wall_force_ticks = 0;
        self.wall_minimum_ascent_ticks = 0;
        self.retained_wall_velocity_x = 0;
        self.wall_velocity_retention_ticks = 0;
        self.wall_ascent_carry_wall = None;
        true
    }

    #[must_use]
    pub fn timed_hazard_is_active(&self, hazard_index: usize) -> Option<bool> {
        self.room
            .timed_hazards
            .get(hazard_index)
            .map(|hazard| hazard.is_active_at(self.state.room_tick))
    }

    pub fn active_timed_hazards(&self) -> impl Iterator<Item = (usize, &TimedHazard)> + '_ {
        self.room
            .timed_hazards
            .iter()
            .enumerate()
            .filter(|(_, hazard)| hazard.is_active_at(self.state.room_tick))
    }

    #[must_use]
    pub fn pickup_is_collected(&self, pickup_index: usize) -> Option<bool> {
        self.state.collected_pickups.get(pickup_index).copied()
    }

    /// Touched pickups only bank once the player next lands somewhere safe
    /// (or leaves through an exit); success criteria should use this.
    pub fn pickup_is_banked(&self, pickup_index: usize) -> Option<bool> {
        Some(
            *self.state.collected_pickups.get(pickup_index)?
                && !*self.state.pending_pickups.get(pickup_index)?,
        )
    }

    pub fn collected_pickups(&self) -> impl Iterator<Item = &Pickup> + '_ {
        self.room
            .pickups
            .iter()
            .zip(
                self.state
                    .collected_pickups
                    .iter()
                    .zip(&self.state.pending_pickups),
            )
            .filter_map(|(pickup, (&collected, &pending))| {
                (collected && !pending).then_some(pickup)
            })
    }

    pub fn reset(&mut self) {
        self.state.room_tick = 0;
        let spawn = self
            .entry_door
            .as_deref()
            .and_then(|id| self.room.doors.iter().find(|door| door.id == id))
            .map_or(self.room.spawn, |door| door.arrival);
        self.state.player = spawn_player(spawn, self.abilities);
        self.state.previous_action = Action::default();
        self.state.reached_exit = None;
        self.state.collected_pickups.fill(false);
        self.state.pending_pickups.fill(false);
        self.recent_wall = None;
        self.wall_grace_ticks = 0;
        self.wall_force_direction = 0;
        self.wall_force_ticks = 0;
        self.wall_minimum_ascent_ticks = 0;
        self.retained_wall_velocity_x = 0;
        self.wall_velocity_retention_ticks = 0;
        self.wall_ascent_carry_wall = None;
    }

    pub fn step(&mut self, mut action: Action) -> StepReport {
        action.move_x = action.move_x.clamp(-1, 1);
        action.move_y = action.move_y.clamp(-1, 1);
        self.state.tick = self.state.tick.wrapping_add(1);
        let mut events = Vec::new();

        if action.restart {
            self.reset();
            events.push(SimulationEvent::Reset);
            return self.report(events);
        }

        if self.state.reached_exit.is_none() {
            self.state.room_tick = self.state.room_tick.wrapping_add(1);
            self.simulate_player(action, &mut events);
            self.process_triggers(&mut events);
        }
        // A room reset restores canonical input-edge state. In particular, a
        // button held on the fatal or restart tick must not leak into the new
        // attempt as though the fresh simulation had already observed it.
        if !events.contains(&SimulationEvent::Reset) {
            self.state.previous_action = action;
        }
        self.report(events)
    }

    #[must_use]
    pub fn digest(&self) -> StateDigest {
        let mut hash = DigestBuilder::new();
        hash.u64(self.room.content_digest());
        hash.bool(self.abilities.wall_jump);
        hash.bool(self.abilities.dash);
        // Keep canonical-spawn digests backward-compatible. A door entry is
        // explicitly domain-separated and identified by its stable id.
        if let Some(entry_door) = self.entry_door.as_deref() {
            hash.bytes(b"door-entry-v1");
            hash.bytes(entry_door.as_bytes());
        }
        hash.u64(self.state.tick);
        hash.u64(self.state.room_tick);
        hash.point(self.state.player.position_subpixels);
        hash.point(self.state.player.velocity_subpixels);
        hash.bool(self.state.player.grounded);
        hash.byte(self.state.player.coyote_ticks);
        hash.byte(self.state.player.jump_buffer_ticks);
        hash.byte(self.state.player.jump_hold_ticks);
        hash.option_wall_side(self.state.player.wall_contact);
        hash.bool(self.state.player.wall_sliding);
        hash.byte(self.state.player.facing as u8);
        hash.bool(self.state.player.dash_available);
        hash.byte(self.state.player.dash_ticks);
        hash.option_dash_direction(self.state.player.dash_direction);
        hash.bool(self.state.player.dash_compressed);
        hash.byte(self.state.player.one_way_drop_ticks);
        if self.human_wall_assists {
            hash.bytes(b"human-wall-assists-v1");
            hash.option_wall_side(self.recent_wall);
            hash.byte(self.wall_grace_ticks);
            hash.byte(self.wall_force_direction as u8);
            hash.byte(self.wall_force_ticks);
            hash.byte(self.wall_minimum_ascent_ticks);
        }
        {
            let tuning = self.movement_tuning;
            hash.bytes(b"movement-tuning-v1");
            hash.u32(u32::from(tuning.top_speed_pixels_per_second));
            hash.u32(u32::from(tuning.acceleration_milliseconds));
            hash.u32(u32::from(tuning.braking_milliseconds));
            hash.u32(u32::from(tuning.wall_ascent_carry_percent));
            hash.u32(u32::from(tuning.wall_carry_percent));
            hash.u32(u32::from(tuning.wall_momentum_milliseconds));
            hash.i32(self.retained_wall_velocity_x);
            hash.byte(self.wall_velocity_retention_ticks);
            hash.option_wall_side(self.wall_ascent_carry_wall);
        }
        hash.byte(self.state.previous_action.move_x as u8);
        hash.byte(self.state.previous_action.move_y as u8);
        hash.bool(self.state.previous_action.jump);
        hash.bool(self.state.previous_action.dash);
        hash.bool(self.state.previous_action.restart);
        hash.u32(self.state.deaths);
        for &pending in &self.state.pending_pickups {
            hash.bool(pending);
        }
        hash.u64(self.state.collected_pickups.len() as u64);
        for &collected in &self.state.collected_pickups {
            hash.bool(collected);
        }
        match self.state.reached_exit.as_deref() {
            Some(id) => {
                hash.byte(1);
                hash.bytes(id.as_bytes());
            }
            None => hash.byte(0),
        }
        StateDigest(hash.finish())
    }

    fn report(&self, events: Vec<SimulationEvent>) -> StepReport {
        StepReport {
            tick: self.state.tick,
            events,
            digest: self.digest(),
        }
    }

    fn simulate_player(&mut self, action: Action, events: &mut Vec<SimulationEvent>) {
        let was_grounded = self.state.player.grounded;
        let jump_pressed = action.jump && !self.state.previous_action.jump;
        let jump_released = !action.jump && self.state.previous_action.jump;
        let dash_pressed = action.dash && !self.state.previous_action.dash;
        let dropped_through = jump_pressed
            && action.move_y > 0
            && self.has_ground_contact_for(Tile::OneWay)
            && !self.has_ground_contact_for(Tile::Solid);
        if action.move_x != 0 {
            self.state.player.facing = action.move_x;
        }
        if dropped_through {
            self.state.player.one_way_drop_ticks = ONE_WAY_DROP_TICKS;
            self.state.player.grounded = false;
            self.state.player.coyote_ticks = 0;
            self.state.player.jump_buffer_ticks = 0;
        } else if jump_pressed {
            self.state.player.jump_buffer_ticks = JUMP_BUFFER_TICKS;
        }

        if dash_pressed && self.abilities.dash && self.state.player.dash_available {
            let direction =
                DashDirection::from_intent(action.move_x, action.move_y, self.state.player.facing);
            self.begin_dash(direction);
            events.push(SimulationEvent::Dashed { direction });
        }
        if self.state.player.dash_ticks > 0 {
            self.simulate_dash(was_grounded, events);
            self.tick_jump_buffer(false);
            self.tick_one_way_drop();
            return;
        }

        self.try_expand_dash_posture();

        let wall_before_move = self.detect_wall_contact(action.move_x);
        self.state.player.wall_contact = wall_before_move;
        if self.human_wall_assists
            && let Some(side) = wall_before_move
        {
            self.recent_wall = Some(side);
            self.wall_grace_ticks = self.human_wall_grace_ticks();
        }
        let effective_wall = wall_before_move.or_else(|| {
            (self.human_wall_assists && self.wall_grace_ticks > 0)
                .then_some(self.recent_wall)
                .flatten()
        });

        let mut jumped = false;
        if self.state.player.jump_buffer_ticks > 0
            && (was_grounded || self.state.player.coyote_ticks > 0)
        {
            let kind = if was_grounded {
                JumpKind::Grounded
            } else {
                JumpKind::Coyote
            };
            self.begin_jump();
            events.push(SimulationEvent::Jumped(kind));
            jumped = true;
        } else if self.state.player.jump_buffer_ticks > 0
            && self.abilities.wall_jump
            && effective_wall.is_some()
            && !was_grounded
        {
            let side = effective_wall.expect("checked above");
            self.begin_wall_jump(side);
            events.push(SimulationEvent::Jumped(JumpKind::Wall { side }));
            jumped = true;
        }

        if jump_released && self.wall_minimum_ascent_ticks == 0 {
            self.state.player.jump_hold_ticks = 0;
            if self.state.player.velocity_subpixels.y < JUMP_CUT_SPEED {
                self.state.player.velocity_subpixels.y = JUMP_CUT_SPEED;
            }
        }

        let effective_move_x = if self.human_wall_assists && self.wall_force_ticks > 0 {
            self.wall_force_direction
        } else {
            action.move_x
        };
        self.tick_retained_wall_momentum(effective_move_x);
        let tuning = horizontal_motion_tuning(self.movement_tuning, was_grounded);
        let desired_x = i32::from(effective_move_x) * tuning.maximum_speed;
        let control_input = effective_move_x;
        // Preserve the initial kick away from a wall for one full tick.
        if !matches!(
            events.last(),
            Some(SimulationEvent::Jumped(JumpKind::Wall { .. }))
        ) {
            self.state.player.velocity_subpixels.x = if control_input == 0 {
                approach(
                    self.state.player.velocity_subpixels.x,
                    0,
                    tuning.release_deceleration,
                )
            } else {
                let current = self.state.player.velocity_subpixels.x;
                let reversing = current != 0 && current.signum() != desired_x.signum();
                approach(
                    current,
                    desired_x,
                    if reversing {
                        tuning.reversal_acceleration
                    } else {
                        tuning.acceleration
                    },
                )
            };
        }

        let boosting = (action.jump || self.wall_minimum_ascent_ticks > 0)
            && self.state.player.jump_hold_ticks > 0
            && self.state.player.velocity_subpixels.y < 0;
        let gravity = if boosting { HELD_JUMP_GRAVITY } else { GRAVITY };
        self.state.player.velocity_subpixels.y =
            (self.state.player.velocity_subpixels.y + gravity).min(MAX_FALL_SPEED);
        if boosting {
            self.state.player.jump_hold_ticks -= 1;
        }

        // A crawl slide cannot stop: while compressed under a ceiling too low
        // to stand, the slide keeps carrying the player toward their facing
        // (input may reverse the facing, but never park them mid-passage).
        if self.state.player.dash_compressed
            && self.state.player.dash_ticks == 0
            && !self.can_stand_here()
        {
            let slide = i32::from(self.state.player.facing) * tuning.maximum_speed;
            if slide > 0 {
                self.state.player.velocity_subpixels.x =
                    self.state.player.velocity_subpixels.x.max(slide);
            } else if slide < 0 {
                self.state.player.velocity_subpixels.x =
                    self.state.player.velocity_subpixels.x.min(slide);
            }
        }

        self.move_horizontal();
        self.state.player.wall_contact = self.detect_wall_contact(effective_move_x);
        if self.state.player.wall_contact.is_none() {
            self.wall_ascent_carry_wall = None;
        }
        self.state.player.wall_sliding = self.abilities.wall_jump
            && !self.state.player.grounded
            && self.state.player.velocity_subpixels.y > WALL_SLIDE_SPEED
            && self
                .state
                .player
                .wall_contact
                .is_some_and(|side| effective_move_x == side.direction());
        if self.state.player.wall_sliding {
            self.state.player.velocity_subpixels.y = WALL_SLIDE_SPEED;
        }

        let hit_ground = self.move_vertical();
        if hit_ground && !was_grounded {
            events.push(SimulationEvent::Landed);
        }

        self.state.player.wall_contact = self.detect_wall_contact(effective_move_x);
        self.state.player.wall_sliding &= !hit_ground && self.state.player.wall_contact.is_some();
        if self.state.player.wall_contact.is_none() {
            self.wall_ascent_carry_wall = None;
        }
        if self.human_wall_assists
            && let Some(side) = self.state.player.wall_contact
        {
            self.recent_wall = Some(side);
            self.wall_grace_ticks = self.human_wall_grace_ticks();
        }

        if hit_ground && self.state.player.jump_buffer_ticks > 0 && !jumped {
            self.begin_jump();
            events.push(SimulationEvent::Jumped(JumpKind::Buffered));
            jumped = true;
        } else if !hit_ground
            && !jumped
            && self.abilities.wall_jump
            && self.state.player.jump_buffer_ticks > 0
            && self.state.player.wall_contact.is_some()
        {
            let side = self.state.player.wall_contact.expect("checked above");
            self.begin_wall_jump(side);
            events.push(SimulationEvent::Jumped(JumpKind::Wall { side }));
            jumped = true;
        }
        if hit_ground && !jumped {
            self.state.player.jump_hold_ticks = 0;
        }

        self.tick_jump_buffer(jumped);
        if self.human_wall_assists {
            self.wall_grace_ticks = self.wall_grace_ticks.saturating_sub(1);
            self.wall_force_ticks = self.wall_force_ticks.saturating_sub(1);
            if self.wall_minimum_ascent_ticks > 0 {
                self.wall_minimum_ascent_ticks -= 1;
                if self.wall_minimum_ascent_ticks == 0
                    && !action.jump
                    && self.state.player.velocity_subpixels.y < JUMP_CUT_SPEED
                {
                    self.state.player.jump_hold_ticks = 0;
                    self.state.player.velocity_subpixels.y = JUMP_CUT_SPEED;
                }
            }
        }
        self.state.player.coyote_ticks = if dropped_through {
            0
        } else if self.state.player.grounded {
            COYOTE_TICKS
        } else if jumped {
            0
        } else if was_grounded {
            COYOTE_TICKS
        } else {
            self.state.player.coyote_ticks.saturating_sub(1)
        };
        if self.abilities.dash && self.state.player.grounded {
            self.state.player.dash_available = true;
        }
        self.tick_one_way_drop();
    }

    fn begin_jump(&mut self) {
        self.state.player.velocity_subpixels.y = JUMP_SPEED;
        self.state.player.grounded = false;
        self.state.player.coyote_ticks = 0;
        self.state.player.jump_buffer_ticks = 0;
        self.state.player.jump_hold_ticks = JUMP_HOLD_TICKS;
        self.state.player.wall_sliding = false;
    }

    fn human_wall_grace_ticks(&self) -> u8 {
        milliseconds_to_ticks(u32::from(self.movement_tuning.wall_momentum_milliseconds))
    }

    fn begin_wall_jump(&mut self, side: WallSide) {
        self.begin_jump();
        let carried_speed = self.retained_wall_velocity_x.abs()
            * i32::from(self.movement_tuning.wall_carry_percent)
            / 100;
        self.retained_wall_velocity_x = 0;
        self.wall_velocity_retention_ticks = 0;
        self.wall_ascent_carry_wall = None;
        self.state.player.velocity_subpixels.x =
            -i32::from(side.direction()) * (WALL_JUMP_HORIZONTAL_SPEED + carried_speed);
        if self.human_wall_assists {
            self.wall_force_direction = -side.direction();
            self.wall_force_ticks = HUMAN_WALL_FORCE_TICKS;
            self.wall_minimum_ascent_ticks = HUMAN_WALL_MINIMUM_ASCENT_TICKS;
            self.wall_grace_ticks = 0;
            self.recent_wall = None;
        }
    }

    fn begin_dash(&mut self, direction: DashDirection) {
        if matches!(direction, DashDirection::Left | DashDirection::Right)
            && !self.state.player.dash_compressed
        {
            self.state.player.position_subpixels.y +=
                (PLAYER_HEIGHT - DASH_PLAYER_HEIGHT) * SUBPIXELS_PER_PIXEL;
            self.state.player.dash_compressed = true;
        }
        self.state.player.velocity_subpixels = direction.velocity();
        self.state.player.grounded = false;
        self.state.player.coyote_ticks = 0;
        self.state.player.jump_hold_ticks = 0;
        self.state.player.wall_sliding = false;
        self.state.player.dash_available = false;
        self.state.player.dash_ticks = DASH_TICKS;
        self.state.player.dash_direction = Some(direction);
        self.wall_force_ticks = 0;
        self.wall_minimum_ascent_ticks = 0;
        self.retained_wall_velocity_x = 0;
        self.wall_velocity_retention_ticks = 0;
        self.wall_ascent_carry_wall = None;
    }

    fn simulate_dash(&mut self, was_grounded: bool, events: &mut Vec<SimulationEvent>) {
        self.move_horizontal();
        let hit_ground = self.move_vertical();
        let grounded = hit_ground || self.has_ground_contact();
        self.state.player.grounded = grounded;
        self.state.player.wall_contact = self.detect_wall_contact(0);
        self.state.player.wall_sliding = false;

        if grounded && !was_grounded {
            events.push(SimulationEvent::Landed);
        }

        self.state.player.dash_ticks -= 1;
        if self.state.player.dash_ticks == 0 {
            // Do not leak dash-speed momentum into ordinary movement.
            self.state.player.velocity_subpixels.x = self
                .state
                .player
                .velocity_subpixels
                .x
                .clamp(-RUN_SPEED, RUN_SPEED);
            self.state.player.velocity_subpixels.y = 0;
            self.state.player.dash_direction = None;
            if self.abilities.dash && grounded {
                self.state.player.dash_available = true;
            }
            self.try_expand_dash_posture();
        }
        self.state.player.coyote_ticks = if grounded { COYOTE_TICKS } else { 0 };
    }

    fn tick_jump_buffer(&mut self, jumped: bool) {
        if !jumped && self.state.player.jump_buffer_ticks > 0 {
            self.state.player.jump_buffer_ticks -= 1;
        }
    }

    fn tick_one_way_drop(&mut self) {
        self.state.player.one_way_drop_ticks =
            self.state.player.one_way_drop_ticks.saturating_sub(1);
    }

    fn move_horizontal(&mut self) {
        let dx = self.state.player.velocity_subpixels.x;
        if dx == 0 {
            return;
        }
        let current = self.player_bounds_subpixels();
        let unrestricted_x = current.x + dx;
        let target_x =
            unrestricted_x.clamp(0, ROOM_WIDTH_PIXELS * SUBPIXELS_PER_PIXEL - current.width);
        let mut resolved_x = target_x;
        // Vertical spike flanks block movement but are not walls: they grant
        // neither ascent carry nor wall-velocity retention below.
        let mut blocked_by_wall = target_x != unrestricted_x;
        let swept_left = current.x.min(target_x);
        let swept_right = current.right().max(target_x + current.width);
        let swept = Rect::new(
            swept_left,
            current.y,
            swept_right - swept_left,
            current.height,
        );
        if let Some((first_x, last_x, first_y, last_y)) = self.tile_range(swept) {
            for y in first_y..=last_y {
                for x in first_x..=last_x {
                    if !self.tile_blocks_horizontal_motion(x, y, dx) {
                        continue;
                    }
                    let tile = scale_rect(self.room.tile_bounds(x, y));
                    if current.y >= tile.bottom() || current.bottom() <= tile.y {
                        continue;
                    }
                    let mut blocked_here = false;
                    if dx > 0 && current.right() <= tile.x && target_x + current.width > tile.x {
                        resolved_x = resolved_x.min(tile.x - current.width);
                        blocked_here = true;
                    } else if dx < 0 && current.x >= tile.right() && target_x < tile.right() {
                        resolved_x = resolved_x.max(tile.right());
                        blocked_here = true;
                    }
                    if blocked_here {
                        let side = if dx < 0 {
                            WallSide::Left
                        } else {
                            WallSide::Right
                        };
                        blocked_by_wall |= self.tile_has_wall_surface(x, y, side);
                    }
                }
            }
        }
        if resolved_x != target_x || target_x != unrestricted_x {
            let collision_side = if dx < 0 {
                WallSide::Left
            } else {
                WallSide::Right
            };
            if blocked_by_wall
                && !self.state.player.grounded
                && self.state.player.velocity_subpixels.y < 0
                && self.wall_ascent_carry_wall != Some(collision_side)
            {
                let upward_carry =
                    dx.abs() * i32::from(self.movement_tuning.wall_ascent_carry_percent) / 100;
                self.state.player.velocity_subpixels.y = (self.state.player.velocity_subpixels.y
                    - upward_carry)
                    .max(WALL_ASCENT_MAX_UPWARD_SPEED);
                self.wall_ascent_carry_wall = Some(collision_side);
            }
            let retention_ticks =
                horizontal_motion_tuning(self.movement_tuning, self.state.player.grounded)
                    .wall_retention_ticks;
            if blocked_by_wall && retention_ticks > 0 && self.wall_velocity_retention_ticks == 0 {
                self.retained_wall_velocity_x = dx;
                self.wall_velocity_retention_ticks = retention_ticks;
            }
            self.state.player.velocity_subpixels.x = 0;
        }
        self.state.player.position_subpixels.x = resolved_x;
    }

    fn tick_retained_wall_momentum(&mut self, move_x: i8) {
        if self.wall_velocity_retention_ticks == 0 {
            return;
        }
        if move_x != 0 && i32::from(move_x).signum() != self.retained_wall_velocity_x.signum() {
            self.retained_wall_velocity_x = 0;
            self.wall_velocity_retention_ticks = 0;
            return;
        }
        let retained_direction = self.retained_wall_velocity_x.signum() as i8;
        if self.detect_wall_contact(retained_direction).is_some() {
            // Contact itself does not consume memory. The duration is a grace window after
            // separating from the wall, not a deadline that can expire while attached.
            return;
        }
        self.wall_velocity_retention_ticks -= 1;
        if self.wall_velocity_retention_ticks == 0 {
            self.retained_wall_velocity_x = 0;
        }
    }

    fn move_vertical(&mut self) -> bool {
        let dy = self.state.player.velocity_subpixels.y;
        let current = self.player_bounds_subpixels();
        let unrestricted_y = current.y + dy;
        let target_y =
            unrestricted_y.clamp(0, ROOM_HEIGHT_PIXELS * SUBPIXELS_PER_PIXEL - current.height);
        let mut resolved_y = target_y;
        let mut hit_ground = dy > 0 && target_y != current.y + dy;
        let swept_top = current.y.min(target_y);
        let swept_bottom = current.bottom().max(target_y + current.height);
        let swept = Rect::new(
            current.x,
            swept_top,
            current.width,
            swept_bottom - swept_top,
        );
        if let Some((first_x, last_x, first_y, last_y)) = self.tile_range(swept) {
            for y in first_y..=last_y {
                for x in first_x..=last_x {
                    let Some(tile_kind) = self.room.tile(x, y) else {
                        continue;
                    };
                    // A hazard is solid only on its back face; every other
                    // face lets the player pass into the tile, where the
                    // overlap trigger kills them.
                    let collides_down = match tile_kind {
                        Tile::Solid => true,
                        Tile::OneWay => self.state.player.one_way_drop_ticks == 0,
                        Tile::HazardUp
                        | Tile::HazardDown
                        | Tile::HazardLeft
                        | Tile::HazardRight => matches!(
                            self.room
                                .hazard_direction(x, y)
                                .expect("hazard tile has a direction"),
                            HazardDirection::Down
                        ),
                        Tile::Empty => false,
                    };
                    let collides_up = match tile_kind {
                        Tile::Solid => true,
                        Tile::HazardUp
                        | Tile::HazardDown
                        | Tile::HazardLeft
                        | Tile::HazardRight => matches!(
                            self.room
                                .hazard_direction(x, y)
                                .expect("hazard tile has a direction"),
                            HazardDirection::Up
                        ),
                        Tile::Empty | Tile::OneWay => false,
                    };
                    if !((dy > 0 && collides_down) || (dy < 0 && collides_up)) {
                        continue;
                    }
                    let tile = scale_rect(self.room.tile_bounds(x, y));
                    if current.x >= tile.right() || current.right() <= tile.x {
                        continue;
                    }
                    if dy > 0
                        && collides_down
                        && current.bottom() <= tile.y
                        && target_y + current.height > tile.y
                    {
                        resolved_y = resolved_y.min(tile.y - current.height);
                        hit_ground = true;
                    } else if dy < 0
                        && collides_up
                        && current.y >= tile.bottom()
                        && target_y < tile.bottom()
                    {
                        resolved_y = resolved_y.max(tile.bottom());
                    }
                }
            }
        }
        if resolved_y != target_y || target_y != unrestricted_y || hit_ground {
            self.state.player.velocity_subpixels.y = 0;
        }
        self.state.player.position_subpixels.y = resolved_y;
        self.state.player.grounded = hit_ground;
        hit_ground
    }

    fn has_ground_contact(&self) -> bool {
        let current = self.player_bounds_subpixels();
        if current.bottom() == ROOM_HEIGHT_PIXELS * SUBPIXELS_PER_PIXEL {
            return true;
        }
        self.has_ground_contact_for(Tile::Solid)
            || (self.state.player.one_way_drop_ticks == 0
                && self.has_ground_contact_for(Tile::OneWay))
            || self.has_hazard_ground_contact()
    }

    fn has_ground_contact_for(&self, kind: Tile) -> bool {
        let current = self.player_bounds_subpixels();
        let candidates = Rect::new(current.x, current.bottom(), current.width, 1);
        let Some((first_x, last_x, first_y, last_y)) = self.tile_range(candidates) else {
            return false;
        };
        (first_y..=last_y).any(|y| {
            (first_x..=last_x).any(|x| {
                if self.room.tile(x, y) != Some(kind) {
                    return false;
                }
                let tile = scale_rect(self.room.tile_bounds(x, y));
                current.bottom() == tile.y && current.x < tile.right() && current.right() > tile.x
            })
        })
    }

    fn detect_wall_contact(&self, preferred_direction: i8) -> Option<WallSide> {
        let left = self.has_wall_contact(WallSide::Left);
        let right = self.has_wall_contact(WallSide::Right);
        match (left, right, preferred_direction) {
            (true, true, direction) if direction > 0 => Some(WallSide::Right),
            (true, true, direction) if direction < 0 => Some(WallSide::Left),
            (true, true, _) => self.state.player.wall_contact.or(Some(WallSide::Left)),
            (true, false, _) => Some(WallSide::Left),
            (false, true, _) => Some(WallSide::Right),
            (false, false, _) => None,
        }
    }

    fn has_wall_contact(&self, side: WallSide) -> bool {
        let current = self.player_bounds_subpixels();
        if (side == WallSide::Left && current.x == 0)
            || (side == WallSide::Right
                && current.right() == ROOM_WIDTH_PIXELS * SUBPIXELS_PER_PIXEL)
        {
            return true;
        }
        let candidates = match side {
            WallSide::Left => Rect::new(current.x - 1, current.y, 1, current.height),
            WallSide::Right => Rect::new(current.right(), current.y, 1, current.height),
        };
        let Some((first_x, last_x, first_y, last_y)) = self.tile_range(candidates) else {
            return false;
        };
        (first_y..=last_y).any(|y| {
            (first_x..=last_x).any(|x| {
                if !self.tile_has_wall_surface(x, y, side) {
                    return false;
                }
                let tile = scale_rect(self.room.tile_bounds(x, y));
                let adjacent = match side {
                    WallSide::Left => current.x == tile.right(),
                    WallSide::Right => current.right() == tile.x,
                };
                adjacent && current.y < tile.bottom() && current.bottom() > tile.y
            })
        })
    }

    fn tile_blocks_horizontal_motion(&self, x: u16, y: u16, dx: i32) -> bool {
        match self.room.tile(x, y) {
            Some(Tile::Solid) => true,
            Some(Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight) => {
                // A vertical spike's flanks are plain obstacles; a horizontal
                // spike blocks only on its back face, and every other face
                // lets the player pass into the lethal overlap.
                match self
                    .room
                    .hazard_direction(x, y)
                    .expect("hazard tile has a direction")
                {
                    HazardDirection::Up | HazardDirection::Down => true,
                    HazardDirection::Left => dx < 0,
                    HazardDirection::Right => dx > 0,
                }
            }
            Some(Tile::Empty | Tile::OneWay) | None => false,
        }
    }

    fn tile_has_wall_surface(&self, x: u16, y: u16, side: WallSide) -> bool {
        match self.room.tile(x, y) {
            Some(Tile::Solid) => true,
            Some(Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight) => {
                let direction = self
                    .room
                    .hazard_direction(x, y)
                    .expect("hazard tile has a direction");
                match side {
                    // A wall to the player's left exposes the tile's right
                    // face, which is safe only as a left spike's back.
                    WallSide::Left => direction == HazardDirection::Left,
                    // A wall to the player's right exposes the tile's left
                    // face, which is safe only as a right spike's back.
                    WallSide::Right => direction == HazardDirection::Right,
                }
            }
            Some(Tile::Empty | Tile::OneWay) | None => false,
        }
    }

    fn has_hazard_ground_contact(&self) -> bool {
        let current = self.player_bounds_subpixels();
        let candidates = Rect::new(current.x, current.bottom(), current.width, 1);
        let Some((first_x, last_x, first_y, last_y)) = self.tile_range(candidates) else {
            return false;
        };
        (first_y..=last_y).any(|y| {
            (first_x..=last_x).any(|x| {
                // Only a down spike's back (its top) supports the player.
                if self.room.hazard_direction(x, y) != Some(HazardDirection::Down) {
                    return false;
                }
                let tile = scale_rect(self.room.tile_bounds(x, y));
                current.bottom() == tile.y && current.x < tile.right() && current.right() > tile.x
            })
        })
    }

    /// Inclusive tile coordinates potentially intersected by a subpixel
    /// rectangle. Collision hot paths use this instead of scanning the room.
    fn tile_range(&self, bounds: Rect) -> Option<(u16, u16, u16, u16)> {
        if bounds.width <= 0 || bounds.height <= 0 {
            return None;
        }
        let tile_size = self.room.tile_size * SUBPIXELS_PER_PIXEL;
        let room_right = ROOM_WIDTH_PIXELS * SUBPIXELS_PER_PIXEL;
        let room_bottom = ROOM_HEIGHT_PIXELS * SUBPIXELS_PER_PIXEL;
        if bounds.right() <= 0
            || bounds.bottom() <= 0
            || bounds.x >= room_right
            || bounds.y >= room_bottom
        {
            return None;
        }
        let first_x = bounds
            .x
            .div_euclid(tile_size)
            .clamp(0, i32::from(self.room.width) - 1) as u16;
        let last_x = (bounds.right() - 1)
            .div_euclid(tile_size)
            .clamp(0, i32::from(self.room.width) - 1) as u16;
        let first_y = bounds
            .y
            .div_euclid(tile_size)
            .clamp(0, i32::from(self.room.height) - 1) as u16;
        let last_y = (bounds.bottom() - 1)
            .div_euclid(tile_size)
            .clamp(0, i32::from(self.room.height) - 1) as u16;
        Some((first_x, last_x, first_y, last_y))
    }

    fn process_triggers(&mut self, events: &mut Vec<SimulationEvent>) {
        let bounds = self.state.player.bounds();
        if let Some((tile_x, tile_y)) = self.room.first_tile_matching(bounds, Tile::is_hazard) {
            self.state.deaths = self.state.deaths.saturating_add(1);
            events.push(SimulationEvent::Died(DeathReason::Hazard {
                tile_x,
                tile_y,
            }));
            self.reset();
            events.push(SimulationEvent::Reset);
            return;
        }
        if let Some((hazard_index, _)) =
            self.room
                .timed_hazards
                .iter()
                .enumerate()
                .find(|(_, hazard)| {
                    hazard.is_active_at(self.state.room_tick) && bounds.intersects(hazard.bounds())
                })
        {
            self.state.deaths = self.state.deaths.saturating_add(1);
            events.push(SimulationEvent::Died(DeathReason::TimedHazard {
                hazard_index,
            }));
            self.reset();
            events.push(SimulationEvent::Reset);
            return;
        }

        for (pickup_index, pickup) in self.room.pickups.iter().enumerate() {
            if !self.state.collected_pickups[pickup_index] && bounds.intersects(pickup.bounds()) {
                self.state.collected_pickups[pickup_index] = true;
                self.state.pending_pickups[pickup_index] = true;
                events.push(SimulationEvent::PickupTouched {
                    id: pickup.id().to_owned(),
                });
            }
        }
        let newly_reached = if let Some(exit) = self
            .room
            .exits
            .iter()
            .find(|exit| bounds.intersects(exit.bounds))
        {
            Some(exit.id.clone())
        } else {
            self.room
                .doors
                .iter()
                .find(|door| bounds.intersects(door.trigger_bounds))
                // Doors intentionally share the legacy exit event/state contract:
                // targeted solvers can reach a door without a parallel objective.
                .map(|door| door.id.clone())
        };
        if let Some(id) = &newly_reached {
            self.state.reached_exit = Some(id.clone());
        }
        // Bank pending pickups once the player comes to rest on the ground
        // somewhere genuinely safe - a timed hazard's square is never safe,
        // even while dormant - or automatically on leaving the room (exit or
        // door) with the coin in hand; on a leaving tick the bank is reported
        // before the exit event.
        let inside_hazard_zone = self
            .room
            .timed_hazards
            .iter()
            .any(|hazard| bounds.intersects(hazard.bounds()));
        let at_rest = self.state.player.grounded
            && self.state.player.velocity_subpixels.x == 0
            && !inside_hazard_zone;
        if at_rest || self.state.reached_exit.is_some() {
            for pickup_index in 0..self.state.pending_pickups.len() {
                if self.state.pending_pickups[pickup_index] {
                    self.state.pending_pickups[pickup_index] = false;
                    events.push(SimulationEvent::PickupCollected {
                        id: self.room.pickups[pickup_index].id().to_owned(),
                    });
                }
            }
        }
        if let Some(id) = newly_reached {
            events.push(SimulationEvent::ExitReached { id });
        }
    }

    fn player_bounds_subpixels(&self) -> Rect {
        Rect::new(
            self.state.player.position_subpixels.x,
            self.state.player.position_subpixels.y,
            PLAYER_WIDTH * SUBPIXELS_PER_PIXEL,
            if self.state.player.dash_compressed {
                DASH_PLAYER_HEIGHT
            } else {
                PLAYER_HEIGHT
            } * SUBPIXELS_PER_PIXEL,
        )
    }

    /// Whether the compressed player has room to stand up in place.
    fn can_stand_here(&self) -> bool {
        let added_height = (PLAYER_HEIGHT - DASH_PLAYER_HEIGHT) * SUBPIXELS_PER_PIXEL;
        let expanded = Rect::new(
            self.state.player.position_subpixels.x,
            self.state.player.position_subpixels.y - added_height,
            PLAYER_WIDTH * SUBPIXELS_PER_PIXEL,
            PLAYER_HEIGHT * SUBPIXELS_PER_PIXEL,
        );
        expanded.y >= 0 && !self.expansion_is_blocked(expanded)
    }

    fn try_expand_dash_posture(&mut self) {
        if !self.state.player.dash_compressed || self.state.player.dash_ticks > 0 {
            return;
        }
        let added_height = (PLAYER_HEIGHT - DASH_PLAYER_HEIGHT) * SUBPIXELS_PER_PIXEL;
        let expanded = Rect::new(
            self.state.player.position_subpixels.x,
            self.state.player.position_subpixels.y - added_height,
            PLAYER_WIDTH * SUBPIXELS_PER_PIXEL,
            PLAYER_HEIGHT * SUBPIXELS_PER_PIXEL,
        );
        if expanded.y < 0 || self.expansion_is_blocked(expanded) {
            return;
        }
        self.state.player.position_subpixels.y -= added_height;
        self.state.player.dash_compressed = false;
    }

    fn expansion_is_blocked(&self, expanded: Rect) -> bool {
        let Some((first_x, last_x, first_y, last_y)) = self.tile_range(expanded) else {
            return true;
        };
        (first_y..=last_y).any(|y| {
            (first_x..=last_x).any(|x| match self.room.tile(x, y) {
                Some(
                    Tile::Solid
                    | Tile::HazardUp
                    | Tile::HazardDown
                    | Tile::HazardLeft
                    | Tile::HazardRight,
                ) => scale_rect(self.room.tile_bounds(x, y)).intersects(expanded),
                Some(Tile::Empty | Tile::OneWay) | None => false,
            })
        })
    }
}

fn spawn_player(spawn: Point, abilities: AbilitySet) -> PlayerState {
    PlayerState {
        position_subpixels: Point::new(
            spawn.x * SUBPIXELS_PER_PIXEL,
            spawn.y * SUBPIXELS_PER_PIXEL,
        ),
        velocity_subpixels: Point::new(0, 0),
        grounded: false,
        coyote_ticks: 0,
        jump_buffer_ticks: 0,
        jump_hold_ticks: 0,
        wall_contact: None,
        wall_sliding: false,
        facing: 1,
        dash_available: abilities.dash,
        dash_ticks: 0,
        dash_direction: None,
        dash_compressed: false,
        one_way_drop_ticks: 0,
    }
}

const fn scale_rect(rect: Rect) -> Rect {
    Rect::new(
        rect.x * SUBPIXELS_PER_PIXEL,
        rect.y * SUBPIXELS_PER_PIXEL,
        rect.width * SUBPIXELS_PER_PIXEL,
        rect.height * SUBPIXELS_PER_PIXEL,
    )
}

fn approach(value: i32, target: i32, amount: i32) -> i32 {
    if value < target {
        (value + amount).min(target)
    } else if value > target {
        (value - amount).max(target)
    } else {
        value
    }
}

struct DigestBuilder(u64);
impl DigestBuilder {
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
    fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }
    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn i32(&mut self, value: i32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }
    fn point(&mut self, value: Point) {
        self.i32(value.x);
        self.i32(value.y);
    }
    fn option_wall_side(&mut self, value: Option<WallSide>) {
        self.byte(match value {
            None => 0,
            Some(WallSide::Left) => 1,
            Some(WallSide::Right) => 2,
        });
    }
    fn option_dash_direction(&mut self, value: Option<DashDirection>) {
        match value {
            None => self.byte(0),
            Some(direction) => {
                self.byte(1);
                self.byte(direction.digest_tag());
            }
        }
    }
    const fn finish(self) -> u64 {
        self.0
    }
}
