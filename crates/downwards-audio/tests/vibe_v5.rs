//! v5 vibe-revision tests: the whole corpus inherits keep-wall-gate's
//! restrained, atmospheric character. Difficulty escalates through tension
//! and low-register drive, never through brightness, chirpiness, or busier
//! high-register activity. keep-wall-gate itself (the approved reference) is
//! pinned bit-for-bit against its v4 serialization, and keep-crown-sanctum —
//! the one room allowed extra lift — gains a fuller, louder final section.

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, LayerGates, Mode, NoteEvent,
    RoomMusicInputs, Track, compose, offline::render,
};

/// Half rate is plenty for a spectral-tilt proxy and keeps the test fast.
const RATE: u32 = 22_050;

fn inputs(slug: &str, difficulty: Difficulty) -> RoomMusicInputs {
    RoomMusicInputs {
        slug: slug.to_owned(),
        difficulty,
        ability_requirement: AbilityReq::None,
        coin_count: 3,
        doors: DoorSet {
            west: true,
            east: true,
            ..DoorSet::default()
        },
        hazards: Vec::new(),
        layer_gates: LayerGates::default(),
    }
}

/// The exact composer inputs of keep-wall-gate (from the checked-in design
/// data: medium band, no coins, no hazards, both inventory layers gated).
fn keep_wall_gate_inputs() -> RoomMusicInputs {
    RoomMusicInputs {
        slug: "keep-wall-gate".to_owned(),
        difficulty: Difficulty::Medium,
        ability_requirement: AbilityReq::None,
        coin_count: 0,
        doors: DoorSet {
            west: true,
            east: true,
            ..DoorSet::default()
        },
        hazards: Vec::new(),
        layer_gates: LayerGates {
            gloves: true,
            boots: true,
        },
    }
}

/// The exact composer inputs of keep-crown-sanctum (final screen).
fn crown_sanctum_inputs() -> RoomMusicInputs {
    RoomMusicInputs {
        slug: "keep-crown-sanctum".to_owned(),
        difficulty: Difficulty::Medium,
        ability_requirement: AbilityReq::None,
        coin_count: 0,
        doors: DoorSet {
            west: true,
            ..DoorSet::default()
        },
        hazards: Vec::new(),
        layer_gates: LayerGates::default(),
    }
}

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt()
}

/// Spectral-tilt proxy: energy of the first-difference signal relative to the
/// signal itself. High-frequency-heavy mixes score high; dark mixes score low.
fn tilt(samples: &[f32]) -> f64 {
    let diff: Vec<f32> = samples.windows(2).map(|pair| pair[1] - pair[0]).collect();
    rms(&diff) / rms(samples).max(1e-9)
}

fn render_one(room: &RoomMusicInputs) -> Vec<f32> {
    let track = compose(room);
    render(&track, &room.hazards, AbilityMask::default(), 1, RATE)
}

fn voice_notes<'a>(track: &'a Track, voice: &str) -> Vec<&'a NoteEvent> {
    let mut notes: Vec<&NoteEvent> = track
        .notes
        .iter()
        .filter(|note| note.voice == voice)
        .collect();
    notes.sort_by_key(|note| (note.start_step, note.pitch.map(|pitch| pitch.midi())));
    notes
}

/// keep-wall-gate is the approved reference: composing it from its exact
/// design inputs must reproduce the v4 artifact byte-for-byte. Any refinement
/// that touches this track needs a fresh designer sign-off first.
#[test]
fn keep_wall_gate_is_bit_stable_against_v4() {
    let track = compose(&keep_wall_gate_inputs());
    let golden = include_str!("golden/keep-wall-gate-v4.track");
    assert_eq!(
        track.serialize(),
        golden,
        "keep-wall-gate drifted from its approved v4 serialization"
    );
}

/// The easy band joins the reference character: dorian (no more major-leaning
/// mixolydian brightness) and soft-shaker percussion instead of crisp ticks.
#[test]
fn easy_band_is_dorian_with_soft_percussion() {
    let track = compose(&inputs("v5-easy-mood", Difficulty::Easy));
    assert_eq!(track.key.mode, Mode::Dorian, "easy band must darken to dorian");
    assert!(
        track.voices.iter().any(|voice| voice.name == "shaker"),
        "easy tracks must carry the soft shaker voice"
    );
    // Pattern percussion is all shaker except the sparse turnaround fill.
    let ticks = voice_notes(&track, "perc").len();
    let shakes = voice_notes(&track, "shaker").len();
    assert!(
        shakes > ticks,
        "easy percussion must be mostly soft shaker ({shakes} shakes vs {ticks} ticks)"
    );
}

/// Difficulty must not escalate through brightness: across seeded no-hazard
/// rooms, the medium and hard bands' spectral tilt stays at or below the easy
/// band's, and every band stays close to the keep-wall-gate reference.
#[test]
fn harder_bands_are_not_brighter() {
    let seeds = 4;
    let mean_tilt = |difficulty: Difficulty| -> f64 {
        (0..seeds)
            .map(|case| tilt(&render_one(&inputs(&format!("tilt-{case}"), difficulty))))
            .sum::<f64>()
            / f64::from(seeds)
    };
    let easy = mean_tilt(Difficulty::Easy);
    let medium = mean_tilt(Difficulty::Medium);
    let hard = mean_tilt(Difficulty::Hard);
    let reference = tilt(&render_one(&keep_wall_gate_inputs()));
    assert!(
        medium <= easy * 1.05,
        "medium band brighter than easy: {medium:.4} vs {easy:.4}"
    );
    assert!(
        hard <= easy * 1.05,
        "hard band brighter than easy: {hard:.4} vs {easy:.4}"
    );
    for (name, value) in [("easy", easy), ("medium", medium), ("hard", hard)] {
        assert!(
            value <= reference * 1.30,
            "{name} band tilt {value:.4} far above the keep-wall-gate reference {reference:.4}"
        );
    }
}

