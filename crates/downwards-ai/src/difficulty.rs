use std::{error::Error, fmt};

use downwards_core::{
    Action, DeathReason, JumpKind, Rect, Simulation, SimulationEvent, StateDigest, Tile,
};

use crate::{InconclusiveReason, ReplayDivergence, SearchStats, Solution, SolveOutcome};

/// Difficulty observations are generation and playtest heuristics. They are
/// deliberately not presented as a prediction of human difficulty.
pub const HEURISTIC_DIFFICULTY_DISCLAIMER: &str =
    "diagnostic heuristics only; calibrate against human playtests";

/// Version of the provisional component scoring and band thresholds.
pub const DIFFICULTY_HEURISTIC_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyInterpretation {
    /// The observations are useful for comparing generated rooms, but have
    /// not been calibrated against human players.
    HeuristicNotHumanDifficulty,
}

/// Controls how long a perturbed input stream may continue after the recorded
/// replay. Continuing the final held action avoids treating a harmless one- or
/// two-tick delay at the very end as an automatic failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DifficultyConfig {
    pub perturbation_grace_ticks: usize,
}

impl Default for DifficultyConfig {
    fn default() -> Self {
        Self {
            perturbation_grace_ticks: 12,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DifficultyReport {
    pub interpretation: DifficultyInterpretation,
    pub exit_id: String,
    /// Ticks from the replay's initial state to its first exit contact.
    pub completion_ticks: usize,
    /// Changes in the held semantic input, including the initial change from
    /// neutral input. This is an input-complexity heuristic, not a move count.
    pub meaningful_input_transitions: usize,
    /// Jump press edges in the replay, whether or not the simulation accepted
    /// the buffered jump.
    pub jump_presses: usize,
    /// Dash press edges in the replay, whether or not a dash was available.
    pub dash_presses: usize,
    /// Jump events authoritatively accepted by the simulation. Wall jumps are
    /// included here and also reported as the subset below.
    pub successful_jumps: usize,
    pub successful_wall_jumps: usize,
    /// Dash events authoritatively accepted by the simulation.
    pub successful_dashes: usize,
    pub search_effort: SearchStats,
    /// Death events in the solved replay before it reaches the exit.
    pub deaths: u32,
    /// Closest hazard encountered by the authoritative solved replay. `None`
    /// means no static hazard tile or currently-active timed hazard was
    /// relevant at any sampled step.
    pub minimum_hazard_clearance: Option<MinimumHazardClearance>,
    pub temporal_robustness: TemporalRobustness,
    pub provisional_complexity: ProvisionalComplexity,
}

/// The minimum integer-pixel edge clearance between the player and a relevant
/// hazard over an authoritative replay.
///
/// After each verified simulation step through the first reached exit, the
/// player's post-step AABB is compared with every static hazard tile and every
/// timed hazard active at that resulting room tick. For each pair, horizontal
/// and vertical edge gaps are clamped to zero and the larger gap is used. This
/// is the integer Chebyshev distance between the two AABBs: overlap and exact
/// edge contact both have zero clearance.
///
/// A death event contributes zero clearance for the hazard reported by the
/// simulation before its automatic reset. Thus every lethal intersection is
/// represented as zero even though the authoritative post-step player state
/// has already returned to spawn. Zero alone does not imply death, because the
/// core uses half-open rectangles and permits exact edge contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimumHazardClearance {
    pub pixels: u32,
    /// One-based frame index in the solution replay.
    pub replay_tick: usize,
    pub hazard: HazardReference,
}

/// Stable identity of the hazard that established a clearance diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardReference {
    StaticTile { tile_x: u16, tile_y: u16 },
    TimedHazard { hazard_index: usize },
}

/// An intentionally broad, uncalibrated ordering aid for generation batches.
/// It must not be presented to players as an objective difficulty rating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComplexityBand {
    Gentle,
    Standard,
    Technical,
}

