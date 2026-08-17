//! Composer-expressiveness tier regression tests (2026-08, round-3 designer
//! critique): phrase-arc dynamics, articulation variety, a real bass part,
//! and a tension/release intensity arc over the 16-bar loop — all
//! composer-only, expressed through the existing `.track` grammar.

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, NoteEvent,
    RoomMusicInputs, Track, compose, offline::render,
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
        for case in 0..6 {
            rooms.push(inputs(
                &format!("expressive-{index}-{case}"),
                difficulty,
                if case % 2 == 0 {
                    vec![HazardTiming {
                        period: 96,
                        active: 20,
                        phase: (case * 13) as u32 % 96,
                    }]
                } else {
                    Vec::new()
                },
            ));
        }
    }
    rooms
}

fn voice_notes<'a>(track: &'a Track, voice: &str) -> Vec<&'a NoteEvent> {
    let mut notes: Vec<_> = track
        .notes
        .iter()
        .filter(|note| note.voice == voice)
        .collect();
    notes.sort_by_key(|note| note.start_step);
    notes
}

/// Feature 1 - Phrase-arc dynamics: every base voice has non-uniform velocities, and
/// each 4-bar lead phrase shapes toward its melodic peak (the loudest note of
/// the phrase is at or adjacent in pitch to the phrase's highest pitch).
#[test]
fn base_voices_have_shaped_velocities() {
    for room in fixture_rooms() {
        let track = compose(&room);
        for voice in ["lead", "arp", "bass", "perc"] {
            let notes = voice_notes(&track, voice);
            assert!(!notes.is_empty(), "{}: no {voice} notes", room.slug);
            let first = notes[0].vel;
            assert!(
                notes.iter().any(|note| note.vel != first),
                "{}: {voice} velocities are uniform ({first})",
                room.slug
            );
        }
    }
}

#[test]
fn lead_phrases_crescendo_toward_their_peak() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let phrase_steps = 4 * track.grid.bar_steps();
        let lead = voice_notes(&track, "lead");
        for phrase in 0..4_u32 {
            let lo = phrase * phrase_steps;
            let hi = lo + phrase_steps;
            let in_phrase: Vec<_> = lead
                .iter()
                .filter(|note| note.start_step >= lo && note.start_step < hi)
                .collect();
            if in_phrase.len() < 3 {
                continue;
            }
            let peak_midi = in_phrase
                .iter()
                .map(|note| note.pitch.expect("lead is pitched").midi())
                .max()
                .expect("non-empty");
            let max_vel = in_phrase.iter().map(|note| note.vel).max().expect("non-empty");
            let loudest_hits_peak = in_phrase.iter().any(|note| {
                note.vel == max_vel
                    && u32::from(peak_midi)
                        .abs_diff(u32::from(note.pitch.expect("lead is pitched").midi()))
                        <= 2
            });
            assert!(
                loudest_hits_peak,
                "{}: phrase {phrase} loudest note is far from its melodic peak",
                room.slug
            );
        }
    }
}

/// Feature 2 - Articulation variety: the lead mixes legato (note fills its gap) and
/// detached notes (audible silence before the next onset), and every phrase
/// boundary is preceded by a micro-rest so the line breathes.
#[test]
fn lead_articulation_varies_and_phrases_breathe() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let lead = voice_notes(&track, "lead");
        let loop_steps = track.grid.loop_steps();
        let mut legato = 0_u32;
        let mut detached = 0_u32;
        for (index, note) in lead.iter().enumerate() {
            let next_start = lead
                .get(index + 1)
                .map_or(loop_steps, |next| next.start_step);
            let gap = next_start - note.start_step;
            if gap == 0 {
                continue;
            }
            if note.len_steps >= gap.min(track.grid.beat_steps) {
                legato += 1;
            } else {
                detached += 1;
            }
        }
        assert!(
            legato > 0 && detached > 0,
            "{}: lead articulation is uniform (legato {legato}, detached {detached})",
            room.slug
        );

        // Micro-rests: before each interior phrase downbeat (bars 4, 8, 12)
        // there is at least one step of lead silence.
        let phrase_steps = 4 * track.grid.bar_steps();
        for phrase in 1..4_u32 {
            let boundary = phrase * phrase_steps;
            let breathes = lead
                .iter()
                .all(|note| note.start_step >= boundary || note.start_step + note.len_steps < boundary);
            assert!(
                breathes,
                "{}: no micro-rest before phrase boundary at step {boundary}",
                room.slug
            );
        }
    }
}

/// Feature 3 - Real bass part: rhythmic (more onsets than the old two half-notes per
/// bar on medium/hard), difficulty-graded density, in a low register, and
/// with every pitch either a chord tone of its bar or a stepwise approach
/// (within 2 semitones) to an adjacent bass note.
#[test]
fn bass_is_rhythmic_and_difficulty_graded() {
    let easy = compose(&inputs("bass-density-easy", Difficulty::Easy, Vec::new()));
    let medium = compose(&inputs("bass-density-medium", Difficulty::Medium, Vec::new()));
    let hard = compose(&inputs("bass-density-hard", Difficulty::Hard, Vec::new()));
    let count = |track: &Track| voice_notes(track, "bass").len();
    let (easy_count, medium_count, hard_count) = (count(&easy), count(&medium), count(&hard));
    assert!(
        easy_count < hard_count,
        "hard rooms must drive harder than easy ({easy_count} vs {hard_count})"
    );
    assert!(
        medium_count > 32,
        "medium bass must be busier than the old 2-per-bar half notes ({medium_count})"
    );
    assert!(
        hard_count > 48,
        "hard bass must be driving ({hard_count})"
    );
}

