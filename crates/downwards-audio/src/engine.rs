//! Live playback: a `cpal` output stream whose callback runs the shared
//! [`Sequencer`] (§7). The callback is wait-free: it reads a `try_lock`
//! mailbox for room swaps (contended ~never — swaps happen only on room
//! entry) and a handful of atomics for the sim clock, abilities, and mute.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{
    sequencer::{AbilityMask, Sequencer, sample_index_for_tick},
    tempo::HazardTiming,
    track::Track,
};

/// Overall output volume of the live engine.
const MASTER_VOLUME: f32 = 0.5;
/// Room transition (§7.6, retuned 2026-08 — designer feedback: the 150 ms
/// hard swap was too abrupt). The outgoing room fades over
/// [`FADE_OUT_SECONDS`] on a raised-cosine curve, a breath of silence
/// [`TRANSITION_GAP_SECONDS`] separates the rooms, then the new room fades in
/// over [`FADE_IN_SECONDS`]. Respawn within a room never takes this path —
/// the music free-runs straight through it (§7.3 as amended), the timeline
/// merely re-basing by whole hazard cycles.
pub const FADE_OUT_SECONDS: f64 = 0.4;
pub const TRANSITION_GAP_SECONDS: f64 = 0.12;
pub const FADE_IN_SECONDS: f64 = 1.5;
/// Mute toggle ramp.
const MUTE_SECONDS: f64 = 0.03;
/// Maximum slew per block, as a fraction of the block length (§7.3).
const SLEW_FRACTION: f64 = 0.001;

struct Shared {
    /// Pending room session, prebuilt off the audio thread.
    pending: Mutex<Option<Box<Sequencer>>>,
    /// Latest simulation tick and the microsecond timestamp it was published.
    published_tick: AtomicU64,
    published_at_micros: AtomicU64,
    /// bit0 = gloves, bit1 = boots.
    abilities: AtomicU64,
    muted: AtomicBool,
    /// While the simulation is not stepping (menus, pause screen) the music
    /// ducks to silence instead of drifting against a frozen clock; the
    /// resume re-base/slew then realigns inaudibly.
    paused: AtomicBool,
}

/// The client-facing engine. Inert (all methods no-ops) when audio is
/// disabled or the output device cannot be opened — audio must never crash
/// the game.
pub struct AudioEngine {
    shared: Option<Arc<Shared>>,
    epoch: Instant,
    sample_rate: u32,
    _stream: Option<cpal::Stream>,
}

impl AudioEngine {
    #[must_use]
    pub fn new(no_audio: bool) -> Self {
        let epoch = Instant::now();
        if no_audio {
            return Self::inert(epoch);
        }
        match Self::open_stream(epoch) {
            Ok(engine) => engine,
            Err(message) => {
                eprintln!("audio disabled: {message}");
                Self::inert(epoch)
            }
        }
    }

    fn inert(epoch: Instant) -> Self {
        Self {
            shared: None,
            epoch,
            sample_rate: 44_100,
            _stream: None,
        }
    }

    fn open_stream(epoch: Instant) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_owned())?;
        let config = device
            .default_output_config()
            .map_err(|error| format!("no default output config: {error}"))?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "unsupported sample format {:?}",
                config.sample_format()
            ));
        }
        let stream_config: cpal::StreamConfig = config.into();
        let sample_rate = stream_config.sample_rate;
        let channels = stream_config.channels as usize;

        let shared = Arc::new(Shared {
            pending: Mutex::new(None),
            published_tick: AtomicU64::new(0),
            published_at_micros: AtomicU64::new(0),
            abilities: AtomicU64::new(0),
            muted: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        });

        let mut callback = Callback::new(Arc::clone(&shared), epoch, sample_rate);
        let mut mono = vec![0.0_f32; 4096];
        let stream = device
            .build_output_stream(
                stream_config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels.max(1);
                    if mono.len() < frames {
                        mono.resize(frames, 0.0);
                    }
                    callback.render(&mut mono[..frames]);
                    for (frame, &sample) in mono[..frames].iter().enumerate() {
                        for channel in 0..channels {
                            data[frame * channels + channel] = sample;
                        }
                    }
                },
                |error| eprintln!("audio stream error: {error}"),
                None,
            )
            .map_err(|error| format!("could not build output stream: {error}"))?;
        stream
            .play()
            .map_err(|error| format!("could not start output stream: {error}"))?;

        Ok(Self {
            shared: Some(shared),
            epoch,
            sample_rate,
            _stream: Some(stream),
        })
    }

    /// Whether a real output stream is running.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.shared.is_some()
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Swap to a new room's track. The sequencer is built here, on the game
    /// thread, so the audio callback never allocates a session.
    pub fn enter_room(&self, track: &Track, hazards: &[HazardTiming], abilities: AbilityMask) {
        let Some(shared) = &self.shared else { return };
        let mut sequencer = Box::new(Sequencer::new(track, hazards, self.sample_rate));
        sequencer.set_layer_gains_immediate(abilities);
        self.set_abilities(abilities);
        *shared.pending.lock().expect("audio mailbox is never poisoned") = Some(sequencer);
    }

    /// Publish the simulation's current room tick (call every fixed step or
    /// once per frame after stepping).
    pub fn publish_tick(&self, room_tick: u64) {
        let Some(shared) = &self.shared else { return };
        let micros = u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        shared.published_tick.store(room_tick, Ordering::Relaxed);
        shared.published_at_micros.store(micros, Ordering::Relaxed);
    }

    pub fn set_abilities(&self, abilities: AbilityMask) {
        let Some(shared) = &self.shared else { return };
        let bits = u64::from(abilities.gloves) | (u64::from(abilities.boots) << 1);
        shared.abilities.store(bits, Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        let Some(shared) = &self.shared else { return };
        shared.muted.store(muted, Ordering::Relaxed);
    }

    /// Duck the music while the simulation clock is not advancing.
    pub fn set_paused(&self, paused: bool) {
        let Some(shared) = &self.shared else { return };
        shared.paused.store(paused, Ordering::Relaxed);
    }
}

