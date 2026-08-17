//! Output conditioning: gentle soft clip plus a DC-blocking one-pole
//! high-pass, applied identically by the live and offline paths.

/// Soft clip with unity gain through the origin; output magnitude < 1.
#[must_use]
pub fn soft_clip(sample: f32) -> f32 {
    (sample as f64).tanh() as f32 * 0.95
}

/// One-pole DC blocker: `y[n] = x[n] - x[n-1] + R * y[n-1]`.
#[derive(Clone, Debug, Default)]
pub struct DcBlocker {
    previous_input: f32,
    previous_output: f32,
}

impl DcBlocker {
    pub fn next(&mut self, input: f32) -> f32 {
        // R = 0.995 puts the cutoff near 35 Hz at 44.1 kHz: well below the
        // lowest bass note, and the filter's memory dies in ~5 ms so gating
        // edges stay sample-tight.
        let output = input - self.previous_input + 0.995 * self.previous_output;
        self.previous_input = input;
        self.previous_output = output;
        output
    }

    pub fn reset(&mut self) {
        self.previous_input = 0.0;
        self.previous_output = 0.0;
    }
}
