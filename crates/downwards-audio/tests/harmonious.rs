//! Designer-feedback regression tests (2026-08): the chiptunes must be
//! gentle and consonant — background music, not attention-demanding.
//!
//! - Melody motion is constrained: no consecutive interval wider than a
//!   fifth, downbeats on chord tones.
//! - Rendered mixes keep headroom: peaks land below 0.85 full scale instead
//!   of riding the soft clipper.
//! - Percussion is sparse on easy rooms.

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs,
    compose, offline::render,
};

const RATE: u32 = 44_100;

fn inputs(slug: &str, difficulty: Difficulty, hazards: Vec<HazardTiming>) -> RoomMusicInputs {
    RoomMusicInputs {
        slug: slug.to_owned(),
        difficulty,
        ability_requirement: AbilityReq::Wall,
        coin_count: 3,
        doors: DoorSet {
            west: true,
            floor: true,
            ..DoorSet::default()
        },
        hazards,
        layer_gates: LayerGates {
            gloves: true,
            boots: true,
        },
    }
}

fn fixture_rooms() -> Vec<RoomMusicInputs> {
    let mut rooms = Vec::new();
    for (index, difficulty) in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard]
        .into_iter()
        .enumerate()
    {
        for case in 0..8 {
            rooms.push(inputs(
                &format!("harmonious-{index}-{case}"),
                difficulty,
                if case % 2 == 0 {
                    vec![HazardTiming {
                        period: 96,
                        active: 20,
                        phase: (case * 11) as u32 % 96,
                    }]
                } else {
                    Vec::new()
                },
            ));
        }
    }
    rooms
}

/// No consecutive lead-melody interval wider than a perfect fifth: the walk
/// must be chord tones and mostly stepwise motion, without wide leaps.
#[test]
fn lead_melody_has_no_wide_leaps() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let mut lead: Vec<_> = track
            .notes
            .iter()
            .filter(|note| note.voice == "lead")
            .collect();
        lead.sort_by_key(|note| note.start_step);
        for pair in lead.windows(2) {
            let a = i32::from(pair[0].pitch.expect("lead notes are pitched").midi());
            let b = i32::from(pair[1].pitch.expect("lead notes are pitched").midi());
            assert!(
                (a - b).abs() <= 7,
                "{}: lead interval {} semitones at step {} exceeds a fifth",
                room.slug,
                (a - b).abs(),
                pair[1].start_step
            );
        }
    }
}

/// Downbeat lead notes (bar starts) must be chord tones of that bar's chord.
#[test]
fn lead_downbeats_are_chord_tones() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let bar_steps = track.grid.bar_steps();
        let progression = downwards_audio::compose::progression(room.difficulty);
        for note in track.notes.iter().filter(|note| note.voice == "lead") {
            if !note.start_step.is_multiple_of(bar_steps) {
                continue;
            }
            let bar = note.start_step / bar_steps;
            let chord = progression[(bar / 4) as usize];
            let classes = track.key.triad_pitch_classes(chord);
            let pitch_class = note.pitch.expect("lead notes are pitched").midi() % 12;
            assert!(
                classes.iter().any(|class| class.semitones() == pitch_class),
                "{}: downbeat at step {} is not a chord tone",
                room.slug,
                note.start_step
            );
        }
    }
}

/// Rendered mixes keep headroom: peak below 0.85 (never riding the clipper)
/// but clearly audible.
#[test]
fn rendered_mix_keeps_headroom() {
    for room in fixture_rooms().into_iter().step_by(3) {
        let track = compose(&room);
        let samples = render(
            &track,
            &room.hazards,
            AbilityMask {
                gloves: true,
                boots: true,
            },
            1,
            RATE,
        );
        let peak = samples.iter().fold(0.0_f32, |acc, &s| acc.max(s.abs()));
        let rms = (samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>()
            / samples.len() as f64)
            .sqrt();
        assert!(
            peak <= 0.85,
            "{}: peak {peak} rides the clipper; want headroom",
            room.slug
        );
        assert!(peak > 0.15, "{}: peak {peak} inaudibly quiet", room.slug);
        assert!(rms > 0.01, "{}: rms {rms} inaudibly quiet", room.slug);
    }
}

/// Easy rooms carry audibly less percussion than hard rooms.
#[test]
fn percussion_is_sparse_on_easy_rooms() {
    let easy = compose(&inputs("perc-density-easy", Difficulty::Easy, Vec::new()));
    let hard = compose(&inputs("perc-density-hard", Difficulty::Hard, Vec::new()));
    let count = |track: &downwards_audio::Track| {
        track
            .notes
            .iter()
            .filter(|note| note.voice == "perc")
            .count()
    };
    let easy_count = count(&easy);
    let hard_count = count(&hard);
    assert!(
        easy_count * 2 <= hard_count,
        "easy rooms must have at most half the hats of hard rooms ({easy_count} vs {hard_count})"
    );
    // And easy percussion is genuinely sparse: at most 2 hats per bar.
    assert!(easy_count <= 32, "easy perc too dense: {easy_count}");
}