struct Callback {
    shared: Arc<Shared>,
    epoch: Instant,
    sample_rate: u32,
    active: Option<Box<Sequencer>>,
    /// Fade state for room transitions: 1.0 = fully open.
    transition: crate::synth::voice::CosRamp,
    mute: crate::synth::voice::CosRamp,
    pause: crate::synth::voice::CosRamp,
    fading_out: bool,
    /// Samples of deliberate quiet still to elapse between the old room's
    /// fade-out and the new room's fade-in.
    gap_remaining: u64,
    /// Whole-hazard-cycle re-basing of the engine timeline versus the sim
    /// clock (in ticks). Lets respawns play through without any seek.
    tick_offset: f64,
    scratch: Vec<f32>,
}

impl Callback {
    fn new(shared: Arc<Shared>, epoch: Instant, sample_rate: u32) -> Self {
        Self {
            shared,
            epoch,
            sample_rate,
            active: None,
            transition: crate::synth::voice::CosRamp::new(1.0),
            mute: crate::synth::voice::CosRamp::new(1.0),
            pause: crate::synth::voice::CosRamp::new(1.0),
            fading_out: false,
            gap_remaining: 0,
            tick_offset: 0.0,
            scratch: vec![0.0; 8192],
        }
    }

    fn estimated_sim_tick(&self) -> f64 {
        let tick = self.shared.published_tick.load(Ordering::Relaxed);
        let at = self.shared.published_at_micros.load(Ordering::Relaxed);
        let now = u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        let elapsed_micros = now.saturating_sub(at) as f64;
        tick as f64 + elapsed_micros * 60.0 / 1_000_000.0
    }