/// Capped component scores make the provisional band explainable. The total
/// is their sum: 0..=4 is Gentle, 5..=10 Standard, and 11+ Technical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComplexityComponents {
    pub completion: u8,
    pub input_transitions: u8,
    pub traversal_verbs: u8,
    pub deaths: u8,
    pub temporal_fragility: u8,
    pub solver_effort: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisionalComplexity {
    pub interpretation: DifficultyInterpretation,
    pub band: ComplexityBand,
    pub component_score: u16,
    pub components: ComplexityComponents,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalRobustness {
    /// Applied perturbations. Boundary shifts which would start before the
    /// replay or finish after it are counted separately as not applicable.
    pub attempted_perturbations: usize,
    pub successful_perturbations: usize,
    /// `None` means the replay had no applicable input-boundary perturbations.
    pub successful_perturbation_ratio: Option<f64>,
    pub not_applicable_perturbations: usize,
    pub earliest_divergence: Option<PerturbationDivergence>,
    pub earliest_failure: Option<PerturbationFailureDiagnostic>,
    pub trials: Vec<PerturbationTrial>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerturbationDivergence {
    /// One-based tick within the replay at which the input transition begins.
    pub transition_tick: usize,
    /// Negative values advance the transition; positive values delay it.
    pub offset_ticks: i8,
    /// One-based tick whose authoritative state first differs from the replay.
    pub replay_tick: usize,
    pub expected: StateDigest,
    pub actual: StateDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerturbationTrial {
    pub transition_tick: usize,
    pub offset_ticks: i8,
    pub first_divergence: Option<PerturbationDivergence>,
    pub deaths: u32,
    pub first_death_tick: Option<usize>,
    pub outcome: PerturbationOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerturbationOutcome {
    ReachedExpectedExit {
        completion_ticks: usize,
    },
    ReachedDifferentExit {
        completion_ticks: usize,
        expected_exit_id: String,
        actual_exit_id: String,
    },
    DidNotReachExit {
        ticks_simulated: usize,
        expected_exit_id: String,
    },
    Died {
        death_tick: usize,
        expected_exit_id: String,
    },
}

impl PerturbationOutcome {
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::ReachedExpectedExit { .. })
    }

    const fn diagnostic_tick(&self) -> usize {
        match self {
            Self::ReachedExpectedExit { completion_ticks }
            | Self::ReachedDifferentExit {
                completion_ticks, ..
            } => *completion_ticks,
            Self::DidNotReachExit {
                ticks_simulated, ..
            } => *ticks_simulated,
            Self::Died { death_tick, .. } => *death_tick,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerturbationFailureDiagnostic {
    pub transition_tick: usize,
    pub offset_ticks: i8,
    pub first_divergence_tick: Option<usize>,
    /// Tick at which death/wrong-exit failure occurred, or the horizon at
    /// which an unreached exit was classified as a failed trial.
    pub diagnostic_tick: usize,
    pub deaths: u32,
    pub first_death_tick: Option<usize>,
    pub outcome: PerturbationOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DifficultyAnalysis {
    Solved(Box<DifficultyReport>),
    /// A bounded solver's non-success is not evidence that a room is
    /// impossible, so no difficulty score is fabricated for it.
    Inconclusive {
        reason: InconclusiveReason,
        search_effort: SearchStats,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DifficultyError {
    ReplayDiverged(ReplayDivergence),
    SolvedReplayDidNotReachExpectedExit {
        expected_exit_id: String,
        actual_exit_id: Option<String>,
    },
}

impl fmt::Display for DifficultyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReplayDiverged(error) => write!(formatter, "cannot analyze replay: {error}"),
            Self::SolvedReplayDidNotReachExpectedExit {
                expected_exit_id,
                actual_exit_id,
            } => write!(
                formatter,
                "solution replay should reach exit {expected_exit_id:?}, but reached {actual_exit_id:?}"
            ),
        }
    }
}

impl Error for DifficultyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReplayDiverged(error) => Some(error),
            Self::SolvedReplayDidNotReachExpectedExit { .. } => None,
        }
    }
}

impl From<ReplayDivergence> for DifficultyError {
    fn from(value: ReplayDivergence) -> Self {
        Self::ReplayDiverged(value)
    }
}

/// A borrowed case for [`analyze_batch`]. The ID is copied into the output so
/// batch callers can freely drop their input collection afterward.
#[derive(Clone, Copy, Debug)]
pub struct DifficultyCase<'a> {
    pub id: &'a str,
    pub initial: &'a Simulation,
    pub outcome: &'a SolveOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchDifficultyResult {
    pub id: String,
    pub analysis: Result<DifficultyAnalysis, DifficultyError>,
}

/// Analyze a solver result without turning an inconclusive search into a
/// misleading numeric difficulty estimate.
pub fn analyze_solve_outcome(
    initial: &Simulation,
    outcome: &SolveOutcome,
    config: &DifficultyConfig,
) -> Result<DifficultyAnalysis, DifficultyError> {
    match outcome {
        SolveOutcome::Solved(solution) => analyze_solution(initial, solution, config)
            .map(Box::new)
            .map(DifficultyAnalysis::Solved),
        SolveOutcome::Inconclusive { reason, stats } => Ok(DifficultyAnalysis::Inconclusive {
            reason: *reason,
            search_effort: *stats,
        }),
    }
}

/// Analyze independent cases in stable input order. Errors are retained per
/// case so one malformed replay does not abort a generation batch.
#[must_use]
pub fn analyze_batch<'a>(
    cases: impl IntoIterator<Item = DifficultyCase<'a>>,
    config: &DifficultyConfig,
) -> Vec<BatchDifficultyResult> {
    cases
        .into_iter()
        .map(|case| BatchDifficultyResult {
            id: case.id.to_owned(),
            analysis: analyze_solve_outcome(case.initial, case.outcome, config),
        })
        .collect()
}

