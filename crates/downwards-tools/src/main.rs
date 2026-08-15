use std::{env, error::Error, process::ExitCode, time::Instant};

use downwards_ai::{
    ComplexityBand, DifficultyConfig, Replay, SolveOutcome, SolverConfig, analyze_solution, solve,
};
use downwards_content::first_steps_room;
use downwards_core::{AbilitySet, Room, Simulation, Tile};
use downwards_gen::{AbilityTier, LayoutFamily, generate_for_abilities};
use downwards_validation::{
    ScenarioObjective, WitnessFingerprint, validate_all_generated_pickups,
    validate_generated_scenario,
};

const USAGE: &str = "\
Downwards headless tools

USAGE:
    downwards-tools solve-first-steps [baseline|wall|dash|all]
    downwards-tools generate <seed> [baseline|wall|dash|all]
    downwards-tools validate-seeds <start-seed> <count> [baseline|wall|dash|all]
";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<ExitCode, Box<dyn Error>> {
    let Some(command) = arguments.first().map(String::as_str) else {
        print!("{USAGE}");
        return Ok(ExitCode::FAILURE);
    };

    match command {
        "solve-first-steps" => match arguments.as_slice() {
            [_] => solve_first_steps(AbilitySet::NONE),
            [_, tier] => solve_first_steps(parse_tier(tier)?.abilities()),
            _ => Err(format!("invalid solve-first-steps arguments\n\n{USAGE}").into()),
        },
        "generate" => match arguments.as_slice() {
            [_, seed] => generate_one(parse_seed(seed)?, AbilitySet::NONE),
            [_, seed, tier] => generate_one(parse_seed(seed)?, parse_tier(tier)?.abilities()),
            _ => Err(format!("invalid generate arguments\n\n{USAGE}").into()),
        },
        "validate-seeds" => match arguments.as_slice() {
            [_, start_seed, count] => validate_seeds(
                parse_seed(start_seed)?,
                parse_count(count)?,
                AbilitySet::NONE,
            ),
            [_, start_seed, count, tier] => validate_seeds(
                parse_seed(start_seed)?,
                parse_count(count)?,
                parse_tier(tier)?.abilities(),
            ),
            _ => Err(format!("invalid validate-seeds arguments\n\n{USAGE}").into()),
        },
        _ => Err(format!("unknown command {command:?}\n\n{USAGE}").into()),
    }
}

fn solve_first_steps(abilities: AbilitySet) -> Result<ExitCode, Box<dyn Error>> {
    solve_and_print(Simulation::with_abilities(first_steps_room(), abilities))
}

fn generate_one(seed: u64, abilities: AbilitySet) -> Result<ExitCode, Box<dyn Error>> {
    let generated = generate_for_abilities(seed, abilities)?;
    let metadata = &generated.metadata;
    println!(
        "generation=v{} seed={} family={:?} tier={:?} solids={} one-way={} static-hazards={} timed-hazards={} pickups={} waypoints={}",
        metadata.generation_version,
        metadata.seed,
        metadata.layout_family,
        metadata.ability_tier,
        metadata.stats.solid_tiles,
        metadata.stats.one_way_tiles,
        metadata.stats.hazard_tiles,
        metadata.stats.timed_hazards,
        metadata.stats.pickups,
        metadata.stats.route_waypoints,
    );
    print_room(&generated.room);

    let pickup_certificates = validate_all_generated_pickups(&generated)?;
    println!(
        "verified pickup targets: {0}/{0}",
        pickup_certificates.len()
    );
    for certificate in &pickup_certificates {
        let solution = certificate.solution();
        println!(
            "  pickup {:?}: {} ticks ({} nodes, {} simulated ticks, witness {})",
            certificate.objective().required_pickup_id,
            solution.replay.frames.len(),
            solution.stats.expanded_nodes,
            solution.stats.simulated_ticks,
            certificate.witness_fingerprint(),
        );
    }

    solve_and_print(Simulation::with_abilities(generated.room, abilities))
}

