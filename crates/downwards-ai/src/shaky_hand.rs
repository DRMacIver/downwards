//! Deterministic noisy-replay studies for exact solver witnesses.
//!
//! A [`ShakyHandStudy`] records every seeded perturbation schedule before it
//! is evaluated. This makes a result reproducible without relying on an
//! implementation-defined random-number generator. Each scheduled action is
//! still passed, one tick at a time, to the authoritative
//! [`Simulation::step`] implementation.
//!
//! The evaluator measures blind continuation of the recorded controller. It
//! can report an exact state-digest convergence after a divergence, but it
//! does not replan from perturbed states. [`ReplanningSupport`] makes that
//! limitation explicit. A timeout or other failed noisy trial is evidence
//! about that input schedule only and is never evidence that a target is
//! unreachable.

use std::{error::Error, fmt};

use downwards_core::{Action, DeathReason, Simulation, SimulationEvent, StateDigest};

use crate::{ReachedTarget, Replay, ReplayDivergence, SearchTarget, TargetSolution};

/// Version of schedule generation, edit application, and outcome policy.
pub const SHAKY_HAND_POLICY_VERSION: u32 = 1;

/// Version of the stable [`ShakyHandConfig`] identity encoding.
pub const SHAKY_HAND_CONFIG_VERSION: u32 = 1;

/// Stable strengths used by boundary and correlated timing curves.
pub const SHAKY_HAND_TIMING_STRENGTHS: [u8; 3] = [1, 2, 4];

/// Reminder attached to reports and suitable for persisted evidence schemas.
pub const SHAKY_HAND_EVIDENCE_DISCLAIMER: &str =
    "bounded noisy replay outcomes are not proof that a target is unreachable";

/// Controls deterministic schedule generation and blind-continuation study.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShakyHandConfig {
    /// Root seed from which every curve/trial seed is independently derived.
    pub seed: u64,
    /// Minimum trials for each non-zero noise family/strength point. Boundary
    /// timing curves grow beyond this minimum when needed to shift every
    /// applicable boundary in both directions.
    pub trials_per_curve_point: usize,
    /// Number of final-action ticks allowed after a scheduled stream ends.
    pub grace_ticks: usize,
    /// Consecutive action boundaries shifted by correlated timing noise.
    pub correlated_boundaries: usize,
    /// Required exact matching suffix after divergence to report convergence.
    pub convergence_confirmation_ticks: usize,
}

impl Default for ShakyHandConfig {
    fn default() -> Self {
        Self {
            seed: 0x51a7_5eed_d15c_a11e,
            trials_per_curve_point: 64,
            grace_ticks: 12,
            correlated_boundaries: 3,
            convergence_confirmation_ticks: 2,
        }
    }
}

impl ShakyHandConfig {
    fn validate(self) -> Result<(), ShakyHandError> {
        if self.trials_per_curve_point == 0 {
            return Err(ShakyHandError::InvalidConfig(
                ShakyHandConfigError::ZeroTrialsPerCurvePoint,
            ));
        }
        if self.correlated_boundaries < 2 {
            return Err(ShakyHandError::InvalidConfig(
                ShakyHandConfigError::CorrelatedBoundariesLessThanTwo,
            ));
        }
        if self.convergence_confirmation_ticks == 0 {
            return Err(ShakyHandError::InvalidConfig(
                ShakyHandConfigError::ZeroConvergenceConfirmationTicks,
            ));
        }
        Ok(())
    }
}

/// Invalid study settings are reported before any schedule is generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShakyHandConfigError {
    ZeroTrialsPerCurvePoint,
    CorrelatedBoundariesLessThanTwo,
    ZeroConvergenceConfirmationTicks,
}

impl fmt::Display for ShakyHandConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroTrialsPerCurvePoint => {
                write!(
                    formatter,
                    "trials_per_curve_point must be greater than zero"
                )
            }
            Self::CorrelatedBoundariesLessThanTwo => {
                write!(formatter, "correlated_boundaries must be at least two")
            }
            Self::ZeroConvergenceConfirmationTicks => write!(
                formatter,
                "convergence_confirmation_ticks must be greater than zero"
            ),
        }
    }
}

impl Error for ShakyHandConfigError {}

/// Versioned identity of one recorded noisy-replay study.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShakyHandStudyIdentity {
    pub policy_version: u32,
    pub config_version: u32,
    /// Stable digest of every setting other than the separately visible seed.
    pub config_digest: u64,
    pub seed: u64,
}

/// Independent perturbation families. Curves are never collapsed into one
/// difficulty score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoiseFamily {
    /// The exact replay, included as an authoritative zero-noise control.
    Exact,
    /// One semantic-action boundary moved early or late.
    BoundaryTiming,
    /// Several consecutive boundaries moved in the same direction.
    CorrelatedTiming,
    /// One control held a frame too long or released a frame too early.
    HoldRelease,
    /// One complete semantic input frame dropped or repeated.
    DropRepeatFrame,
}

impl NoiseFamily {
    const fn tag(self) -> u8 {
        match self {
            Self::Exact => 0,
            Self::BoundaryTiming => 1,
            Self::CorrelatedTiming => 2,
            Self::HoldRelease => 3,
            Self::DropRepeatFrame => 4,
        }
    }
}

/// One semantic input channel changed by a hold/release error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticControl {
    Horizontal,
    Vertical,
    Jump,
    Dash,
}

/// A fully recorded edit. Replay ticks are one-based throughout this API.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PerturbationEdit {
    ShiftBoundary {
        replay_tick: usize,
        /// Negative is early; positive is late.
        offset_ticks: i8,
    },
    ShiftCorrelatedBoundaries {
        replay_ticks: Vec<usize>,
        /// Negative is early; positive is late.
        offset_ticks: i8,
    },
    HoldControlOneFrame {
        replay_tick: usize,
        control: SemanticControl,
    },
    ReleaseControlOneFrameEarly {
        replay_tick: usize,
        control: SemanticControl,
    },
    DropSemanticFrame {
        replay_tick: usize,
    },
    RepeatSemanticFrame {
        replay_tick: usize,
    },
}

/// Seed and edits for one deterministic trial.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PerturbationSchedule {
    pub trial_index: usize,
    pub schedule_seed: u64,
    pub edits: Vec<PerturbationEdit>,
}