/// Derive deterministic diagnostic heuristics from a verified solution replay.
pub fn analyze_solution(
    initial: &Simulation,
    solution: &Solution,
    config: &DifficultyConfig,
) -> Result<DifficultyReport, DifficultyError> {
    solution.replay.verify(initial)?;

    let baseline = inspect_baseline(initial, solution)?;
    // Ignore any inert frames a hand-authored replay happens to contain after
    // the first exit contact. Solver witnesses normally end on that frame.
    let actions: Vec<_> = solution
        .replay
        .actions()
        .take(baseline.completion_ticks)
        .collect();
    let transitions = input_transition_indices(&actions);
    let jump_presses = count_press_edges(&actions, |action| action.jump);
    let dash_presses = count_press_edges(&actions, dash_held);
    let temporal_robustness =
        analyze_temporal_robustness(initial, solution, config, &actions, &transitions);
    let provisional_complexity = provisional_complexity(
        baseline.completion_ticks,
        transitions.len(),
        baseline.successful_jumps,
        baseline.successful_wall_jumps,
        baseline.successful_dashes,
        baseline.deaths,
        solution.stats,
        &temporal_robustness,
    );

    Ok(DifficultyReport {
        interpretation: DifficultyInterpretation::HeuristicNotHumanDifficulty,
        exit_id: solution.exit_id.clone(),
        completion_ticks: baseline.completion_ticks,
        meaningful_input_transitions: transitions.len(),
        jump_presses,
        dash_presses,
        successful_jumps: baseline.successful_jumps,
        successful_wall_jumps: baseline.successful_wall_jumps,
        successful_dashes: baseline.successful_dashes,
        search_effort: solution.stats,
        deaths: baseline.deaths,
        minimum_hazard_clearance: baseline.minimum_hazard_clearance,
        temporal_robustness,
        provisional_complexity,
    })
}

/// Compute only the minimum hazard-clearance diagnostic for a solution.
///
/// The replay is verified before it is sampled and must reach the solution's
/// expected exit. `None` is returned when no static hazard exists and no timed
/// hazard is active during any sampled post-step state.
pub fn minimum_hazard_clearance(
    initial: &Simulation,
    solution: &Solution,
) -> Result<Option<MinimumHazardClearance>, DifficultyError> {
    solution.replay.verify(initial)?;
    inspect_baseline(initial, solution).map(|inspection| inspection.minimum_hazard_clearance)
}

