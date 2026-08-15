//! Non-overwriting CLI adapter for one seed's deep room analysis.

use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use super::{
    CandidateKeyRecord, CorpusAnalysisArtifactBundle, CorpusAnalysisArtifactRoomRef,
    CorpusBuildConfigV1, CorpusRoomAnalysis, CorpusRoomAnalysisConfig, RoomMetricSummary,
    analyze_corpus_room, evaluate_route_matrices, generate_seed_block,
    parse_and_verify_seed_analysis_artifact, render_seed_analysis_artifact, summarize_room_metrics,
};

struct AnalyzedRoom {
    generation_key: CandidateKeyRecord,
    analysis: CorpusRoomAnalysis,
    metrics: RoomMetricSummary,
}

/// Read and independently regenerate/replay a persisted deep-analysis shard.
pub fn run_verify_deep_shard_cli(output_directory: &str) -> Result<(), Box<dyn Error>> {
    let output_directory = Path::new(output_directory);
    let bundle = CorpusAnalysisArtifactBundle {
        manifest_json: fs::read(output_directory.join("manifest.json"))?,
        rooms_jsonl: fs::read(output_directory.join("rooms.jsonl"))?,
        controller_witnesses_jsonl: fs::read(output_directory.join("controller-witnesses.jsonl"))?,
    };
    let verified = parse_and_verify_seed_analysis_artifact(&bundle)?;
    println!(
        "deep shard verified: seed={} rooms={} controller-witnesses={} canonical-route-vectors={} input={}",
        verified.seed,
        verified.rooms,
        verified.retained_controller_witnesses,
        verified.canonical_route_vectors,
        output_directory.display(),
    );
    Ok(())
}

/// Generate, fully evaluate, deeply analyze, persist, and independently
/// verify one seed's gate-passing rooms.
///
/// The output directory must not already exist. A failed or interrupted run
/// never overwrites prior evidence.
pub fn run_deep_shard_cli(seed: &str, output_directory: &str) -> Result<(), Box<dyn Error>> {
    let seed = seed.parse::<u64>()?;
    let output_directory = Path::new(output_directory);
    if fs::symlink_metadata(output_directory).is_ok() {
        return Err(format!(
            "deep-analysis output already exists: {}",
            output_directory.display()
        )
        .into());
    }

    eprintln!("deep shard: generating and evaluating seed {seed}");
    let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(seed, 1))?;
    let evaluated = evaluate_route_matrices(generated)?;
    let candidates = evaluated
        .rooms
        .into_iter()
        .filter(|room| room.construction_loadout_gate_passes() && room.complete_kit_gate_passes())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(format!("seed {seed} has no construction+complete-kit passing rooms").into());
    }

    let config = CorpusRoomAnalysisConfig::default();
    let candidate_count = candidates.len();
    let mut analyzed = Vec::with_capacity(candidate_count);
    for (index, room) in candidates.into_iter().enumerate() {
        let generation_key = CandidateKeyRecord::from_staged_key(
            room.generated
                .variants
                .first()
                .expect("generated corpus rooms retain a canonical variant")
                .key,
        );
        let analysis = analyze_corpus_room(&room, &config)?;
        let metrics = summarize_room_metrics(&analysis)?;
        analyzed.push(AnalyzedRoom {
            generation_key,
            analysis,
            metrics,
        });
        eprintln!(
            "deep shard: analyzed {}/{} rooms for seed {seed}",
            index + 1,
            candidate_count
        );
    }

    let inputs = analyzed.iter().map(|room| CorpusAnalysisArtifactRoomRef {
        generation_key: &room.generation_key,
        room_id: &room.analysis.room_id,
        analysis: &room.analysis,
        metrics: &room.metrics,
    });
    let bundle = render_seed_analysis_artifact(seed, &config, inputs)?;
    let verified = parse_and_verify_seed_analysis_artifact(&bundle)?;

    fs::create_dir(output_directory)?;
    write_new(
        &output_directory.join("manifest.json"),
        &bundle.manifest_json,
    )?;
    write_new(&output_directory.join("rooms.jsonl"), &bundle.rooms_jsonl)?;
    write_new(
        &output_directory.join("controller-witnesses.jsonl"),
        &bundle.controller_witnesses_jsonl,
    )?;
    println!(
        "deep shard written and verified: seed={} rooms={} controller-witnesses={} canonical-route-vectors={} output={}",
        verified.seed,
        verified.rooms,
        verified.retained_controller_witnesses,
        verified.canonical_route_vectors,
        output_directory.display(),
    );
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