/// Recorded schedules for one point on a family-specific success curve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedNoiseCurve {
    pub family: NoiseFamily,
    /// Maximum timing displacement. Hold/release and drop/repeat use one.
    pub strength_ticks: u8,
    pub requested_trials: usize,
    /// Requested trials for which no valid edit exists on this replay.
    pub not_applicable_trials: usize,
    pub schedules: Vec<PerturbationSchedule>,
}

/// Reusable, replay-bound perturbation schedules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShakyHandStudy {
    pub identity: ShakyHandStudyIdentity,
    /// Stable fingerprint of the complete exact replay, including digests.
    pub replay_fingerprint: u64,
    pub config: ShakyHandConfig,
    pub curves: Vec<RecordedNoiseCurve>,
}

/// Generate and record every schedule without executing a simulation.
pub fn record_shaky_hand_study(
    replay: &Replay,
    config: ShakyHandConfig,
) -> Result<ShakyHandStudy, ShakyHandError> {
    config.validate()?;
    let identity = ShakyHandStudyIdentity {
        policy_version: SHAKY_HAND_POLICY_VERSION,
        config_version: SHAKY_HAND_CONFIG_VERSION,
        config_digest: config_digest(config),
        seed: config.seed,
    };
    let actions: Vec<_> = replay.actions().collect();
    let transitions = transition_indices(&actions);
    let mut curves = Vec::with_capacity(9);

    curves.push(RecordedNoiseCurve {
        family: NoiseFamily::Exact,
        strength_ticks: 0,
        requested_trials: 1,
        not_applicable_trials: 0,
        schedules: vec![PerturbationSchedule {
            trial_index: 0,
            schedule_seed: derive_schedule_seed(config.seed, NoiseFamily::Exact, 0, 0),
            edits: Vec::new(),
        }],
    });

    for strength_ticks in SHAKY_HAND_TIMING_STRENGTHS {
        let candidates = boundary_candidates(&actions, &transitions, strength_ticks);
        curves.push(record_curve(
            config,
            NoiseFamily::BoundaryTiming,
            strength_ticks,
            candidates,
            true,
        ));
    }
    for strength_ticks in SHAKY_HAND_TIMING_STRENGTHS {
        let candidates = correlated_candidates(
            &actions,
            &transitions,
            strength_ticks,
            config.correlated_boundaries,
        );
        curves.push(record_curve(
            config,
            NoiseFamily::CorrelatedTiming,
            strength_ticks,
            candidates,
            false,
        ));
    }
    curves.push(record_curve(
        config,
        NoiseFamily::HoldRelease,
        1,
        hold_release_candidates(&actions),
        false,
    ));
    curves.push(record_curve(
        config,
        NoiseFamily::DropRepeatFrame,
        1,
        drop_repeat_candidates(&actions),
        false,
    ));

    Ok(ShakyHandStudy {
        identity,
        replay_fingerprint: replay_fingerprint(replay),
        config,
        curves,
    })
}

fn record_curve(
    config: ShakyHandConfig,
    family: NoiseFamily,
    strength_ticks: u8,
    candidates: Vec<PerturbationEdit>,
    exhaustive: bool,
) -> RecordedNoiseCurve {
    let requested_trials = if exhaustive {
        config.trials_per_curve_point.max(candidates.len())
    } else {
        config.trials_per_curve_point
    };
    if candidates.is_empty() {
        return RecordedNoiseCurve {
            family,
            strength_ticks,
            requested_trials,
            not_applicable_trials: requested_trials,
            schedules: Vec::new(),
        };
    }

    let curve_seed = derive_schedule_seed(config.seed, family, strength_ticks, usize::MAX);
    let candidate_offset = usize::try_from(
        splitmix64(curve_seed) % u64::try_from(candidates.len()).unwrap_or(u64::MAX),
    )
    .unwrap_or(0);
    let schedules = (0..requested_trials)
        .map(|trial_index| {
            let schedule_seed =
                derive_schedule_seed(config.seed, family, strength_ticks, trial_index);
            // A seed-rotated cycle avoids accidental duplicate-only samples.
            // Exhaustive boundary curves contain at least one full cycle.
            let candidate_index = candidate_offset.wrapping_add(trial_index) % candidates.len();
            PerturbationSchedule {
                trial_index,
                schedule_seed,
                edits: vec![candidates[candidate_index].clone()],
            }
        })
        .collect();
    RecordedNoiseCurve {
        family,
        strength_ticks,
        requested_trials,
        not_applicable_trials: 0,
        schedules,
    }
}

/// Convenience entry point that records schedules and immediately evaluates
/// them against an exact target witness.
pub fn evaluate_shaky_hand(
    initial: &Simulation,
    solution: &TargetSolution,
    config: ShakyHandConfig,
) -> Result<ShakyHandReport, ShakyHandError> {
    let study = record_shaky_hand_study(&solution.replay, config)?;
    evaluate_recorded_shaky_hand_study(initial, solution, &study)
}

/// Evaluate an already-recorded schedule set using authoritative simulation
/// ticks. This is the reproducibility entry point for persisted studies.
pub fn evaluate_recorded_shaky_hand_study(
    initial: &Simulation,
    solution: &TargetSolution,
    study: &ShakyHandStudy,
) -> Result<ShakyHandReport, ShakyHandError> {
    validate_study(solution, study)?;
    solution.replay.verify(initial)?;
    validate_exact_target_solution(initial, solution)?;

    let mut curves = Vec::with_capacity(study.curves.len());
    for recorded_curve in &study.curves {
        let mut trials = Vec::with_capacity(recorded_curve.schedules.len());
        for schedule in &recorded_curve.schedules {
            let scheduled_actions = apply_schedule(&solution.replay, schedule)?;
            trials.push(run_trial(
                initial,
                solution,
                recorded_curve,
                schedule,
                &scheduled_actions,
                study.config,
            ));
        }
        curves.push(summarize_curve(recorded_curve, trials));
    }

    let exact_control_succeeded = curves
        .first()
        .and_then(|curve| curve.trials_detail.first())
        .is_some_and(|trial| trial.outcome.succeeded());
    if !exact_control_succeeded {
        return Err(ShakyHandError::ExactControlFailed);
    }

    let first_divergence = curves
        .iter()
        .filter_map(|curve| curve.first_divergence.as_ref())
        .min_by_key(|diagnostic| diagnostic_sort_key(diagnostic))
        .cloned();
    let first_failure = curves
        .iter()
        .filter_map(|curve| curve.first_failure.as_ref())
        .min_by_key(|diagnostic| failure_sort_key(diagnostic))
        .cloned();

    Ok(ShakyHandReport {
        interpretation: ShakyHandInterpretation::NoisyReplayHeuristic,
        evidence_disclaimer: SHAKY_HAND_EVIDENCE_DISCLAIMER,
        study: study.clone(),
        target: solution.target.clone(),
        exact_reached: solution.reached.clone(),
        replanning: ReplanningSupport::Unsupported,
        exact_control_succeeded,
        first_divergence,
        first_failure,
        curves,
    })
}

