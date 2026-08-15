use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use downwards_ai::{
    DifficultyReport, NoiseFamily, ReachedTarget, ReplanningSupport, SearchStats, ShakyHandReport,
};

use crate::{SemanticEvent, SuccessfulWitnessObservation, TraversalCell};

/// Version of the route-difficulty evidence schema and comparison semantics.
pub const ROUTE_DIFFICULTY_VECTOR_VERSION: u32 = 1;

/// Version of the perfect-control-only comparison coordinate set and
/// semantics. This comparison deliberately stops before noisy-control
/// evidence; missing shaky-hand studies are neither errors nor optimistic
/// zeros here.
pub const PERFECT_CONTROL_ROUTE_DIFFICULTY_COMPARISON_VERSION: u32 = 1;

/// Standard non-zero noisy-control points expected from the current
/// shaky-hand policy. Reports may carry extra points, but missing standard
/// points remain explicit instead of being interpreted as robustness.
pub const ROUTE_DIFFICULTY_NOISE_POINTS: [TimingNoiseKey; 8] = [
    TimingNoiseKey::new(NoiseFamily::BoundaryTiming, 1),
    TimingNoiseKey::new(NoiseFamily::BoundaryTiming, 2),
    TimingNoiseKey::new(NoiseFamily::BoundaryTiming, 4),
    TimingNoiseKey::new(NoiseFamily::CorrelatedTiming, 1),
    TimingNoiseKey::new(NoiseFamily::CorrelatedTiming, 2),
    TimingNoiseKey::new(NoiseFamily::CorrelatedTiming, 4),
    TimingNoiseKey::new(NoiseFamily::HoldRelease, 1),
    TimingNoiseKey::new(NoiseFamily::DropRepeatFrame, 1),
];

/// These measurements are corpus diagnostics, not calibrated predictions of
/// human difficulty. They are intentionally not collapsed into a scalar.
pub const ROUTE_DIFFICULTY_DISCLAIMER: &str =
    "route evidence only; retain the full vector and calibrate against human playtests";

/// Transparent player-facing evidence for one successful directed route.
///
/// The caller owns the source-door, target-door, and loadout identity. Solver
/// cost is retained solely for capacity planning and is excluded from
/// [`compare_route_difficulty`]. This describes the supplied positive witness;
/// it does not prove that a simpler route or controller does not exist.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteDifficultyVector {
    pub version: u32,
    pub interpretation: &'static str,
    pub target_id: String,
    pub traversal: TraversalDemand,
    pub control: ControlDemand,
    pub hazards: HazardPressure,
    pub timing: TimingRobustnessVector,
    pub operational_solver_cost: OperationalSolverCost,
}

/// Spatial demand measured on the witness's declared traversal grid.
/// Distances use Manhattan grid-cell units and are comparable only when both
/// routes use the same grid dimensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalDemand {
    pub grid_columns: u16,
    pub grid_rows: u16,
    pub completion_ticks: usize,
    pub coarse_path_length_cells: f64,
    pub horizontal_travel_cells: f64,
    pub vertical_travel_cells: f64,
    pub horizontal_span_cells: u16,
    pub vertical_span_cells: u16,
    /// Changes between non-stationary coarse movement vectors. Magnitude is
    /// discarded, but axis and sign are retained.
    pub spatial_direction_changes: usize,
    /// Consecutive coarse movement vectors with a negative dot product.
    pub spatial_reversals: usize,
    pub visited_cell_count: usize,
}

/// Controller demand and traversal verbs accepted by the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlDemand {
    pub meaningful_input_transitions: usize,
    pub movement_direction_changes: usize,
    pub horizontal_input_reversals: usize,
    pub vertical_input_reversals: usize,
    pub jump_presses: usize,
    pub dash_presses: usize,
    pub restart_presses: usize,
    pub active_control_ticks: usize,
    pub simultaneous_control_ticks: usize,
    pub accepted_movement: AcceptedMovement,
}

/// Movement events accepted by the authoritative simulation rather than
/// merely requested by button presses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcceptedMovement {
    pub grounded_jumps: usize,
    pub coyote_jumps: usize,
    pub buffered_jumps: usize,
    pub wall_jumps: usize,
    pub dashes: usize,
}

impl AcceptedMovement {
    #[must_use]
    pub const fn total_jumps(self) -> usize {
        self.grounded_jumps
            .saturating_add(self.coyote_jumps)
            .saturating_add(self.buffered_jumps)
            .saturating_add(self.wall_jumps)
    }

    #[must_use]
    pub const fn used_wall_jump(self) -> bool {
        self.wall_jumps > 0
    }

    #[must_use]
    pub const fn used_dash(self) -> bool {
        self.dashes > 0
    }
}

/// Hazard demand on the successful perfect-control replay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HazardPressure {
    pub deaths_before_completion: u32,
    pub minimum_clearance: HazardClearanceEvidence,
    /// Higher-is-harder transform of minimum clearance: `1 / (1 + pixels)`.
    /// It is zero when no hazard was relevant.
    pub pressure: f64,
}

/// `NotApplicable` is stronger than missing evidence: the authoritative
/// analyzer observed no relevant static or active timed hazard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardClearanceEvidence {
    Observed { pixels: u32, replay_tick: usize },
    NotApplicableNoRelevantHazard,
}

/// One successful exact replay and an optional noisy-control study. The
/// perfect-control point is positive replay evidence, not an estimated human
/// success probability.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingRobustnessVector {
    pub perfect_control: TimingOutcomeProbabilities,
    pub shaky_hand: ShakyHandEvidence,
}

/// Availability and stable configuration identity of a noisy-control study.
#[derive(Clone, Debug, PartialEq)]
pub enum ShakyHandEvidence {
    Missing {
        reason: MissingEvidenceReason,
    },
    Observed {
        policy_version: u32,
        config_version: u32,
        config_digest: u64,
        seed: u64,
        replanning: ReplanningSupport,
        curves: Box<[TimingNoisePoint]>,
    },
}

/// Stable identity of a family-specific noisy-control point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimingNoiseKey {
    pub family: NoiseFamily,
    pub strength_ticks: u8,
}

