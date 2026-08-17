//! The sequencer: `Track` + live `HazardTiming` set + a musical sample clock
//! → mono f32 sample blocks. This exact code drives both the offline renderer
//! and the live `cpal` callback (§7.2, §8), which is the determinism oracle:
//! rendering the same session in different block sizes is byte-identical.
//!
//! Timeline: sample 0 of the sequencer clock is room tick 0. Pattern events
//! live on grid steps at ticks `φ + k·u`, indexed positionally into the loop,
//! so looping is arithmetic (seamless by construction) and coexists with
//! hazards of any period. Hazard event times are always recomputed from the
//! hazard clocks — the same arithmetic as `TimedHazard::is_active_at`
//! (`downwards-core/src/room.rs:124`) — never stored in the artifact.

use crate::{
    synth::{
        mixer::{DcBlocker, soft_clip},
        voice::{CosRamp, GateEnv, NoiseLfsr, PulseOsc, TriangleOsc},
    },
    tempo::HazardTiming,
    theory::Key,
    track::{HazardVoice, TempoGrid, Track, VoiceKind},
};

/// Which traversal abilities the player owns; gates the inventory layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityMask {
    pub gloves: bool,
    pub boots: bool,
}

/// The single tick↔sample conversion shared by live and offline paths.
#[must_use]
pub const fn sample_index_for_tick(tick: u64, sample_rate: u32) -> u64 {
    tick * sample_rate as u64 / 60
}

/// Inverse (floor) conversion.
#[must_use]
pub const fn tick_for_sample(sample: u64, sample_rate: u32) -> u64 {
    sample * 60 / sample_rate as u64
}

/// Pre-gain before the soft clipper; tuned so a full mix stays musical.
const PRE_GAIN: f32 = 0.55;
/// Melodic attack/release slew times in seconds.
const ATTACK_SECONDS: f64 = 0.0012;
const RELEASE_SECONDS: f64 = 0.004;
/// Layer enable/disable ramp (§7.4).
const LAYER_RAMP_SECONDS: f64 = 0.4;
/// Bed gate and hard-reset fade time (§6.6, §7.3).
const SHORT_RAMP_SECONDS: f64 = 0.005;

/// Noise timbre parameters: (clock Hz, short mode, decay time constant s).
const HAT_NOISE: (f64, bool, f64) = (12_000.0, false, 0.025);
const SNARE_NOISE: (f64, bool, f64) = (6_000.0, false, 0.06);
const CRASH_NOISE: (f64, bool, f64) = (3_000.0, false, 0.2);
const BED_NOISE: (f64, bool) = (1_400.0, true);
const SWEEP_LOW_HZ: f64 = 500.0;
const SWEEP_HIGH_HZ: f64 = 8_000.0;

/// Pooled hazard voice gains (§6.3).
const STAB_GAIN: f32 = 10.0 / 15.0;
const SNARE_GAIN: f32 = 10.0 / 15.0;
const CRASH_GAIN: f32 = 12.0 / 15.0;
const HIT_VEL: f32 = 12.0 / 15.0;
const WINDUP_VEL: f32 = 8.0 / 15.0;
const STAB_VEL: f32 = 10.0 / 15.0;
const SWEEP_VEL: f32 = 9.0 / 15.0;

#[derive(Clone, Debug)]
struct PatternNote {
    voice: usize,
    frequency_hz: f64,
    is_hat: bool,
    level: f32,
    len_ticks: u64,
}

#[derive(Clone, Debug)]
struct MelVoice {
    kind: VoiceKind,
    layer: usize,
    gain: f32,
    pulse: PulseOsc,
    triangle: TriangleOsc,
    noise: NoiseLfsr,
    env: GateEnv,
    frequency_hz: f64,
    level: f32,
    gated: bool,
    off_sample: u64,
    shot_amp: f32,
    shot_decay: f32,
}