fn validate_study(solution: &TargetSolution, study: &ShakyHandStudy) -> Result<(), ShakyHandError> {
    if study.identity.policy_version != SHAKY_HAND_POLICY_VERSION {
        return Err(ShakyHandError::UnsupportedPolicyVersion {
            actual: study.identity.policy_version,
            supported: SHAKY_HAND_POLICY_VERSION,
        });
    }
    if study.identity.config_version != SHAKY_HAND_CONFIG_VERSION {
        return Err(ShakyHandError::UnsupportedConfigVersion {
            actual: study.identity.config_version,
            supported: SHAKY_HAND_CONFIG_VERSION,
        });
    }
    study.config.validate()?;
    let actual_config_digest = config_digest(study.config);
    if study.identity.seed != study.config.seed
        || study.identity.config_digest != actual_config_digest
    {
        return Err(ShakyHandError::ConfigIdentityMismatch {
            expected_seed: study.identity.seed,
            actual_seed: study.config.seed,
            expected_digest: study.identity.config_digest,
            actual_digest: actual_config_digest,
        });
    }
    let actual = replay_fingerprint(&solution.replay);
    if study.replay_fingerprint != actual {
        return Err(ShakyHandError::ReplayFingerprintMismatch {
            expected: study.replay_fingerprint,
            actual,
        });
    }
    Ok(())
}

fn validate_exact_target_solution(
    initial: &Simulation,
    solution: &TargetSolution,
) -> Result<(), ShakyHandError> {
    let expected = expected_reached_target(&solution.target)?;
    if solution.reached != expected {
        return Err(ShakyHandError::TargetSolutionMismatch {
            target: solution.target.clone(),
            declared_reached: solution.reached.clone(),
        });
    }

    let mut simulation = initial.clone();
    if target_reached(&simulation, &expected) {
        if solution.replay.frames.is_empty() {
            return Ok(());
        }
        return Err(ShakyHandError::ExactReplayHasTrailingFrames {
            completion_tick: 0,
            replay_frames: solution.replay.frames.len(),
        });
    }
    for (frame_index, frame) in solution.replay.frames.iter().enumerate() {
        simulation.step(frame.action);
        if target_reached(&simulation, &expected) {
            if frame_index + 1 != solution.replay.frames.len() {
                return Err(ShakyHandError::ExactReplayHasTrailingFrames {
                    completion_tick: frame_index + 1,
                    replay_frames: solution.replay.frames.len(),
                });
            }
            return Ok(());
        }
        if simulation.reached_exit().is_some() {
            break;
        }
    }
    Err(ShakyHandError::ExactReplayDidNotReachTarget {
        expected,
        actual_terminal: reached_exit_target(&simulation),
    })
}

fn expected_reached_target(target: &SearchTarget) -> Result<ReachedTarget, ShakyHandError> {
    match target {
        SearchTarget::AnyExit => Err(ShakyHandError::TargetIsNotExact),
        SearchTarget::Exit(id) => Ok(ReachedTarget::Exit(id.clone())),
        SearchTarget::Door(id) => Ok(ReachedTarget::Door(id.clone())),
        SearchTarget::Pickup(id) => Ok(ReachedTarget::Pickup(id.clone())),
    }
}

/// Why an exact witness could not be studied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShakyHandError {
    InvalidConfig(ShakyHandConfigError),
    ReplayDiverged(ReplayDivergence),
    TargetIsNotExact,
    TargetSolutionMismatch {
        target: SearchTarget,
        declared_reached: ReachedTarget,
    },
    ExactReplayDidNotReachTarget {
        expected: ReachedTarget,
        actual_terminal: Option<ReachedTarget>,
    },
    ExactReplayHasTrailingFrames {
        completion_tick: usize,
        replay_frames: usize,
    },
    UnsupportedPolicyVersion {
        actual: u32,
        supported: u32,
    },
    UnsupportedConfigVersion {
        actual: u32,
        supported: u32,
    },
    ReplayFingerprintMismatch {
        expected: u64,
        actual: u64,
    },
    ConfigIdentityMismatch {
        expected_seed: u64,
        actual_seed: u64,
        expected_digest: u64,
        actual_digest: u64,
    },
    MalformedSchedule {
        trial_index: usize,
        reason: MalformedScheduleReason,
    },
    ExactControlFailed,
}

impl fmt::Display for ShakyHandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(error) => write!(formatter, "invalid shaky-hand config: {error}"),
            Self::ReplayDiverged(error) => {
                write!(
                    formatter,
                    "cannot evaluate a divergent exact replay: {error}"
                )
            }
            Self::TargetIsNotExact => write!(
                formatter,
                "shaky-hand evaluation requires an exact exit, door, or pickup target"
            ),
            Self::TargetSolutionMismatch {
                target,
                declared_reached,
            } => write!(
                formatter,
                "target solution for {target:?} declares incompatible result {declared_reached:?}"
            ),
            Self::ExactReplayDidNotReachTarget {
                expected,
                actual_terminal,
            } => write!(
                formatter,
                "exact replay did not reach {expected:?}; terminal trigger was {actual_terminal:?}"
            ),
            Self::ExactReplayHasTrailingFrames {
                completion_tick,
                replay_frames,
            } => write!(
                formatter,
                "exact target was first reached at tick {completion_tick}, but the replay has {replay_frames} frames"
            ),
            Self::UnsupportedPolicyVersion { actual, supported } => write!(
                formatter,
                "shaky-hand policy version {actual} is unsupported; this build supports {supported}"
            ),
            Self::UnsupportedConfigVersion { actual, supported } => write!(
                formatter,
                "shaky-hand config version {actual} is unsupported; this build supports {supported}"
            ),
            Self::ReplayFingerprintMismatch { expected, actual } => write!(
                formatter,
                "recorded study belongs to replay {expected:016x}, not {actual:016x}"
            ),
            Self::ConfigIdentityMismatch {
                expected_seed,
                actual_seed,
                expected_digest,
                actual_digest,
            } => write!(
                formatter,
                "recorded config identity seed/digest {expected_seed:016x}/{expected_digest:016x} does not match config {actual_seed:016x}/{actual_digest:016x}"
            ),
            Self::MalformedSchedule {
                trial_index,
                reason,
            } => write!(
                formatter,
                "malformed perturbation schedule for trial {trial_index}: {reason}"
            ),
            Self::ExactControlFailed => write!(
                formatter,
                "the recorded zero-noise control unexpectedly failed its exact target"
            ),
        }
    }
}

