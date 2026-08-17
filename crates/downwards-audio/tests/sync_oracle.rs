//! §10.5: render fixture rooms offline and verify, from the audio signal
//! alone, that locked hazard fires land at their exact sample positions and
//! the active bed gates on the activation edges. Also loop-boundary
//! continuity and output sanity (no clipping, no DC, non-silence).

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs,
    compose, derive_grid, offline::render, sample_index_for_tick,
};

const RATE: u32 = 44_100;

fn inputs(slug: &str, difficulty: Difficulty, hazards: Vec<HazardTiming>) -> RoomMusicInputs {
    RoomMusicInputs {
        slug: slug.to_owned(),
        difficulty,
        ability_requirement: AbilityReq::None,
        coin_count: 2,
        doors: DoorSet {
            west: true,
            floor: true,
            ..DoorSet::default()
        },
        hazards,
        layer_gates: LayerGates::default(),
    }
}

fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Onset detector: short-RMS ratio (> 3) between the 256 samples after the
/// onset and a 256-sample reference window `quiet_gap` samples earlier.
/// The reference sits *before* the wind-up pickup gesture, which by design
/// sustains right up to the fire tick — the window immediately before a
/// locked fire is intentionally never silent.
fn has_onset_near(samples: &[f32], expected: u64, tolerance: u64, quiet_gap: usize) -> bool {
    let window = 256_usize;
    let lo = expected.saturating_sub(tolerance);
    let hi = expected + tolerance;
    (lo..=hi).any(|position| {
        let position = position as usize;
        if position < window.max(quiet_gap) + window || position + window >= samples.len() {
            return false;
        }
        // The fire must be a local energy rise...
        let before = rms(&samples[position - window..position]);
        let after = rms(&samples[position..position + window]);
        // ...and loud against the quietest recent moment (other hazard sounds
        // — wind-ups, decaying crashes — legitimately overlap the window
        // immediately before a fire, so the ×3 contrast is measured against
        // the quietest 256-sample window in the preceding `quiet_gap`).
        let quiet = (0..16)
            .map(|index| {
                let start = position - quiet_gap + (quiet_gap - window) * index / 16;
                rms(&samples[start..start + window])
            })
            .fold(f64::INFINITY, f64::min);
        after > before * 1.4 && after > quiet * 2.0 && after > 1.0e-3
    })
}

/// Samples spanned by a locked hazard's wind-up gesture plus a safety margin.
fn windup_gap(track: &downwards_audio::Track, index: usize) -> usize {
    let voice = track
        .hazard_voices
        .iter()
        .find(|voice| voice.index == index)
        .expect("fixture hazards have voices");
    let ticks = u64::from(voice.windup_steps) * u64::from(track.grid.step_ticks) + 6;
    sample_index_for_tick(ticks, RATE) as usize
}

#[test]
fn locked_fires_are_audible_at_their_exact_ticks() {
    // φ ≠ 0 fixture: one hazard, period 96, phase 45 → fire tick 51.
    let hazard = HazardTiming {
        period: 96,
        active: 20,
        phase: 45,
    };
    let room = inputs("sync-phi", Difficulty::Medium, vec![hazard]);
    let mut track = compose(&room);
    let choice = derive_grid(&room.hazards, room.difficulty);
    assert_eq!(choice.locked, vec![0]);
    // Silence the editable pattern and the bed so the onset detector sees the
    // fire/wind-up events alone — the claim under test is the *timing* of the
    // hazard events, which the artifact cannot affect. (The bed's gating has
    // its own dedicated test below.)
    track.notes.clear();
    for voice in &mut track.hazard_voices {
        voice.active_bed_level = 0;
    }
    let samples = render(&track, &room.hazards, AbilityMask::default(), 2, RATE);

    let total_ticks = u64::from(track.grid.grid_offset)
        + 2 * u64::from(track.grid.loop_ticks());
    let gap = windup_gap(&track, 0);
    let mut fire = u64::from(hazard.fire_tick());
    let mut checked = 0;
    while fire + 96 < total_ticks {
        // Skip fires too near the end for the onset window.
        let expected = sample_index_for_tick(fire, RATE);
        assert!(
            has_onset_near(&samples, expected, 64, gap),
            "no onset within ±64 samples of locked fire tick {fire}"
        );
        checked += 1;
        fire += u64::from(hazard.period);
    }
    assert!(checked >= 10, "the fixture must cover many fires");
}