#[test]
fn bass_stays_low_and_harmonically_anchored() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let bass = voice_notes(&track, "bass");
        let progression = downwards_audio::compose::progression(room.difficulty);
        let bar_steps = track.grid.bar_steps();
        for (index, note) in bass.iter().enumerate() {
            let midi = note.pitch.expect("bass is pitched").midi();
            assert!(
                (24..=55).contains(&midi),
                "{}: bass note {midi} out of the low register",
                room.slug
            );
            let bar = note.start_step / bar_steps;
            let chord = progression[((bar / 4) % 4) as usize];
            let classes = track.key.triad_pitch_classes(chord);
            let is_chord_tone = classes.iter().any(|class| class.semitones() == midi % 12);
            let near = |other: Option<&&NoteEvent>| {
                other.is_some_and(|other| {
                    let o = other.pitch.expect("bass is pitched").midi();
                    midi.abs_diff(o) <= 2
                })
            };
            assert!(
                is_chord_tone
                    || near(bass.get(index + 1))
                    || near(index.checked_sub(1).and_then(|i| bass.get(i))),
                "{}: bass note {midi} at step {} is neither chord tone nor stepwise approach",
                room.slug,
                note.start_step
            );
        }
    }
}

/// Feature 4 - Intensity arc: the breathing bars (9th–10th of the loop) carry fewer
/// base-layer onsets than the peak bars, and the final bar has a turnaround
/// (a percussion fill on medium/hard: more perc onsets than a mid-loop bar).
#[test]
fn loop_has_a_breathing_passage_and_turnaround() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let bar_steps = track.grid.bar_steps();
        let base_onsets_in = |from_bar: u32, to_bar: u32| {
            track
                .notes
                .iter()
                .filter(|note| {
                    matches!(note.voice.as_str(), "lead" | "arp" | "bass" | "perc")
                        && note.start_step >= from_bar * bar_steps
                        && note.start_step < to_bar * bar_steps
                })
                .count()
        };
        let breathe = base_onsets_in(8, 10);
        let peak = base_onsets_in(12, 14);
        assert!(
            breathe < peak,
            "{}: breathing bars ({breathe} onsets) not thinner than peak bars ({peak})",
            room.slug
        );
        if room.difficulty != Difficulty::Easy {
            let perc_in = |bar: u32| {
                track
                    .notes
                    .iter()
                    .filter(|note| {
                        note.voice == "perc"
                            && note.start_step >= bar * bar_steps
                            && note.start_step < (bar + 1) * bar_steps
                    })
                    .count()
            };
            assert!(
                perc_in(15) > perc_in(5),
                "{}: bar 16 has no turnaround fill",
                room.slug
            );
        }
    }
}

/// Loop-seam continuity: no note overhangs the loop end, and the loop still
/// resolves home — the last lead pitch is the tonic or its lower neighbour
/// (a pickup rebuilding into the loop start).
#[test]
fn loop_seam_is_intentional() {
    for room in fixture_rooms() {
        let track = compose(&room);
        let loop_steps = track.grid.loop_steps();
        for note in &track.notes {
            assert!(
                note.start_step + note.len_steps <= loop_steps,
                "{}: {} note at step {} overhangs the loop",
                room.slug,
                note.voice,
                note.start_step
            );
        }
        let lead = voice_notes(&track, "lead");
        let last = lead.last().expect("lead is non-empty");
        let tonic = track.key.tonic_in_octave(4).midi();
        let last_midi = last.pitch.expect("lead is pitched").midi();
        assert!(
            last_midi == tonic || (tonic.saturating_sub(4)..tonic).contains(&last_midi),
            "{}: loop ends on {last_midi}, neither tonic {tonic} nor a pickup below it",
            room.slug
        );
    }
}

/// Determinism of the whole expressive tier, and the serialized artifact
/// still round-trips through the unchanged grammar.
#[test]
fn expressive_tracks_are_deterministic_and_round_trip() {
    for room in fixture_rooms() {
        let first = compose(&room).serialize();
        let second = compose(&room).serialize();
        assert_eq!(first, second, "{}: non-deterministic", room.slug);
        let (parsed, _) = Track::parse(&first).expect("composed tracks parse");
        assert_eq!(parsed.serialize(), first, "{}: round-trip drift", room.slug);
    }
}

/// The new dynamics stay inside the headroom brief and audibly move: the
/// per-second RMS of a rendered loop varies across the loop.
#[test]
fn rendered_dynamics_move_but_keep_headroom() {
    for room in fixture_rooms().into_iter().step_by(4) {
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
        assert!(peak <= 0.85, "{}: peak {peak} rides the clipper", room.slug);
        let window = RATE as usize;
        let rms_per_second: Vec<f64> = samples
            .chunks(window)
            .filter(|chunk| chunk.len() == window)
            .map(|chunk| {
                (chunk.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>()
                    / chunk.len() as f64)
                    .sqrt()
            })
            .collect();
        let max = rms_per_second.iter().copied().fold(0.0_f64, f64::max);
        let min = rms_per_second.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            max > min * 1.15,
            "{}: loop dynamics are flat (rms {min:.4}..{max:.4})",
            room.slug
        );
    }
}
