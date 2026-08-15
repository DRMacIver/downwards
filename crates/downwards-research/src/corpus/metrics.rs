//! Exact and noisy measurements for one replay-certified directed door route.

use std::{error::Error, fmt};

use downwards_ai::{
    DifficultyConfig, DifficultyError, DifficultyReport, ReachedTarget, ShakyHandConfig,
    ShakyHandError, ShakyHandReport, Solution, TargetSolution, analyze_solution,
    evaluate_shaky_hand,
};
use downwards_core::{DoorEntryError, Simulation};
use downwards_gen::GeneratedLevel;
use downwards_lab::{
    LandingPrecisionError, LandingPrecisionReport, RouteDifficultyVector,
    RouteDifficultyVectorError, SuccessfulWitnessObservation, TraversalGrid,
    WitnessObservationError, analyze_replay_landings, observe_solution, route_difficulty_vector,
};
use downwards_validation::{ReplayCertifiedTargetEvidence, WitnessFingerprint};

use super::EvaluationLoadout;

pub const CORPUS_ROUTE_MEASUREMENT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct CorpusRouteMeasurement {
    pub version: u32,
    pub source_door_id: String,
    pub target_door_id: String,
    pub loadout: EvaluationLoadout,
    pub witness_fingerprint: WitnessFingerprint,
    pub observation: SuccessfulWitnessObservation,
    /// Exact support overlap and signed edge margins for authoritative
    /// landings through first target contact. This is geometric evidence, not
    /// a claim about a human input window.
    pub landing_precision: LandingPrecisionReport,
    pub exact_difficulty: DifficultyReport,
    pub shaky_hand: Option<ShakyHandReport>,
    pub vector: RouteDifficultyVector,
}

/// Measure one exact positive door witness under the loadout in which it was
/// certified. Added abilities are never substituted because they can change
/// the trajectory produced by identical buttons.
pub fn measure_door_route(
    generated: &GeneratedLevel,
    source_door_id: &str,
    loadout: EvaluationLoadout,
    evidence: &ReplayCertifiedTargetEvidence,
    difficulty_config: &DifficultyConfig,
    shaky_hand_config: Option<&ShakyHandConfig>,
) -> Result<CorpusRouteMeasurement, CorpusRouteMeasurementError> {
    let target_solution = evidence.solution();
    let target_door_id = match &target_solution.reached {
        ReachedTarget::Door(id) => id.clone(),
        reached => {
            return Err(CorpusRouteMeasurementError::NotDoorTarget {
                reached: reached.clone(),
            });
        }
    };
    let initial =
        Simulation::enter_via_door(generated.room.clone(), loadout.abilities(), source_door_id)
            .map_err(CorpusRouteMeasurementError::DoorEntry)?;
    let solution = door_solution(target_solution, &target_door_id);
    let observation = observe_solution(&initial, &solution, TraversalGrid::default())
        .map_err(CorpusRouteMeasurementError::Observation)?;
    let landing_precision = analyze_replay_landings(
        &initial,
        &target_solution.replay,
        observation.completion_ticks,
    )
    .map_err(CorpusRouteMeasurementError::LandingPrecision)?;
    let exact_difficulty = analyze_solution(&initial, &solution, difficulty_config)
        .map_err(CorpusRouteMeasurementError::Difficulty)?;
    let shaky_hand = shaky_hand_config
        .map(|config| evaluate_shaky_hand(&initial, target_solution, *config))
        .transpose()
        .map_err(CorpusRouteMeasurementError::ShakyHand)?;
    let vector = route_difficulty_vector(&observation, &exact_difficulty, shaky_hand.as_ref())
        .map_err(CorpusRouteMeasurementError::Vector)?;
    Ok(CorpusRouteMeasurement {
        version: CORPUS_ROUTE_MEASUREMENT_VERSION,
        source_door_id: source_door_id.to_owned(),
        target_door_id,
        loadout,
        witness_fingerprint: evidence.witness_fingerprint(),
        observation,
        landing_precision,
        exact_difficulty,
        shaky_hand,
        vector,
    })
}

fn door_solution(target: &TargetSolution, target_door_id: &str) -> Solution {
    Solution {
        exit_id: target_door_id.to_owned(),
        replay: target.replay.clone(),
        stats: target.stats,
    }
}

