//! v4 vibe-revision tests: register/mood mapping, same-band variety with a
//! structural-distance floor, coherence guarantees (A A B A block form, one
//! arp motif per track), audible inventory layers, and the v2 artifact
//! grammar (pad voice, reverb sends, vibrato, soft noise, aeolian).

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, LayerGates, Mode, NoteEvent,
    RoomMusicInputs, Track, compose, offline::render,
};

const RATE: u32 = 44_100;

fn inputs(slug: &str, difficulty: Difficulty, gates: LayerGates) -> RoomMusicInputs {
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
        layer_gates: gates,
    }
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

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt()
}

/// Register + mood (work item 1): the difficulty ladder maps to
/// mixolydian/dorian/aeolian — no plain major anywhere — and the lead lives
/// in the dropped C3..C5 register instead of the old piercing C4..C6.
#[test]
fn modes_are_darkened_and_lead_register_is_low() {
    // v5 restraint pass darkened the easy band from mixolydian to dorian —
    // the whole ladder now stays minor-leaning.
    for (difficulty, mode) in [
        (Difficulty::Easy, Mode::Dorian),
        (Difficulty::Medium, Mode::Dorian),
        (Difficulty::Hard, Mode::Aeolian),
    ] {
        let track = compose(&inputs("vibe-mode", difficulty, LayerGates::default()));
        assert_eq!(track.key.mode, mode, "{difficulty:?} mode");
        for note in voice_notes(&track, "lead") {
            let midi = note.pitch.expect("lead is pitched").midi();
            assert!(
                (48..=72).contains(&midi),
                "{difficulty:?}: lead note {midi} outside C3..C5"
            );
        }
        // The pad underlay exists and carries the biggest reverb send.
        let pad = track
            .voices
            .iter()
            .find(|voice| voice.name == "pad")
            .expect("every track carries the pad underlay");
        assert!(pad.send > 0, "pad must feed the reverb");
        assert!(!voice_notes(&track, "pad").is_empty());
    }
}

/// Structural axes of one composed track, extracted from the artifact alone.
struct Axes {
    /// Rank pattern of the first bar's arp (motif shape).
    arp_shape: Vec<usize>,
    /// Bass rhythm signature of a pure A-block bar (bar 1).
    bass_a: Vec<(u32, u32)>,
    /// Bass rhythm signature of the B-block bar (bar 10).
    bass_b: Vec<(u32, u32)>,
    /// Percussion signature (offset, voice-is-tick, vel) of bar 0.
    perc_a: Vec<(u32, bool, u8)>,
    /// Percussion signature of bar 10 (B block, same cell half as bar 0).
    perc_b: Vec<(u32, bool, u8)>,
}

fn bar_notes<'a>(track: &'a Track, voice: &str, bar: u32) -> Vec<&'a NoteEvent> {
    let bar_steps = track.grid.bar_steps();
    voice_notes(track, voice)
        .into_iter()
        .filter(|note| note.start_step / bar_steps == bar)
        .collect()
}

fn axes(track: &Track) -> Axes {
    let bar_steps = track.grid.bar_steps();
    let arp_bar: Vec<u8> = bar_notes(track, "arp", 0)
        .iter()
        .map(|note| note.pitch.expect("arp is pitched").midi())
        .collect();
    let mut sorted = arp_bar.clone();
    sorted.sort_unstable();
    sorted.dedup();
    let arp_shape = arp_bar
        .iter()
        .map(|midi| sorted.iter().position(|other| other == midi).expect("member"))
        .collect();
    let bass_sig = |bar: u32| {
        bar_notes(track, "bass", bar)
            .iter()
            .map(|note| (note.start_step % bar_steps, note.len_steps))
            .collect()
    };
    let perc_sig = |bar: u32| {
        let mut events: Vec<(u32, bool, u8)> = track
            .notes
            .iter()
            .filter(|note| {
                (note.voice == "perc" || note.voice == "shaker")
                    && note.start_step / bar_steps == bar
            })
            .map(|note| (note.start_step % bar_steps, note.voice == "perc", note.vel))
            .collect();
        events.sort_unstable();
        events
    };
    Axes {
        arp_shape,
        bass_a: bass_sig(1),
        bass_b: bass_sig(10),
        perc_a: perc_sig(0),
        perc_b: perc_sig(10),
    }
}

/// Same-band variety (work item 3): a dozen same-difficulty rooms must all
/// serialize differently, and each pair must differ on at least one seeded
/// accompaniment axis (arp shape, bass patterns, perc patterns), with a mean
/// structural distance well above the floor.
#[test]
fn same_band_rooms_differ_structurally() {
    for difficulty in [Difficulty::Medium, Difficulty::Hard] {
        let tracks: Vec<Track> = (0..12)
            .map(|case| {
                compose(&inputs(
                    &format!("band-{difficulty:?}-{case}"),
                    difficulty,
                    LayerGates::default(),
                ))
            })
            .collect();
        let serialized: Vec<String> = tracks.iter().map(Track::serialize).collect();
        let extracted: Vec<Axes> = tracks.iter().map(axes).collect();

        let mut total_distance = 0_usize;
        let mut pairs = 0_usize;
        for i in 0..tracks.len() {
            for j in i + 1..tracks.len() {
                assert_ne!(serialized[i], serialized[j], "identical same-band tracks");
                let a = &extracted[i];
                let b = &extracted[j];
                let distance = usize::from(a.arp_shape != b.arp_shape)
                    + usize::from((&a.bass_a, &a.bass_b) != (&b.bass_a, &b.bass_b))
                    + usize::from((&a.perc_a, &a.perc_b) != (&b.perc_a, &b.perc_b));
                assert!(
                    distance >= 1,
                    "{difficulty:?}: rooms {i} and {j} share every accompaniment axis"
                );
                total_distance += distance;
                pairs += 1;
            }
        }
        assert!(
            total_distance * 2 >= pairs * 3,
            "{difficulty:?}: mean structural distance {} / {pairs} below 1.5",
            total_distance
        );
    }
}

