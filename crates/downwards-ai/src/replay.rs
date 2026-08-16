use std::{error::Error, fmt};

use downwards_core::{
    Action, DashDirection, DeathReason, JumpKind, Simulation, SimulationEvent, StateDigest,
    WallSide,
};

/// Version of the stable byte encoding used by [`EventDigest`].
pub const EVENT_DIGEST_VERSION: u32 = 1;

/// Stable, non-cryptographic digest of one tick's ordered simulation events.
///
/// The encoding includes the event count, event order, variant tags, and every
/// event payload. It is intended for deterministic replay integrity and
/// regression fingerprints, not adversarial tamper resistance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventDigest(pub u64);

impl EventDigest {
    /// Digest an exact ordered event stream using the current stable encoding.
    #[must_use]
    pub fn from_events(events: &[SimulationEvent]) -> Self {
        digest_events(events)
    }
}

impl fmt::Display for EventDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016x}", self.0)
    }
}

/// One authoritative simulation tick in a replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayFrame {
    pub action: Action,
    pub expected_digest: StateDigest,
    pub expected_event_digest: EventDigest,
}

/// A replay is tied to an exact initial simulation state and records the
/// expected state and event-stream digests after every action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    pub initial_digest: StateDigest,
    pub frames: Vec<ReplayFrame>,
}

impl Replay {
    /// Record actions by stepping a clone of `initial` through the real core
    /// simulation. The supplied simulation is not mutated.
    #[must_use]
    pub fn record(initial: &Simulation, actions: impl IntoIterator<Item = Action>) -> Self {
        record_replay(initial, actions)
    }

    /// Verify this replay from `initial`, returning the first mismatch.
    pub fn verify(&self, initial: &Simulation) -> Result<ReplayVerification, ReplayDivergence> {
        verify_replay(initial, self)
    }

    #[must_use]
    pub fn actions(&self) -> impl ExactSizeIterator<Item = Action> + '_ {
        self.frames.iter().map(|frame| frame.action)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayVerification {
    pub frames_verified: usize,
    pub final_tick: u64,
    pub final_digest: StateDigest,
    pub reached_exit: Option<String>,
    /// Stable IDs of every pickup collected by the exact replay, in room
    /// declaration order. This lets callers verify pickup objectives without
    /// interpreting state-digest internals.
    pub collected_pickup_ids: Vec<String>,
}

/// A replay always reports the earliest known point at which execution
/// differs from the recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayDivergence {
    InitialState {
        expected: StateDigest,
        actual: StateDigest,
    },
    Frame {
        frame_index: usize,
        tick: u64,
        action: Action,
        expected: StateDigest,
        actual: StateDigest,
    },
    EventStream {
        frame_index: usize,
        tick: u64,
        action: Action,
        expected: EventDigest,
        actual: EventDigest,
        actual_events: Vec<SimulationEvent>,
    },
}

impl fmt::Display for ReplayDivergence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitialState { expected, actual } => write!(
                formatter,
                "replay initial state differs: expected {expected}, got {actual}"
            ),
            Self::Frame {
                frame_index,
                tick,
                action,
                expected,
                actual,
            } => write!(
                formatter,
                "replay first diverged at frame {frame_index} (simulation tick {tick}, action {action:?}): expected {expected}, got {actual}"
            ),
            Self::EventStream {
                frame_index,
                tick,
                action,
                expected,
                actual,
                actual_events,
            } => write!(
                formatter,
                "replay event stream first diverged at frame {frame_index} (simulation tick {tick}, action {action:?}): expected event digest {expected}, got {actual} from {actual_events:?}"
            ),
        }
    }
}

impl Error for ReplayDivergence {}

#[must_use]
pub fn record_replay(initial: &Simulation, actions: impl IntoIterator<Item = Action>) -> Replay {
    let mut simulation = initial.clone();
    let frames = actions
        .into_iter()
        .map(|action| {
            let report = simulation.step(action);
            ReplayFrame {
                action,
                expected_digest: report.digest,
                expected_event_digest: digest_events(&report.events),
            }
        })
        .collect();
    Replay {
        initial_digest: initial.digest(),
        frames,
    }
}