#[derive(Debug)]
pub enum CorpusRouteMeasurementError {
    NotDoorTarget { reached: ReachedTarget },
    DoorEntry(DoorEntryError),
    Observation(WitnessObservationError),
    LandingPrecision(LandingPrecisionError),
    Difficulty(DifficultyError),
    ShakyHand(ShakyHandError),
    Vector(RouteDifficultyVectorError),
}

impl fmt::Display for CorpusRouteMeasurementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDoorTarget { reached } => {
                write!(formatter, "corpus door measurement received {reached:?}")
            }
            Self::DoorEntry(error) => write!(formatter, "could not enter source door: {error}"),
            Self::Observation(error) => write!(formatter, "could not observe witness: {error}"),
            Self::LandingPrecision(error) => {
                write!(formatter, "could not measure landing geometry: {error}")
            }
            Self::Difficulty(error) => write!(formatter, "could not measure exact route: {error}"),
            Self::ShakyHand(error) => write!(formatter, "could not run shaky-hand study: {error}"),
            Self::Vector(error) => write!(formatter, "could not assemble route vector: {error}"),
        }
    }
}

impl Error for CorpusRouteMeasurementError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NotDoorTarget { .. } => None,
            Self::DoorEntry(error) => Some(error),
            Self::Observation(error) => Some(error),
            Self::LandingPrecision(error) => Some(error),
            Self::Difficulty(error) => Some(error),
            Self::ShakyHand(error) => Some(error),
            Self::Vector(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{DifficultyConfig, ShakyHandConfig};
    use downwards_gen::{
        CompositionalFeatureSet, CompositionalKey, CompositionalProfile, StagedCompositionalKey,
        experimental::{ChallengeIntent, GenerationStrategy},
        generate_staged_compositional,
    };
    use downwards_validation::{
        BoundedTargetEvidence, ValidationConfig, evaluate_generated_door_targets_for_loadout,
    };

    use super::*;

    fn positive_fixture() -> (
        downwards_gen::StagedCompositionalCandidate,
        String,
        ReplayCertifiedTargetEvidence,
    ) {
        let loadout = EvaluationLoadout::Both;
        let key = StagedCompositionalKey::new(
            CompositionalKey::new(
                0,
                CompositionalProfile::new(
                    loadout.abilities(),
                    GenerationStrategy::CyclicGraph,
                    ChallengeIntent::Gentle,
                ),
            ),
            CompositionalFeatureSet::TerrainOnly,
        );
        let candidate = generate_staged_compositional(key).unwrap();
        let matrix = evaluate_generated_door_targets_for_loadout(
            &candidate.generated,
            loadout.abilities(),
            &ValidationConfig::for_loadout(loadout.abilities()),
        )
        .unwrap();
        let row = matrix
            .door_routes()
            .iter()
            .find_map(|row| match &row.evidence {
                BoundedTargetEvidence::Positive(evidence) => {
                    Some((row.source_door_id.clone(), evidence.clone()))
                }
                BoundedTargetEvidence::Inconclusive(_) => None,
            })
            .unwrap();
        (candidate, row.0, row.1)
    }

    #[test]
    fn exact_and_shaky_measurements_are_bound_to_one_certified_route() {
        let (candidate, source, evidence) = positive_fixture();
        let config = ShakyHandConfig {
            trials_per_curve_point: 4,
            ..ShakyHandConfig::default()
        };
        let first = measure_door_route(
            &candidate.generated,
            &source,
            EvaluationLoadout::Both,
            &evidence,
            &DifficultyConfig::default(),
            Some(&config),
        )
        .unwrap();
        let second = measure_door_route(
            &candidate.generated,
            &source,
            EvaluationLoadout::Both,
            &evidence,
            &DifficultyConfig::default(),
            Some(&config),
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.source_door_id, source);
        assert_eq!(first.vector.target_id, first.target_door_id);
        assert_eq!(
            first.landing_precision.inspected_ticks,
            first.observation.completion_ticks
        );
        assert_eq!(
            first.landing_precision.landing_event_count,
            first.landing_precision.samples.len()
                + first.landing_precision.unmeasured_landing_ticks.len()
        );
        assert!(first.shaky_hand.is_some());
        assert_eq!(
            first.vector.operational_solver_cost.simulated_ticks,
            evidence.solution().stats.simulated_ticks
        );
    }
}
