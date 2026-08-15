//! Resumable artifact-v3 deep-cache and selection pipeline.
//!
//! Cache rows and provisional selections are operational observations. Only
//! `finalize` rehydrates the exact source shards and recomputes every selected
//! room before writing a final-recomputed artifact.

use std::{env, error::Error, path::Path};

#[path = "../corpus/mod.rs"]
#[allow(dead_code, unused_imports, clippy::enum_variant_names)]
mod corpus;
#[path = "../structural.rs"]
#[allow(dead_code)]
mod structural;

use corpus::{
    CorpusRoomAnalysisConfig, CorpusSelectionConfig, DEFAULT_CORPUS_PLAYTEST_REQUESTED_ROOMS,
    OfflineSelectionPublicationStateV1, ProvisionalPlaytestPolicy,
    build_or_resume_offline_cache_v1, build_or_resume_offline_cache_v1_with_workers,
    export_corpus_playtest_manifest, finalize_offline_selection_v1,
    finalize_offline_selection_v1_with_workers, load_offline_selection_artifact_v1,
    load_source_bound_offline_cache_v1, select_and_write_provisional_offline_cache_v1,
    verify_final_offline_selection_v1, verify_final_offline_selection_v1_with_workers,
};

const USAGE: &str = "\
usage:
  corpus_v3_offline_selection cache <shard-root> <start-seed> <seed-count> <cache-root>
  corpus_v3_offline_selection cache <shard-root> <start-seed> <seed-count> <cache-root> <room-workers:1|2>
  corpus_v3_offline_selection verify-cache <cache-root> <shard-root>
  corpus_v3_offline_selection select <cache-root> <shard-root> <minimum> <maximum> <elites-per-cell> <new-output-directory>
  corpus_v3_offline_selection finalize <cache-root> <shard-root> <provisional-directory> <new-output-directory>
  corpus_v3_offline_selection finalize <cache-root> <shard-root> <provisional-directory> <new-output-directory> <room-workers:1|2>
  corpus_v3_offline_selection verify-final <cache-root> <shard-root> <selection-directory>
  corpus_v3_offline_selection verify-final <cache-root> <shard-root> <selection-directory> <room-workers:1|2>
  corpus_v3_offline_selection inspect-selection <selection-directory>
  corpus_v3_offline_selection export-playtest <cache-root> <shard-root> <selection-directory> <new-manifest-file> [requested-rooms]
  corpus_v3_offline_selection export-playtest-dev-provisional <cache-root> <shard-root> <selection-directory> <new-manifest-file> [requested-rooms]