impl TimingNoiseKey {
    #[must_use]
    pub const fn new(family: NoiseFamily, strength_ticks: u8) -> Self {
        Self {
            family,
            strength_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimingNoisePoint {
    pub key: TimingNoiseKey,
    pub evidence: TimingPointEvidence,
}

/// `NotApplicable` means schedules could not be constructed for this exact
/// replay. It is never silently substituted with either success or failure.
#[derive(Clone, Debug, PartialEq)]
pub enum TimingPointEvidence {
    Observed(TimingOutcomeProbabilities),
    NotApplicable { requested_trials: usize },
    Missing { reason: MissingEvidenceReason },
}

/// Outcome probabilities for one fixed perturbation family and strength.
/// Death is orthogonal to terminal outcome: a replay can die, reset, and then
/// reach the requested target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimingOutcomeProbabilities {
    pub trials: usize,
    pub success_probability: f64,
    pub failure_probability: f64,
    pub death_probability: f64,
    pub wrong_target_probability: f64,
    pub timeout_probability: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingEvidenceReason {
    ShakyHandStudyNotProvided,
    CurveNotRecorded,
}

/// Search work is deliberately segregated from player-facing measurements.
/// It diagnoses pipeline capacity or solver blind spots; a solver taking more
/// work is not evidence that the player route is harder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperationalSolverCost {
    pub expanded_nodes: usize,
    pub generated_nodes: usize,
    pub simulated_ticks: usize,
    pub deepest_path_ticks: usize,
}

impl From<SearchStats> for OperationalSolverCost {
    fn from(value: SearchStats) -> Self {
        Self {
            expanded_nodes: value.expanded_nodes,
            generated_nodes: value.generated_nodes,
            simulated_ticks: value.simulated_ticks,
            deepest_path_ticks: value.deepest_path_ticks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteDifficultyVectorError {
    TargetMismatch {
        witness: String,
        difficulty: String,
    },
    ShakyHandTargetMismatch {
        witness: String,
        shaky_hand: String,
    },
    BaselineCountMismatch {
        coordinate: &'static str,
        witness: usize,
        difficulty: usize,
    },
    BaselineDeathsMismatch {
        witness: usize,
        difficulty: u32,
    },
    ShakyHandExactControlFailed,
    DuplicateNoisePoint(TimingNoiseKey),
    InvalidNoisePoint {
        key: TimingNoiseKey,
        reason: InvalidNoisePointReason,
    },
}

impl fmt::Display for RouteDifficultyVectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetMismatch {
                witness,
                difficulty,
            } => write!(
                formatter,
                "witness target {witness:?} does not match difficulty target {difficulty:?}"
            ),
            Self::ShakyHandTargetMismatch {
                witness,
                shaky_hand,
            } => write!(
                formatter,
                "witness target {witness:?} does not match shaky-hand target {shaky_hand:?}"
            ),
            Self::BaselineCountMismatch {
                coordinate,
                witness,
                difficulty,
            } => write!(
                formatter,
                "witness {coordinate}={witness} does not match difficulty report value {difficulty}"
            ),
            Self::BaselineDeathsMismatch {
                witness,
                difficulty,
            } => write!(
                formatter,
                "witness deaths={witness} does not match difficulty report value {difficulty}"
            ),
            Self::ShakyHandExactControlFailed => {
                formatter.write_str("shaky-hand report does not have a successful exact control")
            }
            Self::DuplicateNoisePoint(key) => write!(
                formatter,
                "shaky-hand report repeats {:?} strength {}",
                key.family, key.strength_ticks
            ),
            Self::InvalidNoisePoint { key, reason } => write!(
                formatter,
                "invalid {:?} strength {} noisy-control point: {reason:?}",
                key.family, key.strength_ticks
            ),
        }
    }
}

impl Error for RouteDifficultyVectorError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidNoisePointReason {
    ExactCurveMustHaveZeroStrength,
    NonExactCurveMustHavePositiveStrength,
    TrialsExceedRequested,
    RequestedTrialAccountingMismatch,
    OutcomeCountExceedsTrials,
    WrongTargetBreakdownMismatch,
    TerminalOutcomeCountsDoNotSumToTrials,
    MissingSuccessProbability,
    UnexpectedSuccessProbability,
    SuccessProbabilityDoesNotMatchCounts,
    NonFiniteProbability,
}

/// Assemble a route vector while checking that the reports describe the same
/// positive witness. `None` records missing shaky-hand evidence; it never
/// assumes the route is robust.
pub fn route_difficulty_vector(
    witness: &SuccessfulWitnessObservation,
    difficulty: &DifficultyReport,
    shaky_hand: Option<&ShakyHandReport>,
) -> Result<RouteDifficultyVector, RouteDifficultyVectorError> {
    validate_baseline(witness, difficulty)?;
    let accepted_movement = accepted_movement(witness);
    let traversal = traversal_demand(witness);
    let control = control_demand(witness, difficulty, accepted_movement);
    let hazards = hazard_pressure(difficulty);
    let timing = TimingRobustnessVector {
        perfect_control: TimingOutcomeProbabilities {
            trials: 1,
            success_probability: 1.0,
            failure_probability: 0.0,
            death_probability: f64::from(difficulty.deaths > 0),
            wrong_target_probability: 0.0,
            timeout_probability: 0.0,
        },
        shaky_hand: shaky_hand_evidence(witness, shaky_hand)?,
    };

    Ok(RouteDifficultyVector {
        version: ROUTE_DIFFICULTY_VECTOR_VERSION,
        interpretation: ROUTE_DIFFICULTY_DISCLAIMER,
        target_id: witness.reached_exit_id.clone(),
        traversal,
        control,
        hazards,
        timing,
        operational_solver_cost: difficulty.search_effort.into(),
    })
}

fn validate_baseline(
    witness: &SuccessfulWitnessObservation,
    difficulty: &DifficultyReport,
) -> Result<(), RouteDifficultyVectorError> {
    if witness.reached_exit_id != difficulty.exit_id {
        return Err(RouteDifficultyVectorError::TargetMismatch {
            witness: witness.reached_exit_id.clone(),
            difficulty: difficulty.exit_id.clone(),
        });
    }
    check_baseline_count(
        "completion_ticks",
        witness.completion_ticks,
        difficulty.completion_ticks,
    )?;
    check_baseline_count(
        "jump_presses",
        witness.actions.jump_presses,
        difficulty.jump_presses,
    )?;
    check_baseline_count(
        "dash_presses",
        witness.actions.dash_presses,
        difficulty.dash_presses,
    )?;
    check_baseline_count(
        "successful_jumps",
        witness.actions.successful_jumps,
        difficulty.successful_jumps,
    )?;
    check_baseline_count(
        "successful_wall_jumps",
        witness.actions.successful_wall_jumps,
        difficulty.successful_wall_jumps,
    )?;
    check_baseline_count(
        "successful_dashes",
        witness.actions.successful_dashes,
        difficulty.successful_dashes,
    )?;
    if u32::try_from(witness.actions.deaths).ok() != Some(difficulty.deaths) {
        return Err(RouteDifficultyVectorError::BaselineDeathsMismatch {
            witness: witness.actions.deaths,
            difficulty: difficulty.deaths,
        });
    }
    Ok(())
}

fn check_baseline_count(
    coordinate: &'static str,
    witness: usize,
    difficulty: usize,
) -> Result<(), RouteDifficultyVectorError> {
    if witness == difficulty {
        Ok(())
    } else {
        Err(RouteDifficultyVectorError::BaselineCountMismatch {
            coordinate,
            witness,
            difficulty,
        })
    }
}

fn traversal_demand(witness: &SuccessfulWitnessObservation) -> TraversalDemand {
    let cells = witness
        .traversal
        .spans
        .iter()
        .map(|span| span.cell)
        .collect::<Vec<_>>();
    let mut horizontal_travel = 0_u64;
    let mut vertical_travel = 0_u64;
    let mut previous_direction = None;
    let mut direction_changes = 0;
    let mut reversals = 0;
    for pair in cells.windows(2) {
        let dx = i32::from(pair[1].x) - i32::from(pair[0].x);
        let dy = i32::from(pair[1].y) - i32::from(pair[0].y);
        horizontal_travel = horizontal_travel.saturating_add(u64::from(dx.unsigned_abs()));
        vertical_travel = vertical_travel.saturating_add(u64::from(dy.unsigned_abs()));
        let direction = (dx.signum(), dy.signum());
        if direction == (0, 0) {
            continue;
        }
        if let Some((previous_x, previous_y)) = previous_direction {
            if direction != (previous_x, previous_y) {
                direction_changes += 1;
            }
            if previous_x * direction.0 + previous_y * direction.1 < 0 {
                reversals += 1;
            }
        }
        previous_direction = Some(direction);
    }
    let (horizontal_span, vertical_span) = cell_spans(&witness.traversal.visited_cells);

    TraversalDemand {
        grid_columns: witness.traversal.grid.columns(),
        grid_rows: witness.traversal.grid.rows(),
        completion_ticks: witness.completion_ticks,
        coarse_path_length_cells: (horizontal_travel + vertical_travel) as f64,
        horizontal_travel_cells: horizontal_travel as f64,
        vertical_travel_cells: vertical_travel as f64,
        horizontal_span_cells: horizontal_span,
        vertical_span_cells: vertical_span,
        spatial_direction_changes: direction_changes,
        spatial_reversals: reversals,
        visited_cell_count: witness.traversal.visited_cells.len(),
    }
}

fn cell_spans(cells: &[TraversalCell]) -> (u16, u16) {
    let Some(first) = cells.first() else {
        return (0, 0);
    };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
    for cell in &cells[1..] {
        min_x = min_x.min(cell.x);
        max_x = max_x.max(cell.x);
        min_y = min_y.min(cell.y);
        max_y = max_y.max(cell.y);
    }
    (max_x - min_x, max_y - min_y)
}

fn accepted_movement(witness: &SuccessfulWitnessObservation) -> AcceptedMovement {
    let mut accepted = AcceptedMovement::default();
    for event in &witness.actions.events {
        match event.event {
            SemanticEvent::GroundJump => accepted.grounded_jumps += 1,
            SemanticEvent::CoyoteJump => accepted.coyote_jumps += 1,
            SemanticEvent::BufferedJump => accepted.buffered_jumps += 1,
            SemanticEvent::WallJump(_) => accepted.wall_jumps += 1,
            SemanticEvent::Dash(_) => accepted.dashes += 1,
            _ => {}
        }
    }
    accepted
}

fn control_demand(
    witness: &SuccessfulWitnessObservation,
    difficulty: &DifficultyReport,
    accepted_movement: AcceptedMovement,
) -> ControlDemand {
    let mut movement_direction_changes = 0;
    let mut horizontal_input_reversals = 0;
    let mut vertical_input_reversals = 0;
    let mut previous_direction = None;
    let mut previous_horizontal = 0;
    let mut previous_vertical = 0;
    let mut active_control_ticks = 0;
    let mut simultaneous_control_ticks = 0;
    for span in &witness.actions.spans {
        let action = span.action;
        let direction = (action.move_x.signum(), action.move_y.signum());
        if direction != (0, 0) {
            if previous_direction.is_some_and(|previous| previous != direction) {
                movement_direction_changes += 1;
            }
            previous_direction = Some(direction);
        }
        if action.move_x != 0 {
            if previous_horizontal != 0 && previous_horizontal != action.move_x.signum() {
                horizontal_input_reversals += 1;
            }
            previous_horizontal = action.move_x.signum();
        }
        if action.move_y != 0 {
            if previous_vertical != 0 && previous_vertical != action.move_y.signum() {
                vertical_input_reversals += 1;
            }
            previous_vertical = action.move_y.signum();
        }
        let held_controls = usize::from(action.move_x != 0)
            + usize::from(action.move_y != 0)
            + usize::from(action.jump_held)
            + usize::from(action.dash_held)
            + usize::from(action.restart);
        if held_controls > 0 {
            active_control_ticks += span.ticks;
        }
        if held_controls > 1 {
            simultaneous_control_ticks += span.ticks;
        }
    }

    ControlDemand {
        meaningful_input_transitions: difficulty.meaningful_input_transitions,
        movement_direction_changes,
        horizontal_input_reversals,
        vertical_input_reversals,
        jump_presses: difficulty.jump_presses,
        dash_presses: difficulty.dash_presses,
        restart_presses: witness.actions.restart_presses,
        active_control_ticks,
        simultaneous_control_ticks,
        accepted_movement,
    }
}

fn hazard_pressure(difficulty: &DifficultyReport) -> HazardPressure {
    let (minimum_clearance, pressure) = difficulty.minimum_hazard_clearance.map_or(
        (HazardClearanceEvidence::NotApplicableNoRelevantHazard, 0.0),
        |clearance| {
            (
                HazardClearanceEvidence::Observed {
                    pixels: clearance.pixels,
                    replay_tick: clearance.replay_tick,
                },
                1.0 / (1.0 + f64::from(clearance.pixels)),
            )
        },
    );
    HazardPressure {
        deaths_before_completion: difficulty.deaths,
        minimum_clearance,
        pressure,
    }
}

fn shaky_hand_evidence(
    witness: &SuccessfulWitnessObservation,
    report: Option<&ShakyHandReport>,
) -> Result<ShakyHandEvidence, RouteDifficultyVectorError> {
    let Some(report) = report else {
        return Ok(ShakyHandEvidence::Missing {
            reason: MissingEvidenceReason::ShakyHandStudyNotProvided,
        });
    };
    let shaky_target = reached_target_id(&report.exact_reached);
    if shaky_target != witness.reached_exit_id {
        return Err(RouteDifficultyVectorError::ShakyHandTargetMismatch {
            witness: witness.reached_exit_id.clone(),
            shaky_hand: shaky_target.to_owned(),
        });
    }
    if !report.exact_control_succeeded {
        return Err(RouteDifficultyVectorError::ShakyHandExactControlFailed);
    }

    let mut points = BTreeMap::<TimingNoiseKey, TimingNoisePoint>::new();
    let mut seen = BTreeSet::new();
    for curve in &report.curves {
        let key = TimingNoiseKey::new(curve.family, curve.strength_ticks);
        if !seen.insert(key) {
            return Err(RouteDifficultyVectorError::DuplicateNoisePoint(key));
        }
        if curve.family == NoiseFamily::Exact {
            if curve.strength_ticks != 0 {
                return Err(RouteDifficultyVectorError::InvalidNoisePoint {
                    key,
                    reason: InvalidNoisePointReason::ExactCurveMustHaveZeroStrength,
                });
            }
            noisy_curve_evidence(curve)?;
            continue;
        }
        if curve.strength_ticks == 0 {
            return Err(RouteDifficultyVectorError::InvalidNoisePoint {
                key,
                reason: InvalidNoisePointReason::NonExactCurveMustHavePositiveStrength,
            });
        }
        let evidence = noisy_curve_evidence(curve)?;
        points.insert(key, TimingNoisePoint { key, evidence });
    }
    for key in ROUTE_DIFFICULTY_NOISE_POINTS {
        points.entry(key).or_insert(TimingNoisePoint {
            key,
            evidence: TimingPointEvidence::Missing {
                reason: MissingEvidenceReason::CurveNotRecorded,
            },
        });
    }

    Ok(ShakyHandEvidence::Observed {
        policy_version: report.study.identity.policy_version,
        config_version: report.study.identity.config_version,
        config_digest: report.study.identity.config_digest,
        seed: report.study.identity.seed,
        replanning: report.replanning,
        curves: points.into_values().collect::<Vec<_>>().into_boxed_slice(),
    })
}

fn noisy_curve_evidence(
    curve: &downwards_ai::NoisyReplayCurve,
) -> Result<TimingPointEvidence, RouteDifficultyVectorError> {
    let key = TimingNoiseKey::new(curve.family, curve.strength_ticks);
    if curve.trials > curve.requested_trials {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::TrialsExceedRequested,
        });
    }
    if curve.trials.checked_add(curve.not_applicable_trials) != Some(curve.requested_trials) {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::RequestedTrialAccountingMismatch,
        });
    }
    if curve.successes > curve.trials
        || curve.trials_with_death > curve.trials
        || curve.wrong_target_outcomes > curve.trials
        || curve.timeouts > curve.trials
    {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::OutcomeCountExceedsTrials,
        });
    }
    if curve
        .other_door_outcomes
        .checked_add(curve.other_exit_outcomes)
        != Some(curve.wrong_target_outcomes)
    {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::WrongTargetBreakdownMismatch,
        });
    }
    if curve
        .successes
        .checked_add(curve.wrong_target_outcomes)
        .and_then(|count| count.checked_add(curve.timeouts))
        != Some(curve.trials)
    {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::TerminalOutcomeCountsDoNotSumToTrials,
        });
    }
    if curve.trials == 0 {
        if curve.success_probability.is_some() {
            return Err(RouteDifficultyVectorError::InvalidNoisePoint {
                key,
                reason: InvalidNoisePointReason::UnexpectedSuccessProbability,
            });
        }
        return Ok(TimingPointEvidence::NotApplicable {
            requested_trials: curve.requested_trials,
        });
    }
    let Some(success_probability) = curve.success_probability else {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: InvalidNoisePointReason::MissingSuccessProbability,
        });
    };
    let calculated_success = curve.successes as f64 / curve.trials as f64;
    if !success_probability.is_finite()
        || (success_probability - calculated_success).abs() > f64::EPSILON
    {
        return Err(RouteDifficultyVectorError::InvalidNoisePoint {
            key,
            reason: if success_probability.is_finite() {
                InvalidNoisePointReason::SuccessProbabilityDoesNotMatchCounts
            } else {
                InvalidNoisePointReason::NonFiniteProbability
            },
        });
    }
    Ok(TimingPointEvidence::Observed(TimingOutcomeProbabilities {
        trials: curve.trials,
        success_probability,
        failure_probability: 1.0 - success_probability,
        death_probability: curve.trials_with_death as f64 / curve.trials as f64,
        wrong_target_probability: curve.wrong_target_outcomes as f64 / curve.trials as f64,
        timeout_probability: curve.timeouts as f64 / curve.trials as f64,
    }))
}