pub fn verify_replay(
    initial: &Simulation,
    replay: &Replay,
) -> Result<ReplayVerification, ReplayDivergence> {
    let actual_initial = initial.digest();
    if actual_initial != replay.initial_digest {
        return Err(ReplayDivergence::InitialState {
            expected: replay.initial_digest,
            actual: actual_initial,
        });
    }

    let mut simulation = initial.clone();
    for (frame_index, frame) in replay.frames.iter().enumerate() {
        let report = simulation.step(frame.action);
        if report.digest != frame.expected_digest {
            return Err(ReplayDivergence::Frame {
                frame_index,
                tick: report.tick,
                action: frame.action,
                expected: frame.expected_digest,
                actual: report.digest,
            });
        }
        let actual_event_digest = digest_events(&report.events);
        if actual_event_digest != frame.expected_event_digest {
            return Err(ReplayDivergence::EventStream {
                frame_index,
                tick: report.tick,
                action: frame.action,
                expected: frame.expected_event_digest,
                actual: actual_event_digest,
                actual_events: report.events,
            });
        }
    }

    Ok(ReplayVerification {
        frames_verified: replay.frames.len(),
        final_tick: simulation.tick(),
        final_digest: simulation.digest(),
        reached_exit: simulation.reached_exit().map(str::to_owned),
        collected_pickup_ids: simulation
            .collected_pickups()
            .map(|pickup| pickup.id().to_owned())
            .collect(),
    })
}

/// Compute the stable digest of an exact ordered event stream.
#[must_use]
pub fn digest_events(events: &[SimulationEvent]) -> EventDigest {
    let mut digest = EventDigestBuilder::new();
    digest.bytes(b"downwards-replay-events");
    digest.u32(EVENT_DIGEST_VERSION);
    digest.u64(events.len() as u64);
    for event in events {
        digest.event(event);
    }
    EventDigest(digest.finish())
}

/// FNV-1a with explicit fixed-width little-endian payload encodings. Keep all
/// enum matches exhaustive so adding a core event cannot silently reuse an
/// existing replay encoding.
struct EventDigestBuilder(u64);

impl EventDigestBuilder {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn event(&mut self, event: &SimulationEvent) {
        match event {
            SimulationEvent::Jumped(kind) => {
                self.byte(0);
                self.jump_kind(*kind);
            }
            SimulationEvent::Dashed { direction } => {
                self.byte(1);
                self.dash_direction(*direction);
            }
            SimulationEvent::Landed => self.byte(2),
            SimulationEvent::Died(reason) => {
                self.byte(3);
                self.death_reason(*reason);
            }
            SimulationEvent::PickupTouched { id } => {
                self.byte(7);
                self.bytes(id.as_bytes());
            }
            SimulationEvent::PickupCollected { id } => {
                self.byte(4);
                self.bytes(id.as_bytes());
            }
            SimulationEvent::Reset => self.byte(5),
            SimulationEvent::ExitReached { id } => {
                self.byte(6);
                self.bytes(id.as_bytes());
            }
        }
    }

    fn jump_kind(&mut self, kind: JumpKind) {
        match kind {
            JumpKind::Grounded => self.byte(0),
            JumpKind::Coyote => self.byte(1),
            JumpKind::Buffered => self.byte(2),
            JumpKind::Wall { side } => {
                self.byte(3);
                self.wall_side(side);
            }
        }
    }

    fn wall_side(&mut self, side: WallSide) {
        self.byte(match side {
            WallSide::Left => 0,
            WallSide::Right => 1,
        });
    }

    fn dash_direction(&mut self, direction: DashDirection) {
        self.byte(match direction {
            DashDirection::Up => 0,
            DashDirection::UpRight => 1,
            DashDirection::Right => 2,
            DashDirection::DownRight => 3,
            DashDirection::Down => 4,
            DashDirection::DownLeft => 5,
            DashDirection::Left => 6,
            DashDirection::UpLeft => 7,
        });
    }

    fn death_reason(&mut self, reason: DeathReason) {
        match reason {
            DeathReason::Hazard { tile_x, tile_y } => {
                self.byte(0);
                self.u16(tile_x);
                self.u16(tile_y);
            }
            DeathReason::TimedHazard { hazard_index } => {
                self.byte(1);
                self.u64(hazard_index as u64);
            }
        }
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        self.u64(values.len() as u64);
        for &value in values {
            self.byte(value);
        }
    }

    fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
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

    const fn finish(self) -> u64 {
        self.0
    }
}