fn validate_seeds(
    start_seed: u64,
    count: usize,
    abilities: AbilitySet,
) -> Result<ExitCode, Box<dyn Error>> {
    if count == 0 {
        return Err("seed count must be greater than zero".into());
    }

    let started = Instant::now();
    let mut accepted = 0_usize;
    let mut exit_accepted = 0_usize;
    let mut pickup_expected = 0_usize;
    let mut pickup_accepted = 0_usize;
    let mut completion_ticks = 0_usize;
    let mut input_transitions = 0_usize;
    let mut robustness_total = 0.0_f64;
    let mut robustness_samples = 0_usize;
    let mut family_counts = [0_usize; 4];
    let mut family_accepted = [0_usize; 4];
    let mut complexity_bands = [0_usize; 3];
    let mut successful_jumps = 0_usize;
    let mut successful_wall_jumps = 0_usize;
    let mut successful_dashes = 0_usize;
    let mut completion_samples = Vec::with_capacity(count);
    let mut transition_samples = Vec::with_capacity(count);
    let mut robustness_values = Vec::with_capacity(count);
    let mut clearance_samples = Vec::with_capacity(count);
    let mut clearance_not_applicable = 0_usize;
    let mut witness_fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    let mut pickup_witness_fingerprint = new_pickup_aggregate_fingerprint();
    let mut failures = Vec::new();

    for offset in 0..count {
        let seed = start_seed.wrapping_add(offset as u64);
        let generated = generate_for_abilities(seed, abilities)?;
        let family_index = family_index(generated.metadata.layout_family);
        family_counts[family_index] += 1;
        pickup_expected += generated.room.pickups().len();
        let objective = ScenarioObjective::for_generated(&generated, "down");
        let exit_certificate = match validate_generated_scenario(generated.clone(), objective) {
            Ok(certificate) => {
                exit_accepted += 1;
                Some(certificate)
            }
            Err(error) => {
                failures.push(format_validation_failure(seed, abilities, "exit", &error));
                None
            }
        };
        let pickups_accepted_for_room = match validate_all_generated_pickups(&generated) {
            Ok(certificates) => {
                pickup_accepted += certificates.len();
                for certificate in &certificates {
                    fold_pickup_witness_fingerprint(
                        seed,
                        &certificate.objective().required_pickup_id,
                        certificate.witness_fingerprint(),
                        &mut pickup_witness_fingerprint,
                    );
                }
                true
            }
            Err(error) => {
                failures.push(format_validation_failure(seed, abilities, "pickup", &error));
                false
            }
        };

        if pickups_accepted_for_room && let Some(certificate) = exit_certificate {
            let solution = certificate.solution();
            let report = certificate.difficulty();
            accepted += 1;
            family_accepted[family_index] += 1;
            completion_ticks += report.completion_ticks;
            input_transitions += report.meaningful_input_transitions;
            complexity_bands[complexity_band_index(report.provisional_complexity.band)] += 1;
            successful_jumps += report.successful_jumps;
            successful_wall_jumps += report.successful_wall_jumps;
            successful_dashes += report.successful_dashes;
            completion_samples.push(report.completion_ticks);
            transition_samples.push(report.meaningful_input_transitions);
            if let Some(clearance) = report.minimum_hazard_clearance {
                clearance_samples.push(clearance.pixels as usize);
            } else {
                clearance_not_applicable += 1;
            }
            fold_replay_fingerprint(seed, &solution.replay, &mut witness_fingerprint);
            if let Some(ratio) = report.temporal_robustness.successful_perturbation_ratio {
                robustness_total += ratio;
                robustness_samples += 1;
                robustness_values.push(ratio);
            }
        }
    }

    let mean_completion = (accepted != 0).then(|| completion_ticks as f64 / accepted as f64);
    let mean_transitions = (accepted != 0).then(|| input_transitions as f64 / accepted as f64);
    let mean_robustness =
        (robustness_samples != 0).then(|| robustness_total / robustness_samples as f64);
    println!(
        "validated {accepted}/{count} generated scenarios from {start_seed} as {:?} in {:.2}s (required exit + every pickup)",
        AbilityTier::from_abilities(abilities),
        started.elapsed().as_secs_f64(),
    );
    println!(
        "accepted certificates: exits={exit_accepted}/{count} pickups={pickup_accepted}/{pickup_expected}"
    );
    println!(
        "families accepted/total: hazard-run={}/{} terraced={}/{} chimney={}/{} dash-gallery={}/{}",
        family_accepted[0],
        family_counts[0],
        family_accepted[1],
        family_counts[1],
        family_accepted[2],
        family_counts[2],
        family_accepted[3],
        family_counts[3]
    );
    println!(
        "means over accepted scenarios: completion={} ticks, transitions={}, temporal robustness={}",
        format_optional(mean_completion),
        format_optional(mean_transitions),
        format_optional(mean_robustness),
    );
    println!(
        "provisional complexity bands: gentle={} standard={} technical={} ({})",
        complexity_bands[0],
        complexity_bands[1],
        complexity_bands[2],
        downwards_ai::HEURISTIC_DIFFICULTY_DISCLAIMER,
    );
    println!(
        "accepted traversal events: jumps={} wall-jumps={} dashes={}",
        successful_jumps, successful_wall_jumps, successful_dashes,
    );
    println!(
        "distributions: completion [{}], transitions [{}], robustness [{}], minimum hazard clearance [{}; n/a={clearance_not_applicable}]",
        summarize_usizes(&mut completion_samples),
        summarize_usizes(&mut transition_samples),
        summarize_f64s(&mut robustness_values),
        summarize_usizes(&mut clearance_samples),
    );
    println!("verified exit witness fingerprint: {witness_fingerprint:016x}");
    println!(
        "verified pickup witness fingerprint: {pickup_witness_fingerprint:016x} ({pickup_accepted} event-aware certificates)"
    );

    if failures.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        for failure in failures.iter().take(20) {
            eprintln!("{failure}");
        }
        if failures.len() > 20 {
            eprintln!("... and {} more failures", failures.len() - 20);
        }
        Ok(ExitCode::from(2))
    }
}

