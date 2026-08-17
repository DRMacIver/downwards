//! Procedural chiptune soundtrack for downwards.
//!
//! Two strictly separated phases:
//!
//! 1. **Compile**: [`compose`] deterministically maps a room's machine
//!    metadata (difficulty, ability requirement, coins, doors, timed-hazard
//!    clocks) to a [`Track`]. `downwards-tools compose-tracks` writes one
//!    human-editable `.track` artifact per room into `downwards-content`.
//! 2. **Play**: the client hands the parsed [`Track`] plus the room's live
//!    [`HazardTiming`] values to the engine; a `cpal` callback (or the
//!    offline renderer) synthesizes NES-style voices. Hazard event *times*
//!    are always recomputed from the hazard clocks; the artifact carries only
//!    the sounds.
//!
//! # Ambient-awareness contract (§6.9)
//!
//! | Hear... | Means... |
//! |---|---|
//! | Mode colour (open / flat-7 / dorian) | difficulty easy / medium / hard |
//! | Tonal centre (C/D/E/G) | ability requirement none/wall/dash/both |
//! | Tempo family | hazard period family (derived, exact) |
//! | Rising pickup → snare + low stab | a locked hazard arming → firing, on the beat |
//! | Noise sweep → crash, off the grid | an incommensurate hazard arming → firing |
//! | Low crackle bed | some timed hazard is live right now |
//! | Extra triangle/pulse counter-melody | you own gloves/boots AND they matter here |
//! | Busier lead | more coins to find here |

#![forbid(unsafe_code)]

pub mod compose;
#[cfg(feature = "live")]
pub mod engine;
pub mod melody;
pub mod offline;
pub mod sequencer;
pub mod synth;
pub mod tempo;
pub mod theory;
pub mod track;
pub mod wav;

pub use compose::{AbilityReq, LayerGates, RoomMusicInputs, compose};
#[cfg(feature = "live")]
pub use engine::AudioEngine;
pub use melody::DoorSet;
pub use sequencer::{AbilityMask, Sequencer, sample_index_for_tick, tick_for_sample};
pub use tempo::{Difficulty, GridChoice, HazardTiming, WARNING_TICKS, derive_grid};
pub use theory::{Key, Mode, Pitch, PitchClass, fnv1a64};
pub use track::{NoteEvent, Track, TrackParseError};