impl Error for ShakyHandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfig(error) => Some(error),
            Self::ReplayDiverged(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ReplayDivergence> for ShakyHandError {
    fn from(value: ReplayDivergence) -> Self {
        Self::ReplayDiverged(value)
    }
}

/// Why a persisted edit cannot be applied to its replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformedScheduleReason {
    TickOutOfBounds,
    OffsetIsZero,
    BoundaryWouldLeaveReplay,
    BoundaryNotFound,
    CorrelatedBoundariesNotIncreasing,
    SemanticControlDidNotChange,
    RestartFrameCannotBeDroppedOrRepeated,
}

impl fmt::Display for MalformedScheduleReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TickOutOfBounds => "replay tick is out of bounds",
            Self::OffsetIsZero => "boundary offset must not be zero",
            Self::BoundaryWouldLeaveReplay => "shifted boundary would leave or cross the replay",
            Self::BoundaryNotFound => "recorded tick is not an input boundary",
            Self::CorrelatedBoundariesNotIncreasing => {
                "correlated boundary ticks must be strictly increasing"
            }
            Self::SemanticControlDidNotChange => {
                "selected semantic control does not change at that boundary"
            }
            Self::RestartFrameCannotBeDroppedOrRepeated => {
                "restart frames cannot be dropped or repeated"
            }
        };
        formatter.write_str(message)
    }
}

/// Interpretation marker: results are deterministic controller diagnostics,
/// not calibrated human success probabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShakyHandInterpretation {
    NoisyReplayHeuristic,
}

/// Controller support represented by this first evaluator slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplanningSupport {
    /// The perturbed state receives the remainder of the recorded action
    /// stream; no solver or policy is asked to recover dynamically.
    Unsupported,
}

/// Complete family-specific noisy-replay result.
#[derive(Clone, Debug, PartialEq)]
pub struct ShakyHandReport {
    pub interpretation: ShakyHandInterpretation,
    pub evidence_disclaimer: &'static str,
    pub study: ShakyHandStudy,
    pub target: SearchTarget,
    pub exact_reached: ReachedTarget,
    pub replanning: ReplanningSupport,
    pub exact_control_succeeded: bool,
    pub first_divergence: Option<NoisyReplayDivergence>,
    pub first_failure: Option<NoisyReplayFailureDiagnostic>,
    pub curves: Vec<NoisyReplayCurve>,
}

/// Aggregate and per-trial results for one family/strength point.
#[derive(Clone, Debug, PartialEq)]
pub struct NoisyReplayCurve {
    pub family: NoiseFamily,
    pub strength_ticks: u8,
    pub requested_trials: usize,
    pub trials: usize,
    pub successes: usize,
    pub success_probability: Option<f64>,
    pub not_applicable_trials: usize,
    pub death_events: u64,
    pub trials_with_death: usize,
    pub successes_after_death: usize,
    pub wrong_target_outcomes: usize,
    pub other_door_outcomes: usize,
    pub other_exit_outcomes: usize,
    pub timeouts: usize,
    /// Successful blind continuations after at least one exact divergence.
    pub divergent_successes: usize,
    pub exact_state_convergences: usize,
    pub first_divergence: Option<NoisyReplayDivergence>,
    pub first_failure: Option<NoisyReplayFailureDiagnostic>,
    pub trials_detail: Vec<NoisyReplayTrial>,
}

/// Outcome of one perturbed semantic input schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoisyReplayTrial {
    pub family: NoiseFamily,
    pub strength_ticks: u8,
    pub schedule: PerturbationSchedule,
    pub actions_simulated: usize,
    pub first_divergence: Option<NoisyReplayDivergence>,
    pub deaths: u32,
    pub first_death: Option<DeathDiagnostic>,
    pub convergence: ExactConvergence,
    pub outcome: NoisyReplayOutcome,
}

/// First authoritative state mismatch against the exact replay at the same
/// simulation tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoisyReplayDivergence {
    pub family: NoiseFamily,
    pub strength_ticks: u8,
    pub trial_index: usize,
    pub schedule_seed: u64,
    pub simulation_tick: usize,
    /// Originating exact replay frame, if the scheduled frame was not
    /// synthetic. This is one-based.
    pub source_replay_tick: Option<usize>,
    pub action: Action,
    pub expected: StateDigest,
    pub actual: StateDigest,
}

/// First death event in a trial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeathDiagnostic {
    pub simulation_tick: usize,
    pub reason: DeathReason,
}

/// Exact same-tick convergence evidence after a state-digest divergence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactConvergence {
    NoDivergence,
    /// A matching suffix at least as long as the configured confirmation
    /// window was observed. Matching uses the complete authoritative digest.
    Observed {
        first_matching_tick: usize,
        matching_ticks: usize,
    },
    NotObserved {
        comparable_ticks_after_divergence: usize,
    },
}

impl ExactConvergence {
    const fn observed(self) -> bool {
        matches!(self, Self::Observed { .. })
    }
}

/// Terminal result of blind continuation. Deaths are orthogonal: a replay
/// can die, reset authoritatively, and still later reach its target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoisyReplayOutcome {
    ReachedTarget {
        completion_ticks: usize,
        target: ReachedTarget,
    },
    ReachedOtherDoor {
        completion_ticks: usize,
        door_id: String,
    },
    ReachedOtherExit {
        completion_ticks: usize,
        exit_id: String,
    },
    Timeout {
        ticks_simulated: usize,
        expected: ReachedTarget,
    },
}

