//! §10.4: `parse(serialize(t)) == t` plus parser rejection cases and the
//! hazard-grid re-validation hook.

use downwards_audio::{
    AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs, Track, compose,
    track::canonical_body_of_file,
};

fn sample_inputs(slug: &str, difficulty: Difficulty, gates: LayerGates) -> RoomMusicInputs {
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
        hazards: vec![
            HazardTiming {
                period: 96,
                active: 20,
                phase: 45,
            },
            HazardTiming {
                period: 157,
                active: 30,
                phase: 0,
            },
        ],
        layer_gates: gates,
    }
}

#[test]
fn composed_tracks_round_trip_exactly() {
    for (difficulty, gates) in [
        (Difficulty::Easy, LayerGates::default()),
        (
            Difficulty::Medium,
            LayerGates {
                gloves: true,
                boots: false,
            },
        ),
        (
            Difficulty::Hard,
            LayerGates {
                gloves: true,
                boots: true,
            },
        ),
    ] {
        let track = compose(&sample_inputs("roundtrip-room", difficulty, gates));
        let text = track.serialize();
        let (parsed, recorded_digest) = Track::parse(&text).expect("serialized tracks parse");
        assert_eq!(parsed, track);
        assert_eq!(recorded_digest, track.body_digest());
        // Round again: serialization is a fixed point.
        assert_eq!(parsed.serialize(), text);
    }
}

#[test]
fn comments_and_blank_lines_are_ignored_and_digest_is_canonical() {
    let track = compose(&sample_inputs("comment-room", Difficulty::Easy, LayerGates::default()));
    let text = track.serialize();
    let commented = text
        .lines()
        .map(|line| format!("{line}   # trailing comment"))
        .collect::<Vec<_>>()
        .join("\n\n# a lonely comment line\n\n");
    let (parsed, _) = Track::parse(&commented).expect("comments never change parsing");
    assert_eq!(parsed, track);
    assert_eq!(
        canonical_body_of_file(&commented),
        canonical_body_of_file(&text),
        "canonical body strips comments and blanks"
    );
}

fn parse_error(source: &str) -> String {
    Track::parse(source).expect_err("must be rejected").message
}

#[test]
fn parser_rejects_malformed_tracks() {
    let track = compose(&sample_inputs("reject-room", Difficulty::Medium, LayerGates::default()));
    let text = track.serialize();

    // Unknown directive.
    let unknown = format!("{text}\nwibble 3\n");
    assert!(parse_error(&unknown).contains("unknown directive"));

    // Note referencing an undeclared voice.
    let bad_voice = text.replace("note lead", "note ghost");
    assert!(parse_error(&bad_voice).contains("undeclared voice"));

    // Voice referencing an undeclared layer.
    let bad_layer = text.replace("layer base gain 7", "layer wings gain 7");
    assert!(parse_error(&bad_layer).contains("undeclared layer"));

    // Out-of-range velocity.
    let (_, _) = Track::parse(&text).unwrap();
    let bad_vel = text.replace("hat perc 1 4", "hat perc 1 99");
    assert!(parse_error(&bad_vel).contains("velocity"));

    // Out-of-range step (edit an existing hat so section order stays legal).
    assert!(track.grid.loop_steps() < 999);
    let bad_step = text.replace("hat perc 1 4", "hat perc 999 4");
    assert!(parse_error(&bad_step).contains("start-step"));

    // Directives after the hazard section violate the fixed section order.
    let out_of_order = format!("{text}\nhat perc 1 4\n");
    assert!(parse_error(&out_of_order).contains("section order"));

    // Missing base layer.
    let no_base = text.replace("layer base\n", "");
    assert!(Track::parse(&no_base).is_err());
}

#[test]
fn hazard_grid_revalidation_catches_broken_edits() {
    let inputs = sample_inputs("sync-room", Difficulty::Medium, LayerGates::default());
    let track = compose(&inputs);
    track
        .check_hazard_sync(&inputs.hazards)
        .expect("freshly composed tracks are in sync");

    // A hand edit that changes the step so the locked hazard misses the grid
    // must hard-error instead of silently breaking sync.
    let text = track.serialize();
    let grid_line = text
        .lines()
        .find(|line| line.starts_with("grid "))
        .unwrap()
        .to_owned();
    let edited = text.replace(&grid_line, "grid step-ticks 7 beat-steps 4 offset 0 loop-bars 16");
    let (edited_track, _) = Track::parse(&edited).expect("the edited grid still parses");
    assert!(
        edited_track.check_hazard_sync(&inputs.hazards).is_err(),
        "locked hazard off the edited grid must be rejected"
    );

    // A locked hazard voice pointing at a nonexistent hazard is rejected.
    assert!(track.check_hazard_sync(&[]).is_err());
}

#[test]
fn hand_tuned_flag_round_trips() {
    let mut track = compose(&sample_inputs("tuned-room", Difficulty::Hard, LayerGates::default()));
    track.hand_tuned = true;
    let text = track.serialize();
    assert!(text.contains("hand-tuned true"));
    let (parsed, _) = Track::parse(&text).unwrap();
    assert!(parsed.hand_tuned);
    assert_eq!(parsed, track);
}
