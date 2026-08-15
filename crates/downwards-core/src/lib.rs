//! Deterministic, renderer-independent game simulation.

#![forbid(unsafe_code)]

mod geometry;
mod room;
mod simulation;

pub use geometry::{Point, Rect};
pub use room::{
    BoundarySide, Door, DoorError, DoorSocket, Exit, Pickup, Room, RoomError, RoomObjectError,
    Tile, TimedHazard,
};
pub use simulation::{
    AbilitySet, Action, COYOTE_TICKS, DASH_TICKS, DashDirection, DeathReason, DoorEntryError,
    JUMP_BUFFER_TICKS, JUMP_HOLD_TICKS, JumpKind, MovementTuning, ONE_WAY_DROP_TICKS,
    PLAYER_HEIGHT, PLAYER_MOVEMENT_POLICY_VERSION, PLAYER_WIDTH, PlayerState, SUBPIXELS_PER_PIXEL,
    Simulation, SimulationEvent, StateDigest, StepReport, TICKS_PER_SECOND, WallSide,
};