/// Coherence (work item 6): the accompaniment holds one motif — bass and
/// perc blocks arranged A A B A with a genuinely contrasting B, and the arp
/// repeating a single shape in every sounding bar.
#[test]
fn accompaniment_is_a_a_b_a_with_one_arp_motif() {
    for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
        for case in 0..6 {
            let track = compose(&inputs(
                &format!("coherent-{difficulty:?}-{case}"),
                difficulty,
                LayerGates::default(),
            ));
            let bar_steps = track.grid.bar_steps();
            let bass_sig = |bar: u32| -> Vec<(u32, u32)> {
                bar_notes(&track, "bass", bar)
                    .iter()
                    .map(|note| (note.start_step % bar_steps, note.len_steps))
                    .collect()
            };
            // Pure pattern bars of blocks 0, 1, 3 (A) and block 2 (B).
            assert_eq!(bass_sig(1), bass_sig(5), "block 1 must repeat block 0");
            assert_eq!(bass_sig(1), bass_sig(13), "block 3 must return to A");
            assert_ne!(bass_sig(1), bass_sig(10), "block 2 must contrast (B)");

            // One arp motif: identical rank shape in every sounding bar.
            let rank_shape = |bar: u32| -> Vec<usize> {
                let pitches: Vec<u8> = bar_notes(&track, "arp", bar)
                    .iter()
                    .map(|note| note.pitch.expect("arp is pitched").midi())
                    .collect();
                let mut sorted = pitches.clone();
                sorted.sort_unstable();
                sorted.dedup();
                pitches
                    .iter()
                    .map(|midi| sorted.iter().position(|other| other == midi).expect("member"))
                    .collect()
            };
            let reference = rank_shape(0);
            assert!(!reference.is_empty());
            for bar in (0..16).filter(|bar| ![8, 9].contains(bar)) {
                assert_eq!(
                    rank_shape(bar),
                    reference,
                    "{difficulty:?}-{case}: arp motif drifts in bar {bar}"
                );
            }
        }
    }
}

/// Audible inventory layers (work item 5): rendering with an ability layer
/// enabled must move the mix well clear of the without-layer render — both
/// in overall energy and as a raw difference signal.
#[test]
fn inventory_layers_are_clearly_audible() {
    let room = inputs(
        "layers-audible",
        Difficulty::Medium,
        LayerGates {
            gloves: true,
            boots: true,
        },
    );
    let track = compose(&room);
    let base = render(&track, &room.hazards, AbilityMask::default(), 1, RATE);
    for (name, abilities) in [
        ("gloves", AbilityMask { gloves: true, boots: false }),
        ("boots", AbilityMask { gloves: false, boots: true }),
    ] {
        let with = render(&track, &room.hazards, abilities, 1, RATE);
        let difference: Vec<f32> = with
            .iter()
            .zip(&base)
            .map(|(&a, &b)| a - b)
            .collect();
        let base_rms = rms(&base);
        let diff_rms = rms(&difference);
        assert!(
            diff_rms > 0.18 * base_rms,
            "{name}: layer difference {diff_rms:.4} inaudible against base {base_rms:.4}"
        );
        let peak = with.iter().fold(0.0_f32, |acc, &s| acc.max(s.abs()));
        assert!(peak <= 0.85, "{name}: peak {peak} rides the clipper");
    }
}

/// The v2 grammar round-trips (pad/send/vib/noise-soft/aeolian) and v1
/// artifacts still parse — hand-tuned protection must keep working across
/// the version bump.
#[test]
fn v2_grammar_round_trips_and_v1_still_parses() {
    let track = compose(&inputs(
        "grammar-v2",
        Difficulty::Hard,
        LayerGates {
            gloves: true,
            boots: true,
        },
    ));
    let text = track.serialize();
    assert!(text.starts_with("downwards-track v2\n"), "format version bump");
    assert!(text.contains(" pad "), "pad voice serialized");
    assert!(text.contains(" send "), "reverb sends serialized");
    assert!(text.contains(" vib "), "vibrato serialized");
    assert!(text.contains(" noise-soft "), "shaker voice serialized");
    assert!(text.contains("key C aeolian"), "aeolian mode serialized");
    let (parsed, digest) = Track::parse(&text).expect("v2 tracks parse");
    assert_eq!(parsed, track);
    assert_eq!(digest, track.body_digest());
    assert_eq!(parsed.serialize(), text);

    // A v1-era artifact (no sends, no pad, ionian) parses with defaults.
    let legacy = "\
downwards-track v1
slug legacy-room
generated-digest 0000000000000000
hand-tuned true

grid step-ticks 8 beat-steps 4 offset 0 loop-bars 16
key C ionian

layer base

voice lead pulse 0.5 layer base gain 7
voice perc noise layer base gain 3

note lead 0 4 C4 12
hat perc 2 4
";
    let (legacy_track, _) = Track::parse(legacy).expect("v1 artifacts stay parseable");
    assert!(legacy_track.hand_tuned);
    assert_eq!(legacy_track.voices[0].send, 0);
    assert_eq!(legacy_track.voices[0].vibrato, None);
}