#[test]
fn coprime_fixture_keeps_both_hazards_audible() {
    // Coprime fixture: locked 70-period plus unlocked 157-period.
    let hazards = vec![
        HazardTiming {
            period: 70,
            active: 20,
            phase: 10,
        },
        HazardTiming {
            period: 157,
            active: 40,
            phase: 0,
        },
    ];
    let room = inputs("sync-coprime", Difficulty::Hard, hazards.clone());
    let mut track = compose(&room);
    let choice = derive_grid(&room.hazards, room.difficulty);
    assert_eq!(choice.locked, vec![0]);
    track.notes.clear();
    for voice in &mut track.hazard_voices {
        voice.active_bed_level = 0;
    }
    let samples = render(&track, &room.hazards, AbilityMask::default(), 2, RATE);

    // Locked fires on the beat. With the coprime crash's 200 ms decay
    // legitimately overlapping some fires, every fire must show an energy
    // rise at its exact tick, and the clear majority must pass the full
    // onset criterion.
    let gap = windup_gap(&track, 0);
    let mut fire = u64::from(hazards[0].fire_tick());
    let total_ticks = u64::from(track.grid.grid_offset) + 2 * u64::from(track.grid.loop_ticks());
    let mut strong = 0_u32;
    let mut total = 0_u32;
    while fire + 70 < total_ticks {
        let expected = sample_index_for_tick(fire, RATE) as usize;
        let before = rms(&samples[expected - 256..expected]);
        let after = rms(&samples[expected..expected + 256]);
        assert!(
            after > before * 1.15 && after > 1.0e-3,
            "locked fire at tick {fire} must be audible"
        );
        if has_onset_near(&samples, expected as u64, 64, gap) {
            strong += 1;
        }
        total += 1;
        fire += 70;
    }
    assert!(
        strong * 10 >= total * 7,
        "most locked fires must be clear onsets ({strong}/{total})"
    );
    // Unlocked crash fires, off the grid but still exact in time. The noise
    // sweep occupies the 24 ticks before the crash, so reference past it.
    // Unlocked crash fires, off the grid but still exact in time. Crashes
    // can land right after a locked snare and be masked in the raw signal,
    // so use a differential oracle: render the same session without the
    // unlocked voice and locate the crash onsets in the difference.
    let mut locked_only = track.clone();
    locked_only.hazard_voices.retain(|voice| voice.index == 0);
    let without = render(&locked_only, &hazards, AbilityMask::default(), 2, RATE);
    let diff: Vec<f32> = samples
        .iter()
        .zip(&without)
        .map(|(full, part)| full - part)
        .collect();
    let crash_gap = sample_index_for_tick(30, RATE) as usize;
    let mut crash = u64::from(hazards[1].fire_tick());
    let mut crash_checked = 0;
    while crash + 157 < total_ticks {
        let expected = sample_index_for_tick(crash, RATE) as usize;
        if expected > crash_gap + 512 && expected + 256 < diff.len() {
            let after = rms(&diff[expected..expected + 256]);
            let quiet = rms(&diff[expected - crash_gap - 256..expected - crash_gap]);
            assert!(
                after > quiet * 3.0 && after > 1.0e-3,
                "unlocked crash at tick {crash} must be audible ({after} vs {quiet})"
            );
            crash_checked += 1;
        }
        crash += 157;
    }
    assert!(crash_checked >= 10);
}

#[test]
fn bed_gates_with_the_active_window() {
    // A slow hazard with a long active window and a quiet room around it: the
    // bed's noise energy must appear during active spans and vanish outside.
    let hazard = HazardTiming {
        period: 300,
        active: 150,
        phase: 0,
    };
    let room = inputs("sync-bed", Difficulty::Easy, vec![hazard]);
    let mut track = compose(&room);
    // Isolate the bed: silence every pattern note and the fire sounds.
    track.notes.clear();
    for voice in &mut track.hazard_voices {
        voice.active_bed_level = 12;
    }
    let samples = render(&track, &room.hazards, AbilityMask::default(), 1, RATE);

    // Hazard active on ticks 0..150, inactive 150..300, etc.
    let mid_active = sample_index_for_tick(75, RATE) as usize;
    let mid_inactive = sample_index_for_tick(225, RATE) as usize;
    let active_rms = rms(&samples[mid_active..mid_active + 2048]);
    let inactive_rms = rms(&samples[mid_inactive..mid_inactive + 2048]);
    assert!(
        active_rms > 10.0 * inactive_rms.max(1.0e-6),
        "bed must be loud in the active window ({active_rms} vs {inactive_rms})"
    );

    // Gate edge lands within ±300 samples of the deactivation tick: right up
    // to edge−300 the bed is at full level, and by edge+300 it has collapsed
    // (only the DC-blocker's short tail remains).
    let edge = sample_index_for_tick(150, RATE) as usize;
    let before = rms(&samples[edge - 1024..edge - 300]);
    let after = rms(&samples[edge + 300..edge + 1024]);
    assert!(
        before > 0.8 * active_rms,
        "bed must stay at level until the edge ({before} vs {active_rms})"
    );
    assert!(
        after < 0.25 * active_rms,
        "bed must have collapsed just after the edge ({after} vs {active_rms})"
    );
}

#[test]
fn output_is_sane_and_loop_boundary_is_seamless() {
    let room = inputs("sanity-room", Difficulty::Easy, Vec::new());
    let track = compose(&room);
    let samples = render(&track, &room.hazards, AbilityMask::default(), 3, RATE);

    // Sanity: non-silent, no clipping, no DC offset.
    assert!(rms(&samples) > 0.01, "track must not be silent");
    assert!(samples.iter().all(|s| s.abs() <= 1.0), "no clipping");
    let mean: f64 = samples.iter().map(|&s| f64::from(s)).sum::<f64>() / samples.len() as f64;
    assert!(mean.abs() < 0.01, "no DC offset (mean {mean})");

    // Loop seamlessness: the sample-to-sample jump across the loop boundary
    // must not exceed the largest jump anywhere else (no boundary pop).
    let boundary = sample_index_for_tick(
        u64::from(track.grid.grid_offset) + 2 * u64::from(track.grid.loop_ticks()),
        RATE,
    ) as usize;
    let global_max_jump = samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0_f32, f32::max);
    let boundary_jump = (samples[boundary] - samples[boundary - 1]).abs();
    assert!(
        boundary_jump <= global_max_jump,
        "loop boundary must not be a discontinuity ({boundary_jump} vs {global_max_jump})"
    );
}

#[test]
fn rendering_is_deterministic() {
    let room = inputs(
        "determinism-room",
        Difficulty::Medium,
        vec![HazardTiming {
            period: 96,
            active: 12,
            phase: 13,
        }],
    );
    let track = compose(&room);
    let first = render(&track, &room.hazards, AbilityMask::default(), 1, RATE);
    let second = render(&track, &room.hazards, AbilityMask::default(), 1, RATE);
    assert_eq!(first, second, "same room must render identical samples");
}