#[derive(Clone, Copy)]
struct BaselineInspection {
    completion_ticks: usize,
    deaths: u32,
    successful_jumps: usize,
    successful_wall_jumps: usize,
    successful_dashes: usize,
    minimum_hazard_clearance: Option<MinimumHazardClearance>,
}

fn inspect_baseline(
    initial: &Simulation,
    solution: &Solution,
) -> Result<BaselineInspection, DifficultyError> {
    if initial.reached_exit() == Some(solution.exit_id.as_str()) {
        return Ok(BaselineInspection {
            completion_ticks: 0,
            deaths: 0,
            successful_jumps: 0,
            successful_wall_jumps: 0,
            successful_dashes: 0,
            minimum_hazard_clearance: None,
        });
    }

    let mut simulation = initial.clone();
    let static_hazards = static_hazard_bounds(initial);
    let mut deaths = 0_u32;
    let mut successful_jumps = 0_usize;
    let mut successful_wall_jumps = 0_usize;
    let mut successful_dashes = 0_usize;
    let mut completion_ticks = None;
    let mut minimum_hazard_clearance = None;
    for (frame_index, frame) in solution.replay.frames.iter().enumerate() {
        let report = simulation.step(frame.action);
        deaths = deaths.saturating_add(count_deaths(&report.events));
        for event in &report.events {
            match event {
                SimulationEvent::Jumped(kind) => {
                    successful_jumps += 1;
                    if matches!(kind, JumpKind::Wall { .. }) {
                        successful_wall_jumps += 1;
                    }
                }
                SimulationEvent::Dashed { .. } => successful_dashes += 1,
                _ => {}
            }
        }
        if completion_ticks.is_none() {
            sample_hazard_clearance(
                &simulation,
                &report.events,
                frame_index + 1,
                &static_hazards,
                &mut minimum_hazard_clearance,
            );
        }
        if completion_ticks.is_none() && simulation.reached_exit().is_some() {
            completion_ticks = Some(frame_index + 1);
        }
    }

    if simulation.reached_exit() != Some(solution.exit_id.as_str()) {
        return Err(DifficultyError::SolvedReplayDidNotReachExpectedExit {
            expected_exit_id: solution.exit_id.clone(),
            actual_exit_id: simulation.reached_exit().map(str::to_owned),
        });
    }

    Ok(BaselineInspection {
        completion_ticks: completion_ticks.unwrap_or(solution.replay.frames.len()),
        deaths,
        successful_jumps,
        successful_wall_jumps,
        successful_dashes,
        minimum_hazard_clearance,
    })
}

fn static_hazard_bounds(simulation: &Simulation) -> Vec<(HazardReference, Rect)> {
    let room = simulation.room();
    let mut hazards = Vec::new();
    for tile_y in 0..room.height() {
        for tile_x in 0..room.width() {
            if room.tile(tile_x, tile_y).is_some_and(Tile::is_hazard) {
                hazards.push((
                    HazardReference::StaticTile { tile_x, tile_y },
                    room.tile_bounds(tile_x, tile_y),
                ));
            }
        }
    }
    hazards
}

fn sample_hazard_clearance(
    simulation: &Simulation,
    events: &[SimulationEvent],
    replay_tick: usize,
    static_hazards: &[(HazardReference, Rect)],
    minimum: &mut Option<MinimumHazardClearance>,
) {
    if let Some(hazard) = events.iter().find_map(|event| match event {
        SimulationEvent::Died(DeathReason::Hazard { tile_x, tile_y }) => {
            Some(HazardReference::StaticTile {
                tile_x: *tile_x,
                tile_y: *tile_y,
            })
        }
        SimulationEvent::Died(DeathReason::TimedHazard { hazard_index }) => {
            Some(HazardReference::TimedHazard {
                hazard_index: *hazard_index,
            })
        }
        _ => None,
    }) {
        consider_hazard_clearance(minimum, 0, replay_tick, hazard);
        return;
    }

    let player = simulation.player().bounds();
    for &(hazard, bounds) in static_hazards {
        consider_hazard_clearance(
            minimum,
            aabb_edge_clearance_pixels(player, bounds),
            replay_tick,
            hazard,
        );
    }
    for (hazard_index, hazard) in simulation.active_timed_hazards() {
        consider_hazard_clearance(
            minimum,
            aabb_edge_clearance_pixels(player, hazard.bounds()),
            replay_tick,
            HazardReference::TimedHazard { hazard_index },
        );
    }
}

