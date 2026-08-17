//! Offline rendering: drives the exact same [`Sequencer`] as the live path
//! (§8), which is the tested determinism oracle.

use crate::{
    sequencer::{AbilityMask, Sequencer, sample_index_for_tick},
    tempo::HazardTiming,
    track::Track,
};

/// Render `loops` passes of the track's loop (φ intro included) to mono f32.
#[must_use]
pub fn render(
    track: &Track,
    hazards: &[HazardTiming],
    abilities: AbilityMask,
    loops: u32,
    sample_rate: u32,
) -> Vec<f32> {
    let mut sequencer = Sequencer::new(track, hazards, sample_rate);
    sequencer.set_layer_gains_immediate(abilities);
    let total_ticks =
        u64::from(track.grid.grid_offset) + u64::from(loops) * u64::from(track.grid.loop_ticks());
    let total_samples = sample_index_for_tick(total_ticks, sample_rate);
    let mut samples = vec![0.0_f32; usize::try_from(total_samples).expect("render fits memory")];
    sequencer.render(&mut samples);
    samples
}