`cache` resumes only independently verified completed rows and writes the run
checkpoint last. `select` is provisional. `finalize` performs mandatory exact
source rehydration and full selected-room recomputation. `inspect-selection`
checks only canonical bytes/checkpoint structure; it is not evidence replay.";

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [
            command,
            shard_root,
            start_seed,
            seed_count,
            cache_root,
            room_workers,
        ] if command == "cache" => {
            let start_seed = start_seed.parse::<u64>()?;
            let seed_count = seed_count.parse::<usize>()?;
            let room_workers = parse_room_workers(room_workers)?;
            let verified = build_or_resume_offline_cache_v1_with_workers(
                Path::new(shard_root),
                start_seed,
                seed_count,
                Path::new(cache_root),
                &CorpusRoomAnalysisConfig::default(),
                room_workers,
            )?;
            println!(
                "cache complete: run={} rooms={} eligible={} workers={} output={}",
                verified.manifest.run_id,
                verified.completion.room_count,
                verified.completion.eligible_room_count,
                room_workers,
                cache_root
            );
        }
        [command, shard_root, start_seed, seed_count, cache_root] if command == "cache" => {
            let start_seed = start_seed.parse::<u64>()?;
            let seed_count = seed_count.parse::<usize>()?;
            let verified = build_or_resume_offline_cache_v1(
                Path::new(shard_root),
                start_seed,
                seed_count,
                Path::new(cache_root),
                &CorpusRoomAnalysisConfig::default(),
            )?;
            println!(
                "cache complete: run={} rooms={} eligible={} output={}",
                verified.manifest.run_id,
                verified.completion.room_count,
                verified.completion.eligible_room_count,
                cache_root
            );
        }
        [command, cache_root, shard_root] if command == "verify-cache" => {
            let verified =
                load_source_bound_offline_cache_v1(Path::new(cache_root), Path::new(shard_root))?;
            println!(
                "cache source-bound: run={} rooms={} eligible={}",
                verified.cache.manifest.run_id,
                verified.cache.completion.room_count,
                verified.validated_descriptors.len()
            );
        }
        [
            command,
            cache_root,
            shard_root,
            minimum,
            maximum,
            elites_per_cell,
            output_directory,
        ] if command == "select" => {
            let config = CorpusSelectionConfig::new(
                minimum.parse::<usize>()?,
                maximum.parse::<usize>()?,
                elites_per_cell.parse::<usize>()?,
            );
            let artifact = select_and_write_provisional_offline_cache_v1(
                Path::new(cache_root),
                Path::new(shard_root),
                config,
                Path::new(output_directory),
            )?;
            println!(
                "provisional selection written: rooms={} output={}",
                artifact.outcome.selected_room_ids.len(),
                output_directory
            );
        }
        [
            command,
            cache_root,
            shard_root,
            provisional_directory,
            output_directory,
            room_workers,
        ] if command == "finalize" => {
            let room_workers = parse_room_workers(room_workers)?;
            let artifact = finalize_offline_selection_v1_with_workers(
                Path::new(cache_root),
                Path::new(shard_root),
                Path::new(provisional_directory),
                Path::new(output_directory),
                room_workers,
            )?;
            println!(
                "final recomputed selection written: rooms={} workers={} output={}",
                artifact.outcome.selected_room_ids.len(),
                room_workers,
                output_directory
            );
        }
        [
            command,
            cache_root,
            shard_root,
            provisional_directory,
            output_directory,
        ] if command == "finalize" => {
            let artifact = finalize_offline_selection_v1(
                Path::new(cache_root),
                Path::new(shard_root),
                Path::new(provisional_directory),
                Path::new(output_directory),
            )?;
            println!(
                "final recomputed selection written: rooms={} output={}",
                artifact.outcome.selected_room_ids.len(),
                output_directory
            );
        }
        [
            command,
            cache_root,
            shard_root,
            selection_directory,
            room_workers,
        ] if command == "verify-final" => {
            let room_workers = parse_room_workers(room_workers)?;
            let artifact = verify_final_offline_selection_v1_with_workers(
                Path::new(cache_root),
                Path::new(shard_root),
                Path::new(selection_directory),
                room_workers,
            )?;
            println!(
                "final selection fully reverified: rooms={} workers={} input={}",
                artifact.outcome.selected_room_ids.len(),
                room_workers,
                selection_directory
            );
        }
        [command, cache_root, shard_root, selection_directory] if command == "verify-final" => {
            let artifact = verify_final_offline_selection_v1(
                Path::new(cache_root),
                Path::new(shard_root),
                Path::new(selection_directory),
            )?;
            println!(
                "final selection fully reverified: rooms={} input={}",
                artifact.outcome.selected_room_ids.len(),
                selection_directory
            );
        }
        [command, selection_directory] if command == "inspect-selection" => {
            let artifact = load_offline_selection_artifact_v1(Path::new(selection_directory))?;
            let state = match artifact.publication_state {
                OfflineSelectionPublicationStateV1::ProvisionalOperationalCache => {
                    "provisional-operational-cache"
                }
                OfflineSelectionPublicationStateV1::FinalRecomputed => "final-recomputed",
            };
            println!(
                "selection storage valid: state={} rooms={} input={} (storage check only)",
                state,
                artifact.outcome.selected_room_ids.len(),
                selection_directory
            );
        }
        [
            command,
            cache_root,
            shard_root,
            selection_directory,
            output_file,
        ] if matches!(
            command.as_str(),
            "export-playtest" | "export-playtest-dev-provisional"
        ) =>
        {
            export_playtest(
                command,
                cache_root,
                shard_root,
                selection_directory,
                output_file,
                DEFAULT_CORPUS_PLAYTEST_REQUESTED_ROOMS,
            )?;
        }
        [
            command,
            cache_root,
            shard_root,
            selection_directory,
            output_file,
            requested_rooms,
        ] if matches!(
            command.as_str(),
            "export-playtest" | "export-playtest-dev-provisional"
        ) =>
        {
            export_playtest(
                command,
                cache_root,
                shard_root,
                selection_directory,
                output_file,
                requested_rooms.parse::<usize>()?,
            )?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn export_playtest(
    command: &str,
    cache_root: &str,
    shard_root: &str,
    selection_directory: &str,
    output_file: &str,
    requested_rooms: usize,
) -> Result<(), Box<dyn Error>> {
    let provisional_policy = if command == "export-playtest-dev-provisional" {
        ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback
    } else {
        ProvisionalPlaytestPolicy::Reject
    };
    let summary = export_corpus_playtest_manifest(
        Path::new(cache_root),
        Path::new(shard_root),
        Path::new(selection_directory),
        Path::new(output_file),
        requested_rooms,
        provisional_policy,
    )?;
    println!(
        "playtest manifest written: requested={} exported={} socket-mates-added={} authored-routes={} fallback-routes={} output={}",
        summary.requested_rooms,
        summary.exported_rooms,
        summary.socket_mates_added,
        summary.authored_routes,
        summary.fallback_routes,
        output_file,
    );
    Ok(())
}

fn parse_room_workers(value: &str) -> Result<usize, Box<dyn Error>> {
    let workers = value.parse::<usize>()?;
    if matches!(workers, 1 | 2) {
        Ok(workers)
    } else {
        Err(format!("room-workers must be 1 or 2, found {workers}").into())
    }
}