fn consider_hazard_clearance(
    minimum: &mut Option<MinimumHazardClearance>,
    pixels: u32,
    replay_tick: usize,
    hazard: HazardReference,
) {
    if minimum
        .as_ref()
        .is_none_or(|current| pixels < current.pixels)
    {
        *minimum = Some(MinimumHazardClearance {
            pixels,
            replay_tick,
            hazard,
        });
    }
}

fn aabb_edge_clearance_pixels(first: Rect, second: Rect) -> u32 {
    let horizontal_gap = if first.right() < second.x {
        second.x - first.right()
    } else if second.right() < first.x {
        first.x - second.right()
    } else {
        0
    };
    let vertical_gap = if first.bottom() < second.y {
        second.y - first.bottom()
    } else if second.bottom() < first.y {
        first.y - second.bottom()
    } else {
        0
    };
    horizontal_gap
        .max(vertical_gap)
        .try_into()
        .unwrap_or(u32::MAX)
}

#[allow(clippy::too_many_arguments)]
fn provisional_complexity(
    completion_ticks: usize,
    input_transitions: usize,
    successful_jumps: usize,
    successful_wall_jumps: usize,
    successful_dashes: usize,
    deaths: u32,
    search_effort: SearchStats,
    temporal_robustness: &TemporalRobustness,
) -> ProvisionalComplexity {
    let traversal_weight = successful_jumps
        .saturating_add(successful_wall_jumps)
        .saturating_add(successful_dashes.saturating_mul(2));
    let components = ComplexityComponents {
        completion: capped_component(completion_ticks / 60, 4),
        input_transitions: capped_component(input_transitions / 4, 4),
        traversal_verbs: capped_component(traversal_weight / 2, 4),
        deaths: capped_component(deaths.saturating_mul(2) as usize, 6),
        temporal_fragility: temporal_robustness
            .successful_perturbation_ratio
            .map_or(0, |ratio| {
                ((1.0 - ratio).clamp(0.0, 1.0) * 8.0).round() as u8
            }),
        solver_effort: match search_effort.simulated_ticks {
            0..10_000 => 0,
            10_000..50_000 => 1,
            50_000..200_000 => 2,
            200_000..500_000 => 3,
            _ => 4,
        },
    };
    let component_score = u16::from(components.completion)
        + u16::from(components.input_transitions)
        + u16::from(components.traversal_verbs)
        + u16::from(components.deaths)
        + u16::from(components.temporal_fragility)
        + u16::from(components.solver_effort);
    let band = match component_score {
        0..=4 => ComplexityBand::Gentle,
        5..=10 => ComplexityBand::Standard,
        _ => ComplexityBand::Technical,
    };
    ProvisionalComplexity {
        interpretation: DifficultyInterpretation::HeuristicNotHumanDifficulty,
        band,
        component_score,
        components,
    }
}

fn capped_component(value: usize, cap: u8) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX).min(cap)
}