    fn render(&mut self, out: &mut [f32]) {
        // Room swap protocol (§7.6): fade out, swap at zero, fade in.
        let pending_waiting = self
            .shared
            .pending
            .try_lock()
            .map(|pending| pending.is_some())
            .unwrap_or(false);
        let fade_out_samples = FADE_OUT_SECONDS * f64::from(self.sample_rate);
        let fade_in_samples = FADE_IN_SECONDS * f64::from(self.sample_rate);
        if pending_waiting && !self.fading_out && self.active.is_some() {
            self.fading_out = true;
            self.gap_remaining =
                (TRANSITION_GAP_SECONDS * f64::from(self.sample_rate)) as u64;
            self.transition.set_target(0.0, fade_out_samples);
        }
        let transition_closed = self.transition.value() <= 1.0e-3 && self.transition.done();
        // Once the fade-out has completed, hold a short breath of silence
        // before the swap so the rooms read as separate spaces.
        if self.fading_out && transition_closed && self.gap_remaining > 0 {
            self.gap_remaining = self.gap_remaining.saturating_sub(out.len() as u64);
        }
        if pending_waiting
            && (self.active.is_none()
                || (self.fading_out && transition_closed && self.gap_remaining == 0))
            && let Ok(mut pending) = self.shared.pending.try_lock()
            && let Some(mut sequencer) = pending.take()
        {
            let target_tick = self.estimated_sim_tick().max(0.0) as u64;
            sequencer.seek(sample_index_for_tick(target_tick, self.sample_rate));
            let abilities = self.ability_mask();
            sequencer.set_layer_targets(abilities);
            self.active = Some(sequencer);
            self.tick_offset = 0.0;
            self.fading_out = false;
            self.transition.snap(0.0);
            self.transition.set_target(1.0, fade_in_samples);
        }

        let est = self.estimated_sim_tick();
        let muted = self.shared.muted.load(Ordering::Relaxed);
        let paused = self.shared.paused.load(Ordering::Relaxed);
        self.pause.set_target(
            if paused { 0.0 } else { 1.0 },
            0.1 * f64::from(self.sample_rate),
        );
        let ability_bits = self.shared.abilities.load(Ordering::Relaxed);
        let Some(sequencer) = self.active.as_mut() else {
            out.fill(0.0);
            return;
        };

        // Ability layers and mute.
        sequencer.set_layer_targets(AbilityMask {
            gloves: ability_bits & 1 != 0,
            boots: ability_bits & 2 != 0,
        });
        self.mute.set_target(
            if muted { 0.0 } else { 1.0 },
            MUTE_SECONDS * f64::from(self.sample_rate),
        );

        // Drift correction (§7.3, amended 2026-08). A death respawn resets
        // room_tick to 0 (simulation.rs); the music must play straight
        // through it with NO interruption — no seek, no fade. Instead the
        // engine re-bases its timeline by a whole number of hazard cycles
        // (a shift that changes no hazard sound, so wind-ups and fires stay
        // exactly aligned with the reset clocks) and lets the normal gentle
        // slew absorb whatever sub-cycle residue remains.
        let engine_tick =
            sequencer.cursor() as f64 * 60.0 / f64::from(self.sample_rate);
        let drift_ticks = engine_tick - (est + self.tick_offset);
        let beat_ticks = f64::from(sequencer.grid().beat_ticks());
        let block = out.len();
        if !paused && drift_ticks.abs() > 2.0 * beat_ticks {
            let cycle = sequencer.hazard_cycle_ticks() as f64;
            self.tick_offset = cycle * ((engine_tick - est) / cycle).round();
        }
        let drift_ticks = engine_tick - (est + self.tick_offset);
        // One output sample expressed in ticks; drift below this is noise.
        let one_sample_ticks = 60.0 / f64::from(self.sample_rate);
        let slew: i64 = if drift_ticks.abs() < one_sample_ticks {
            0
        } else {
            let max_slew = ((block as f64) * SLEW_FRACTION).ceil() as i64;
            if drift_ticks > 0.0 { -max_slew } else { max_slew }
        };

        // Render block ± slew musical samples, then map onto the output.
        let want = (block as i64 + slew).max(1) as usize;
        if self.scratch.len() < want {
            self.scratch.resize(want, 0.0);
        }
        sequencer.render(&mut self.scratch[..want]);
        match want.cmp(&block) {
            std::cmp::Ordering::Equal => out.copy_from_slice(&self.scratch[..block]),
            std::cmp::Ordering::Greater => {
                // Engine was behind: drop the first extra samples.
                out.copy_from_slice(&self.scratch[want - block..want]);
            }
            std::cmp::Ordering::Less => {
                // Engine was ahead: hold the final sample for the remainder.
                out[..want].copy_from_slice(&self.scratch[..want]);
                let last = self.scratch[want - 1];
                out[want..].fill(last);
            }
        }

        for sample in out.iter_mut() {
            *sample *=
                self.transition.advance() * self.mute.advance() * self.pause.advance() * MASTER_VOLUME;
        }
    }