impl NoisyReplayOutcome {
    /// Whether this schedule reached its exact requested trigger.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::ReachedTarget { .. })
    }

    const fn terminal_tick(&self) -> usize {
        match self {
            Self::ReachedTarget {
                completion_ticks, ..
            }
            | Self::ReachedOtherDoor {
                completion_ticks, ..
            }
            | Self::ReachedOtherExit {
                completion_ticks, ..
            } => *completion_ticks,
            Self::Timeout {
                ticks_simulated, ..
            } => *ticks_simulated,
        }
    }
}

/// Earliest terminal failure together with the first causal diagnostics that
/// were observable. It does not claim that the target is unreachable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoisyReplayFailureDiagnostic {
    pub family: NoiseFamily,
    pub strength_ticks: u8,
    pub trial_index: usize,
    pub schedule_seed: u64,
    pub first_divergence: Option<NoisyReplayDivergence>,
    pub first_death: Option<DeathDiagnostic>,
    pub deaths: u32,
    pub outcome: NoisyReplayOutcome,
}

#[derive(Clone, Copy)]
struct ScheduledAction {
    action: Action,
    source_replay_tick: Option<usize>,
}

fn run_trial(
    initial: &Simulation,
    solution: &TargetSolution,
    curve: &RecordedNoiseCurve,
    schedule: &PerturbationSchedule,
    actions: &[ScheduledAction],
    config: ShakyHandConfig,
) -> NoisyReplayTrial {
    let expected = expected_reached_target(&solution.target)
        .expect("target was validated before running noisy trials");
    let mut simulation = initial.clone();
    let final_action = actions
        .last()
        .map_or_else(Action::default, |scheduled| scheduled.action);
    let total_ticks = actions.len().saturating_add(config.grace_ticks);
    let mut first_divergence = None;
    let mut first_death = None;
    let mut deaths = 0_u32;
    let mut digest_matches = Vec::new();
    let mut outcome = None;
    let mut actions_simulated = 0;

    if target_reached(&simulation, &expected) {
        outcome = Some(NoisyReplayOutcome::ReachedTarget {
            completion_ticks: 0,
            target: expected.clone(),
        });
    }

    for tick_index in 0..total_ticks {
        if outcome.is_some() {
            break;
        }
        let scheduled = actions.get(tick_index).copied().unwrap_or(ScheduledAction {
            action: final_action,
            source_replay_tick: None,
        });
        let report = simulation.step(scheduled.action);
        actions_simulated = tick_index + 1;
        for event in &report.events {
            if let SimulationEvent::Died(reason) = event {
                deaths = deaths.saturating_add(1);
                if first_death.is_none() {
                    first_death = Some(DeathDiagnostic {
                        simulation_tick: tick_index + 1,
                        reason: *reason,
                    });
                }
            }
        }

        if let Some(exact_frame) = solution.replay.frames.get(tick_index) {
            let matches = exact_frame.expected_digest == report.digest;
            digest_matches.push(matches);
            if !matches && first_divergence.is_none() {
                first_divergence = Some(NoisyReplayDivergence {
                    family: curve.family,
                    strength_ticks: curve.strength_ticks,
                    trial_index: schedule.trial_index,
                    schedule_seed: schedule.schedule_seed,
                    simulation_tick: tick_index + 1,
                    source_replay_tick: scheduled.source_replay_tick,
                    action: scheduled.action,
                    expected: exact_frame.expected_digest,
                    actual: report.digest,
                });
            }
        }

        if target_reached(&simulation, &expected) {
            outcome = Some(NoisyReplayOutcome::ReachedTarget {
                completion_ticks: tick_index + 1,
                target: expected.clone(),
            });
        } else if let Some(reached) = reached_exit_target(&simulation) {
            outcome = Some(match reached {
                ReachedTarget::Door(door_id) => NoisyReplayOutcome::ReachedOtherDoor {
                    completion_ticks: tick_index + 1,
                    door_id,
                },
                ReachedTarget::Exit(exit_id) => NoisyReplayOutcome::ReachedOtherExit {
                    completion_ticks: tick_index + 1,
                    exit_id,
                },
                ReachedTarget::Pickup(_) => {
                    unreachable!("reached_exit_target never returns pickups")
                }
            });
        }
    }

    let outcome = outcome.unwrap_or(NoisyReplayOutcome::Timeout {
        ticks_simulated: actions_simulated,
        expected,
    });
    let convergence = classify_convergence(
        first_divergence.as_ref(),
        &digest_matches,
        config.convergence_confirmation_ticks,
    );
    NoisyReplayTrial {
        family: curve.family,
        strength_ticks: curve.strength_ticks,
        schedule: schedule.clone(),
        actions_simulated,
        first_divergence,
        deaths,
        first_death,
        convergence,
        outcome,
    }
}

fn classify_convergence(
    first_divergence: Option<&NoisyReplayDivergence>,
    digest_matches: &[bool],
    confirmation_ticks: usize,
) -> ExactConvergence {
    let Some(divergence) = first_divergence else {
        return ExactConvergence::NoDivergence;
    };
    let suffix_matches = digest_matches
        .iter()
        .rev()
        .take_while(|&&matches| matches)
        .count();
    if suffix_matches >= confirmation_ticks {
        ExactConvergence::Observed {
            first_matching_tick: digest_matches.len() - suffix_matches + 1,
            matching_ticks: suffix_matches,
        }
    } else {
        ExactConvergence::NotObserved {
            comparable_ticks_after_divergence: digest_matches
                .len()
                .saturating_sub(divergence.simulation_tick),
        }
    }
}