fn analyze_temporal_robustness(
    initial: &Simulation,
    solution: &Solution,
    config: &DifficultyConfig,
    actions: &[Action],
    transitions: &[usize],
) -> TemporalRobustness {
    const OFFSETS: [i8; 4] = [-2, -1, 1, 2];

    let mut trials = Vec::with_capacity(transitions.len() * OFFSETS.len());
    let mut not_applicable_perturbations = 0;
    for &transition_index in transitions {
        for offset_ticks in OFFSETS {
            let Some(perturbed_actions) =
                perturb_transition(actions, transition_index, offset_ticks)
            else {
                not_applicable_perturbations += 1;
                continue;
            };
            trials.push(run_perturbation(
                initial,
                solution,
                config,
                transition_index,
                offset_ticks,
                &perturbed_actions,
            ));
        }
    }

    let successful_perturbations = trials
        .iter()
        .filter(|trial| trial.outcome.succeeded())
        .count();
    let attempted_perturbations = trials.len();
    let successful_perturbation_ratio = (attempted_perturbations != 0)
        .then(|| successful_perturbations as f64 / attempted_perturbations as f64);
    let earliest_divergence = trials
        .iter()
        .filter_map(|trial| trial.first_divergence.as_ref())
        .min_by_key(|divergence| {
            (
                divergence.replay_tick,
                divergence.transition_tick,
                offset_order(divergence.offset_ticks),
            )
        })
        .cloned();
    let earliest_failure = trials
        .iter()
        .filter(|trial| !trial.outcome.succeeded())
        .min_by_key(|trial| {
            (
                trial
                    .first_divergence
                    .as_ref()
                    .map_or(usize::MAX, |divergence| divergence.replay_tick),
                trial.transition_tick,
                offset_order(trial.offset_ticks),
            )
        })
        .map(failure_diagnostic);

    TemporalRobustness {
        attempted_perturbations,
        successful_perturbations,
        successful_perturbation_ratio,
        not_applicable_perturbations,
        earliest_divergence,
        earliest_failure,
        trials,
    }
}