    fn ability_mask(&self) -> AbilityMask {
        let bits = self.shared.abilities.load(Ordering::Relaxed);
        AbilityMask {
            gloves: bits & 1 != 0,
            boots: bits & 2 != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{AbilityReq, LayerGates, RoomMusicInputs, compose};
    use crate::melody::DoorSet;

    const RATE: u32 = 44_100;

    fn fade_out_samples() -> u64 {
        (FADE_OUT_SECONDS * f64::from(RATE)) as u64
    }
    fn gap_samples() -> u64 {
        (TRANSITION_GAP_SECONDS * f64::from(RATE)) as u64
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn transition_lengths_are_gentle() {
        // Designer feedback 2026-08: substantially longer than the old 150 ms
        // halves, and the fade-in specifically "a second or two".
        assert!(FADE_OUT_SECONDS >= 0.3);
        assert!((1.0..=2.0).contains(&FADE_IN_SECONDS));
        assert!(TRANSITION_GAP_SECONDS >= 0.05);
        let total = FADE_OUT_SECONDS + TRANSITION_GAP_SECONDS + FADE_IN_SECONDS;
        assert!((1.4..=2.6).contains(&total), "total transition {total}s");
    }

    fn test_shared() -> Arc<Shared> {
        Arc::new(Shared {
            pending: Mutex::new(None),
            published_tick: AtomicU64::new(0),
            published_at_micros: AtomicU64::new(0),
            abilities: AtomicU64::new(0),
            muted: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        })
    }

    fn test_sequencer_with(slug: &str, hazards: Vec<crate::tempo::HazardTiming>) -> Box<Sequencer> {
        let inputs = RoomMusicInputs {
            slug: slug.to_owned(),
            difficulty: crate::tempo::Difficulty::Easy,
            ability_requirement: AbilityReq::None,
            coin_count: 2,
            doors: DoorSet::default(),
            hazards: hazards.clone(),
            layer_gates: LayerGates::default(),
        };
        let track = compose(&inputs);
        Box::new(Sequencer::new(&track, &hazards, RATE))
    }

    fn test_sequencer(slug: &str) -> Box<Sequencer> {
        test_sequencer_with(slug, Vec::new())
    }

    /// A same-room respawn (sim room_tick jumping back to 0) must not
    /// interrupt the music at all: no seek, no fade — playback free-runs and
    /// the timeline re-bases by whole hazard cycles.
    #[test]
    fn respawn_plays_straight_through() {
        let shared = test_shared();
        let mut callback = Callback::new(Arc::clone(&shared), Instant::now(), RATE);
        let block = 512_usize;
        let mut out = vec![0.0_f32; block];
        let hazard = crate::tempo::HazardTiming {
            period: 90,
            active: 30,
            phase: 0,
        };

        // Enter a room 60 simulated seconds in.
        shared.published_tick.store(3600, Ordering::Relaxed);
        *shared.pending.lock().unwrap() =
            Some(test_sequencer_with("respawn-room", vec![hazard]));
        for _ in 0..100 {
            callback.render(&mut out);
        }
        let cursor_before = callback.active.as_ref().unwrap().cursor();
        let last_sample = out[block - 1];

        // Respawn: the sim clock snaps back to zero.
        shared.published_tick.store(0, Ordering::Relaxed);
        callback.render(&mut out);

        // No seek: the sequencer cursor advanced by at most the rendered
        // block plus the ±1-sample slew, never jumped.
        let cursor_after = callback.active.as_ref().unwrap().cursor();
        let advanced = cursor_after - cursor_before;
        assert!(
            (advanced as i64 - block as i64).abs() <= 2,
            "cursor must advance continuously through a respawn (advanced {advanced})"
        );
        // No click at the boundary.
        let boundary_jump = (out[0] - last_sample).abs();
        assert!(
            boundary_jump < 0.2,
            "respawn must not produce a discontinuity (jump {boundary_jump})"
        );
        // The timeline re-based by a whole number of hazard cycles, so the
        // hazard voices stay exactly aligned with the reset clocks.
        let cycles = callback.tick_offset / 90.0;
        assert!(callback.tick_offset > 0.0, "respawn must re-base the timeline");
        assert!(
            (cycles - cycles.round()).abs() < 1.0e-9,
            "re-base must be a whole number of hazard cycles ({})",
            callback.tick_offset
        );
        // And the music keeps sounding.
        for _ in 0..20 {
            callback.render(&mut out);
        }
        assert!(out.iter().any(|sample| sample.abs() > 0.01));
    }

    /// Push a pending session into an already-playing callback and verify the
    /// swap happens only after the full fade-out plus the silent gap, with
    /// output actually quiet at the swap point.
    #[test]
    fn room_swap_waits_for_fade_out_and_gap() {
        let shared = test_shared();
        let mut callback = Callback::new(Arc::clone(&shared), Instant::now(), RATE);
        let block = 512_usize;
        let mut out = vec![0.0_f32; block];

        // First room: swaps in immediately (no previous session).
        *shared.pending.lock().unwrap() = Some(test_sequencer("engine-room-a"));
        callback.render(&mut out);
        assert!(callback.active.is_some(), "first room must start playing");

        // Let it run past its own fade-in.
        for _ in 0..200 {
            callback.render(&mut out);
        }

        // Now request a room change.
        *shared.pending.lock().unwrap() = Some(test_sequencer("engine-room-b"));
        let earliest = fade_out_samples() + gap_samples();
        let mut rendered = 0_u64;
        let mut swap_at = None;
        let mut quiet_at_swap = true;
        for _ in 0..1000 {
            let was_fading = callback.fading_out;
            callback.render(&mut out);
            rendered += block as u64;
            if was_fading && !callback.fading_out {
                swap_at = Some(rendered);
                quiet_at_swap = out.iter().all(|sample| sample.abs() < 0.05);
                break;
            }
        }
        let swap_at = swap_at.expect("the pending room must eventually swap in");
        assert!(
            swap_at + (block as u64) >= earliest,
            "swap after {swap_at} samples, before fade-out + gap ({earliest})"
        );
        assert!(
            swap_at <= earliest + 20 * block as u64,
            "swap after {swap_at} samples: gap far longer than intended ({earliest})"
        );
        assert!(quiet_at_swap, "the swap block must be near-silent");
    }
}