fn reached_target_id(target: &ReachedTarget) -> &str {
    match target {
        ReachedTarget::Exit(id) | ReachedTarget::Door(id) | ReachedTarget::Pickup(id) => id,
    }
}

/// Coordinates included in the conservative Pareto comparison. Values are
/// oriented higher-is-harder; clearance uses the monotone hazard-pressure
/// transform rather than comparing lower-is-harder pixels directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DifficultyCoordinate {
    CompletionTicks,
    CoarsePathLength,
    HorizontalTravel,
    VerticalTravel,
    HorizontalSpan,
    VerticalSpan,
    SpatialDirectionChanges,
    SpatialReversals,
    VisitedCells,
    MeaningfulInputTransitions,
    MovementDirectionChanges,
    HorizontalInputReversals,
    VerticalInputReversals,
    JumpPresses,
    DashPresses,
    RestartPresses,
    ActiveControlTicks,
    SimultaneousControlTicks,
    GroundedJumps,
    CoyoteJumps,
    BufferedJumps,
    WallJumps,
    Dashes,
    BaselineDeaths,
    HazardPressure,
    PerfectControlFailureProbability,
    PerfectControlDeathProbability,
    NoisyFailureProbability(TimingNoiseKey),
    NoisyDeathProbability(TimingNoiseKey),
    NoisyWrongTargetProbability(TimingNoiseKey),
    NoisyTimeoutProbability(TimingNoiseKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoordinateKind {
    Tick,
    Count,
    Distance,
    Pressure,
    Probability,
}

impl DifficultyCoordinate {
    const fn kind(self) -> CoordinateKind {
        match self {
            Self::CompletionTicks | Self::ActiveControlTicks | Self::SimultaneousControlTicks => {
                CoordinateKind::Tick
            }
            Self::CoarsePathLength
            | Self::HorizontalTravel
            | Self::VerticalTravel
            | Self::HorizontalSpan
            | Self::VerticalSpan => CoordinateKind::Distance,
            Self::HazardPressure => CoordinateKind::Pressure,
            Self::PerfectControlFailureProbability
            | Self::PerfectControlDeathProbability
            | Self::NoisyFailureProbability(_)
            | Self::NoisyDeathProbability(_)
            | Self::NoisyWrongTargetProbability(_)
            | Self::NoisyTimeoutProbability(_) => CoordinateKind::Probability,
            _ => CoordinateKind::Count,
        }
    }
}

/// Absolute tolerances applied independently to transparent coordinate types.
/// No weights or scalar aggregation are involved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DifficultyComparisonTolerances {
    pub ticks: f64,
    pub counts: f64,
    pub coarse_distance_cells: f64,
    pub hazard_pressure: f64,
    pub probabilities: f64,
}