fn input_transition_indices(actions: &[Action]) -> Vec<usize> {
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

fn count_press_edges(actions: &[Action], held: impl Fn(Action) -> bool) -> usize {
    let mut previous = false;
    actions
        .iter()
        .filter(|&&action| {
            let current = held(action);
            let pressed = current && !previous;
            previous = current;
            pressed
        })
        .count()
}

const fn dash_held(action: Action) -> bool {
    action.dash
}

fn perturb_transition(
    actions: &[Action],
    transition_index: usize,
    offset_ticks: i8,
) -> Option<Vec<Action>> {
    debug_assert!(matches!(offset_ticks, -2 | -1 | 1 | 2));
    let previous = transition_index
        .checked_sub(1)
        .map_or_else(Action::default, |index| actions[index]);
    let next = actions[transition_index];
    let amount = usize::from(offset_ticks.unsigned_abs());
    let mut perturbed = actions.to_vec();

    if offset_ticks < 0 {
        let start = transition_index.checked_sub(amount)?;
        perturbed[start..transition_index].fill(next);
    } else {
        let end = transition_index.checked_add(amount)?;
        if end > actions.len() {
            return None;
        }
        perturbed[transition_index..end].fill(previous);
    }
    Some(perturbed)
}

fn run_perturbation(
    initial: &Simulation,
    solution: &Solution,
    config: &DifficultyConfig,
    transition_index: usize,
    offset_ticks: i8,
    actions: &[Action],
) -> PerturbationTrial {
    let mut simulation = initial.clone();
    let mut first_divergence = None;
    let mut deaths = 0_u32;
    let mut first_death_tick = None;
    let mut reached = initial.reached_exit().map(str::to_owned);
    let mut completion_ticks = reached.as_ref().map(|_| 0);
    let total_ticks = actions
        .len()
        .saturating_add(config.perturbation_grace_ticks);
    let final_action = actions.last().copied().unwrap_or_default();

    for tick_index in 0..total_ticks {
        if reached.is_some() {
            break;
        }
        let action = actions.get(tick_index).copied().unwrap_or(final_action);
        let report = simulation.step(action);
        let tick_deaths = count_deaths(&report.events);
        if tick_deaths != 0 && first_death_tick.is_none() {
            first_death_tick = Some(tick_index + 1);
        }
        deaths = deaths.saturating_add(tick_deaths);

        if first_divergence.is_none()
            && let Some(frame) = solution.replay.frames.get(tick_index)
            && frame.expected_digest != report.digest
        {
            first_divergence = Some(PerturbationDivergence {
                transition_tick: transition_index + 1,
                offset_ticks,
                replay_tick: tick_index + 1,
                expected: frame.expected_digest,
                actual: report.digest,
            });
        }
        if let Some(exit_id) = simulation.reached_exit() {
            reached = Some(exit_id.to_owned());
            completion_ticks = Some(tick_index + 1);
        }
        if tick_deaths != 0 {
            break;
        }
    }

    let ticks_simulated = completion_ticks.unwrap_or(total_ticks);
    let outcome = if let Some(death_tick) = first_death_tick {
        PerturbationOutcome::Died {
            death_tick,
            expected_exit_id: solution.exit_id.clone(),
        }
    } else {
        match (reached, completion_ticks) {
            (Some(actual_exit_id), Some(completion_ticks))
                if actual_exit_id == solution.exit_id =>
            {
                PerturbationOutcome::ReachedExpectedExit { completion_ticks }
            }
            (Some(actual_exit_id), Some(completion_ticks)) => {
                PerturbationOutcome::ReachedDifferentExit {
                    completion_ticks,
                    expected_exit_id: solution.exit_id.clone(),
                    actual_exit_id,
                }
            }
            _ => PerturbationOutcome::DidNotReachExit {
                ticks_simulated,
                expected_exit_id: solution.exit_id.clone(),
            },
        }
    };

    PerturbationTrial {
        transition_tick: transition_index + 1,
        offset_ticks,
        first_divergence,
        deaths,
        first_death_tick,
        outcome,
    }
}

fn failure_diagnostic(trial: &PerturbationTrial) -> PerturbationFailureDiagnostic {
    PerturbationFailureDiagnostic {
        transition_tick: trial.transition_tick,
        offset_ticks: trial.offset_ticks,
        first_divergence_tick: trial
            .first_divergence
            .as_ref()
            .map(|divergence| divergence.replay_tick),
        diagnostic_tick: trial.outcome.diagnostic_tick(),
        deaths: trial.deaths,
        first_death_tick: trial.first_death_tick,
        outcome: trial.outcome.clone(),
    }
}

fn count_deaths(events: &[SimulationEvent]) -> u32 {
    events
        .iter()
        .filter(|event| matches!(event, SimulationEvent::Died(_)))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

const fn offset_order(offset: i8) -> u8 {
    match offset {
        -2 => 0,
        -1 => 1,
        1 => 2,
        2 => 3,
        _ => u8::MAX,
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{Exit, Point, Rect, Room, Tile, TimedHazard};

    use super::*;
    use crate::{Replay, SearchStats};

    fn retry_only_room() -> Room {
        let width = 32_usize;
        let height = 18_usize;
        let mut tiles = vec![Tile::Empty; width * height];
        for x in 0..width {
            tiles[16 * width + x] = Tile::Solid;
        }
        tiles[16 * width + 4] = Tile::HazardUp;
        Room::new(
            "difficulty-retry-only",
            "Difficulty retry-only",
            width as u16,
            height as u16,
            10,
            tiles,
            Point::new(20, 148),
            vec![Exit {
                id: "west".into(),
                bounds: Rect::new(0, 140, 10, 20),
                destination: None,
                destination_entrance: None,
            }],
        )
        .unwrap()
    }

    fn clearance_room(
        hazard_tile: Option<(usize, usize)>,
        timed_hazards: Vec<TimedHazard>,
    ) -> Room {
        let width = 32_usize;
        let height = 18_usize;
        let mut tiles = vec![Tile::Empty; width * height];
        for x in 0..width {
            tiles[16 * width + x] = Tile::Solid;
        }
        if let Some((x, y)) = hazard_tile {
            tiles[y * width + x] = Tile::HazardUp;
        }
        Room::new(
            "clearance",
            "Clearance",
            width as u16,
            height as u16,
            10,
            tiles,
            Point::new(60, 148),
            vec![Exit {
                id: "east".into(),
                bounds: Rect::new(90, 140, 10, 20),
                destination: None,
                destination_entrance: None,
            }],
        )
        .unwrap()
        .with_objects(timed_hazards, Vec::new())
        .unwrap()
    }

    fn clearance_over_actions(
        initial: &Simulation,
        actions: impl IntoIterator<Item = Action>,
    ) -> Option<MinimumHazardClearance> {
        let static_hazards = static_hazard_bounds(initial);
        let mut simulation = initial.clone();
        let mut minimum = None;
        for (frame_index, action) in actions.into_iter().enumerate() {
            let report = simulation.step(action);
            sample_hazard_clearance(
                &simulation,
                &report.events,
                frame_index + 1,
                &static_hazards,
                &mut minimum,
            );
        }
        minimum
    }

    #[test]
    fn route_nearer_a_static_hazard_has_lower_clearance_than_safer_route() {
        let initial = Simulation::new(clearance_room(Some((12, 15)), Vec::new()));
        let toward_hazard = Action {
            move_x: 1,
            ..Action::default()
        };
        let away_from_hazard = Action {
            move_x: -1,
            ..Action::default()
        };

        let near = clearance_over_actions(&initial, [toward_hazard; 16]).unwrap();
        let safe = clearance_over_actions(&initial, [away_from_hazard; 16]).unwrap();

        assert!(near.pixels < safe.pixels, "near={near:?}, safe={safe:?}");
        assert!(matches!(
            near.hazard,
            HazardReference::StaticTile {
                tile_x: 12,
                tile_y: 15
            }
        ));
    }

    #[test]
    fn timed_hazard_only_contributes_during_its_active_phase() {
        let timed = TimedHazard::new(Rect::new(200, 140, 8, 12), 4, 1, 0).unwrap();
        let initial = Simulation::new(clearance_room(None, vec![timed]));

        assert_eq!(
            clearance_over_actions(&initial, [Action::default(); 3]),
            None
        );
        let clearance =
            clearance_over_actions(&initial, [Action::default(); 4]).expect("active on tick four");
        assert_eq!(clearance.replay_tick, 4);
        assert_eq!(
            clearance.hazard,
            HazardReference::TimedHazard { hazard_index: 0 }
        );
    }

    #[test]
    fn lethal_intersection_is_zero_even_though_step_resets_player() {
        let initial = Simulation::new(clearance_room(Some((8, 15)), Vec::new()));
        let right = Action {
            move_x: 1,
            ..Action::default()
        };

        let clearance = clearance_over_actions(&initial, [right; 20]).unwrap();

        assert_eq!(clearance.pixels, 0);
        assert_eq!(
            clearance.hazard,
            HazardReference::StaticTile {
                tile_x: 8,
                tile_y: 15
            }
        );
    }

    #[test]
    fn difficulty_report_and_standalone_api_export_clearance() {
        let timed = TimedHazard::new(Rect::new(200, 140, 8, 12), 4, 1, 0).unwrap();
        let initial = Simulation::new(clearance_room(None, vec![timed]));
        let right = Action {
            move_x: 1,
            ..Action::default()
        };
        let solution = Solution {
            exit_id: "east".into(),
            replay: Replay::record(&initial, [right; 40]),
            stats: SearchStats::default(),
        };

        let standalone = minimum_hazard_clearance(&initial, &solution).unwrap();
        let report = analyze_solution(&initial, &solution, &DifficultyConfig::default()).unwrap();

        assert!(standalone.is_some());
        assert_eq!(report.minimum_hazard_clearance, standalone);
    }

    #[test]
    fn perturbation_death_is_failure_even_if_later_inputs_could_recover() {
        let initial = Simulation::new(retry_only_room());
        let left = Action {
            move_x: -1,
            ..Action::default()
        };
        let solution = Solution {
            exit_id: "west".into(),
            replay: Replay::record(&initial, [left; 20]),
            stats: SearchStats::default(),
        };
        solution.replay.verify(&initial).unwrap();

        let right = Action {
            move_x: 1,
            ..Action::default()
        };
        let mut retry_actions = vec![right; 20];
        retry_actions.extend([left; 30]);
        let mut permissive = initial.clone();
        for &action in &retry_actions {
            permissive.step(action);
        }
        assert!(permissive.deaths() > 0);
        assert_eq!(permissive.reached_exit(), Some("west"));

        let trial = run_perturbation(
            &initial,
            &solution,
            &DifficultyConfig {
                perturbation_grace_ticks: 0,
            },
            0,
            1,
            &retry_actions,
        );
        assert_eq!(trial.deaths, 1);
        assert!(!trial.outcome.succeeded());
        assert!(matches!(trial.outcome, PerturbationOutcome::Died { .. }));
    }
}
