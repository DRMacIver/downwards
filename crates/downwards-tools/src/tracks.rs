//! The `compose-tracks` artifact pipeline (§5 of the soundtrack spec):
//! generate-to-editable-artifact with a digest protocol so hand edits
//! survive regeneration.

use std::{error::Error, fs, path::Path};

use downwards_audio::{RoomMusicInputs, compose, fnv1a64, track::canonical_body_of_file};

/// How `compose_tracks` treats the filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Write machine-owned files, protect hand-edited ones.
    Write,
    /// Write nothing; report what would change (unified-ish diff on stdout).
    Diff,
    /// Write nothing; exit nonzero if a machine-owned file is stale.
    Check,
    /// Overwrite protected files too, loudly.
    Force,
}

/// Result of one `compose_tracks` run.
#[derive(Clone, Debug, Default)]
pub struct ComposeReport {
    pub written: Vec<String>,
    pub protected: Vec<String>,
    pub stale: Vec<String>,
    pub diffs: Vec<(String, String)>,
    pub forced: Vec<String>,
}

impl ComposeReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "written {}, protected {}, stale {}",
            self.written.len(),
            self.protected.len(),
            self.stale.len()
        )
    }

    /// `--check` fails only when a *machine-owned* file is stale.
    #[must_use]
    pub fn check_failed(&self) -> bool {
        !self.stale.is_empty()
    }
}

/// The `hand-tuned` flag recorded in an existing artifact file.
fn hand_tuned_flag(source: &str) -> bool {
    source.lines().any(|line| {
        downwards_audio::track::strip_comment(line).trim() == "hand-tuned true"
    })
}

/// The `generated-digest` recorded in an existing artifact file.
fn recorded_digest(source: &str) -> Option<u64> {
    source.lines().find_map(|line| {
        let line = downwards_audio::track::strip_comment(line).trim().to_owned();
        let value = line.strip_prefix("generated-digest ")?;
        u64::from_str_radix(value.trim(), 16).ok()
    })
}

fn simple_diff(current: &str, generated: &str) -> String {
    let mut out = String::new();
    for line in current.lines() {
        if !generated.lines().any(|other| other == line) {
            out.push_str(&format!("- {line}\n"));
        }
    }
    for line in generated.lines() {
        if !current.lines().any(|other| other == line) {
            out.push_str(&format!("+ {line}\n"));
        }
    }
    out
}

/// Run the §5.3 protocol over the given room inputs, writing artifacts into
/// `tracks_dir` (`crates/downwards-content/tracks/rooms-v2` in the real
/// repository; tests use temporary directories).
pub fn compose_tracks(
    tracks_dir: &Path,
    rooms: &[RoomMusicInputs],
    mode: WriteMode,
    slug_filter: Option<&str>,
) -> Result<ComposeReport, Box<dyn Error>> {
    let mut report = ComposeReport::default();
    if matches!(mode, WriteMode::Write | WriteMode::Force) {
        fs::create_dir_all(tracks_dir)?;
    }
    for inputs in rooms {
        if slug_filter.is_some_and(|filter| filter != inputs.slug) {
            continue;
        }
        let generated = compose(inputs).serialize();
        let path = tracks_dir.join(format!("{}.track", inputs.slug));
        let existing = fs::read_to_string(&path).ok();

        let decision = match &existing {
            None => Decision::WriteFresh,
            Some(current) => {
                if current == &generated {
                    Decision::UpToDate
                } else {
                    let body_digest = canonical_body_of_file(current)
                        .map(|body| fnv1a64(body.as_bytes()));
                    let machine_owned = !hand_tuned_flag(current)
                        && recorded_digest(current).is_some()
                        && body_digest == recorded_digest(current);
                    if machine_owned {
                        Decision::StaleMachineOwned
                    } else {
                        Decision::Protected
                    }
                }
            }
        };

        match (decision, mode) {
            (Decision::UpToDate, _) => {}
            (Decision::WriteFresh | Decision::StaleMachineOwned, WriteMode::Write | WriteMode::Force) => {
                fs::write(&path, &generated)?;
                report.written.push(inputs.slug.clone());
            }
            (Decision::WriteFresh, WriteMode::Diff | WriteMode::Check) => {
                report.stale.push(inputs.slug.clone());
                if mode == WriteMode::Diff {
                    report.diffs.push((inputs.slug.clone(), generated.clone()));
                }
            }
            (Decision::StaleMachineOwned, WriteMode::Diff | WriteMode::Check) => {
                report.stale.push(inputs.slug.clone());
                if mode == WriteMode::Diff {
                    let current = existing.as_deref().unwrap_or("");
                    report
                        .diffs
                        .push((inputs.slug.clone(), simple_diff(current, &generated)));
                }
            }
            (Decision::Protected, WriteMode::Force) => {
                fs::write(&path, &generated)?;
                report.written.push(inputs.slug.clone());
                report.forced.push(inputs.slug.clone());
            }
            (Decision::Protected, _) => {
                report.protected.push(inputs.slug.clone());
                if mode == WriteMode::Diff {
                    let current = existing.as_deref().unwrap_or("");
                    report
                        .diffs
                        .push((inputs.slug.clone(), simple_diff(current, &generated)));
                }
            }
        }
    }
    Ok(report)
}

enum Decision {
    WriteFresh,
    UpToDate,
    StaleMachineOwned,
    Protected,
}

/// Deterministic source of `downwards-content/src/generated_tracks.rs`,
/// mirroring the `dungeon_v2.rs` `include_str!` pattern.
#[must_use]
pub fn generated_index_source(slugs: &[String]) -> String {
    let mut out = String::from(
        "// @generated by compose-tracks\n//! Checked-in per-room soundtrack artifacts. Regenerate with\n//! `cargo run -p downwards-tools -- compose-tracks`.\n\n/// Source text of the `.track` artifact for a rooms-v2 slug, if one is\n/// checked in.\n#[must_use]\npub fn track_source(slug: &str) -> Option<&'static str> {\n    match slug {\n",
    );
    for slug in slugs {
        out.push_str(&format!(
            "        \"{slug}\" => Some(include_str!(\"../tracks/rooms-v2/{slug}.track\")),\n"
        ));
    }
    out.push_str(
        "        _ => None,\n    }\n}\n\n/// Every slug with a checked-in track, sorted.\n#[must_use]\npub fn track_slugs() -> &'static [&'static str] {\n    &[\n",
    );
    for slug in slugs {
        out.push_str(&format!("        \"{slug}\",\n"));
    }
    out.push_str("    ]\n}\n");
    out
}

/// The golden digest list (§10.3): `fnv1a64(serialized composed track)` per
/// slug, in slug order.
#[must_use]
pub fn golden_digests(rooms: &[RoomMusicInputs]) -> String {
    let mut out = String::from("# track digests v1: fnv1a64 of the composed serialized track\n");
    for inputs in rooms {
        let digest = fnv1a64(compose(inputs).serialize().as_bytes());
        out.push_str(&format!("{} {digest:016x}\n", inputs.slug));
    }
    out
}