fn summarize_curve(
    recorded: &RecordedNoiseCurve,
    trials_detail: Vec<NoisyReplayTrial>,
) -> NoisyReplayCurve {
    let trials = trials_detail.len();
    let successes = trials_detail
        .iter()
        .filter(|trial| trial.outcome.succeeded())
        .count();
    let success_probability = (trials != 0).then(|| successes as f64 / trials as f64);
    let death_events = trials_detail
        .iter()
        .map(|trial| u64::from(trial.deaths))
        .sum();
    let trials_with_death = trials_detail
        .iter()
        .filter(|trial| trial.deaths != 0)
        .count();
    let successes_after_death = trials_detail
        .iter()
        .filter(|trial| trial.deaths != 0 && trial.outcome.succeeded())
        .count();
    let other_door_outcomes = trials_detail
        .iter()
        .filter(|trial| matches!(trial.outcome, NoisyReplayOutcome::ReachedOtherDoor { .. }))
        .count();
    let other_exit_outcomes = trials_detail
        .iter()
        .filter(|trial| matches!(trial.outcome, NoisyReplayOutcome::ReachedOtherExit { .. }))
        .count();
    let wrong_target_outcomes = other_door_outcomes + other_exit_outcomes;
    let timeouts = trials_detail
        .iter()
        .filter(|trial| matches!(trial.outcome, NoisyReplayOutcome::Timeout { .. }))
        .count();
    let divergent_successes = trials_detail
        .iter()
        .filter(|trial| trial.first_divergence.is_some() && trial.outcome.succeeded())
        .count();
    let exact_state_convergences = trials_detail
        .iter()
        .filter(|trial| trial.convergence.observed())
        .count();
    let first_divergence = trials_detail
        .iter()
        .filter_map(|trial| trial.first_divergence.as_ref())
        .min_by_key(|diagnostic| diagnostic_sort_key(diagnostic))
        .cloned();
    let first_failure = trials_detail
        .iter()
        .filter(|trial| !trial.outcome.succeeded())
        .map(failure_diagnostic)
        .min_by_key(failure_sort_key);

    NoisyReplayCurve {
        family: recorded.family,
        strength_ticks: recorded.strength_ticks,
        requested_trials: recorded.requested_trials,
        trials,
        successes,
        success_probability,
        not_applicable_trials: recorded.not_applicable_trials,
        death_events,
        trials_with_death,
        successes_after_death,
        wrong_target_outcomes,
        other_door_outcomes,
        other_exit_outcomes,
        timeouts,
        divergent_successes,
        exact_state_convergences,
        first_divergence,
        first_failure,
        trials_detail,
    }
}

fn failure_diagnostic(trial: &NoisyReplayTrial) -> NoisyReplayFailureDiagnostic {
    NoisyReplayFailureDiagnostic {
        family: trial.family,
        strength_ticks: trial.strength_ticks,
        trial_index: trial.schedule.trial_index,
        schedule_seed: trial.schedule.schedule_seed,
        first_divergence: trial.first_divergence.clone(),
        first_death: trial.first_death,
        deaths: trial.deaths,
        outcome: trial.outcome.clone(),
    }
}

fn diagnostic_sort_key(diagnostic: &NoisyReplayDivergence) -> (usize, u8, u8, usize) {
    (
        diagnostic.simulation_tick,
        diagnostic.family.tag(),
        diagnostic.strength_ticks,
        diagnostic.trial_index,
    )
}

fn failure_sort_key(diagnostic: &NoisyReplayFailureDiagnostic) -> (usize, u8, u8, usize) {
    (
        diagnostic.outcome.terminal_tick(),
        diagnostic.family.tag(),
        diagnostic.strength_ticks,
        diagnostic.trial_index,
    )
}

fn target_reached(simulation: &Simulation, expected: &ReachedTarget) -> bool {
    match expected {
        ReachedTarget::Exit(id) | ReachedTarget::Door(id) => simulation.reached_exit() == Some(id),
        ReachedTarget::Pickup(id) => simulation
            .collected_pickups()
            .any(|pickup| pickup.id() == id),
    }
}

fn reached_exit_target(simulation: &Simulation) -> Option<ReachedTarget> {
    let id = simulation.reached_exit()?;
    if simulation.room().doors().iter().any(|door| door.id == id) {
        Some(ReachedTarget::Door(id.to_owned()))
    } else {
        Some(ReachedTarget::Exit(id.to_owned()))
    }
}

fn transition_indices(actions: &[Action]) -> Vec<usize> {
    let mut previous = Action::default();
    actions
        .iter()
        .enumerate()
        .filter_map(|(index, &action)| {
            let changed = action != previous;
            previous = action;
            changed.then_some(index)
        })
        .collect()
}

fn boundary_candidates(
    actions: &[Action],
    transitions: &[usize],
    strength_ticks: u8,
) -> Vec<PerturbationEdit> {
    let amount = usize::from(strength_ticks);
    let mut candidates = Vec::new();
    for &transition in transitions {
        if transition >= amount {
            candidates.push(PerturbationEdit::ShiftBoundary {
                replay_tick: transition + 1,
                offset_ticks: -(strength_ticks as i8),
            });
        }
        if transition.saturating_add(amount) <= actions.len() {
            candidates.push(PerturbationEdit::ShiftBoundary {
                replay_tick: transition + 1,
                offset_ticks: strength_ticks as i8,
            });
        }
    }
    candidates
}

fn correlated_candidates(
    actions: &[Action],
    transitions: &[usize],
    strength_ticks: u8,
    boundary_count: usize,
) -> Vec<PerturbationEdit> {
    let amount = usize::from(strength_ticks);
    let mut candidates = Vec::new();
    for window in transitions.windows(boundary_count) {
        for offset_ticks in [-(strength_ticks as i8), strength_ticks as i8] {
            if shifted_boundaries_valid(
                actions.len(),
                transitions,
                window,
                amount,
                offset_ticks < 0,
            ) {
                candidates.push(PerturbationEdit::ShiftCorrelatedBoundaries {
                    replay_ticks: window.iter().map(|index| index + 1).collect(),
                    offset_ticks,
                });
            }
        }
    }
    candidates
}

fn shifted_boundaries_valid(
    action_count: usize,
    all_boundaries: &[usize],
    selected: &[usize],
    amount: usize,
    early: bool,
) -> bool {
    let mut shifted = all_boundaries.to_vec();
    for boundary in selected {
        let Some(position) = all_boundaries
            .iter()
            .position(|candidate| candidate == boundary)
        else {
            return false;
        };
        let Some(value) = (if early {
            boundary.checked_sub(amount)
        } else {
            boundary.checked_add(amount)
        }) else {
            return false;
        };
        if value >= action_count {
            return false;
        }
        shifted[position] = value;
    }
    shifted.windows(2).all(|pair| pair[0] < pair[1])
}