fn solve_and_print(initial: Simulation) -> Result<ExitCode, Box<dyn Error>> {
    let outcome = solve(&initial, &solver_config(initial.abilities()))?;
    match outcome {
        SolveOutcome::Solved(solution) => {
            let verification = solution.replay.verify(&initial)?;
            let difficulty = analyze_solution(&initial, &solution, &DifficultyConfig::default())?;
            println!(
                "solved exit {:?} in {} ticks ({} nodes, {} simulated ticks, digest {})",
                solution.exit_id,
                difficulty.completion_ticks,
                solution.stats.expanded_nodes,
                solution.stats.simulated_ticks,
                verification.final_digest,
            );
            println!(
                "difficulty heuristics: transitions={}, jump presses={}, dash presses={}, robustness={}",
                difficulty.meaningful_input_transitions,
                difficulty.jump_presses,
                difficulty.dash_presses,
                format_optional(difficulty.temporal_robustness.successful_perturbation_ratio),
            );
            Ok(ExitCode::SUCCESS)
        }
        SolveOutcome::Inconclusive { reason, stats } => {
            eprintln!(
                "inconclusive ({reason:?}): {} nodes expanded, {} ticks simulated, deepest path {} ticks",
                stats.expanded_nodes, stats.simulated_ticks, stats.deepest_path_ticks
            );
            Ok(ExitCode::from(2))
        }
    }
}

fn solver_config(abilities: AbilitySet) -> SolverConfig {
    SolverConfig::for_abilities(abilities)
}

fn parse_seed(value: &str) -> Result<u64, Box<dyn Error>> {
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.map_err(|error| format!("invalid seed {value:?}: {error}").into())
}

fn parse_count(value: &str) -> Result<usize, Box<dyn Error>> {
    value
        .parse()
        .map_err(|error| format!("invalid count {value:?}: {error}").into())
}

