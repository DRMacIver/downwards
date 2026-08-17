//! Oscillators and envelopes. All state advances one sample at a time so the
//! output is bit-identical regardless of render block sizes.

/// Naive NES-style pulse oscillator, centred so duty changes carry no DC.
#[derive(Clone, Debug, Default)]
pub struct PulseOsc {
    phase: f64,
}

impl PulseOsc {
    pub fn next(&mut self, frequency_hz: f64, duty: f64, sample_rate: f64) -> f32 {
        let value = if self.phase < duty { 1.0 - duty } else { -duty };
        self.phase += frequency_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        (value * 2.0) as f32
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// NES triangle: a 32-position sequence of 16 amplitude levels.
#[derive(Clone, Debug, Default)]
pub struct TriangleOsc {
    phase: f64,
}

impl TriangleOsc {
    pub fn next(&mut self, frequency_hz: f64, sample_rate: f64) -> f32 {
        let position = (self.phase * 32.0) as u32 % 32;
        let level = if position < 16 {
            15 - position
        } else {
            position - 16
        };
        self.phase += frequency_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        (f64::from(level) / 7.5 - 1.0) as f32
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// 15-bit LFSR noise, long mode (feedback bit 1) or short mode (bit 6).
#[derive(Clone, Debug)]
pub struct NoiseLfsr {
    register: u16,
    accumulator: f64,
}

impl Default for NoiseLfsr {
    fn default() -> Self {
        Self {
            register: 1,
            accumulator: 0.0,
        }
    }
}

impl NoiseLfsr {
    pub fn next(&mut self, clock_hz: f64, short_mode: bool, sample_rate: f64) -> f32 {
        self.accumulator += clock_hz / sample_rate;
        while self.accumulator >= 1.0 {
            self.accumulator -= 1.0;
            let tap = if short_mode { 6 } else { 1 };
            let feedback = (self.register ^ (self.register >> tap)) & 1;
            self.register = (self.register >> 1) | (feedback << 14);
        }
        if self.register & 1 == 1 { 1.0 } else { -1.0 }
    }
}

/// The pad voice (format v2): two saw oscillators detuned ±0.4 % averaged
/// through a one-pole low-pass — a soft, slowly beating sustain layer under
/// the chip voices. Sample-at-a-time and bit-identical like everything else.
#[derive(Clone, Debug, Default)]
pub struct PadOsc {
    phase_a: f64,
    phase_b: f64,
    low_pass: f32,
}

impl PadOsc {
    /// Low-pass cutoff: dark enough to sit under the pulses, bright enough
    /// to read as harmony rather than rumble.
    const CUTOFF_HZ: f64 = 950.0;
    const DETUNE: f64 = 0.004;

    pub fn next(&mut self, frequency_hz: f64, sample_rate: f64) -> f32 {
        let saw_a = (self.phase_a * 2.0 - 1.0) as f32;
        let saw_b = (self.phase_b * 2.0 - 1.0) as f32;
        self.phase_a += frequency_hz * (1.0 + Self::DETUNE) / sample_rate;
        self.phase_b += frequency_hz * (1.0 - Self::DETUNE) / sample_rate;
        if self.phase_a >= 1.0 {
            self.phase_a -= self.phase_a.floor();
        }
        if self.phase_b >= 1.0 {
            self.phase_b -= self.phase_b.floor();
        }
        let input = (saw_a + saw_b) * 0.5;
        let alpha = (1.0
            - (-2.0 * std::f64::consts::PI * Self::CUTOFF_HZ / sample_rate).exp())
            as f32;
        self.low_pass += alpha * (input - self.low_pass);
        self.low_pass
    }
}

/// A raised-cosine ramp between gain targets; retargeting mid-ramp restarts
/// the cosine from the current value, so it can never pop.
#[derive(Clone, Debug)]
pub struct CosRamp {
    start: f32,
    target: f32,
    progress: f64,
    step: f64,
}

impl CosRamp {
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self {
            start: value,
            target: value,
            progress: 1.0,
            step: 1.0,
        }
    }

    pub fn set_target(&mut self, target: f32, ramp_samples: f64) {
        if (target - self.target).abs() < f32::EPSILON {
            return;
        }
        self.start = self.value();
        self.target = target;
        self.progress = 0.0;
        self.step = 1.0 / ramp_samples.max(1.0);
    }

    /// Immediately jump to a value (used only on hard resets, where the
    /// caller separately fades the master output).
    pub fn snap(&mut self, value: f32) {
        self.start = value;
        self.target = value;
        self.progress = 1.0;
    }

    #[must_use]
    pub fn value(&self) -> f32 {
        let shaped = 0.5 - 0.5 * (std::f64::consts::PI * self.progress.min(1.0)).cos();
        self.start + (self.target - self.start) * shaped as f32
    }

    #[must_use]
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Advance one sample and return the current value.
    pub fn advance(&mut self) -> f32 {
        let value = self.value();
        if self.progress < 1.0 {
            self.progress += self.step;
        }
        value
    }

    #[must_use]
    pub fn done(&self) -> bool {
        self.progress >= 1.0
    }
}

/// Linear-slew amplitude used by melodic voices: attack toward the note level
/// while gated, release toward zero after the gate closes.
#[derive(Clone, Debug, Default)]
pub struct GateEnv {
    amplitude: f32,
}

impl GateEnv {
    pub fn next(&mut self, target: f32, attack_per_sample: f32, release_per_sample: f32) -> f32 {
        let rate = if target > self.amplitude {
            attack_per_sample
        } else {
            release_per_sample
        };
        if (target - self.amplitude).abs() <= rate {
            self.amplitude = target;
        } else if target > self.amplitude {
            self.amplitude += rate;
        } else {
            self.amplitude -= rate;
        }
        self.amplitude
    }

    pub fn kill(&mut self) {
        self.amplitude = 0.0;
    }
}