impl MelVoice {
    fn new(kind: VoiceKind, layer: usize, gain: f32) -> Self {
        Self {
            kind,
            layer,
            gain,
            pulse: PulseOsc::default(),
            triangle: TriangleOsc::default(),
            noise: NoiseLfsr::default(),
            env: GateEnv::default(),
            frequency_hz: 440.0,
            level: 0.0,
            gated: false,
            off_sample: 0,
            shot_amp: 0.0,
            shot_decay: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
struct OneShot {
    noise: NoiseLfsr,
    clock_hz: f64,
    short_mode: bool,
    amp: f32,
    decay: f32,
}

impl OneShot {
    fn new() -> Self {
        Self {
            noise: NoiseLfsr::default(),
            clock_hz: 6_000.0,
            short_mode: false,
            amp: 0.0,
            decay: 0.0,
        }
    }

    fn trigger(&mut self, level: f32, params: (f64, bool, f64), sample_rate: f64) {
        self.clock_hz = params.0;
        self.short_mode = params.1;
        self.amp = level;
        // Exponential decay with time constant τ: per-sample factor e^(-1/(τ·rate)).
        self.decay = (-1.0 / (params.2 * sample_rate)).exp() as f32;
    }

    fn next(&mut self, sample_rate: f64) -> f32 {
        if self.amp < 1.0e-4 {
            self.amp = 0.0;
            return 0.0;
        }
        let value = self.noise.next(self.clock_hz, self.short_mode, sample_rate) * self.amp;
        self.amp *= self.decay;
        value
    }
}

#[derive(Clone, Debug)]
struct Sweep {
    noise: NoiseLfsr,
    start_sample: u64,
    end_sample: u64,
    active: bool,
}

#[derive(Clone, Debug)]
struct HazardSlot {
    timing: HazardTiming,
    voice: Option<HazardVoice>,
}

/// One playing room session.
#[derive(Clone, Debug)]
pub struct Sequencer {
    sample_rate: u32,
    grid: TempoGrid,
    key: Key,
    step_events: Vec<Vec<PatternNote>>,
    voices: Vec<MelVoice>,
    layer_names: Vec<String>,
    layers: Vec<CosRamp>,
    hazards: Vec<HazardSlot>,
    stab: MelVoice,
    snare: OneShot,
    crash: OneShot,
    sweeps: Vec<Sweep>,
    bed: NoiseLfsr,
    bed_ramp: CosRamp,
    fade_in: CosRamp,
    dc: DcBlocker,
    cursor: u64,
    next_tick: u64,
    next_tick_sample: u64,
}

impl Sequencer {
    #[must_use]
    pub fn new(track: &Track, hazards: &[HazardTiming], sample_rate: u32) -> Self {
        let grid = track.grid;
        let mut voices = Vec::with_capacity(track.voices.len());
        let layer_names = track.layers.clone();
        for definition in &track.voices {
            let layer = layer_names
                .iter()
                .position(|name| *name == definition.layer)
                .expect("parsed tracks only reference declared layers");
            voices.push(MelVoice::new(
                definition.kind,
                layer,
                f32::from(definition.gain) / 15.0,
            ));
        }

        let mut step_events: Vec<Vec<PatternNote>> =
            vec![Vec::new(); grid.loop_steps() as usize];
        for note in &track.notes {
            let voice_index = track
                .voices
                .iter()
                .position(|definition| definition.name == note.voice)
                .expect("parsed tracks only reference declared voices");
            let is_noise = matches!(track.voices[voice_index].kind, VoiceKind::Noise);
            step_events[note.start_step as usize].push(PatternNote {
                voice: voice_index,
                frequency_hz: note.pitch.map_or(0.0, |pitch| pitch.frequency_hz()),
                is_hat: note.pitch.is_none() || is_noise,
                level: f32::from(note.vel) / 15.0,
                len_ticks: u64::from(note.len_steps) * u64::from(grid.step_ticks),
            });
        }

        let hazard_slots: Vec<HazardSlot> = hazards
            .iter()
            .enumerate()
            .map(|(index, &timing)| HazardSlot {
                timing,
                voice: track
                    .hazard_voices
                    .iter()
                    .find(|voice| voice.index == index)
                    .cloned(),
            })
            .collect();
        let sweeps = hazard_slots
            .iter()
            .map(|_| Sweep {
                noise: NoiseLfsr::default(),
                start_sample: 0,
                end_sample: 0,
                active: false,
            })
            .collect();

        let layers = layer_names
            .iter()
            .map(|name| CosRamp::new(if name == "base" { 1.0 } else { 0.0 }))
            .collect();

        Self {
            sample_rate,
            grid,
            key: track.key,
            step_events,
            voices,
            layer_names,
            layers,
            hazards: hazard_slots,
            stab: MelVoice::new(
                VoiceKind::Pulse {
                    duty: crate::track::Duty::Quarter,
                },
                0,
                STAB_GAIN,
            ),
            snare: OneShot::new(),
            crash: OneShot::new(),
            sweeps,
            bed: NoiseLfsr::default(),
            bed_ramp: CosRamp::new(0.0),
            fade_in: CosRamp::new(1.0),
            dc: DcBlocker::default(),
            cursor: 0,
            next_tick: 0,
            next_tick_sample: 0,
        }
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn grid(&self) -> TempoGrid {
        self.grid
    }

    #[must_use]
    pub const fn cursor(&self) -> u64 {
        self.cursor
    }

    /// Slew the inventory layers toward the ownership targets (§7.4). Voices
    /// keep synthesizing regardless, so enabling a layer never causes a phase
    /// discontinuity.
    pub fn set_layer_targets(&mut self, abilities: AbilityMask) {
        let ramp_samples = LAYER_RAMP_SECONDS * f64::from(self.sample_rate);
        for (name, ramp) in self.layer_names.iter().zip(&mut self.layers) {
            let target = match name.as_str() {
                "gloves" => abilities.gloves,
                "boots" => abilities.boots,
                _ => true,
            };
            ramp.set_target(if target { 1.0 } else { 0.0 }, ramp_samples);
        }
    }

    /// Set layer gains with no ramp (session start / offline renders).
    pub fn set_layer_gains_immediate(&mut self, abilities: AbilityMask) {
        for (name, ramp) in self.layer_names.iter().zip(&mut self.layers) {
            let target = match name.as_str() {
                "gloves" => abilities.gloves,
                "boots" => abilities.boots,
                _ => true,
            };
            ramp.snap(if target { 1.0 } else { 0.0 });
        }
    }

    /// Hard-reposition the musical clock (death respawn, room entry drift
    /// snap). Kills all sounding voices and fades back in over 5 ms.
    pub fn seek(&mut self, sample: u64) {
        self.cursor = sample;
        self.next_tick = sample
            .saturating_mul(60)
            .div_ceil(u64::from(self.sample_rate));
        self.next_tick_sample = sample_index_for_tick(self.next_tick, self.sample_rate);
        for voice in &mut self.voices {
            voice.gated = false;
            voice.level = 0.0;
            voice.env.kill();
            voice.shot_amp = 0.0;
        }
        self.stab.gated = false;
        self.stab.level = 0.0;
        self.stab.env.kill();
        self.snare.amp = 0.0;
        self.crash.amp = 0.0;
        for sweep in &mut self.sweeps {
            sweep.active = false;
        }
        self.bed_ramp.snap(0.0);
        self.fade_in.snap(0.0);
        self.fade_in
            .set_target(1.0, SHORT_RAMP_SECONDS * f64::from(self.sample_rate));
    }

    /// Render the next `out.len()` samples of the session clock.
    pub fn render(&mut self, out: &mut [f32]) {
        for slot in out.iter_mut() {
            while self.cursor == self.next_tick_sample {
                let tick = self.next_tick;
                self.process_tick(tick);
                self.next_tick += 1;
                self.next_tick_sample = sample_index_for_tick(self.next_tick, self.sample_rate);
            }
            *slot = self.synthesize_sample();
            self.cursor += 1;
        }
    }

    /// Does any hazard fire hit land within `[tick, tick + window)`? Used to
    /// suppress pattern hats on fire steps (§6.3).
    fn fire_lands_within(&self, tick: u64, window: u64) -> bool {
        self.hazards.iter().any(|slot| {
            if slot.voice.is_none() {
                return false;
            }
            let period = u64::from(slot.timing.period);
            let fire = u64::from(slot.timing.fire_tick());
            let delta = (fire + period - tick % period) % period;
            delta < window
        })
    }

    fn process_tick(&mut self, tick: u64) {
        let step_ticks = u64::from(self.grid.step_ticks);
        let phi = u64::from(self.grid.grid_offset);

        // Pattern step boundary.
        if tick >= phi && (tick - phi).is_multiple_of(step_ticks) {
            let step =
                ((tick - phi) / step_ticks) % u64::from(self.grid.loop_steps());
            let suppress_hats = self.fire_lands_within(tick, step_ticks);
            // Take/replace the step's event list so triggering can borrow the
            // voices mutably without cloning (no steady-state allocation).
            let events = std::mem::take(&mut self.step_events[step as usize]);
            for note in &events {
                if note.is_hat && suppress_hats {
                    continue;
                }
                self.trigger_pattern_note(note, tick);
            }
            self.step_events[step as usize] = events;
        }

        // Hazard clocks (§6.6). Collect triggers first to keep borrows simple.
        let mut fire_snare = false;
        let mut fire_crash = false;
        let mut fire_stab: Option<crate::theory::Pitch> = None;
        let mut windup: Option<crate::theory::Pitch> = None;
        let mut sweep_start_mask = 0_u64;
        let mut bed_level = 0_u8;

        for (index, slot) in self.hazards.iter().enumerate() {
            let Some(voice) = &slot.voice else { continue };
            let period = u64::from(slot.timing.period);
            let fire_r = u64::from(slot.timing.fire_tick());
            let tick_r = tick % period;

            if voice.active_bed_level > 0 && slot.timing.is_active_at(tick) {
                bed_level = bed_level.max(voice.active_bed_level);
            }

            if voice.locked {
                if tick_r == fire_r {
                    fire_snare = true;
                    if fire_stab.is_none() {
                        fire_stab = voice.fire_stab;
                    }
                }
                let windup_count = u64::from(voice.windup_steps);
                for back in 1..=windup_count {
                    let distance = back * step_ticks;
                    if distance >= period {
                        continue;
                    }
                    if tick_r == (fire_r + period - distance % period) % period {
                        let degree_index = (windup_count - back) as usize;
                        let degree = voice.windup_degrees[degree_index];
                        windup = Some(self.key.degree_in_octave(degree, 3));
                    }
                }
            } else {
                if tick_r == fire_r {
                    fire_crash = true;
                }
                if period > u64::from(crate::tempo::WARNING_TICKS) && index < 64 {
                    let warn = u64::from(crate::tempo::WARNING_TICKS);
                    if tick_r == (fire_r + period - warn) % period {
                        sweep_start_mask |= 1 << index;
                    }
                }
            }
        }

        let rate = f64::from(self.sample_rate);
        if fire_snare {
            self.snare.trigger(HIT_VEL * SNARE_GAIN, SNARE_NOISE, rate);
        }
        if fire_crash {
            self.crash.trigger(HIT_VEL * CRASH_GAIN, CRASH_NOISE, rate);
        }
        // Fire stab wins over a same-tick wind-up note on the pooled voice.
        if let Some(pitch) = fire_stab {
            self.trigger_stab(pitch, STAB_VEL, tick);
        } else if let Some(pitch) = windup {
            self.trigger_stab(pitch, WINDUP_VEL, tick);
        }
        for (index, sweep) in self.sweeps.iter_mut().enumerate() {
            if index < 64 && sweep_start_mask & (1 << index) != 0 {
                sweep.active = true;
                sweep.start_sample = sample_index_for_tick(tick, self.sample_rate);
                sweep.end_sample = sample_index_for_tick(
                    tick + u64::from(crate::tempo::WARNING_TICKS),
                    self.sample_rate,
                );
            }
        }

        let bed_target = f32::from(bed_level) / 15.0;
        self.bed_ramp
            .set_target(bed_target, SHORT_RAMP_SECONDS * rate);
    }

    fn trigger_pattern_note(&mut self, note: &PatternNote, tick: u64) {
        let voice = &mut self.voices[note.voice];
        if note.is_hat {
            // Noise voices are one-shots with a fixed hat timbre.
            voice.shot_amp = note.level * voice.gain;
            voice.shot_decay =
                (-1.0 / (HAT_NOISE.2 * f64::from(self.sample_rate))).exp() as f32;
        } else {
            voice.frequency_hz = note.frequency_hz;
            voice.level = note.level;
            voice.gated = true;
            voice.off_sample =
                sample_index_for_tick(tick + note.len_ticks, self.sample_rate);
        }
    }

    fn trigger_stab(&mut self, pitch: crate::theory::Pitch, velocity: f32, tick: u64) {
        self.stab.frequency_hz = pitch.frequency_hz();
        self.stab.level = velocity;
        self.stab.gated = true;
        self.stab.off_sample = sample_index_for_tick(
            tick + u64::from(self.grid.step_ticks),
            self.sample_rate,
        );
    }

    fn synthesize_sample(&mut self) -> f32 {
        let rate = f64::from(self.sample_rate);
        let attack = (1.0 / (ATTACK_SECONDS * rate)) as f32;
        let release = (1.0 / (RELEASE_SECONDS * rate)) as f32;

        let mut layer_gains = [0.0_f32; 8];
        for (index, ramp) in self.layers.iter_mut().enumerate() {
            layer_gains[index] = ramp.advance();
        }

        let cursor = self.cursor;
        let mut mix = 0.0_f32;
        for voice in &mut self.voices {
            if voice.gated && cursor >= voice.off_sample {
                voice.gated = false;
            }
            let layer_gain = layer_gains[voice.layer];
            match voice.kind {
                VoiceKind::Noise => {
                    if voice.shot_amp > 1.0e-4 {
                        let value =
                            voice
                                .noise
                                .next(HAT_NOISE.0, HAT_NOISE.1, rate);
                        mix += value * voice.shot_amp * layer_gain;
                        voice.shot_amp *= voice.shot_decay;
                    }
                }
                VoiceKind::Pulse { duty } => {
                    let target = if voice.gated { voice.level } else { 0.0 };
                    let amp = voice.env.next(target, attack, release);
                    if amp > 0.0 {
                        let value =
                            voice
                                .pulse
                                .next(voice.frequency_hz, duty.fraction(), rate);
                        mix += value * amp * voice.gain * layer_gain;
                    } else {
                        voice.pulse.next(voice.frequency_hz, duty.fraction(), rate);
                    }
                }
                VoiceKind::Triangle => {
                    let target = if voice.gated { voice.level } else { 0.0 };
                    let amp = voice.env.next(target, attack, release);
                    if amp > 0.0 {
                        let value = voice.triangle.next(voice.frequency_hz, rate);
                        mix += value * amp * voice.gain * layer_gain;
                    } else {
                        voice.triangle.next(voice.frequency_hz, rate);
                    }
                }
            }
        }

        // Pooled hazard stab (base layer).
        {
            let stab = &mut self.stab;
            if stab.gated && cursor >= stab.off_sample {
                stab.gated = false;
            }
            let target = if stab.gated { stab.level } else { 0.0 };
            let amp = stab.env.next(target, attack, release);
            let value = stab.pulse.next(stab.frequency_hz, 0.25, rate);
            mix += value * amp * stab.gain;
        }

        mix += self.snare.next(rate);
        mix += self.crash.next(rate);

        for sweep in &mut self.sweeps {
            if !sweep.active {
                continue;
            }
            if cursor >= sweep.end_sample {
                sweep.active = false;
                continue;
            }
            let span = (sweep.end_sample - sweep.start_sample).max(1) as f64;
            let progress =
                (cursor.saturating_sub(sweep.start_sample)) as f64 / span;
            let clock = SWEEP_LOW_HZ + (SWEEP_HIGH_HZ - SWEEP_LOW_HZ) * progress;
            let value = sweep.noise.next(clock, false, rate);
            mix += value * (progress as f32) * SWEEP_VEL * SNARE_GAIN;
        }

        let bed_gain = self.bed_ramp.advance();
        if bed_gain > 1.0e-5 {
            mix += self.bed.next(BED_NOISE.0, BED_NOISE.1, rate) * bed_gain;
        } else {
            self.bed.next(BED_NOISE.0, BED_NOISE.1, rate);
        }

        let faded = mix * self.fade_in.advance();
        self.dc.next(soft_clip(faded * PRE_GAIN))
    }
}