fn parse_tier(value: &str) -> Result<AbilityTier, Box<dyn Error>> {
    match value {
        "baseline" | "none" => Ok(AbilityTier::Baseline),
        "wall" | "wall-jump" => Ok(AbilityTier::WallJump),
        "dash" => Ok(AbilityTier::Dash),
        "all" | "wall-dash" => Ok(AbilityTier::WallJumpAndDash),
        _ => Err(
            format!("unknown ability tier {value:?}; expected baseline, wall, dash, or all").into(),
        ),
    }
}

const fn tier_cli_name(abilities: AbilitySet) -> &'static str {
    match AbilityTier::from_abilities(abilities) {
        AbilityTier::Baseline => "baseline",
        AbilityTier::WallJump => "wall",
        AbilityTier::Dash => "dash",
        AbilityTier::WallJumpAndDash => "all",
    }
}

fn format_validation_failure(
    seed: u64,
    abilities: AbilitySet,
    target_kind: &str,
    error: &impl std::fmt::Display,
) -> String {
    format!(
        "seed {seed}: {target_kind} certificate failed: {error}; reproduce with: downwards-tools generate {seed} {}",
        tier_cli_name(abilities),
    )
}

fn family_index(family: LayoutFamily) -> usize {
    match family {
        LayoutFamily::HazardRun => 0,
        LayoutFamily::TerracedAscent => 1,
        LayoutFamily::Chimney => 2,
        LayoutFamily::DashGallery => 3,
    }
}

const fn complexity_band_index(band: ComplexityBand) -> usize {
    match band {
        ComplexityBand::Gentle => 0,
        ComplexityBand::Standard => 1,
        ComplexityBand::Technical => 2,
    }
}

fn print_room(room: &Room) {
    for y in 0..room.height() {
        let row: String = (0..room.width())
            .map(
                |x| match room.tile(x, y).expect("coordinates are in room") {
                    Tile::Empty => '.',
                    Tile::Solid => '#',
                    Tile::OneWay => '=',
                    Tile::Hazard => '^',
                },
            )
            .collect();
        println!("{row}");
    }
}

fn format_optional(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format!("{value:.3}"))
}

fn summarize_usizes(values: &mut [usize]) -> String {
    if values.is_empty() {
        return "n/a".to_owned();
    }
    values.sort_unstable();
    format!(
        "min={} p50={} p95={} max={}",
        values[0],
        values[quantile_index(values.len(), 50)],
        values[quantile_index(values.len(), 95)],
        values[values.len() - 1],
    )
}

fn summarize_f64s(values: &mut [f64]) -> String {
    if values.is_empty() {
        return "n/a".to_owned();
    }
    values.sort_by(f64::total_cmp);
    format!(
        "min={:.3} p50={:.3} p95={:.3} max={:.3}",
        values[0],
        values[quantile_index(values.len(), 50)],
        values[quantile_index(values.len(), 95)],
        values[values.len() - 1],
    )
}

const fn quantile_index(len: usize, percentile: usize) -> usize {
    (len - 1) * percentile / 100
}

fn fold_replay_fingerprint(seed: u64, replay: &Replay, aggregate: &mut u64) {
    fold_u64(aggregate, seed);
    fold_u64(aggregate, replay.initial_digest.0);
    fold_u64(aggregate, replay.frames.len() as u64);
    for frame in &replay.frames {
        let action = frame.action;
        fold_byte(aggregate, action.move_x as u8);
        fold_byte(aggregate, action.move_y as u8);
        fold_byte(aggregate, u8::from(action.jump));
        fold_byte(aggregate, u8::from(action.dash));
        fold_byte(aggregate, u8::from(action.restart));
        fold_u64(aggregate, frame.expected_digest.0);
        fold_u64(aggregate, frame.expected_event_digest.0);
    }
}

fn new_pickup_aggregate_fingerprint() -> u64 {
    let mut aggregate = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in b"downwards-tools-pickup-witness-aggregate-v1" {
        fold_byte(&mut aggregate, byte);
    }
    aggregate
}