fn hold_release_candidates(actions: &[Action]) -> Vec<PerturbationEdit> {
    let controls = [
        SemanticControl::Horizontal,
        SemanticControl::Vertical,
        SemanticControl::Jump,
        SemanticControl::Dash,
    ];
    let mut candidates = Vec::new();
    for transition in transition_indices(actions) {
        let previous = transition
            .checked_sub(1)
            .map_or_else(Action::default, |index| actions[index]);
        let next = actions[transition];
        for control in controls {
            if control_value(previous, control) == control_value(next, control) {
                continue;
            }
            if control_is_active(previous, control) {
                candidates.push(PerturbationEdit::HoldControlOneFrame {
                    replay_tick: transition + 1,
                    control,
                });
                if transition != 0 && !control_is_active(next, control) {
                    candidates.push(PerturbationEdit::ReleaseControlOneFrameEarly {
                        replay_tick: transition + 1,
                        control,
                    });
                }
            }
        }
    }
    candidates
}

fn drop_repeat_candidates(actions: &[Action]) -> Vec<PerturbationEdit> {
    let mut candidates = Vec::with_capacity(actions.len().saturating_mul(2));
    for (index, action) in actions.iter().enumerate() {
        if action.restart {
            continue;
        }
        candidates.push(PerturbationEdit::DropSemanticFrame {
            replay_tick: index + 1,
        });
        candidates.push(PerturbationEdit::RepeatSemanticFrame {
            replay_tick: index + 1,
        });
    }
    candidates
}

fn apply_schedule(
    replay: &Replay,
    schedule: &PerturbationSchedule,
) -> Result<Vec<ScheduledAction>, ShakyHandError> {
    let mut actions: Vec<_> = replay
        .actions()
        .enumerate()
        .map(|(index, action)| ScheduledAction {
            action,
            source_replay_tick: Some(index + 1),
        })
        .collect();
    for edit in &schedule.edits {
        apply_edit(&mut actions, edit).map_err(|reason| ShakyHandError::MalformedSchedule {
            trial_index: schedule.trial_index,
            reason,
        })?;
    }
    Ok(actions)
}

fn apply_edit(
    actions: &mut Vec<ScheduledAction>,
    edit: &PerturbationEdit,
) -> Result<(), MalformedScheduleReason> {
    match edit {
        PerturbationEdit::ShiftBoundary {
            replay_tick,
            offset_ticks,
        } => shift_boundary(actions, *replay_tick, *offset_ticks),
        PerturbationEdit::ShiftCorrelatedBoundaries {
            replay_ticks,
            offset_ticks,
        } => shift_correlated_boundaries(actions, replay_ticks, *offset_ticks),
        PerturbationEdit::HoldControlOneFrame {
            replay_tick,
            control,
        } => alter_control_boundary(actions, *replay_tick, *control, false),
        PerturbationEdit::ReleaseControlOneFrameEarly {
            replay_tick,
            control,
        } => alter_control_boundary(actions, *replay_tick, *control, true),
        PerturbationEdit::DropSemanticFrame { replay_tick } => {
            let index = replay_tick
                .checked_sub(1)
                .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
            let action = actions
                .get(index)
                .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
            if action.action.restart {
                return Err(MalformedScheduleReason::RestartFrameCannotBeDroppedOrRepeated);
            }
            actions.remove(index);
            Ok(())
        }
        PerturbationEdit::RepeatSemanticFrame { replay_tick } => {
            let index = replay_tick
                .checked_sub(1)
                .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
            let action = *actions
                .get(index)
                .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
            if action.action.restart {
                return Err(MalformedScheduleReason::RestartFrameCannotBeDroppedOrRepeated);
            }
            actions.insert(index + 1, action);
            Ok(())
        }
    }
}

fn shift_boundary(
    actions: &mut [ScheduledAction],
    replay_tick: usize,
    offset_ticks: i8,
) -> Result<(), MalformedScheduleReason> {
    if offset_ticks == 0 {
        return Err(MalformedScheduleReason::OffsetIsZero);
    }
    let index = replay_tick
        .checked_sub(1)
        .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
    let next = *actions
        .get(index)
        .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
    let previous = index.checked_sub(1).map_or(
        ScheduledAction {
            action: Action::default(),
            source_replay_tick: None,
        },
        |previous_index| actions[previous_index],
    );
    if next.action == previous.action {
        return Err(MalformedScheduleReason::BoundaryNotFound);
    }
    let amount = usize::from(offset_ticks.unsigned_abs());
    if offset_ticks < 0 {
        let start = index
            .checked_sub(amount)
            .ok_or(MalformedScheduleReason::BoundaryWouldLeaveReplay)?;
        actions[start..index].fill(next);
    } else {
        let end = index
            .checked_add(amount)
            .filter(|&end| end <= actions.len())
            .ok_or(MalformedScheduleReason::BoundaryWouldLeaveReplay)?;
        actions[index..end].fill(previous);
    }
    Ok(())
}

fn shift_correlated_boundaries(
    actions: &mut [ScheduledAction],
    replay_ticks: &[usize],
    offset_ticks: i8,
) -> Result<(), MalformedScheduleReason> {
    if offset_ticks == 0 {
        return Err(MalformedScheduleReason::OffsetIsZero);
    }
    if replay_ticks.len() < 2 || replay_ticks.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(MalformedScheduleReason::CorrelatedBoundariesNotIncreasing);
    }
    let original = actions.to_vec();
    let transitions = transition_indices(
        &original
            .iter()
            .map(|scheduled| scheduled.action)
            .collect::<Vec<_>>(),
    );
    let selected: Result<Vec<_>, _> = replay_ticks
        .iter()
        .map(|tick| {
            tick.checked_sub(1)
                .filter(|index| transitions.contains(index))
                .ok_or(MalformedScheduleReason::BoundaryNotFound)
        })
        .collect();
    let selected = selected?;
    let amount = usize::from(offset_ticks.unsigned_abs());
    if !shifted_boundaries_valid(
        actions.len(),
        &transitions,
        &selected,
        amount,
        offset_ticks < 0,
    ) {
        return Err(MalformedScheduleReason::BoundaryWouldLeaveReplay);
    }

    let mut shifted = transitions.clone();
    for selected_boundary in &selected {
        let position = transitions
            .iter()
            .position(|boundary| boundary == selected_boundary)
            .ok_or(MalformedScheduleReason::BoundaryNotFound)?;
        shifted[position] = if offset_ticks < 0 {
            selected_boundary - amount
        } else {
            selected_boundary + amount
        };
    }
    let boundary_actions: Vec<_> = transitions
        .iter()
        .map(|&boundary| original[boundary])
        .collect();
    let mut current = ScheduledAction {
        action: Action::default(),
        source_replay_tick: None,
    };
    let mut boundary_index = 0;
    for (tick, output) in actions.iter_mut().enumerate() {
        if shifted.get(boundary_index) == Some(&tick) {
            current = boundary_actions[boundary_index];
            boundary_index += 1;
        }
        *output = current;
    }
    Ok(())
}

