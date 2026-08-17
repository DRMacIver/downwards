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
/// Room transition fade halves (§7.6).
const TRANSITION_SECONDS: f64 = 0.15;
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
    /// resume drift-snap then realigns inaudibly.
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
        let ramp_samples = TRANSITION_SECONDS * f64::from(self.sample_rate);
        if pending_waiting && !self.fading_out && self.active.is_some() {
            self.fading_out = true;
            self.transition.set_target(0.0, ramp_samples);
        }
        let transition_closed = self.transition.value() <= 1.0e-3 && self.transition.done();
        if pending_waiting
            && (self.active.is_none() || (self.fading_out && transition_closed))
            && let Ok(mut pending) = self.shared.pending.try_lock()
            && let Some(mut sequencer) = pending.take()
        {
            let target_tick = self.estimated_sim_tick().max(0.0) as u64;
            sequencer.seek(sample_index_for_tick(target_tick, self.sample_rate));
            let abilities = self.ability_mask();
            sequencer.set_layer_targets(abilities);
            self.active = Some(sequencer);
            self.fading_out = false;
            self.transition.snap(0.0);
            self.transition.set_target(1.0, ramp_samples);
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

        // Drift correction (§7.3).
        let engine_tick =
            sequencer.cursor() as f64 * 60.0 / f64::from(self.sample_rate);
        let drift_ticks = engine_tick - est;
        let beat_ticks = f64::from(sequencer.grid().beat_ticks());
        let block = out.len();
        if !paused && drift_ticks.abs() > 2.0 * beat_ticks {
            // Death respawn resets room_tick to 0 (simulation.rs) or a long
            // hitch: hard-snap in lockstep with the reset hazards.
            let target = est.max(0.0) as u64;
            sequencer.seek(sample_index_for_tick(target, self.sample_rate));
        }
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