fn fold_pickup_witness_fingerprint(
    seed: u64,
    pickup_id: &str,
    witness: WitnessFingerprint,
    aggregate: &mut u64,
) {
    fold_u64(aggregate, seed);
    fold_u64(aggregate, pickup_id.len() as u64);
    for byte in pickup_id.bytes() {
        fold_byte(aggregate, byte);
    }
    // Validation witness fingerprints cover the exact action, state-digest,
    // and event-digest stream. Folding that versioned value retains those
    // semantics while also binding the batch to seed and declaration order.
    fold_u64(aggregate, witness.as_u64());
}

fn fold_u64(aggregate: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        fold_byte(aggregate, byte);
    }
}

fn fold_byte(aggregate: &mut u64, value: u8) {
    *aggregate = (*aggregate ^ u64::from(value)).wrapping_mul(0x100_0000_01b3);
}

#[cfg(test)]
mod tests {
    use downwards_ai::{EventDigest, ReplayFrame};
    use downwards_core::{Action, StateDigest};
    use downwards_validation::fingerprint_pickup_witness;

    use super::*;

    #[test]
    fn aggregate_witness_fingerprint_covers_event_digests() {
        let mut replay = Replay {
            initial_digest: StateDigest(1),
            frames: vec![ReplayFrame {
                action: Action::default(),
                expected_digest: StateDigest(2),
                expected_event_digest: EventDigest(3),
            }],
        };
        let mut original = 0xcbf2_9ce4_8422_2325;
        fold_replay_fingerprint(4, &replay, &mut original);

        replay.frames[0].expected_event_digest = EventDigest(5);
        let mut changed = 0xcbf2_9ce4_8422_2325;
        fold_replay_fingerprint(4, &replay, &mut changed);

        assert_ne!(original, changed);
    }

    #[test]
    fn aggregate_pickup_fingerprint_is_deterministic_and_event_aware() {
        let generated = generate_for_abilities(1, AbilitySet::NONE).unwrap();
        let certificates = validate_all_generated_pickups(&generated).unwrap();
        let certificate = certificates.first().expect("generator defines a pickup");
        let pickup_id = &certificate.objective().required_pickup_id;

        let mut first = new_pickup_aggregate_fingerprint();
        fold_pickup_witness_fingerprint(
            generated.metadata.seed,
            pickup_id,
            certificate.witness_fingerprint(),
            &mut first,
        );
        let mut repeated = new_pickup_aggregate_fingerprint();
        fold_pickup_witness_fingerprint(
            generated.metadata.seed,
            pickup_id,
            certificate.witness_fingerprint(),
            &mut repeated,
        );
        assert_eq!(first, repeated);

        let mut tampered_solution = certificate.solution().clone();
        tampered_solution.replay.frames[0].expected_event_digest.0 ^= 1;
        let tampered_witness = fingerprint_pickup_witness(
            certificate.generated_level(),
            certificate.objective(),
            &tampered_solution,
        );
        let mut changed = new_pickup_aggregate_fingerprint();
        fold_pickup_witness_fingerprint(
            generated.metadata.seed,
            pickup_id,
            tampered_witness,
            &mut changed,
        );

        assert_ne!(first, changed);
    }

    #[test]
    fn aggregate_pickup_fingerprint_covers_seed_and_pickup_id() {
        let generated = generate_for_abilities(0, AbilitySet::NONE).unwrap();
        let certificate = validate_all_generated_pickups(&generated)
            .unwrap()
            .remove(0);

        let aggregate = |seed, pickup_id: &str| {
            let mut value = new_pickup_aggregate_fingerprint();
            fold_pickup_witness_fingerprint(
                seed,
                pickup_id,
                certificate.witness_fingerprint(),
                &mut value,
            );
            value
        };

        assert_ne!(aggregate(0, "coin-a"), aggregate(1, "coin-a"));
        assert_ne!(aggregate(0, "coin-a"), aggregate(0, "coin-b"));
    }
}
