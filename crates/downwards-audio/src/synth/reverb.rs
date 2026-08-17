//! A small deterministic Schroeder reverb for the per-voice send bus
//! (format v2, research survey §5/§6.4: mix depth as the atmosphere lever).
//!
//! Four damped feedback combs in parallel into two series allpasses. Pure
//! per-sample f32 state machinery — bit-identical regardless of render block
//! sizes, exactly like the oscillators.

#[derive(Clone, Debug)]
struct Comb {
    buffer: Vec<f32>,
    cursor: usize,
    damp_state: f32,
}

impl Comb {
    const FEEDBACK: f32 = 0.77;
    const DAMP: f32 = 0.30;

    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            cursor: 0,
            damp_state: 0.0,
        }
    }

    fn next(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.cursor];
        self.damp_state = output * (1.0 - Self::DAMP) + self.damp_state * Self::DAMP;
        self.buffer[self.cursor] = input + self.damp_state * Self::FEEDBACK;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.damp_state = 0.0;
    }
}

#[derive(Clone, Debug)]
struct Allpass {
    buffer: Vec<f32>,
    cursor: usize,
}

impl Allpass {
    const GAIN: f32 = 0.5;

    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            cursor: 0,
        }
    }

    fn next(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.cursor];
        let output = delayed - input * Self::GAIN;
        self.buffer[self.cursor] = input + delayed * Self::GAIN;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// The send-bus reverb. Feed it the summed per-voice send signal each
/// sample; add its return (already wet-scaled) to the mix.
#[derive(Clone, Debug)]
pub struct Reverb {
    combs: [Comb; 4],
    allpasses: [Allpass; 2],
}

impl Reverb {
    /// Wet return level relative to the send signal.
    const WET: f32 = 0.55;

    /// Freeverb-family delay lengths, defined at 44.1 kHz and scaled to the
    /// actual rate so decay character is rate-independent.
    const COMB_LENS: [usize; 4] = [1422, 1491, 1557, 1617];
    const ALLPASS_LENS: [usize; 2] = [225, 556];

    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let scale = |len: usize| (len * sample_rate as usize) / 44_100;
        Self {
            combs: Self::COMB_LENS.map(|len| Comb::new(scale(len))),
            allpasses: Self::ALLPASS_LENS.map(|len| Allpass::new(scale(len))),
        }
    }

    pub fn next(&mut self, input: f32) -> f32 {
        let mut sum = 0.0;
        for comb in &mut self.combs {
            sum += comb.next(input);
        }
        let mut out = sum * 0.25;
        for allpass in &mut self.allpasses {
            out = allpass.next(out);
        }
        out * Self::WET
    }

    pub fn reset(&mut self) {
        for comb in &mut self.combs {
            comb.reset();
        }
        for allpass in &mut self.allpasses {
            allpass.reset();
        }
    }
}
