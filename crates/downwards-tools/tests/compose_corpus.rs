//! §10.3: golden digests — compose every rooms-v2 room from the real loaders
//! and compare against the checked-in digest list. Regenerate with
//! `cargo run -p downwards-tools -- compose-tracks --print-digests`.
//!
//! (This test lives in `downwards-tools` rather than `downwards-audio`
//! because building the composer inputs needs the design JSONs, and the
//! audio crate is deliberately dependency-free.)

use std::collections::BTreeMap;

use downwards_audio::{Track, compose, fnv1a64};
use downwards_tools::music_inputs::{all_room_inputs, repo_root};

#[test]
fn composed_corpus_matches_the_golden_digests() {
    let golden_path = repo_root().join("crates/downwards-audio/tests/golden/track-digests-v1.txt");
    let golden = std::fs::read_to_string(&golden_path)
        .expect("golden digest list is checked in (compose-tracks --print-digests)");
    let mut expected = BTreeMap::new();
    for line in golden.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (slug, digest) = line.split_once(' ').expect("golden line is `slug digest`");
        expected.insert(
            slug.to_owned(),
            u64::from_str_radix(digest, 16).expect("golden digest is hex"),
        );
    }

    let records = all_room_inputs().expect("design data parses");
    assert_eq!(
        records.len(),
        expected.len(),
        "golden list must cover every rooms-v2 room; regenerate with --print-digests"
    );
    for record in &records {
        let serialized = compose(&record.inputs).serialize();
        let digest = fnv1a64(serialized.as_bytes());
        assert_eq!(
            Some(&digest),
            expected.get(&record.inputs.slug),
            "composed track for {} changed; regenerate goldens if the mapping change is intended",
            record.inputs.slug
        );
    }
}

#[test]
fn checked_in_artifacts_parse_and_stay_in_sync_with_live_hazards() {
    let records = all_room_inputs().expect("design data parses");
    for record in &records {
        let slug = &record.inputs.slug;
        let source = downwards_content::generated_tracks::track_source(slug)
            .unwrap_or_else(|| panic!("no checked-in track for {slug}"));
        let (track, recorded) = Track::parse(source)
            .unwrap_or_else(|error| panic!("track for {slug} fails to parse: {error}"));
        assert_eq!(track.slug, *slug);
        // The artifact must agree with the live hazard clocks.
        track
            .check_hazard_sync(&record.inputs.hazards)
            .unwrap_or_else(|error| panic!("track for {slug} lost hazard sync: {error}"));
        // Machine-owned artifacts carry a truthful digest.
        if !track.hand_tuned {
            assert_eq!(
                recorded,
                track.body_digest(),
                "machine-owned track {slug} has a stale generated-digest"
            );
        }
    }
}

#[test]
fn every_room_gets_a_track_and_layer_gates_reflect_passability() {
    let records = all_room_inputs().expect("design data parses");
    assert_eq!(records.len(), downwards_content::rooms_v2_slugs().len());
    // At least one room in the corpus must gate each ability layer, and
    // non-gated rooms must have no such layer (hearing one always means the
    // ability matters).
    let mut any_gloves = false;
    let mut any_boots = false;
    for record in &records {
        let track = compose(&record.inputs);
        assert_eq!(
            track.layers.contains(&"gloves".to_owned()),
            record.inputs.layer_gates.gloves
        );
        assert_eq!(
            track.layers.contains(&"boots".to_owned()),
            record.inputs.layer_gates.boots
        );
        any_gloves |= record.inputs.layer_gates.gloves;
        any_boots |= record.inputs.layer_gates.boots;
    }
    assert!(any_gloves, "some corpus room should gate on gloves");
    assert!(any_boots, "some corpus room should gate on boots");
}