/// Escalation comes from the low registers: the hard band's lead thins out
/// rather than getting busier, while its bass gets more insistent.
#[test]
fn hard_band_trades_lead_density_for_bass_drive() {
    let seeds = 6;
    let density = |difficulty: Difficulty, voice: &str| -> f64 {
        (0..seeds)
            .map(|case| {
                let track = compose(&inputs(&format!("density-{case}"), difficulty));
                let seconds =
                    f64::from(track.grid.loop_steps() * track.grid.step_ticks) / 60.0;
                voice_notes(&track, voice).len() as f64 / seconds
            })
            .sum::<f64>()
            / f64::from(seeds)
    };
    let easy_lead = density(Difficulty::Easy, "lead");
    let hard_lead = density(Difficulty::Hard, "lead");
    assert!(
        hard_lead < easy_lead,
        "hard lead ({hard_lead:.2}/s) must be sparser than easy ({easy_lead:.2}/s)"
    );
    let easy_bass = density(Difficulty::Easy, "bass");
    let hard_bass = density(Difficulty::Hard, "bass");
    assert!(
        hard_bass > 1.5 * easy_bass,
        "hard bass ({hard_bass:.2}/s) must drive well past easy ({easy_bass:.2}/s)"
    );
}

/// Hard-band arps never read as rising enthusiasm: within every bar the
/// contour falls at least as often as it rises.
#[test]
fn hard_arp_contours_do_not_rise() {
    for case in 0..12 {
        let track = compose(&inputs(&format!("contour-{case}"), Difficulty::Hard));
        let bar_steps = track.grid.bar_steps();
        for bar in (0..16u32).filter(|bar| ![8, 9].contains(bar)) {
            let pitches: Vec<u8> = voice_notes(&track, "arp")
                .iter()
                .filter(|note| note.start_step / bar_steps == bar)
                .map(|note| note.pitch.expect("arp is pitched").midi())
                .collect();
            let rises = pitches.windows(2).filter(|pair| pair[1] > pair[0]).count();
            let falls = pitches.windows(2).filter(|pair| pair[1] < pair[0]).count();
            assert!(
                rises <= falls,
                "contour-{case} bar {bar}: arp rises {rises} > falls {falls} ({pitches:?})"
            );
        }
    }
}

/// The hard band trades speed for weight: its no-hazard tempo must not exceed
/// 130 BPM (v4 targeted 136 and hazard rooms landed as high as 150).
#[test]
fn hard_band_tempo_is_restrained() {
    let track = compose(&inputs("tempo-cap", Difficulty::Hard));
    assert!(
        track.grid.bpm() <= 130.0,
        "hard fallback tempo {:.1} BPM exceeds the restraint cap",
        track.grid.bpm()
    );
    assert!(Difficulty::Hard.target_bpm() <= 126, "hard target BPM must stay restrained");
}

/// keep-crown-sanctum, the final screen, keeps the shared character but earns
/// a finale lift: fuller pad harmony and lifted dynamics in the last section,
/// which no other room receives.
#[test]
fn crown_sanctum_finale_lifts_its_last_section() {
    let crown = compose(&crown_sanctum_inputs());
    let control = compose(&RoomMusicInputs {
        slug: "not-the-sanctum".to_owned(),
        ..crown_sanctum_inputs()
    });
    let bar_steps = crown.grid.bar_steps();
    let pad_in = |track: &Track, lo: u32, hi: u32| -> usize {
        voice_notes(track, "pad")
            .iter()
            .filter(|note| (lo..hi).contains(&(note.start_step / bar_steps)))
            .count()
    };
    // Fuller harmony: the finale's last section carries added pad chord
    // tones; the control (and the crown's own earlier sections) do not.
    assert!(
        pad_in(&crown, 12, 16) > pad_in(&crown, 0, 4),
        "finale must thicken the pad in bars 13-16"
    );
    assert_eq!(
        pad_in(&control, 12, 16),
        pad_in(&control, 0, 4),
        "non-finale rooms keep the plain pad"
    );

    // Lifted dynamics: the final section is louder than the opening.
    let mean_vel = |track: &Track, lo: u32, hi: u32| -> f64 {
        let vels: Vec<f64> = track
            .notes
            .iter()
            .filter(|note| (lo..hi).contains(&(note.start_step / bar_steps)))
            .map(|note| f64::from(note.vel))
            .collect();
        vels.iter().sum::<f64>() / vels.len().max(1) as f64
    };
    let crown_lift = mean_vel(&crown, 12, 16) - mean_vel(&crown, 0, 4);
    let control_lift = mean_vel(&control, 12, 16) - mean_vel(&control, 0, 4);
    assert!(
        crown_lift > control_lift + 0.3,
        "finale dynamic lift {crown_lift:.2} not clearly above control {control_lift:.2}"
    );

    // Same character otherwise: same grid, same mode, and the early sections
    // stay at the band's plain pad density.
    assert_eq!(crown.grid, control.grid);
    assert_eq!(crown.key, control.key);
    assert_eq!(pad_in(&crown, 0, 4), pad_in(&control, 0, 4));
}
