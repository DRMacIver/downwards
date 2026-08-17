//! §10.4 (protocol half): the hand-tune protection protocol of
//! `compose-tracks`, exercised against a temporary directory.

use std::{fs, path::PathBuf};

use downwards_audio::{
    AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs,
};
use downwards_tools::tracks::{WriteMode, compose_tracks};

fn sample_rooms() -> Vec<RoomMusicInputs> {
    vec![
        RoomMusicInputs {
            slug: "proto-a".to_owned(),
            difficulty: Difficulty::Easy,
            ability_requirement: AbilityReq::None,
            coin_count: 1,
            doors: DoorSet::default(),
            hazards: vec![HazardTiming {
                period: 120,
                active: 30,
                phase: 0,
            }],
            layer_gates: LayerGates::default(),
        },
        RoomMusicInputs {
            slug: "proto-b".to_owned(),
            difficulty: Difficulty::Hard,
            ability_requirement: AbilityReq::Both,
            coin_count: 4,
            doors: DoorSet {
                floor: true,
                ..DoorSet::default()
            },
            hazards: Vec::new(),
            layer_gates: LayerGates {
                gloves: true,
                boots: true,
            },
        },
    ]
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "downwards-track-protocol-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fresh_write_then_machine_owned_overwrite() {
    let dir = temp_dir("fresh");
    let rooms = sample_rooms();
    let report = compose_tracks(&dir, &rooms, WriteMode::Write, None).unwrap();
    assert_eq!(report.written, vec!["proto-a", "proto-b"]);
    assert_eq!(report.summary(), "written 2, protected 0, stale 0");

    // Unchanged rerun: nothing to do.
    let rerun = compose_tracks(&dir, &rooms, WriteMode::Write, None).unwrap();
    assert_eq!(rerun.summary(), "written 0, protected 0, stale 0");

    // A mapping change (different inputs) overwrites machine-owned files.
    let mut changed = rooms.clone();
    changed[0].difficulty = Difficulty::Hard;
    let overwrite = compose_tracks(&dir, &changed, WriteMode::Write, None).unwrap();
    assert_eq!(overwrite.written, vec!["proto-a"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn body_edits_protect_and_force_overrides() {
    let dir = temp_dir("edit");
    let rooms = sample_rooms();
    compose_tracks(&dir, &rooms, WriteMode::Write, None).unwrap();

    // Hand edit: change a note velocity (breaks the digest braces).
    let path = dir.join("proto-a.track");
    let source = fs::read_to_string(&path).unwrap();
    let edited = source.replacen(" 12\n", " 13\n", 1);
    assert_ne!(source, edited);
    fs::write(&path, &edited).unwrap();

    let mut changed = rooms.clone();
    changed[0].difficulty = Difficulty::Hard;
    let report = compose_tracks(&dir, &changed, WriteMode::Write, None).unwrap();
    assert_eq!(report.protected, vec!["proto-a"]);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        edited,
        "hand-edited body must survive regeneration"
    );

    // --force overwrites protected files, loudly.
    let forced = compose_tracks(&dir, &changed, WriteMode::Force, None).unwrap();
    assert_eq!(forced.forced, vec!["proto-a"]);
    assert_ne!(fs::read_to_string(&path).unwrap(), edited);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hand_tuned_flag_protects_even_with_matching_digest() {
    let dir = temp_dir("belt");
    let rooms = sample_rooms();
    compose_tracks(&dir, &rooms, WriteMode::Write, None).unwrap();

    // Belt: hand-tuned true with an untouched body still protects.
    let path = dir.join("proto-a.track");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, source.replace("hand-tuned false", "hand-tuned true")).unwrap();

    let mut changed = rooms.clone();
    changed[0].difficulty = Difficulty::Hard;
    let report = compose_tracks(&dir, &changed, WriteMode::Write, None).unwrap();
    assert_eq!(report.protected, vec!["proto-a"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_fails_only_on_stale_machine_owned_files() {
    let dir = temp_dir("check");
    let rooms = sample_rooms();
    compose_tracks(&dir, &rooms, WriteMode::Write, None).unwrap();

    // Clean tree: check passes.
    let clean = compose_tracks(&dir, &rooms, WriteMode::Check, None).unwrap();
    assert!(!clean.check_failed());

    // Hand-edited file: protected, never fails check.
    let path = dir.join("proto-a.track");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, source.replacen(" 12\n", " 13\n", 1)).unwrap();
    let protected = compose_tracks(&dir, &rooms, WriteMode::Check, None).unwrap();
    assert!(!protected.check_failed());
    assert_eq!(protected.protected, vec!["proto-a"]);

    // Stale machine-owned file (mapping changed underneath it): check fails.
    let mut changed = rooms.clone();
    changed[1].difficulty = Difficulty::Easy;
    let stale = compose_tracks(&dir, &changed, WriteMode::Check, None).unwrap();
    assert!(stale.check_failed());
    assert_eq!(stale.stale, vec!["proto-b"]);

    // Diff mode reports the same staleness and writes nothing.
    let before = fs::read_to_string(dir.join("proto-b.track")).unwrap();
    let diff = compose_tracks(&dir, &changed, WriteMode::Diff, None).unwrap();
    assert_eq!(diff.stale, vec!["proto-b"]);
    assert!(!diff.diffs.is_empty());
    assert_eq!(fs::read_to_string(dir.join("proto-b.track")).unwrap(), before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slug_filter_restricts_the_run() {
    let dir = temp_dir("slug");
    let rooms = sample_rooms();
    let report = compose_tracks(&dir, &rooms, WriteMode::Write, Some("proto-b")).unwrap();
    assert_eq!(report.written, vec!["proto-b"]);
    assert!(!dir.join("proto-a.track").exists());
    let _ = fs::remove_dir_all(&dir);
}