impl Default for DifficultyComparisonTolerances {
    fn default() -> Self {
        Self {
            ticks: 0.0,
            counts: 0.0,
            coarse_distance_cells: 0.0,
            hazard_pressure: 0.0,
            probabilities: 0.02,
        }
    }
}

impl DifficultyComparisonTolerances {
    fn for_coordinate(self, coordinate: DifficultyCoordinate) -> f64 {
        match coordinate.kind() {
            CoordinateKind::Tick => self.ticks,
            CoordinateKind::Count => self.counts,
            CoordinateKind::Distance => self.coarse_distance_cells,
            CoordinateKind::Pressure => self.hazard_pressure,
            CoordinateKind::Probability => self.probabilities,
        }
    }

    fn invalid_fields(self) -> Vec<ToleranceField> {
        [
            (ToleranceField::Ticks, self.ticks),
            (ToleranceField::Counts, self.counts),
            (
                ToleranceField::CoarseDistanceCells,
                self.coarse_distance_cells,
            ),
            (ToleranceField::HazardPressure, self.hazard_pressure),
            (ToleranceField::Probabilities, self.probabilities),
        ]
        .into_iter()
        .filter_map(|(field, value)| (!value.is_finite() || value < 0.0).then_some(field))
        .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToleranceField {
    Ticks,
    Counts,
    CoarseDistanceCells,
    HazardPressure,
    Probabilities,
}

/// Conservative partial comparison of two route vectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteDifficultyComparison {
    /// The left route is no easier on every comparable coordinate and is
    /// strictly harder on at least one.
    LeftClearlyHarder {
        strict_coordinates: Box<[DifficultyCoordinate]>,
    },
    RightClearlyHarder {
        strict_coordinates: Box<[DifficultyCoordinate]>,
    },
    EquivalentWithinTolerance,
    /// Both routes have complete evidence but exhibit trade-offs, or a valid
    /// noisy-control coordinate applies to only one route.
    Incomparable {
        left_harder: Box<[DifficultyCoordinate]>,
        right_harder: Box<[DifficultyCoordinate]>,
        applicability_mismatches: Box<[DifficultyCoordinate]>,
    },
    /// A dominance claim would require evidence that was not measured or was
    /// produced under an incompatible schema/configuration.
    InsufficientEvidence {
        issues: Box<[InsufficientEvidenceIssue]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InsufficientEvidenceIssue {
    UnsupportedVectorVersion {
        left: u32,
        right: u32,
    },
    TraversalGridMismatch {
        left_columns: u16,
        left_rows: u16,
        right_columns: u16,
        right_rows: u16,
    },
    MissingShakyHandStudy {
        side: ComparisonSide,
    },
    IncompatibleShakyHandConfiguration,
    MissingCoordinate {
        side: ComparisonSide,
        coordinate: DifficultyCoordinate,
        reason: MissingEvidenceReason,
    },
    InvalidCoordinate {
        side: ComparisonSide,
        coordinate: DifficultyCoordinate,
    },
    DuplicateNoisePoint {
        side: ComparisonSide,
        key: TimingNoiseKey,
    },
    InvalidTolerance(ToleranceField),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonSide {
    Left,
    Right,
}

/// Compare player-facing coordinates without consulting solver effort.
///
/// Missing noisy-control evidence yields `InsufficientEvidence`.
/// `NotApplicable` on both sides removes one curve point; applicability on
/// only one side yields `Incomparable`. Evidence gaps are never filled with
/// optimistic zeros.
#[must_use]
pub fn compare_route_difficulty(
    left: &RouteDifficultyVector,
    right: &RouteDifficultyVector,
    tolerances: DifficultyComparisonTolerances,
) -> RouteDifficultyComparison {
    let (mut coordinates, mut issues) = prepare_perfect_control_comparison(left, right, tolerances);
    append_timing_coordinates(left, right, &mut coordinates, &mut issues);
    finish_comparison(coordinates, issues, tolerances)
}

/// Compare only exact perfect-control player-facing coordinates.
///
/// This uses the same vector schema, validation, coordinate orientation, and
/// Pareto classification as [`compare_route_difficulty`], but intentionally
/// excludes every `Noisy*` coordinate. Missing shaky-hand evidence is outside
/// this comparison rather than filled in. Operational solver cost is never a
/// coordinate.
#[must_use]
pub fn compare_perfect_control_route_difficulty(
    left: &RouteDifficultyVector,
    right: &RouteDifficultyVector,
    tolerances: DifficultyComparisonTolerances,
) -> RouteDifficultyComparison {
    let (coordinates, issues) = prepare_perfect_control_comparison(left, right, tolerances);
    finish_comparison(coordinates, issues, tolerances)
}

fn prepare_perfect_control_comparison(
    left: &RouteDifficultyVector,
    right: &RouteDifficultyVector,
    tolerances: DifficultyComparisonTolerances,
) -> (Vec<ComparableCoordinate>, Vec<InsufficientEvidenceIssue>) {
    let mut issues = Vec::new();
    if left.version != ROUTE_DIFFICULTY_VECTOR_VERSION
        || right.version != ROUTE_DIFFICULTY_VECTOR_VERSION
        || left.version != right.version
    {
        issues.push(InsufficientEvidenceIssue::UnsupportedVectorVersion {
            left: left.version,
            right: right.version,
        });
    }
    if left.traversal.grid_columns != right.traversal.grid_columns
        || left.traversal.grid_rows != right.traversal.grid_rows
    {
        issues.push(InsufficientEvidenceIssue::TraversalGridMismatch {
            left_columns: left.traversal.grid_columns,
            left_rows: left.traversal.grid_rows,
            right_columns: right.traversal.grid_columns,
            right_rows: right.traversal.grid_rows,
        });
    }
    issues.extend(
        tolerances
            .invalid_fields()
            .into_iter()
            .map(InsufficientEvidenceIssue::InvalidTolerance),
    );
    validate_vector_coordinates(left, ComparisonSide::Left, &mut issues);
    validate_vector_coordinates(right, ComparisonSide::Right, &mut issues);

    let coordinates = baseline_coordinates(left)
        .into_iter()
        .zip(baseline_coordinates(right))
        .map(|((coordinate, left), (right_coordinate, right))| {
            debug_assert_eq!(coordinate, right_coordinate);
            ComparableCoordinate {
                coordinate,
                left: CoordinateEvidence::Observed(left),
                right: CoordinateEvidence::Observed(right),
            }
        })
        .collect::<Vec<_>>();
    (coordinates, issues)
}

fn finish_comparison(
    coordinates: Vec<ComparableCoordinate>,
    issues: Vec<InsufficientEvidenceIssue>,
    tolerances: DifficultyComparisonTolerances,
) -> RouteDifficultyComparison {
    if !issues.is_empty() {
        return RouteDifficultyComparison::InsufficientEvidence {
            issues: issues.into_boxed_slice(),
        };
    }

    let mut left_harder = BTreeSet::new();
    let mut right_harder = BTreeSet::new();
    let mut applicability_mismatches = BTreeSet::new();
    for pair in coordinates {
        match (pair.left, pair.right) {
            (CoordinateEvidence::Observed(left), CoordinateEvidence::Observed(right)) => {
                let tolerance = tolerances.for_coordinate(pair.coordinate);
                if left > right + tolerance {
                    left_harder.insert(pair.coordinate);
                } else if right > left + tolerance {
                    right_harder.insert(pair.coordinate);
                }
            }
            (CoordinateEvidence::NotApplicable, CoordinateEvidence::NotApplicable) => {}
            (CoordinateEvidence::NotApplicable, CoordinateEvidence::Observed(_))
            | (CoordinateEvidence::Observed(_), CoordinateEvidence::NotApplicable) => {
                applicability_mismatches.insert(pair.coordinate);
            }
            (CoordinateEvidence::Missing(_), _) | (_, CoordinateEvidence::Missing(_)) => {
                unreachable!("missing coordinate evidence was converted into an issue")
            }
        }
    }

    if !applicability_mismatches.is_empty() || (!left_harder.is_empty() && !right_harder.is_empty())
    {
        return RouteDifficultyComparison::Incomparable {
            left_harder: left_harder
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            right_harder: right_harder
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            applicability_mismatches: applicability_mismatches
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
    }
    if !left_harder.is_empty() {
        RouteDifficultyComparison::LeftClearlyHarder {
            strict_coordinates: left_harder
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    } else if !right_harder.is_empty() {
        RouteDifficultyComparison::RightClearlyHarder {
            strict_coordinates: right_harder
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    } else {
        RouteDifficultyComparison::EquivalentWithinTolerance
    }
}

#[derive(Clone, Copy)]
struct ComparableCoordinate {
    coordinate: DifficultyCoordinate,
    left: CoordinateEvidence,
    right: CoordinateEvidence,
}

#[derive(Clone, Copy)]
enum CoordinateEvidence {
    Observed(f64),
    NotApplicable,
    Missing(MissingEvidenceReason),
}

fn validate_vector_coordinates(
    vector: &RouteDifficultyVector,
    side: ComparisonSide,
    issues: &mut Vec<InsufficientEvidenceIssue>,
) {
    for (coordinate, value) in baseline_coordinates(vector) {
        let in_range = match coordinate.kind() {
            CoordinateKind::Probability | CoordinateKind::Pressure => {
                value.is_finite() && (0.0..=1.0).contains(&value)
            }
            CoordinateKind::Tick | CoordinateKind::Count | CoordinateKind::Distance => {
                value.is_finite() && value >= 0.0
            }
        };
        if !in_range {
            issues.push(InsufficientEvidenceIssue::InvalidCoordinate { side, coordinate });
        }
    }
    if !valid_probabilities(vector.timing.perfect_control)
        || vector.timing.perfect_control.trials != 1
        || vector.timing.perfect_control.success_probability != 1.0
    {
        issues.push(InsufficientEvidenceIssue::InvalidCoordinate {
            side,
            coordinate: DifficultyCoordinate::PerfectControlFailureProbability,
        });
    }
    let expected_pressure = match vector.hazards.minimum_clearance {
        HazardClearanceEvidence::Observed { pixels, .. } => 1.0 / (1.0 + f64::from(pixels)),
        HazardClearanceEvidence::NotApplicableNoRelevantHazard => 0.0,
    };
    if (vector.hazards.pressure - expected_pressure).abs() > f64::EPSILON {
        issues.push(InsufficientEvidenceIssue::InvalidCoordinate {
            side,
            coordinate: DifficultyCoordinate::HazardPressure,
        });
    }
}

fn valid_probabilities(probabilities: TimingOutcomeProbabilities) -> bool {
    let values = [
        probabilities.success_probability,
        probabilities.failure_probability,
        probabilities.death_probability,
        probabilities.wrong_target_probability,
        probabilities.timeout_probability,
    ];
    probabilities.trials > 0
        && values
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        && (probabilities.success_probability + probabilities.failure_probability - 1.0).abs()
            <= f64::EPSILON
        && (probabilities.wrong_target_probability + probabilities.timeout_probability
            - probabilities.failure_probability)
            .abs()
            <= f64::EPSILON
}

fn baseline_coordinates(vector: &RouteDifficultyVector) -> [(DifficultyCoordinate, f64); 27] {
    let traversal = vector.traversal;
    let control = vector.control;
    let accepted = control.accepted_movement;
    [
        (
            DifficultyCoordinate::CompletionTicks,
            traversal.completion_ticks as f64,
        ),
        (
            DifficultyCoordinate::CoarsePathLength,
            traversal.coarse_path_length_cells,
        ),
        (
            DifficultyCoordinate::HorizontalTravel,
            traversal.horizontal_travel_cells,
        ),
        (
            DifficultyCoordinate::VerticalTravel,
            traversal.vertical_travel_cells,
        ),
        (
            DifficultyCoordinate::HorizontalSpan,
            f64::from(traversal.horizontal_span_cells),
        ),
        (
            DifficultyCoordinate::VerticalSpan,
            f64::from(traversal.vertical_span_cells),
        ),
        (
            DifficultyCoordinate::SpatialDirectionChanges,
            traversal.spatial_direction_changes as f64,
        ),
        (
            DifficultyCoordinate::SpatialReversals,
            traversal.spatial_reversals as f64,
        ),
        (
            DifficultyCoordinate::VisitedCells,
            traversal.visited_cell_count as f64,
        ),
        (
            DifficultyCoordinate::MeaningfulInputTransitions,
            control.meaningful_input_transitions as f64,
        ),
        (
            DifficultyCoordinate::MovementDirectionChanges,
            control.movement_direction_changes as f64,
        ),
        (
            DifficultyCoordinate::HorizontalInputReversals,
            control.horizontal_input_reversals as f64,
        ),
        (
            DifficultyCoordinate::VerticalInputReversals,
            control.vertical_input_reversals as f64,
        ),
        (
            DifficultyCoordinate::JumpPresses,
            control.jump_presses as f64,
        ),
        (
            DifficultyCoordinate::DashPresses,
            control.dash_presses as f64,
        ),
        (
            DifficultyCoordinate::RestartPresses,
            control.restart_presses as f64,
        ),
        (
            DifficultyCoordinate::ActiveControlTicks,
            control.active_control_ticks as f64,
        ),
        (
            DifficultyCoordinate::SimultaneousControlTicks,
            control.simultaneous_control_ticks as f64,
        ),
        (
            DifficultyCoordinate::GroundedJumps,
            accepted.grounded_jumps as f64,
        ),
        (
            DifficultyCoordinate::CoyoteJumps,
            accepted.coyote_jumps as f64,
        ),
        (
            DifficultyCoordinate::BufferedJumps,
            accepted.buffered_jumps as f64,
        ),
        (DifficultyCoordinate::WallJumps, accepted.wall_jumps as f64),
        (DifficultyCoordinate::Dashes, accepted.dashes as f64),
        (
            DifficultyCoordinate::BaselineDeaths,
            f64::from(vector.hazards.deaths_before_completion),
        ),
        (
            DifficultyCoordinate::HazardPressure,
            vector.hazards.pressure,
        ),
        (
            DifficultyCoordinate::PerfectControlFailureProbability,
            vector.timing.perfect_control.failure_probability,
        ),
        (
            DifficultyCoordinate::PerfectControlDeathProbability,
            vector.timing.perfect_control.death_probability,
        ),
    ]
}

fn append_timing_coordinates(
    left: &RouteDifficultyVector,
    right: &RouteDifficultyVector,
    coordinates: &mut Vec<ComparableCoordinate>,
    issues: &mut Vec<InsufficientEvidenceIssue>,
) {
    let (left_identity, left_points) = match &left.timing.shaky_hand {
        ShakyHandEvidence::Missing { .. } => {
            issues.push(InsufficientEvidenceIssue::MissingShakyHandStudy {
                side: ComparisonSide::Left,
            });
            (None, BTreeMap::new())
        }
        ShakyHandEvidence::Observed {
            policy_version,
            config_version,
            config_digest,
            replanning,
            curves,
            ..
        } => (
            Some((
                *policy_version,
                *config_version,
                *config_digest,
                *replanning,
            )),
            curve_map(curves, ComparisonSide::Left, issues),
        ),
    };
    let (right_identity, right_points) = match &right.timing.shaky_hand {
        ShakyHandEvidence::Missing { .. } => {
            issues.push(InsufficientEvidenceIssue::MissingShakyHandStudy {
                side: ComparisonSide::Right,
            });
            (None, BTreeMap::new())
        }
        ShakyHandEvidence::Observed {
            policy_version,
            config_version,
            config_digest,
            replanning,
            curves,
            ..
        } => (
            Some((
                *policy_version,
                *config_version,
                *config_digest,
                *replanning,
            )),
            curve_map(curves, ComparisonSide::Right, issues),
        ),
    };
    let (Some(left_identity), Some(right_identity)) = (left_identity, right_identity) else {
        return;
    };
    if left_identity != right_identity {
        issues.push(InsufficientEvidenceIssue::IncompatibleShakyHandConfiguration);
        return;
    }

    let keys = ROUTE_DIFFICULTY_NOISE_POINTS
        .into_iter()
        .chain(left_points.keys().copied())
        .chain(right_points.keys().copied())
        .collect::<BTreeSet<_>>();
    for key in keys {
        for (outcome, coordinate) in [
            (
                CurveOutcome::Failure,
                DifficultyCoordinate::NoisyFailureProbability(key),
            ),
            (
                CurveOutcome::Death,
                DifficultyCoordinate::NoisyDeathProbability(key),
            ),
            (
                CurveOutcome::WrongTarget,
                DifficultyCoordinate::NoisyWrongTargetProbability(key),
            ),
            (
                CurveOutcome::Timeout,
                DifficultyCoordinate::NoisyTimeoutProbability(key),
            ),
        ] {
            let left = left_points.get(&key).map_or(
                CoordinateEvidence::Missing(MissingEvidenceReason::CurveNotRecorded),
                |point| curve_coordinate(&point.evidence, outcome),
            );
            let right = right_points.get(&key).map_or(
                CoordinateEvidence::Missing(MissingEvidenceReason::CurveNotRecorded),
                |point| curve_coordinate(&point.evidence, outcome),
            );
            append_curve_coordinate(coordinate, left, right, coordinates, issues);
        }
    }
}

fn curve_map<'a>(
    curves: &'a [TimingNoisePoint],
    side: ComparisonSide,
    issues: &mut Vec<InsufficientEvidenceIssue>,
) -> BTreeMap<TimingNoiseKey, &'a TimingNoisePoint> {
    let mut points = BTreeMap::new();
    for point in curves {
        if points.insert(point.key, point).is_some() {
            issues.push(InsufficientEvidenceIssue::DuplicateNoisePoint {
                side,
                key: point.key,
            });
        }
        if let TimingPointEvidence::Observed(probabilities) = point.evidence
            && !valid_probabilities(probabilities)
        {
            issues.push(InsufficientEvidenceIssue::InvalidCoordinate {
                side,
                coordinate: DifficultyCoordinate::NoisyFailureProbability(point.key),
            });
        }
    }
    points
}

#[derive(Clone, Copy)]
enum CurveOutcome {
    Failure,
    Death,
    WrongTarget,
    Timeout,
}

fn curve_coordinate(evidence: &TimingPointEvidence, outcome: CurveOutcome) -> CoordinateEvidence {
    match evidence {
        TimingPointEvidence::Observed(probabilities) => {
            CoordinateEvidence::Observed(match outcome {
                CurveOutcome::Failure => probabilities.failure_probability,
                CurveOutcome::Death => probabilities.death_probability,
                CurveOutcome::WrongTarget => probabilities.wrong_target_probability,
                CurveOutcome::Timeout => probabilities.timeout_probability,
            })
        }
        TimingPointEvidence::NotApplicable { .. } => CoordinateEvidence::NotApplicable,
        TimingPointEvidence::Missing { reason } => CoordinateEvidence::Missing(*reason),
    }
}

fn append_curve_coordinate(
    coordinate: DifficultyCoordinate,
    left: CoordinateEvidence,
    right: CoordinateEvidence,
    coordinates: &mut Vec<ComparableCoordinate>,
    issues: &mut Vec<InsufficientEvidenceIssue>,
) {
    if let CoordinateEvidence::Missing(reason) = left {
        issues.push(InsufficientEvidenceIssue::MissingCoordinate {
            side: ComparisonSide::Left,
            coordinate,
            reason,
        });
    }
    if let CoordinateEvidence::Missing(reason) = right {
        issues.push(InsufficientEvidenceIssue::MissingCoordinate {
            side: ComparisonSide::Right,
            coordinate,
            reason,
        });
    }
    coordinates.push(ComparableCoordinate {
        coordinate,
        left,
        right,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing_probabilities(
        successes: usize,
        deaths: usize,
        wrong: usize,
        timeouts: usize,
    ) -> TimingOutcomeProbabilities {
        let trials = successes + wrong + timeouts;
        TimingOutcomeProbabilities {
            trials,
            success_probability: successes as f64 / trials as f64,
            failure_probability: (wrong + timeouts) as f64 / trials as f64,
            death_probability: deaths as f64 / trials as f64,
            wrong_target_probability: wrong as f64 / trials as f64,
            timeout_probability: timeouts as f64 / trials as f64,
        }
    }

    fn complete_shaky(
        successes: usize,
        deaths: usize,
        wrong: usize,
        timeouts: usize,
    ) -> ShakyHandEvidence {
        let probabilities = timing_probabilities(successes, deaths, wrong, timeouts);
        ShakyHandEvidence::Observed {
            policy_version: 1,
            config_version: 1,
            config_digest: 7,
            seed: 11,
            replanning: ReplanningSupport::Unsupported,
            curves: ROUTE_DIFFICULTY_NOISE_POINTS
                .into_iter()
                .map(|key| TimingNoisePoint {
                    key,
                    evidence: TimingPointEvidence::Observed(probabilities),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    fn vector(shaky_hand: ShakyHandEvidence) -> RouteDifficultyVector {
        RouteDifficultyVector {
            version: ROUTE_DIFFICULTY_VECTOR_VERSION,
            interpretation: ROUTE_DIFFICULTY_DISCLAIMER,
            target_id: "target".to_owned(),
            traversal: TraversalDemand {
                grid_columns: 16,
                grid_rows: 9,
                completion_ticks: 100,
                coarse_path_length_cells: 12.0,
                horizontal_travel_cells: 10.0,
                vertical_travel_cells: 2.0,
                horizontal_span_cells: 8,
                vertical_span_cells: 2,
                spatial_direction_changes: 1,
                spatial_reversals: 0,
                visited_cell_count: 10,
            },
            control: ControlDemand {
                meaningful_input_transitions: 4,
                movement_direction_changes: 1,
                horizontal_input_reversals: 0,
                vertical_input_reversals: 0,
                jump_presses: 1,
                dash_presses: 0,
                restart_presses: 0,
                active_control_ticks: 90,
                simultaneous_control_ticks: 10,
                accepted_movement: AcceptedMovement {
                    grounded_jumps: 1,
                    ..AcceptedMovement::default()
                },
            },
            hazards: HazardPressure {
                deaths_before_completion: 0,
                minimum_clearance: HazardClearanceEvidence::Observed {
                    pixels: 9,
                    replay_tick: 50,
                },
                pressure: 0.1,
            },
            timing: TimingRobustnessVector {
                perfect_control: TimingOutcomeProbabilities {
                    trials: 1,
                    success_probability: 1.0,
                    failure_probability: 0.0,
                    death_probability: 0.0,
                    wrong_target_probability: 0.0,
                    timeout_probability: 0.0,
                },
                shaky_hand,
            },
            operational_solver_cost: OperationalSolverCost::default(),
        }
    }

    #[test]
    fn fragile_route_is_clearly_harder_than_otherwise_equal_robust_route() {
        let robust = vector(complete_shaky(100, 0, 0, 0));
        let fragile = vector(complete_shaky(60, 25, 10, 30));
        let comparison =
            compare_route_difficulty(&fragile, &robust, DifficultyComparisonTolerances::default());
        let RouteDifficultyComparison::LeftClearlyHarder { strict_coordinates } = comparison else {
            panic!("expected fragile route to dominate robust route");
        };
        assert!(strict_coordinates.iter().any(|coordinate| matches!(
            coordinate,
            DifficultyCoordinate::NoisyFailureProbability(_)
        )));
        assert!(strict_coordinates.iter().any(|coordinate| matches!(
            coordinate,
            DifficultyCoordinate::NoisyDeathProbability(_)
        )));
    }

    #[test]
    fn longer_but_safer_routes_are_incomparable() {
        let mut longer_safer = vector(complete_shaky(100, 0, 0, 0));
        longer_safer.traversal.completion_ticks = 180;
        longer_safer.traversal.coarse_path_length_cells = 20.0;
        longer_safer.traversal.horizontal_travel_cells = 18.0;
        let shorter_fragile = vector(complete_shaky(60, 20, 10, 30));

        assert!(matches!(
            compare_route_difficulty(
                &longer_safer,
                &shorter_fragile,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::Incomparable {
                left_harder,
                right_harder,
                ..
            } if !left_harder.is_empty() && !right_harder.is_empty()
        ));
    }

    #[test]
    fn missing_noise_evidence_is_insufficient() {
        let missing = vector(ShakyHandEvidence::Missing {
            reason: MissingEvidenceReason::ShakyHandStudyNotProvided,
        });
        let observed = vector(complete_shaky(100, 0, 0, 0));

        assert!(matches!(
            compare_route_difficulty(
                &missing,
                &observed,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::InsufficientEvidence { issues }
                if issues.contains(&InsufficientEvidenceIssue::MissingShakyHandStudy {
                    side: ComparisonSide::Left
                })
        ));
    }

    #[test]
    fn perfect_control_comparison_does_not_require_shaky_hand_evidence() {
        let easier = vector(ShakyHandEvidence::Missing {
            reason: MissingEvidenceReason::ShakyHandStudyNotProvided,
        });
        let mut harder = easier.clone();
        harder.hazards.minimum_clearance = HazardClearanceEvidence::Observed {
            pixels: 0,
            replay_tick: 50,
        };
        harder.hazards.pressure = 1.0;

        assert!(matches!(
            compare_perfect_control_route_difficulty(
                &easier,
                &harder,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::RightClearlyHarder { strict_coordinates }
                if strict_coordinates.as_ref() == [DifficultyCoordinate::HazardPressure]
        ));
    }

    #[test]
    fn perfect_control_comparison_retains_player_facing_tradeoffs() {
        let mut slower_safer = vector(ShakyHandEvidence::Missing {
            reason: MissingEvidenceReason::ShakyHandStudyNotProvided,
        });
        slower_safer.traversal.completion_ticks = 120;
        let mut faster_riskier = slower_safer.clone();
        faster_riskier.traversal.completion_ticks = 80;
        faster_riskier.hazards.minimum_clearance = HazardClearanceEvidence::Observed {
            pixels: 0,
            replay_tick: 50,
        };
        faster_riskier.hazards.pressure = 1.0;

        assert!(matches!(
            compare_perfect_control_route_difficulty(
                &slower_safer,
                &faster_riskier,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::Incomparable {
                left_harder,
                right_harder,
                applicability_mismatches,
            } if left_harder.as_ref() == [DifficultyCoordinate::CompletionTicks]
                && right_harder.as_ref() == [DifficultyCoordinate::HazardPressure]
                && applicability_mismatches.is_empty()
        ));
    }

    #[test]
    fn solver_effort_is_excluded_from_player_difficulty_comparison() {
        let first = vector(complete_shaky(90, 2, 3, 7));
        let mut second = first.clone();
        second.operational_solver_cost = OperationalSolverCost {
            expanded_nodes: 9_000_000,
            generated_nodes: 20_000_000,
            simulated_ticks: 80_000_000,
            deepest_path_ticks: 10_000,
        };

        assert_eq!(
            compare_route_difficulty(&first, &second, DifficultyComparisonTolerances::default()),
            RouteDifficultyComparison::EquivalentWithinTolerance
        );

        assert_eq!(
            compare_perfect_control_route_difficulty(
                &first,
                &second,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::EquivalentWithinTolerance
        );
    }

    #[test]
    fn one_sided_not_applicable_curve_is_incomparable_not_missing() {
        let mut not_applicable = vector(complete_shaky(90, 2, 3, 7));
        let observed = not_applicable.clone();
        let ShakyHandEvidence::Observed { curves, .. } = &mut not_applicable.timing.shaky_hand
        else {
            unreachable!();
        };
        curves[0].evidence = TimingPointEvidence::NotApplicable {
            requested_trials: 100,
        };

        assert!(matches!(
            compare_route_difficulty(
                &not_applicable,
                &observed,
                DifficultyComparisonTolerances::default()
            ),
            RouteDifficultyComparison::Incomparable {
                applicability_mismatches,
                ..
            } if !applicability_mismatches.is_empty()
        ));
    }
}