fn alter_control_boundary(
    actions: &mut [ScheduledAction],
    replay_tick: usize,
    control: SemanticControl,
    release_early: bool,
) -> Result<(), MalformedScheduleReason> {
    let index = replay_tick
        .checked_sub(1)
        .ok_or(MalformedScheduleReason::TickOutOfBounds)?;
    let next = actions
        .get(index)
        .ok_or(MalformedScheduleReason::TickOutOfBounds)?
        .action;
    let previous = index
        .checked_sub(1)
        .map_or_else(Action::default, |previous_index| {
            actions[previous_index].action
        });
    if control_value(previous, control) == control_value(next, control) {
        return Err(MalformedScheduleReason::SemanticControlDidNotChange);
    }
    let output_index = if release_early {
        index
            .checked_sub(1)
            .ok_or(MalformedScheduleReason::TickOutOfBounds)?
    } else {
        index
    };
    let value = if release_early {
        control_value(next, control)
    } else {
        control_value(previous, control)
    };
    set_control_value(&mut actions[output_index].action, control, value);
    Ok(())
}

const fn control_value(action: Action, control: SemanticControl) -> i8 {
    match control {
        SemanticControl::Horizontal => action.move_x,
        SemanticControl::Vertical => action.move_y,
        SemanticControl::Jump => action.jump as i8,
        SemanticControl::Dash => action.dash as i8,
    }
}

const fn control_is_active(action: Action, control: SemanticControl) -> bool {
    control_value(action, control) != 0
}

fn set_control_value(action: &mut Action, control: SemanticControl, value: i8) {
    match control {
        SemanticControl::Horizontal => action.move_x = value,
        SemanticControl::Vertical => action.move_y = value,
        SemanticControl::Jump => action.jump = value != 0,
        SemanticControl::Dash => action.dash = value != 0,
    }
}

fn derive_schedule_seed(
    root_seed: u64,
    family: NoiseFamily,
    strength_ticks: u8,
    trial_index: usize,
) -> u64 {
    let mut value = root_seed ^ 0xa076_1d64_78bd_642f;
    value = splitmix64(value ^ u64::from(family.tag()));
    value = splitmix64(value ^ (u64::from(strength_ticks) << 32));
    splitmix64(value ^ u64::try_from(trial_index).unwrap_or(u64::MAX))
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn config_digest(config: ShakyHandConfig) -> u64 {
    let mut digest = StableDigest::new(b"downwards-shaky-hand-config");
    digest.u32(SHAKY_HAND_CONFIG_VERSION);
    digest.usize(config.trials_per_curve_point);
    digest.usize(config.grace_ticks);
    digest.usize(config.correlated_boundaries);
    digest.usize(config.convergence_confirmation_ticks);
    digest.finish()
}

fn replay_fingerprint(replay: &Replay) -> u64 {
    let mut digest = StableDigest::new(b"downwards-shaky-hand-replay");
    digest.u32(SHAKY_HAND_POLICY_VERSION);
    digest.u64(replay.initial_digest.0);
    digest.usize(replay.frames.len());
    for frame in &replay.frames {
        digest.action(frame.action);
        digest.u64(frame.expected_digest.0);
        digest.u64(frame.expected_event_digest.0);
    }
    digest.finish()
}

struct StableDigest(u64);

impl StableDigest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new(domain: &[u8]) -> Self {
        let mut digest = Self(Self::OFFSET);
        digest.bytes(domain);
        digest
    }

    fn action(&mut self, action: Action) {
        self.byte(action.move_x as u8);
        self.byte(action.move_y as u8);
        self.byte(action.jump as u8);
        self.byte(action.dash as u8);
        self.byte(action.restart as u8);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.usize(bytes.len());
        for &byte in bytes {
            self.byte(byte);
        }
    }

    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn usize(&mut self, value: usize) {
        self.u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial(index: usize, outcome: NoisyReplayOutcome) -> NoisyReplayTrial {
        NoisyReplayTrial {
            family: NoiseFamily::BoundaryTiming,
            strength_ticks: 1,
            schedule: PerturbationSchedule {
                trial_index: index,
                schedule_seed: index as u64,
                edits: Vec::new(),
            },
            actions_simulated: outcome.terminal_tick(),
            first_divergence: None,
            deaths: 0,
            first_death: None,
            convergence: ExactConvergence::NoDivergence,
            outcome,
        }
    }

    #[test]
    fn curve_keeps_other_doors_other_exits_and_timeouts_distinct() {
        let recorded = RecordedNoiseCurve {
            family: NoiseFamily::BoundaryTiming,
            strength_ticks: 1,
            requested_trials: 3,
            not_applicable_trials: 0,
            schedules: Vec::new(),
        };
        let curve = summarize_curve(
            &recorded,
            vec![
                trial(
                    0,
                    NoisyReplayOutcome::ReachedOtherDoor {
                        completion_ticks: 7,
                        door_id: "west".into(),
                    },
                ),
                trial(
                    1,
                    NoisyReplayOutcome::ReachedOtherExit {
                        completion_ticks: 8,
                        exit_id: "legacy".into(),
                    },
                ),
                trial(
                    2,
                    NoisyReplayOutcome::Timeout {
                        ticks_simulated: 20,
                        expected: ReachedTarget::Pickup("coin".into()),
                    },
                ),
            ],
        );

        assert_eq!(curve.wrong_target_outcomes, 2);
        assert_eq!(curve.other_door_outcomes, 1);
        assert_eq!(curve.other_exit_outcomes, 1);
        assert_eq!(curve.timeouts, 1);
        assert_eq!(curve.successes, 0);
        assert!(matches!(
            curve.first_failure.unwrap().outcome,
            NoisyReplayOutcome::ReachedOtherDoor { .. }
        ));
    }
}
