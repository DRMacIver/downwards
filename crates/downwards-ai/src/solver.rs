use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt,
    rc::Rc,
};

use downwards_core::{
    AbilitySet, Action, BoundarySide, DASH_TICKS, DashDirection, ONE_WAY_DROP_TICKS, Rect,
    SUBPIXELS_PER_PIXEL, Simulation, SimulationEvent, WallSide,
};

use crate::{Replay, ReplayFrame, digest_events};

/// Version of the deterministic search policy and built-in probe vocabulary.
///
/// Catalogue artifacts record this separately from numeric [`SolverConfig`]
/// fields because changing probe behavior can change the first exact witness
/// even when every configured budget remains identical.
pub const SOLVER_POLICY_VERSION: u32 = 3;

/// A short sequence of held semantic inputs. Search operates over these
/// chunks but every action is still applied one tick at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionMacro {
    pub name: String,
    pub actions: Vec<Action>,
}

impl ActionMacro {
    #[must_use]
    pub fn held(name: impl Into<String>, action: Action, ticks: u8) -> Self {
        Self {
            name: name.into(),
            actions: vec![action; usize::from(ticks)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolverConfig {
    /// Maximum states removed from the beam and expanded.
    pub max_expanded_nodes: usize,
    /// Maximum core `Simulation::step` calls across the whole search.
    pub max_simulated_ticks: usize,
    /// Maximum length of a candidate witness.
    pub max_ticks_per_path: usize,
    /// Maximum states retained after each macro-depth.
    pub beam_width: usize,
    /// Position bucket size in subpixels for visited-state pruning.
    pub position_quantum: i32,
    /// Velocity bucket size in subpixels per tick.
    pub velocity_quantum: i32,
    /// First try a small deterministic policy suite: monotonic running,
    /// periodic run/jump cadences, staged detour-and-home climbing, wall
    /// climbing, and loadout-aware dashing. Static authored routes then cost
    /// a few rollouts instead of a full beam; any failure falls back to the
    /// authoritative search.
    pub probe_direct_routes: bool,
    /// Maximum nodes spent on a baseline-only preview before capability-rich
    /// search. At most half of the global remaining budget is used, so an
    /// impossible baseline route cannot starve required dash exploration.
    pub baseline_preview_max_expanded_nodes: usize,
    /// Maximum simulated ticks spent on the baseline-only preview. This is
    /// likewise capped to half of the remaining global tick budget.
    pub baseline_preview_max_simulated_ticks: usize,
    pub macros: Vec<ActionMacro>,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self::for_abilities(AbilitySet::NONE)
    }
}

impl SolverConfig {
    /// Construct the deterministic movement vocabulary appropriate for a
    /// scenario's authoritative ability loadout. Dash adds eight committed
    /// press/release macros; baseline and wall-jump-only searches avoid those
    /// branches entirely. The ordinary jump macros already discover wall
    /// jumps when the simulation enables that ability.
    #[must_use]
    pub fn for_abilities(abilities: AbilitySet) -> Self {
        let mut macros = baseline_macros();
        if abilities.dash {
            macros.extend(dash_macros());
        }
        Self {
            // A full-width room takes roughly 200 ticks to cross at the
            // prototype's current run speed. The direct-route probe handles
            // that common case, while this focused fallback beam retains the
            // precision vocabulary needed by obstacle rooms.
            max_expanded_nodes: 60_000,
            max_simulated_ticks: 2_000_000,
            max_ticks_per_path: 600,
            beam_width: 96,
            position_quantum: 2 * SUBPIXELS_PER_PIXEL,
            velocity_quantum: SUBPIXELS_PER_PIXEL / 2,
            probe_direct_routes: true,
            baseline_preview_max_expanded_nodes: 10_000,
            baseline_preview_max_simulated_ticks: 250_000,
            macros,
        }
    }
}

fn baseline_macros() -> Vec<ActionMacro> {
    const PRECISION_TICKS: u8 = 4;
    let mut macros = Vec::with_capacity(9);
    for (name, move_x) in [("left", -1), ("idle", 0), ("right", 1)] {
        macros.push(ActionMacro::held(
            name,
            Action {
                move_x,
                ..Action::default()
            },
            PRECISION_TICKS,
        ));
    }
    for (name, move_x) in [("jump-left", -1), ("jump", 0), ("jump-right", 1)] {
        macros.push(ActionMacro::held(
            name,
            Action {
                move_x,
                jump: true,
                ..Action::default()
            },
            PRECISION_TICKS,
        ));
    }
    for (name, move_x) in [("drop-left", -1), ("drop", 0), ("drop-right", 1)] {
        let release = Action {
            move_x,
            move_y: 1,
            ..Action::default()
        };
        let press = Action {
            jump: true,
            ..release
        };
        let mut actions = vec![press];
        actions.resize(usize::from(ONE_WAY_DROP_TICKS) + 1, release);
        macros.push(ActionMacro {
            name: name.to_owned(),
            actions,
        });
    }
    macros
}

fn dash_macros() -> impl IntoIterator<Item = ActionMacro> {
    [
        ("dash-up", 0, -1),
        ("dash-up-right", 1, -1),
        ("dash-right", 1, 0),
        ("dash-down-right", 1, 1),
        ("dash-down", 0, 1),
        ("dash-down-left", -1, 1),
        ("dash-left", -1, 0),
        ("dash-up-left", -1, -1),
    ]
    .map(|(name, move_x, move_y)| {
        let release = Action {
            move_x,
            move_y,
            ..Action::default()
        };
        let press = Action {
            dash: true,
            ..release
        };
        let mut actions = vec![press];
        actions.resize(usize::from(DASH_TICKS), release);
        ActionMacro {
            name: name.to_owned(),
            actions,
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolverConfigError {
    ZeroBeamWidth,
    ZeroPathHorizon,
    InvalidPositionQuantum(i32),
    InvalidVelocityQuantum(i32),
    NoMacros,
    EmptyMacro { index: usize, name: String },
    RestartMacro { index: usize, name: String },
}

impl fmt::Display for SolverConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroBeamWidth => write!(formatter, "solver beam_width must be greater than zero"),
            Self::ZeroPathHorizon => {
                write!(
                    formatter,
                    "solver max_ticks_per_path must be greater than zero"
                )
            }
            Self::InvalidPositionQuantum(value) => {
                write!(formatter, "position_quantum must be positive, got {value}")
            }
            Self::InvalidVelocityQuantum(value) => {
                write!(formatter, "velocity_quantum must be positive, got {value}")
            }
            Self::NoMacros => write!(formatter, "solver needs at least one action macro"),
            Self::EmptyMacro { index, name } => {
                write!(formatter, "action macro {index} ({name:?}) is empty")
            }
            Self::RestartMacro { index, name } => write!(
                formatter,
                "action macro {index} ({name:?}) contains restart; solver macros may not reset the room"
            ),
        }
    }
}

impl Error for SolverConfigError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub expanded_nodes: usize,
    pub generated_nodes: usize,
    pub simulated_ticks: usize,
    pub deepest_path_ticks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InconclusiveReason {
    NoExitsDefined,
    ExpandedNodeBudget,
    SimulatedTickBudget,
    PathHorizon,
    FrontierExhausted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Solution {
    pub exit_id: String,
    pub replay: Replay,
    pub stats: SearchStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolveOutcome {
    Solved(Solution),
    /// Search limits and beam pruning mean this is never a proof that the
    /// room is impossible.
    Inconclusive {
        reason: InconclusiveReason,
        stats: SearchStats,
    },
}

/// A precise reachability objective for the game-playing search.
///
/// `AnyExit` preserves room-completion semantics across legacy exits and
/// boundary doors. Exact exits, doors, and pickups remain separate variants,
/// so one trigger kind can never be mistaken for another in a typed witness.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SearchTarget {
    AnyExit,
    Exit(String),
    Door(String),
    Pickup(String),
}

impl SearchTarget {
    #[must_use]
    pub fn exit(id: impl Into<String>) -> Self {
        Self::Exit(id.into())
    }

    #[must_use]
    pub fn pickup(id: impl Into<String>) -> Self {
        Self::Pickup(id.into())
    }

    #[must_use]
    pub fn door(id: impl Into<String>) -> Self {
        Self::Door(id.into())
    }
}

/// The exact trigger reached by a targeted solver witness.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReachedTarget {
    Exit(String),
    Door(String),
    Pickup(String),
}

/// An exact per-tick positive witness for a [`SearchTarget`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSolution {
    pub target: SearchTarget,
    pub reached: ReachedTarget,
    pub replay: Replay,
    pub stats: SearchStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSolveOutcome {
    Solved(TargetSolution),
    /// Search limits and beam pruning mean this is never proof that the
    /// target is impossible to reach.
    Inconclusive {
        reason: InconclusiveReason,
        stats: SearchStats,
    },
}

/// One entry in a shared multi-target search, kept in the same order as the
/// caller's requested targets.
///
/// The outcome's [`SearchStats`] are a snapshot of cumulative shared work at
/// the instant that target was found. An inconclusive entry receives the
/// final cumulative stats for the shared search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchTargetResult {
    pub target: SearchTarget,
    pub outcome: TargetSolveOutcome,
}

/// Results and total work from one shared exploration frontier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchTargetSolveOutcome {
    pub results: Vec<BatchTargetResult>,
    pub stats: SearchStats,
}

/// Version of the deterministic direct-controller diagnostic vocabulary.
///
/// This is intentionally separate from [`SOLVER_POLICY_VERSION`]: auditing
/// every built-in probe does not change which witness the normal solver
/// returns.
pub const DIRECT_PROBE_AUDIT_VERSION: u32 = 2;

/// Stable identity of one built-in direct-controller policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DirectProbePolicy {
    DropThrough,
    Run,
    PeriodicJump {
        period: usize,
        hold_ticks: usize,
        phase: usize,
    },
    ReactiveJump {
        lookahead_pixels: i32,
        hold_ticks: u8,
    },
    AutoJump,
    WallClimb,
    BufferedWallClimb,
    Dash {
        move_y: i8,
        jump_period: usize,
        jump_hold_ticks: usize,
        dash_delay_ticks: usize,
    },
    StagedDash {
        period: usize,
        jump_start: usize,
        jump_hold_ticks: usize,
        dash_tick: usize,
    },
    ReactiveDash {
        lookahead_pixels: i32,
        move_y: i8,
    },
    DetourJump {
        turn_tick: usize,
        period: usize,
        hold_ticks: usize,
        phase: usize,
    },
    DetourClimb {
        turn_tick: usize,
        lookahead_pixels: i32,
        hold_ticks: u8,
    },
    /// First travel away from an elevated exact target, then reverse and
    /// home horizontally while auto-jumping. This composes the two halves of
    /// a shelf-return route without assuming that continuing in the return
    /// direction is safe once the player reaches narrow upper platforms.
    DetourHomingClimb {
        turn_tick: usize,
        target_center_x: i32,
        deadzone_pixels: i32,
        hold_ticks: u8,
    },
}

/// Position of a probe in the canonical, target-interleaved audit order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirectProbeProvenance {
    pub ordinal: usize,
    pub move_x: i8,
    pub policy: DirectProbePolicy,
}

/// One unique exact action prefix that reached a requested target.
///
/// Multiple probes can produce the same prefix. Such aliases are folded into
/// `probes` in discovery order. An empty `probes` list means the target was
/// already reached in the supplied initial simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectProbeWitness {
    pub target_index: usize,
    pub target: SearchTarget,
    pub reached: ReachedTarget,
    pub replay: Replay,
    pub stats_at_first_discovery: SearchStats,
    pub probes: Vec<DirectProbeProvenance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectProbeBudgetLimit {
    ExpandedNodes,
    SimulatedTicks,
}

/// Whether every probe in the finite built-in vocabulary was evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectProbeAuditStatus {
    Complete,
    BudgetLimited(DirectProbeBudgetLimit),
}

/// All distinct positive witnesses found by a bounded direct-controller
/// audit, plus the total work needed to obtain them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectProbeAudit {
    pub witnesses: Vec<DirectProbeWitness>,
    pub stats: SearchStats,
    pub status: DirectProbeAuditStatus,
}

/// A malformed or undefined target is distinct from a bounded search that
/// simply failed to find a witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSolveError {
    SolverConfiguration(SolverConfigError),
    EmptyExitId,
    ExitNotDefined { exit_id: String },
    EmptyDoorId,
    DoorNotDefined { door_id: String },
    EmptyPickupId,
    PickupNotDefined { pickup_id: String },
}

impl fmt::Display for TargetSolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SolverConfiguration(error) => {
                write!(formatter, "invalid solver configuration: {error}")
            }
            Self::EmptyExitId => write!(formatter, "target exit ID must not be empty"),
            Self::ExitNotDefined { exit_id } => {
                write!(
                    formatter,
                    "target exit {exit_id:?} is not defined by the room"
                )
            }
            Self::EmptyDoorId => write!(formatter, "target door ID must not be empty"),
            Self::DoorNotDefined { door_id } => {
                write!(
                    formatter,
                    "target door {door_id:?} is not defined by the room"
                )
            }
            Self::EmptyPickupId => write!(formatter, "target pickup ID must not be empty"),
            Self::PickupNotDefined { pickup_id } => {
                write!(
                    formatter,
                    "target pickup {pickup_id:?} is not defined by the room"
                )
            }
        }
    }
}

impl Error for TargetSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SolverConfiguration(error) => Some(error),
            _ => None,
        }
    }
}

/// Configuration and target-definition errors detected before a batch search
/// begins. Target errors carry the original request index, so invalid input
/// cannot be confused with a bounded inconclusive search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchTargetSolveError {
    SolverConfiguration(SolverConfigError),
    InvalidTarget {
        index: usize,
        target: SearchTarget,
        error: TargetSolveError,
    },
}

impl fmt::Display for BatchTargetSolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SolverConfiguration(error) => {
                write!(formatter, "invalid solver configuration: {error}")
            }
            Self::InvalidTarget {
                index,
                target,
                error,
            } => write!(
                formatter,
                "invalid batch target at index {index} ({target:?}): {error}"
            ),
        }
    }
}

impl Error for BatchTargetSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SolverConfiguration(error) => Some(error),
            Self::InvalidTarget { error, .. } => Some(error),
        }
    }
}

#[derive(Clone, Copy)]
enum ResolvedTarget<'a> {
    AnyExit,
    Exit {
        id: &'a str,
        bounds: Rect,
    },
    Door {
        id: &'a str,
        bounds: Rect,
        side: BoundarySide,
    },
    Pickup {
        id: &'a str,
        index: usize,
        bounds: Rect,
    },
}

struct BatchSearch<'a> {
    targets: Vec<SearchTarget>,
    resolved: Vec<ResolvedTarget<'a>>,
    outcomes: Vec<Option<TargetSolveOutcome>>,
}

impl<'a> BatchSearch<'a> {
    fn new(targets: &[SearchTarget], resolved: Vec<ResolvedTarget<'a>>) -> Self {
        Self {
            targets: targets.to_vec(),
            outcomes: vec![None; targets.len()],
            resolved,
        }
    }

    fn unresolved_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.outcomes
            .iter()
            .enumerate()
            .filter_map(|(index, outcome)| outcome.is_none().then_some(index))
    }

    fn is_complete(&self) -> bool {
        self.outcomes.iter().all(Option::is_some)
    }

    fn newly_reached(&self, simulation: &Simulation) -> Vec<(usize, ReachedTarget)> {
        self.unresolved_indices()
            .filter_map(|index| {
                reached_target(simulation, self.resolved[index]).map(|reached| (index, reached))
            })
            .collect()
    }

    fn record_solutions(
        &mut self,
        initial: &Simulation,
        reached: Vec<(usize, ReachedTarget)>,
        actions: &[Action],
        stats: SearchStats,
    ) {
        if reached.is_empty() {
            return;
        }
        let replay = Replay::record(initial, actions.iter().copied());
        for (index, reached) in reached {
            self.outcomes[index] = Some(TargetSolveOutcome::Solved(TargetSolution {
                target: self.targets[index].clone(),
                reached,
                replay: replay.clone(),
                stats,
            }));
        }
    }

    fn mark_inconclusive(&mut self, reason: InconclusiveReason, stats: SearchStats) {
        for outcome in &mut self.outcomes {
            if outcome.is_none() {
                *outcome = Some(target_inconclusive(reason, stats));
            }
        }
    }

    fn finish(self, stats: SearchStats) -> BatchTargetSolveOutcome {
        let results = self
            .targets
            .into_iter()
            .zip(self.outcomes)
            .map(|(target, outcome)| BatchTargetResult {
                target,
                outcome: outcome.expect("every batch target has a terminal outcome"),
            })
            .collect();
        BatchTargetSolveOutcome { results, stats }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchSearchTermination {
    Complete,
    Stopped(InconclusiveReason),
}

struct Node {
    simulation: Simulation,
    path: Option<Rc<PathSegment>>,
    path_ticks: usize,
    previous_action: Action,
    score: i64,
    serial: u64,
}

/// Search nodes share immutable macro-index chains. An exact per-tick action
/// vector is materialized only for the successful witness, rather than cloned
/// for every child in the beam.
struct PathSegment {
    parent: Option<Rc<Self>>,
    macro_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FutureStateKey {
    position_x: i32,
    position_y: i32,
    velocity_x: i32,
    velocity_y: i32,
    grounded: bool,
    coyote_ticks: u8,
    jump_buffer_ticks: u8,
    previous_move_x: i8,
    previous_move_y: i8,
    previous_jump: bool,
    previous_dash: bool,
    wall_contact: Option<WallSide>,
    wall_sliding: bool,
    facing: i8,
    dash_available: bool,
    dash_ticks: u8,
    dash_direction: Option<DashDirection>,
    one_way_drop_ticks: u8,
    jump_hold_ticks: u8,
    /// Exact time is future-relevant only for rooms whose hazards use it.
    room_tick: Option<u64>,
    /// Collection state can affect optional-route and scripted objectives even
    /// when current prototype pickups do not directly alter physics.
    collected_pickups: Box<[bool]>,
}

/// Search for any exit and return an exact per-tick witness on success.
/// Every non-success outcome is deliberately labelled inconclusive.
pub fn solve(
    initial: &Simulation,
    config: &SolverConfig,
) -> Result<SolveOutcome, SolverConfigError> {
    validate_config(config)?;
    let outcome = solve_resolved(initial, config, ResolvedTarget::AnyExit);
    Ok(match outcome {
        TargetSolveOutcome::Solved(solution) => {
            let exit_id = match solution.reached {
                ReachedTarget::Exit(id) | ReachedTarget::Door(id) => id,
                ReachedTarget::Pickup(_) => {
                    unreachable!("an any-exit search cannot reach a pickup")
                }
            };
            SolveOutcome::Solved(Solution {
                exit_id,
                replay: solution.replay,
                stats: solution.stats,
            })
        }
        TargetSolveOutcome::Inconclusive { reason, stats } => {
            SolveOutcome::Inconclusive { reason, stats }
        }
    })
}

/// Search for one explicitly typed exit, door, or pickup target.
///
/// On success the replay ends on the first tick that reaches the requested
/// trigger. A different exit is a dead end for an exact-target search because
/// the authoritative simulation stops advancing after room completion.
pub fn solve_target(
    initial: &Simulation,
    target: SearchTarget,
    config: &SolverConfig,
) -> Result<TargetSolveOutcome, TargetSolveError> {
    validate_config(config).map_err(TargetSolveError::SolverConfiguration)?;
    let resolved = resolve_target(initial, &target)?;
    Ok(solve_resolved_with_public_target(
        initial, config, resolved, target,
    ))
}

/// Search for several exact targets with one shared exploration frontier.
///
/// Results preserve request order (including duplicate targets). A branch
/// that reaches any exit or boundary door is terminal in the authoritative
/// simulation: requested targets reached on that tick are recorded, then the
/// solver continues with the other frontier branches. Pickup witnesses may be
/// recorded partway through a branch that subsequently reaches another
/// target.
///
/// As with [`solve_target`], every witness is an exact replay and every
/// bounded non-success is explicitly inconclusive. Shared beam pruning means
/// a batch search is not a proof that an unresolved target is impossible.
pub fn solve_targets(
    initial: &Simulation,
    targets: &[SearchTarget],
    config: &SolverConfig,
) -> Result<BatchTargetSolveOutcome, BatchTargetSolveError> {
    validate_config(config).map_err(BatchTargetSolveError::SolverConfiguration)?;
    let resolved = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            resolve_target(initial, target).map_err(|error| BatchTargetSolveError::InvalidTarget {
                index,
                target: target.clone(),
                error,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = BatchSearch::new(targets, resolved);
    let mut stats = SearchStats::default();

    for index in 0..batch.targets.len() {
        if matches!(batch.resolved[index], ResolvedTarget::AnyExit)
            && initial.room().exits().is_empty()
            && initial.room().doors().is_empty()
        {
            batch.outcomes[index] = Some(target_inconclusive(
                InconclusiveReason::NoExitsDefined,
                stats,
            ));
        }
    }
    let initially_reached = batch.newly_reached(initial);
    batch.record_solutions(initial, initially_reached, &[], stats);
    if batch.is_complete() {
        return Ok(batch.finish(stats));
    }
    if config.max_expanded_nodes == 0 {
        batch.mark_inconclusive(InconclusiveReason::ExpandedNodeBudget, stats);
        return Ok(batch.finish(stats));
    }
    if config.max_simulated_ticks == 0 {
        batch.mark_inconclusive(InconclusiveReason::SimulatedTickBudget, stats);
        return Ok(batch.finish(stats));
    }

    if config.probe_direct_routes {
        match probe_direct_routes_batch(initial, config, &mut batch, &mut stats) {
            BatchSearchTermination::Complete => return Ok(batch.finish(stats)),
            BatchSearchTermination::Stopped(
                reason @ (InconclusiveReason::ExpandedNodeBudget
                | InconclusiveReason::SimulatedTickBudget),
            ) => {
                batch.mark_inconclusive(reason, stats);
                return Ok(batch.finish(stats));
            }
            BatchSearchTermination::Stopped(_) => {}
        }
    }

    let all_macros: Vec<_> = (0..config.macros.len()).collect();
    let baseline_macros: Vec<_> = config
        .macros
        .iter()
        .enumerate()
        .filter_map(|(index, action_macro)| {
            (!action_macro.actions.iter().any(|action| action.dash)).then_some(index)
        })
        .collect();

    if baseline_macros.len() != all_macros.len()
        && !baseline_macros.is_empty()
        && let Some(preview_config) = baseline_preview_config(config, &stats)
    {
        let _ = search_batch_with_macros(
            initial,
            &preview_config,
            &baseline_macros,
            &mut batch,
            &mut stats,
        );
        if batch.is_complete() {
            return Ok(batch.finish(stats));
        }
    }

    let termination =
        search_batch_with_macros(initial, config, &all_macros, &mut batch, &mut stats);
    if batch.is_complete() {
        return Ok(batch.finish(stats));
    }

    let termination = if baseline_macros.len() != all_macros.len()
        && !baseline_macros.is_empty()
        && matches!(
            termination,
            BatchSearchTermination::Stopped(
                InconclusiveReason::FrontierExhausted | InconclusiveReason::PathHorizon
            )
        )
        && stats.expanded_nodes < config.max_expanded_nodes
        && stats.simulated_ticks < config.max_simulated_ticks
    {
        search_batch_with_macros(initial, config, &baseline_macros, &mut batch, &mut stats)
    } else {
        termination
    };

    if batch.is_complete() {
        return Ok(batch.finish(stats));
    }
    let BatchSearchTermination::Stopped(reason) = termination else {
        unreachable!("an incomplete batch search must report why it stopped")
    };
    batch.mark_inconclusive(reason, stats);
    Ok(batch.finish(stats))
}

/// Exhaust the finite built-in direct-controller vocabulary and retain every
/// distinct positive action trace found for the requested targets.
///
/// Unlike [`solve_targets`], reaching a target does not remove it from the
/// audit: later probes continue to run and can contribute easier alternate
/// witnesses. Exact duplicate action prefixes for one request index are
/// folded together while preserving every probe's canonical ordinal. The
/// `probe_direct_routes` switch is deliberately ignored because this API is
/// explicitly an audit of that vocabulary; all numeric solver budgets still
/// apply. In a multi-target audit, a target-aware probe contributes evidence
/// only to the target vocabularies which generated that exact probe; identical
/// probes shared by several targets are still executed once.
pub fn audit_direct_controller_probes(
    initial: &Simulation,
    targets: &[SearchTarget],
    config: &SolverConfig,
) -> Result<DirectProbeAudit, BatchTargetSolveError> {
    validate_config(config).map_err(BatchTargetSolveError::SolverConfiguration)?;
    let resolved = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            resolve_target(initial, target).map_err(|error| BatchTargetSolveError::InvalidTarget {
                index,
                target: target.clone(),
                error,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let probe_lists = resolved
        .iter()
        .map(|&target| route_probes(initial, target))
        .collect::<Vec<_>>();
    let probes = interleave_targeted_probes(&probe_lists);
    Ok(run_direct_probe_audit(
        initial, targets, &resolved, config, &probes,
    ))
}

fn run_direct_probe_audit(
    initial: &Simulation,
    targets: &[SearchTarget],
    resolved: &[ResolvedTarget<'_>],
    config: &SolverConfig,
    probes: &[AuditedRouteProbe],
) -> DirectProbeAudit {
    debug_assert_eq!(targets.len(), resolved.len());
    let mut audit = DirectProbeAudit {
        witnesses: Vec::new(),
        stats: SearchStats::default(),
        status: DirectProbeAuditStatus::Complete,
    };
    let mut trace_indices = HashMap::<(usize, Vec<Action>), usize>::new();
    let initially_reached = resolved
        .iter()
        .map(|&target| reached_target(initial, target))
        .collect::<Vec<_>>();
    for (target_index, reached) in initially_reached.iter().enumerate() {
        if let Some(reached) = reached {
            record_direct_probe_witness(
                &mut audit.witnesses,
                &mut trace_indices,
                initial,
                targets,
                target_index,
                reached.clone(),
                &[],
                audit.stats,
                None,
            );
        }
    }

    for (ordinal, audited_probe) in probes.iter().enumerate() {
        let probe = audited_probe.probe;
        if audit.stats.expanded_nodes >= config.max_expanded_nodes {
            audit.status =
                DirectProbeAuditStatus::BudgetLimited(DirectProbeBudgetLimit::ExpandedNodes);
            return audit;
        }
        audit.stats.expanded_nodes += 1;
        let provenance = DirectProbeProvenance {
            ordinal,
            move_x: probe.move_x,
            policy: probe.policy,
        };
        let mut simulation = initial.clone();
        let mut frames = Vec::new();
        let mut generated_counted = false;
        let mut reached_by_probe = initially_reached
            .iter()
            .enumerate()
            .map(|(target_index, reached)| {
                reached.is_some() || !audited_probe.target_indices.contains(&target_index)
            })
            .collect::<Vec<_>>();

        for probe_tick in 0..config.max_ticks_per_path {
            if audit.stats.simulated_ticks >= config.max_simulated_ticks {
                audit.status =
                    DirectProbeAuditStatus::BudgetLimited(DirectProbeBudgetLimit::SimulatedTicks);
                return audit;
            }
            let previous_action = frames
                .last()
                .map(|frame: &ReplayFrame| frame.action)
                .unwrap_or_default();
            let action = probe.action(&simulation, previous_action, probe_tick);
            let report = simulation.step(action);
            audit.stats.simulated_ticks += 1;
            frames.push(ReplayFrame {
                action,
                expected_digest: report.digest,
                expected_event_digest: digest_events(&report.events),
            });
            audit.stats.deepest_path_ticks = audit.stats.deepest_path_ticks.max(frames.len());

            for (target_index, &target) in resolved.iter().enumerate() {
                if reached_by_probe[target_index] {
                    continue;
                }
                if let Some(reached) = reached_target(&simulation, target) {
                    reached_by_probe[target_index] = true;
                    if !generated_counted {
                        audit.stats.generated_nodes += 1;
                        generated_counted = true;
                    }
                    record_direct_probe_witness(
                        &mut audit.witnesses,
                        &mut trace_indices,
                        initial,
                        targets,
                        target_index,
                        reached,
                        &frames,
                        audit.stats,
                        Some(provenance),
                    );
                }
            }

            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
                || simulation.reached_exit().is_some()
                || reached_by_probe.iter().all(|reached| *reached)
            {
                break;
            }
        }
        if !generated_counted {
            audit.stats.generated_nodes += 1;
        }
    }
    audit
}

#[allow(clippy::too_many_arguments)]
fn record_direct_probe_witness(
    witnesses: &mut Vec<DirectProbeWitness>,
    trace_indices: &mut HashMap<(usize, Vec<Action>), usize>,
    initial: &Simulation,
    targets: &[SearchTarget],
    target_index: usize,
    reached: ReachedTarget,
    frames: &[ReplayFrame],
    stats: SearchStats,
    provenance: Option<DirectProbeProvenance>,
) {
    let key = (
        target_index,
        frames.iter().map(|frame| frame.action).collect::<Vec<_>>(),
    );
    if let Some(&witness_index) = trace_indices.get(&key) {
        let witness = &mut witnesses[witness_index];
        debug_assert_eq!(witness.reached, reached);
        if let Some(provenance) = provenance {
            witness.probes.push(provenance);
        }
        return;
    }

    let probes = provenance.into_iter().collect();
    let witness_index = witnesses.len();
    witnesses.push(DirectProbeWitness {
        target_index,
        target: targets[target_index].clone(),
        reached,
        replay: Replay {
            initial_digest: initial.digest(),
            frames: frames.to_vec(),
        },
        stats_at_first_discovery: stats,
        probes,
    });
    trace_indices.insert(key, witness_index);
}

fn solve_resolved(
    initial: &Simulation,
    config: &SolverConfig,
    target: ResolvedTarget<'_>,
) -> TargetSolveOutcome {
    let public_target = match target {
        ResolvedTarget::AnyExit => SearchTarget::AnyExit,
        ResolvedTarget::Exit { id, .. } => SearchTarget::Exit(id.to_owned()),
        ResolvedTarget::Door { id, .. } => SearchTarget::Door(id.to_owned()),
        ResolvedTarget::Pickup { id, .. } => SearchTarget::Pickup(id.to_owned()),
    };
    solve_resolved_with_public_target(initial, config, target, public_target)
}

fn solve_resolved_with_public_target(
    initial: &Simulation,
    config: &SolverConfig,
    target: ResolvedTarget<'_>,
    public_target: SearchTarget,
) -> TargetSolveOutcome {
    let mut stats = SearchStats::default();
    if matches!(target, ResolvedTarget::AnyExit)
        && initial.room().exits().is_empty()
        && initial.room().doors().is_empty()
    {
        return target_inconclusive(InconclusiveReason::NoExitsDefined, stats);
    }
    if let Some(reached) = reached_target(initial, target) {
        return TargetSolveOutcome::Solved(TargetSolution {
            target: public_target,
            reached,
            replay: Replay::record(initial, []),
            stats,
        });
    }
    if config.max_expanded_nodes == 0 {
        return target_inconclusive(InconclusiveReason::ExpandedNodeBudget, stats);
    }
    if config.max_simulated_ticks == 0 {
        return target_inconclusive(InconclusiveReason::SimulatedTickBudget, stats);
    }
    if config.probe_direct_routes
        && let Some(outcome) =
            probe_direct_routes(initial, config, target, &public_target, &mut stats)
    {
        return outcome;
    }

    let all_macros: Vec<_> = (0..config.macros.len()).collect();
    let baseline_macros: Vec<_> = config
        .macros
        .iter()
        .enumerate()
        .filter_map(|(index, action_macro)| {
            (!action_macro.actions.iter().any(|action| action.dash)).then_some(index)
        })
        .collect();
    if baseline_macros.len() == all_macros.len() || baseline_macros.is_empty() {
        return search_with_macros(
            initial,
            config,
            target,
            &public_target,
            &all_macros,
            &mut stats,
        );
    }

    // Capability-specific branches can crowd a valid baseline route out of a
    // finite beam. Try a bounded baseline-only preview first, reserving at
    // least half of both global budgets for ability-specific exploration.
    // This makes common superset validation cheap without starving rooms that
    // genuinely require dash.
    if let Some(preview_config) = baseline_preview_config(config, &stats) {
        let preview = search_with_macros(
            initial,
            &preview_config,
            target,
            &public_target,
            &baseline_macros,
            &mut stats,
        );
        if matches!(preview, TargetSolveOutcome::Solved(_)) {
            return preview;
        }
    }

    let outcome = search_with_macros(
        initial,
        config,
        target,
        &public_target,
        &all_macros,
        &mut stats,
    );
    let should_try_baseline_fallback = matches!(
        outcome,
        TargetSolveOutcome::Inconclusive {
            reason: InconclusiveReason::FrontierExhausted | InconclusiveReason::PathHorizon,
            ..
        }
    );
    if should_try_baseline_fallback
        && stats.expanded_nodes < config.max_expanded_nodes
        && stats.simulated_ticks < config.max_simulated_ticks
    {
        // A final fresh baseline frontier uses whatever global budget remains
        // if the bounded preview was insufficient and rich search exhausted
        // structurally rather than by budget.
        search_with_macros(
            initial,
            config,
            target,
            &public_target,
            &baseline_macros,
            &mut stats,
        )
    } else {
        outcome
    }
}

fn baseline_preview_config(config: &SolverConfig, stats: &SearchStats) -> Option<SolverConfig> {
    let remaining_nodes = config
        .max_expanded_nodes
        .saturating_sub(stats.expanded_nodes);
    let remaining_ticks = config
        .max_simulated_ticks
        .saturating_sub(stats.simulated_ticks);
    let preview_nodes = config
        .baseline_preview_max_expanded_nodes
        .min(remaining_nodes / 2);
    let preview_ticks = config
        .baseline_preview_max_simulated_ticks
        .min(remaining_ticks / 2);
    if preview_nodes == 0 || preview_ticks == 0 {
        return None;
    }

    let mut preview = config.clone();
    preview.max_expanded_nodes = stats.expanded_nodes.saturating_add(preview_nodes);
    preview.max_simulated_ticks = stats.simulated_ticks.saturating_add(preview_ticks);
    preview.probe_direct_routes = false;
    Some(preview)
}

fn search_with_macros(
    initial: &Simulation,
    config: &SolverConfig,
    target: ResolvedTarget<'_>,
    public_target: &SearchTarget,
    macro_indices: &[usize],
    stats: &mut SearchStats,
) -> TargetSolveOutcome {
    let initial_action = Action::default();
    let initial_node = Node {
        simulation: initial.clone(),
        path: None,
        path_ticks: 0,
        previous_action: initial_action,
        score: distance_to_target(initial, target),
        serial: 0,
    };
    let initial_key = state_key(&initial_node.simulation, initial_action, config);
    let mut visited = HashMap::from([(initial_key, 0_usize)]);
    let mut frontier = vec![initial_node];
    let mut serial = 1_u64;
    let mut reached_horizon = false;

    while !frontier.is_empty() {
        let mut candidates = Vec::new();
        for node in frontier {
            if stats.expanded_nodes >= config.max_expanded_nodes {
                return target_inconclusive(InconclusiveReason::ExpandedNodeBudget, *stats);
            }
            stats.expanded_nodes += 1;

            for &macro_index in macro_indices {
                let action_macro = &config.macros[macro_index];
                if node.path_ticks + action_macro.actions.len() > config.max_ticks_per_path {
                    reached_horizon = true;
                    continue;
                }
                let mut child_simulation = node.simulation.clone();
                let mut died = false;
                let mut applied_actions = 0_usize;
                for (action_index, &action) in action_macro.actions.iter().enumerate() {
                    if stats.simulated_ticks >= config.max_simulated_ticks {
                        return target_inconclusive(
                            InconclusiveReason::SimulatedTickBudget,
                            *stats,
                        );
                    }
                    let report = child_simulation.step(action);
                    stats.simulated_ticks += 1;
                    applied_actions = action_index + 1;
                    if report
                        .events
                        .iter()
                        .any(|event| matches!(event, SimulationEvent::Died(_)))
                    {
                        died = true;
                        break;
                    }
                    if let Some(reached) = reached_target(&child_simulation, target) {
                        let path_ticks = node.path_ticks + action_index + 1;
                        stats.generated_nodes += 1;
                        stats.deepest_path_ticks = stats.deepest_path_ticks.max(path_ticks);
                        let actions = reconstruct_actions(
                            node.path.as_ref(),
                            config,
                            Some((macro_index, action_index + 1)),
                            path_ticks,
                        );
                        return TargetSolveOutcome::Solved(TargetSolution {
                            target: public_target.clone(),
                            reached,
                            replay: Replay::record(initial, actions),
                            stats: *stats,
                        });
                    }
                    if child_simulation.reached_exit().is_some() {
                        // The authoritative simulation freezes at an exit.
                        // A different exact target can no longer be reached
                        // without a reset, which solver witnesses forbid.
                        died = true;
                        break;
                    }
                }
                stats.generated_nodes += 1;
                stats.deepest_path_ticks = stats
                    .deepest_path_ticks
                    .max(node.path_ticks + applied_actions);
                if died {
                    continue;
                }
                let path_ticks = node.path_ticks + action_macro.actions.len();
                let last_action = *action_macro.actions.last().unwrap_or(&node.previous_action);
                let key = state_key(&child_simulation, last_action, config);
                if visited
                    .get(&key)
                    .is_some_and(|&best_ticks| best_ticks <= path_ticks)
                {
                    continue;
                }
                visited.insert(key, path_ticks);
                let child = Node {
                    score: score(&child_simulation, target, path_ticks),
                    simulation: child_simulation,
                    path: Some(Rc::new(PathSegment {
                        parent: node.path.clone(),
                        macro_index,
                    })),
                    path_ticks,
                    previous_action: last_action,
                    serial,
                };
                serial = serial.wrapping_add(1);
                candidates.push(child);
            }
        }

        frontier = select_frontier(candidates, config.beam_width, target);
    }

    let reason = if reached_horizon {
        InconclusiveReason::PathHorizon
    } else {
        InconclusiveReason::FrontierExhausted
    };
    target_inconclusive(reason, *stats)
}

fn search_batch_with_macros(
    initial: &Simulation,
    config: &SolverConfig,
    macro_indices: &[usize],
    batch: &mut BatchSearch<'_>,
    stats: &mut SearchStats,
) -> BatchSearchTermination {
    if batch.is_complete() {
        return BatchSearchTermination::Complete;
    }

    let initial_action = Action::default();
    let initial_node = Node {
        simulation: initial.clone(),
        path: None,
        path_ticks: 0,
        previous_action: initial_action,
        score: distance_to_batch_targets(initial, batch),
        serial: 0,
    };
    let initial_key = state_key(&initial_node.simulation, initial_action, config);
    let mut visited = HashMap::from([(initial_key, 0_usize)]);
    let mut frontier = vec![initial_node];
    let mut serial = 1_u64;
    let mut reached_horizon = false;

    while !frontier.is_empty() {
        let mut candidates = Vec::new();
        for node in frontier {
            if stats.expanded_nodes >= config.max_expanded_nodes {
                return BatchSearchTermination::Stopped(InconclusiveReason::ExpandedNodeBudget);
            }
            stats.expanded_nodes += 1;

            for &macro_index in macro_indices {
                let action_macro = &config.macros[macro_index];
                if node.path_ticks + action_macro.actions.len() > config.max_ticks_per_path {
                    reached_horizon = true;
                    continue;
                }
                let mut child_simulation = node.simulation.clone();
                let mut terminal = false;
                let mut applied_actions = 0_usize;
                let mut generated_counted = false;
                for (action_index, &action) in action_macro.actions.iter().enumerate() {
                    if stats.simulated_ticks >= config.max_simulated_ticks {
                        return BatchSearchTermination::Stopped(
                            InconclusiveReason::SimulatedTickBudget,
                        );
                    }
                    let report = child_simulation.step(action);
                    stats.simulated_ticks += 1;
                    applied_actions = action_index + 1;
                    let path_ticks = node.path_ticks + applied_actions;
                    stats.deepest_path_ticks = stats.deepest_path_ticks.max(path_ticks);

                    let reached = batch.newly_reached(&child_simulation);
                    if !reached.is_empty() {
                        if !generated_counted {
                            stats.generated_nodes += 1;
                            generated_counted = true;
                        }
                        let actions = reconstruct_actions(
                            node.path.as_ref(),
                            config,
                            Some((macro_index, applied_actions)),
                            path_ticks,
                        );
                        batch.record_solutions(initial, reached, &actions, *stats);
                        if batch.is_complete() {
                            return BatchSearchTermination::Complete;
                        }
                    }

                    if report
                        .events
                        .iter()
                        .any(|event| matches!(event, SimulationEvent::Died(_)))
                        || child_simulation.reached_exit().is_some()
                    {
                        // Every exit and door freezes authoritative simulation.
                        // Record requested terminal targets above, then leave
                        // this branch while preserving all sibling branches.
                        terminal = true;
                        break;
                    }
                }
                if !generated_counted {
                    stats.generated_nodes += 1;
                }
                stats.deepest_path_ticks = stats
                    .deepest_path_ticks
                    .max(node.path_ticks + applied_actions);
                if terminal {
                    continue;
                }

                let path_ticks = node.path_ticks + action_macro.actions.len();
                let last_action = *action_macro.actions.last().unwrap_or(&node.previous_action);
                let key = state_key(&child_simulation, last_action, config);
                if visited
                    .get(&key)
                    .is_some_and(|&best_ticks| best_ticks <= path_ticks)
                {
                    continue;
                }
                visited.insert(key, path_ticks);
                candidates.push(Node {
                    score: distance_to_batch_targets(&child_simulation, batch)
                        + i64::try_from(path_ticks / 4).unwrap_or(i64::MAX / 4),
                    simulation: child_simulation,
                    path: Some(Rc::new(PathSegment {
                        parent: node.path.clone(),
                        macro_index,
                    })),
                    path_ticks,
                    previous_action: last_action,
                    serial,
                });
                serial = serial.wrapping_add(1);
            }
        }

        frontier = select_batch_frontier(candidates, config.beam_width, batch);
    }

    BatchSearchTermination::Stopped(if reached_horizon {
        InconclusiveReason::PathHorizon
    } else {
        InconclusiveReason::FrontierExhausted
    })
}

fn select_batch_frontier(
    candidates: Vec<Node>,
    beam_width: usize,
    batch: &BatchSearch<'_>,
) -> Vec<Node> {
    if candidates.is_empty() {
        return candidates;
    }

    let active_targets = batch.unresolved_indices().collect::<Vec<_>>();
    if active_targets.is_empty() {
        return Vec::new();
    }

    // Build one deterministic, spatially diverse ranking per objective. The
    // round-robin merge below gives distant targets frontier capacity instead
    // of allowing the nearest trigger to monopolise the finite beam.
    let rankings = active_targets
        .into_iter()
        .map(|target_index| {
            ranked_candidate_indices(
                &candidates,
                batch.resolved[target_index],
                !matches!(batch.resolved[target_index], ResolvedTarget::AnyExit),
            )
        })
        .collect::<Vec<_>>();
    let mut cursors = vec![0_usize; rankings.len()];
    let mut selected = vec![false; candidates.len()];
    let selection_limit = beam_width.min(candidates.len());
    let mut selected_indices = Vec::with_capacity(selection_limit);

    while selected_indices.len() < selection_limit {
        let mut added = false;
        for (ranking, cursor) in rankings.iter().zip(&mut cursors) {
            while *cursor < ranking.len() && selected[ranking[*cursor]] {
                *cursor += 1;
            }
            if *cursor < ranking.len() {
                let index = ranking[*cursor];
                *cursor += 1;
                selected[index] = true;
                selected_indices.push(index);
                added = true;
                if selected_indices.len() == selection_limit {
                    break;
                }
            }
        }
        if !added {
            break;
        }
    }

    let mut slots = candidates.into_iter().map(Some).collect::<Vec<_>>();
    selected_indices
        .into_iter()
        .map(|index| slots[index].take().expect("candidate selected only once"))
        .collect()
}

fn ranked_candidate_indices(
    candidates: &[Node],
    target: ResolvedTarget<'_>,
    diversify_regions: bool,
) -> Vec<usize> {
    let mut indices = (0..candidates.len()).collect::<Vec<_>>();
    indices.sort_unstable_by_key(|&index| {
        let node = &candidates[index];
        (
            score(&node.simulation, target, node.path_ticks),
            node.path_ticks,
            node.serial,
        )
    });
    if !diversify_regions {
        return indices;
    }

    const REGION_PIXELS: i32 = 16;
    let mut region_indices = HashMap::<(i32, i32), usize>::new();
    let mut regions = Vec::<VecDeque<usize>>::new();
    for index in indices {
        let bounds = candidates[index].simulation.player().bounds();
        let region = (
            bounds.x.div_euclid(REGION_PIXELS),
            bounds.y.div_euclid(REGION_PIXELS),
        );
        let region_index = if let Some(&region_index) = region_indices.get(&region) {
            region_index
        } else {
            let region_index = regions.len();
            region_indices.insert(region, region_index);
            regions.push(VecDeque::new());
            region_index
        };
        regions[region_index].push_back(index);
    }

    let mut diversified = Vec::with_capacity(candidates.len());
    while diversified.len() < candidates.len() {
        for region in &mut regions {
            if let Some(index) = region.pop_front() {
                diversified.push(index);
            }
        }
    }
    diversified
}

fn select_frontier(
    mut candidates: Vec<Node>,
    beam_width: usize,
    target: ResolvedTarget<'_>,
) -> Vec<Node> {
    candidates.sort_unstable_by_key(|node| (node.score, node.path_ticks, node.serial));
    if matches!(target, ResolvedTarget::AnyExit) || candidates.len() <= beam_width {
        candidates.truncate(beam_width);
        return candidates;
    }

    // Exact optional objectives often require a deliberate detour before the
    // player can get closer to the trigger. Preserve the best state in each
    // coarse screen region before taking a second state from any one region;
    // this avoids a nearest-target local minimum monopolising the finite beam.
    // Groups retain score/serial insertion order, so selection stays fully
    // deterministic.
    const REGION_PIXELS: i32 = 16;
    let mut region_indices = HashMap::<(i32, i32), usize>::new();
    let mut regions = Vec::<VecDeque<Node>>::new();
    for node in candidates {
        let bounds = node.simulation.player().bounds();
        let region = (
            bounds.x.div_euclid(REGION_PIXELS),
            bounds.y.div_euclid(REGION_PIXELS),
        );
        let index = if let Some(&index) = region_indices.get(&region) {
            index
        } else {
            let index = regions.len();
            region_indices.insert(region, index);
            regions.push(VecDeque::new());
            index
        };
        regions[index].push_back(node);
    }

    let mut selected = Vec::with_capacity(beam_width);
    while selected.len() < beam_width {
        let mut added = false;
        for region in &mut regions {
            if let Some(node) = region.pop_front() {
                selected.push(node);
                added = true;
                if selected.len() == beam_width {
                    break;
                }
            }
        }
        if !added {
            break;
        }
    }
    selected
}

fn probe_direct_routes(
    initial: &Simulation,
    config: &SolverConfig,
    target: ResolvedTarget<'_>,
    public_target: &SearchTarget,
    stats: &mut SearchStats,
) -> Option<TargetSolveOutcome> {
    for route_probe in route_probes(initial, target) {
        if stats.expanded_nodes >= config.max_expanded_nodes {
            return Some(target_inconclusive(
                InconclusiveReason::ExpandedNodeBudget,
                *stats,
            ));
        }
        stats.expanded_nodes += 1;
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        for probe_tick in 0..config.max_ticks_per_path {
            if stats.simulated_ticks >= config.max_simulated_ticks {
                return Some(target_inconclusive(
                    InconclusiveReason::SimulatedTickBudget,
                    *stats,
                ));
            }
            let previous_action = actions.last().copied().unwrap_or_default();
            let action = route_probe.action(&simulation, previous_action, probe_tick);
            let report = simulation.step(action);
            stats.simulated_ticks += 1;
            actions.push(action);
            stats.deepest_path_ticks = stats.deepest_path_ticks.max(actions.len());
            if let Some(reached) = reached_target(&simulation, target) {
                stats.generated_nodes += 1;
                return Some(TargetSolveOutcome::Solved(TargetSolution {
                    target: public_target.clone(),
                    reached,
                    replay: Replay::record(initial, actions),
                    stats: *stats,
                }));
            }
            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
            {
                break;
            }
            if simulation.reached_exit().is_some() {
                break;
            }
        }
        stats.generated_nodes += 1;
    }
    None
}

fn probe_direct_routes_batch(
    initial: &Simulation,
    config: &SolverConfig,
    batch: &mut BatchSearch<'_>,
    stats: &mut SearchStats,
) -> BatchSearchTermination {
    let probe_lists = batch
        .unresolved_indices()
        .map(|target_index| route_probes(initial, batch.resolved[target_index]))
        .collect::<Vec<_>>();
    let probes = interleave_unique_probes(&probe_lists);

    for route_probe in probes {
        if stats.expanded_nodes >= config.max_expanded_nodes {
            return BatchSearchTermination::Stopped(InconclusiveReason::ExpandedNodeBudget);
        }
        stats.expanded_nodes += 1;
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        let mut generated_counted = false;
        for probe_tick in 0..config.max_ticks_per_path {
            if stats.simulated_ticks >= config.max_simulated_ticks {
                return BatchSearchTermination::Stopped(InconclusiveReason::SimulatedTickBudget);
            }
            let previous_action = actions.last().copied().unwrap_or_default();
            let action = route_probe.action(&simulation, previous_action, probe_tick);
            let report = simulation.step(action);
            stats.simulated_ticks += 1;
            actions.push(action);
            stats.deepest_path_ticks = stats.deepest_path_ticks.max(actions.len());

            let reached = batch.newly_reached(&simulation);
            if !reached.is_empty() {
                if !generated_counted {
                    stats.generated_nodes += 1;
                    generated_counted = true;
                }
                batch.record_solutions(initial, reached, &actions, *stats);
                if batch.is_complete() {
                    return BatchSearchTermination::Complete;
                }
            }

            if report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
                || simulation.reached_exit().is_some()
            {
                break;
            }
        }
        if !generated_counted {
            stats.generated_nodes += 1;
        }
    }

    BatchSearchTermination::Stopped(InconclusiveReason::FrontierExhausted)
}

fn interleave_unique_probes(probe_lists: &[Vec<RouteProbe>]) -> Vec<RouteProbe> {
    let mut cursors = vec![0_usize; probe_lists.len()];
    let mut probes = Vec::new();
    loop {
        let mut advanced = false;
        for (probe_list, cursor) in probe_lists.iter().zip(&mut cursors) {
            if let Some(&probe) = probe_list.get(*cursor) {
                *cursor += 1;
                advanced = true;
                if !probes.contains(&probe) {
                    probes.push(probe);
                }
            }
        }
        if !advanced {
            break;
        }
    }
    probes
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuditedRouteProbe {
    probe: RouteProbe,
    target_indices: Vec<usize>,
}

/// Interleave target-specific controller vocabularies without allowing a
/// probe parameterized for one target to become positive evidence for a
/// different target.
///
/// Identical probes are still executed only once, with their sorted target
/// membership merged. This preserves the exact per-target vocabulary while
/// retaining the shared operational cost of a source-batched audit.
fn interleave_targeted_probes(probe_lists: &[Vec<RouteProbe>]) -> Vec<AuditedRouteProbe> {
    let mut cursors = vec![0_usize; probe_lists.len()];
    let mut probes = Vec::<AuditedRouteProbe>::new();
    loop {
        let mut advanced = false;
        for (target_index, (probe_list, cursor)) in probe_lists.iter().zip(&mut cursors).enumerate()
        {
            let Some(&probe) = probe_list.get(*cursor) else {
                continue;
            };
            *cursor += 1;
            advanced = true;
            if let Some(existing) = probes.iter_mut().find(|existing| existing.probe == probe) {
                if !existing.target_indices.contains(&target_index) {
                    existing.target_indices.push(target_index);
                }
            } else {
                probes.push(AuditedRouteProbe {
                    probe,
                    target_indices: vec![target_index],
                });
            }
        }
        if !advanced {
            break;
        }
    }
    for audited_probe in &mut probes {
        audited_probe.target_indices.sort_unstable();
    }
    probes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteProbe {
    move_x: i8,
    policy: DirectProbePolicy,
}

impl RouteProbe {
    fn action(self, simulation: &Simulation, previous_action: Action, probe_tick: usize) -> Action {
        match self.policy {
            DirectProbePolicy::DropThrough => Action {
                move_x: self.move_x,
                move_y: 1,
                jump: probe_tick.is_multiple_of(12) && !previous_action.jump,
                ..Action::default()
            },
            DirectProbePolicy::Run => Action {
                move_x: self.move_x,
                ..Action::default()
            },
            DirectProbePolicy::PeriodicJump {
                period,
                hold_ticks,
                phase,
            } => Action {
                move_x: self.move_x,
                jump: (probe_tick + phase) % period < hold_ticks,
                ..Action::default()
            },
            DirectProbePolicy::ReactiveJump {
                lookahead_pixels,
                hold_ticks,
            } => {
                let route_feature_ahead =
                    route_feature_ahead(simulation, self.move_x, lookahead_pixels);
                let hold_threshold = 10_u8.saturating_sub(hold_ticks);
                Action {
                    move_x: self.move_x,
                    jump: (previous_action.jump
                        && simulation.player().jump_hold_ticks_remaining() > hold_threshold)
                        || (route_feature_ahead
                            && simulation.player().grounded()
                            && !previous_action.jump),
                    ..Action::default()
                }
            }
            DirectProbePolicy::AutoJump => Action {
                move_x: self.move_x,
                jump: simulation.player().jump_hold_ticks_remaining() > 0
                    || (simulation.player().grounded() && !previous_action.jump),
                ..Action::default()
            },
            DirectProbePolicy::WallClimb => {
                let velocity_x = simulation.player().velocity_subpixels().x;
                let move_x = match simulation.player().wall_contact() {
                    Some(WallSide::Left) => -1,
                    Some(WallSide::Right) => 1,
                    None if velocity_x < 0 => -1,
                    None if velocity_x > 0 => 1,
                    None => self.move_x,
                };
                Action {
                    move_x,
                    jump: simulation.player().wall_contact().is_some() && !previous_action.jump,
                    ..Action::default()
                }
            }
            DirectProbePolicy::BufferedWallClimb => {
                let velocity = simulation.player().velocity_subpixels();
                let move_x = match simulation.player().wall_contact() {
                    Some(WallSide::Left) => -1,
                    Some(WallSide::Right) => 1,
                    None => self.move_x,
                };
                // A wall lip needs two distinct phases: rise while pressing
                // into its face, then retain the wall-jump arc while steering
                // back towards the lip. Pressing during the descent also keeps
                // the authoritative jump buffer live, so the jump fires on
                // the first contact tick instead of one tick too late.
                let continue_jump = previous_action.jump
                    && if velocity.x == 0 {
                        simulation.player().jump_hold_ticks_remaining() > 0
                    } else {
                        velocity.y < 0
                    };
                let begin_jump = !previous_action.jump
                    && (simulation.player().grounded()
                        || simulation.player().wall_contact().is_some()
                        || velocity.y > 0);
                Action {
                    move_x,
                    jump: continue_jump || begin_jump,
                    ..Action::default()
                }
            }
            DirectProbePolicy::Dash {
                move_y,
                jump_period,
                jump_hold_ticks,
                dash_delay_ticks,
            } => Action {
                move_x: self.move_x,
                move_y,
                jump: probe_tick % jump_period < jump_hold_ticks,
                dash: probe_tick >= dash_delay_ticks
                    && simulation.player().dash_available()
                    && !previous_action.dash,
                ..Action::default()
            },
            DirectProbePolicy::StagedDash {
                period,
                jump_start,
                jump_hold_ticks,
                dash_tick,
            } => {
                let cycle_tick = probe_tick % period;
                Action {
                    move_x: self.move_x,
                    move_y: -1,
                    jump: (jump_start..jump_start + jump_hold_ticks).contains(&cycle_tick),
                    dash: cycle_tick == dash_tick
                        && simulation.player().dash_available()
                        && !previous_action.dash,
                    ..Action::default()
                }
            }
            DirectProbePolicy::ReactiveDash {
                lookahead_pixels,
                move_y,
            } => {
                let route_feature_ahead =
                    route_feature_ahead(simulation, self.move_x, lookahead_pixels);
                Action {
                    move_x: self.move_x,
                    move_y,
                    jump: simulation.player().jump_hold_ticks_remaining() > 0
                        || (route_feature_ahead && !previous_action.jump),
                    dash: route_feature_ahead
                        && simulation.player().dash_available()
                        && !previous_action.dash
                        && !simulation.player().grounded()
                        && simulation.player().jump_hold_ticks_remaining() <= 7,
                    ..Action::default()
                }
            }
            DirectProbePolicy::DetourJump {
                turn_tick,
                period,
                hold_ticks,
                phase,
            } => Action {
                move_x: if probe_tick < turn_tick {
                    self.move_x
                } else {
                    -self.move_x
                },
                jump: (probe_tick + phase) % period < hold_ticks,
                ..Action::default()
            },
            DirectProbePolicy::DetourClimb {
                turn_tick,
                lookahead_pixels,
                hold_ticks,
            } => {
                let outbound = probe_tick < turn_tick;
                let move_x = if outbound { self.move_x } else { -self.move_x };
                let hold_threshold = 10_u8.saturating_sub(hold_ticks);
                let continue_jump = previous_action.jump
                    && simulation.player().jump_hold_ticks_remaining() > hold_threshold;
                let begin_jump = if outbound {
                    route_feature_ahead(simulation, move_x, lookahead_pixels)
                        && simulation.player().grounded()
                        && !previous_action.jump
                } else {
                    simulation.player().grounded() && !previous_action.jump
                };
                Action {
                    move_x,
                    jump: continue_jump || begin_jump,
                    ..Action::default()
                }
            }
            DirectProbePolicy::DetourHomingClimb {
                turn_tick,
                target_center_x,
                deadzone_pixels,
                hold_ticks,
            } => {
                let move_x = if probe_tick < turn_tick {
                    self.move_x
                } else {
                    let player = simulation.player().bounds();
                    let player_center_x = player.x + player.width / 2;
                    let offset = target_center_x - player_center_x;
                    if offset.abs() <= deadzone_pixels {
                        0
                    } else {
                        i8::try_from(offset.signum())
                            .expect("a horizontal sign always fits the input axis")
                    }
                };
                let hold_threshold = 10_u8.saturating_sub(hold_ticks);
                Action {
                    move_x,
                    jump: (previous_action.jump
                        && simulation.player().jump_hold_ticks_remaining() > hold_threshold)
                        || (simulation.player().grounded() && !previous_action.jump),
                    ..Action::default()
                }
            }
        }
    }
}

fn route_feature_ahead(simulation: &Simulation, move_x: i8, lookahead_pixels: i32) -> bool {
    let player = simulation.player().bounds();
    let ahead_start = if move_x < 0 { player.x } else { player.right() };
    simulation
        .room()
        .tiles()
        .iter()
        .enumerate()
        .any(|(index, &tile)| {
            if tile != downwards_core::Tile::Solid && !tile.is_hazard() {
                return false;
            }
            let width = usize::from(simulation.room().width());
            let tile_x = u16::try_from(index % width).expect("room width fits u16");
            let tile_y = u16::try_from(index / width).expect("room height fits u16");
            let bounds = simulation.room().tile_bounds(tile_x, tile_y);
            let horizontal_gap = if move_x < 0 {
                ahead_start - bounds.right()
            } else {
                bounds.x - ahead_start
            };
            if !(0..=lookahead_pixels).contains(&horizontal_gap) {
                return false;
            }

            match tile {
                downwards_core::Tile::HazardUp
                | downwards_core::Tile::HazardDown
                | downwards_core::Tile::HazardLeft
                | downwards_core::Tile::HazardRight => {
                    bounds.y >= player.y - 4 && bounds.y <= player.bottom() + 12
                }
                downwards_core::Tile::Solid => bounds.y < player.y && player.y - bounds.y <= 55,
                _ => false,
            }
        })
}

fn route_probes(initial: &Simulation, target: ResolvedTarget<'_>) -> Vec<RouteProbe> {
    const JUMP_CADENCES: [(usize, usize, usize); 8] = [
        (18, 5, 0),
        (22, 7, 0),
        (26, 9, 0),
        (30, 10, 0),
        (34, 8, 0),
        (22, 7, 8),
        (26, 9, 10),
        (30, 10, 14),
    ];
    const DASH_POLICIES: [(i8, usize, usize, usize); 6] = [
        (-1, 24, 8, 0),
        (-1, 30, 10, 0),
        (-1, 24, 8, 8),
        (0, 24, 8, 0),
        (0, 30, 10, 8),
        (0, 34, 10, 14),
    ];
    const STAGED_DASH_POLICIES: [(usize, usize, usize, usize); 10] = [
        (34, 3, 10, 22),
        (34, 4, 10, 24),
        (38, 3, 10, 20),
        (40, 3, 10, 20),
        (40, 3, 10, 22),
        (40, 3, 10, 24),
        (42, 3, 10, 22),
        (42, 3, 10, 24),
        (44, 3, 10, 24),
        (44, 3, 10, 26),
    ];
    const REACTIVE_LOOKAHEADS: [i32; 8] = [0, 3, 4, 8, 12, 18, 26, 34];
    const REACTIVE_HOLD_TICKS: [u8; 5] = [2, 4, 6, 8, 10];
    const DETOUR_TURN_TICKS: [usize; 9] = [56, 72, 88, 104, 120, 144, 168, 192, 224];
    const DETOUR_JUMP_CADENCES: [(usize, usize, usize); 6] = [
        (18, 5, 0),
        (22, 7, 0),
        (26, 9, 0),
        (30, 10, 0),
        (22, 7, 8),
        (30, 10, 14),
    ];
    const DETOUR_CLIMB_LOOKAHEADS: [i32; 7] = [0, 4, 8, 12, 18, 26, 34];
    const DETOUR_CLIMB_HOLDS: [u8; 3] = [8, 9, 10];
    const DETOUR_HOMING_DEADZONE_PIXELS: i32 = 8;
    const DETOUR_HOMING_HOLD_TICKS: u8 = 6;

    let directions = direct_route_directions(initial, target);
    let elevated_exact_target_center = elevated_exact_target_center(initial, target);
    let detour_probe_count = usize::from(!matches!(target, ResolvedTarget::AnyExit))
        * DETOUR_TURN_TICKS.len()
        * (DETOUR_JUMP_CADENCES.len() + DETOUR_CLIMB_LOOKAHEADS.len() * DETOUR_CLIMB_HOLDS.len());
    let detour_homing_probe_count =
        usize::from(elevated_exact_target_center.is_some()) * DETOUR_TURN_TICKS.len();
    let floor_door = matches!(
        target,
        ResolvedTarget::Door {
            side: BoundarySide::Floor,
            ..
        }
    );
    let per_direction = usize::from(floor_door)
        + 1
        + JUMP_CADENCES.len()
        + REACTIVE_LOOKAHEADS.len() * REACTIVE_HOLD_TICKS.len()
        + 1
        + usize::from(initial.abilities().wall_jump) * 2
        + detour_probe_count
        + detour_homing_probe_count
        + usize::from(initial.abilities().dash)
            * (DASH_POLICIES.len() + STAGED_DASH_POLICIES.len() + REACTIVE_LOOKAHEADS.len() * 2);
    let mut probes = Vec::with_capacity(directions.len() * per_direction);
    for move_x in directions {
        if floor_door {
            probes.push(RouteProbe {
                move_x,
                policy: DirectProbePolicy::DropThrough,
            });
        }
        probes.push(RouteProbe {
            move_x,
            policy: DirectProbePolicy::Run,
        });
        probes.extend(JUMP_CADENCES.map(|(period, hold_ticks, phase)| RouteProbe {
            move_x,
            policy: DirectProbePolicy::PeriodicJump {
                period,
                hold_ticks,
                phase,
            },
        }));
        for hold_ticks in REACTIVE_HOLD_TICKS {
            probes.extend(REACTIVE_LOOKAHEADS.map(|lookahead_pixels| RouteProbe {
                move_x,
                policy: DirectProbePolicy::ReactiveJump {
                    lookahead_pixels,
                    hold_ticks,
                },
            }));
        }
        probes.push(RouteProbe {
            move_x,
            policy: DirectProbePolicy::AutoJump,
        });
        if initial.abilities().wall_jump {
            probes.push(RouteProbe {
                move_x,
                policy: DirectProbePolicy::WallClimb,
            });
            probes.push(RouteProbe {
                move_x,
                policy: DirectProbePolicy::BufferedWallClimb,
            });
        }
        if !matches!(target, ResolvedTarget::AnyExit) {
            for turn_tick in DETOUR_TURN_TICKS {
                probes.extend(
                    DETOUR_JUMP_CADENCES.map(|(period, hold_ticks, phase)| RouteProbe {
                        move_x,
                        policy: DirectProbePolicy::DetourJump {
                            turn_tick,
                            period,
                            hold_ticks,
                            phase,
                        },
                    }),
                );
                for hold_ticks in DETOUR_CLIMB_HOLDS {
                    probes.extend(DETOUR_CLIMB_LOOKAHEADS.map(|lookahead_pixels| RouteProbe {
                        move_x,
                        policy: DirectProbePolicy::DetourClimb {
                            turn_tick,
                            lookahead_pixels,
                            hold_ticks,
                        },
                    }));
                }
                if let Some(target_center_x) = elevated_exact_target_center {
                    probes.push(RouteProbe {
                        move_x,
                        policy: DirectProbePolicy::DetourHomingClimb {
                            turn_tick,
                            target_center_x,
                            deadzone_pixels: DETOUR_HOMING_DEADZONE_PIXELS,
                            hold_ticks: DETOUR_HOMING_HOLD_TICKS,
                        },
                    });
                }
            }
        }
        if initial.abilities().dash {
            probes.extend(DASH_POLICIES.map(
                |(move_y, jump_period, jump_hold_ticks, dash_delay_ticks)| RouteProbe {
                    move_x,
                    policy: DirectProbePolicy::Dash {
                        move_y,
                        jump_period,
                        jump_hold_ticks,
                        dash_delay_ticks,
                    },
                },
            ));
            probes.extend(STAGED_DASH_POLICIES.map(
                |(period, jump_start, jump_hold_ticks, dash_tick)| RouteProbe {
                    move_x,
                    policy: DirectProbePolicy::StagedDash {
                        period,
                        jump_start,
                        jump_hold_ticks,
                        dash_tick,
                    },
                },
            ));
            for move_y in [-1, 0] {
                probes.extend(REACTIVE_LOOKAHEADS.map(|lookahead_pixels| RouteProbe {
                    move_x,
                    policy: DirectProbePolicy::ReactiveDash {
                        lookahead_pixels,
                        move_y,
                    },
                }));
            }
        }
    }
    probes
}

fn elevated_exact_target_center(initial: &Simulation, target: ResolvedTarget<'_>) -> Option<i32> {
    let bounds = match target {
        ResolvedTarget::Exit { bounds, .. }
        | ResolvedTarget::Door { bounds, .. }
        | ResolvedTarget::Pickup { bounds, .. } => bounds,
        ResolvedTarget::AnyExit => return None,
    };
    let player = initial.player().bounds();
    (bounds.bottom() < player.y).then(|| bounds.x + bounds.width / 2)
}

fn direct_route_directions(initial: &Simulation, target: ResolvedTarget<'_>) -> Vec<i8> {
    let player = initial.player().bounds();
    let player_center = i64::from(player.x) * 2 + i64::from(player.width);
    let mut nearest_left = None::<i64>;
    let mut nearest_right = None::<i64>;
    for bounds in target_bounds(initial, target) {
        let exit_center = i64::from(bounds.x) * 2 + i64::from(bounds.width);
        let offset = exit_center - player_center;
        let nearest = if offset < 0 {
            &mut nearest_left
        } else if offset > 0 {
            &mut nearest_right
        } else {
            continue;
        };
        let distance = offset.abs();
        *nearest = Some(nearest.map_or(distance, |current| current.min(distance)));
    }

    let mut directions = Vec::with_capacity(2);
    if let Some(distance) = nearest_left {
        directions.push((distance, -1));
    }
    if let Some(distance) = nearest_right {
        directions.push((distance, 1));
    }
    if directions.is_empty() && !target_bounds(initial, target).is_empty() {
        // Vertically aligned targets may need an initial approach to either wall.
        directions.extend([(0, -1), (0, 1)]);
    }
    directions.sort_unstable();
    directions
        .into_iter()
        .map(|(_, direction)| direction)
        .collect()
}

pub(crate) fn validate_config(config: &SolverConfig) -> Result<(), SolverConfigError> {
    if config.beam_width == 0 {
        return Err(SolverConfigError::ZeroBeamWidth);
    }
    if config.max_ticks_per_path == 0 {
        return Err(SolverConfigError::ZeroPathHorizon);
    }
    if config.position_quantum <= 0 {
        return Err(SolverConfigError::InvalidPositionQuantum(
            config.position_quantum,
        ));
    }
    if config.velocity_quantum <= 0 {
        return Err(SolverConfigError::InvalidVelocityQuantum(
            config.velocity_quantum,
        ));
    }
    if config.macros.is_empty() {
        return Err(SolverConfigError::NoMacros);
    }
    for (index, action_macro) in config.macros.iter().enumerate() {
        if action_macro.actions.is_empty() {
            return Err(SolverConfigError::EmptyMacro {
                index,
                name: action_macro.name.clone(),
            });
        }
        if action_macro.actions.iter().any(|action| action.restart) {
            return Err(SolverConfigError::RestartMacro {
                index,
                name: action_macro.name.clone(),
            });
        }
    }
    Ok(())
}

pub(crate) fn state_key(
    simulation: &Simulation,
    previous_action: Action,
    config: &SolverConfig,
) -> FutureStateKey {
    let player = simulation.player();
    let position = player.position_subpixels();
    let velocity = player.velocity_subpixels();
    FutureStateKey {
        position_x: position.x.div_euclid(config.position_quantum),
        position_y: position.y.div_euclid(config.position_quantum),
        velocity_x: velocity.x.div_euclid(config.velocity_quantum),
        velocity_y: velocity.y.div_euclid(config.velocity_quantum),
        grounded: player.grounded(),
        coyote_ticks: player.coyote_ticks_remaining(),
        jump_buffer_ticks: player.jump_buffer_ticks_remaining(),
        previous_move_x: previous_action.move_x,
        previous_move_y: previous_action.move_y,
        previous_jump: previous_action.jump,
        previous_dash: previous_action.dash,
        wall_contact: player.wall_contact(),
        wall_sliding: player.wall_sliding(),
        facing: player.facing(),
        dash_available: player.dash_available(),
        dash_ticks: player.dash_ticks_remaining(),
        dash_direction: player.dash_direction(),
        one_way_drop_ticks: player.one_way_drop_ticks_remaining(),
        jump_hold_ticks: player.jump_hold_ticks_remaining(),
        room_tick: (!simulation.room().timed_hazards().is_empty())
            .then_some(simulation.room_tick()),
        collected_pickups: (0..simulation.room().pickups().len())
            .map(|index| simulation.pickup_is_collected(index).unwrap_or(false))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

fn reconstruct_actions(
    path: Option<&Rc<PathSegment>>,
    config: &SolverConfig,
    trailing: Option<(usize, usize)>,
    total_ticks: usize,
) -> Vec<Action> {
    let mut macro_indices = Vec::new();
    let mut cursor = path;
    while let Some(segment) = cursor {
        macro_indices.push(segment.macro_index);
        cursor = segment.parent.as_ref();
    }

    let mut actions = Vec::with_capacity(total_ticks);
    for macro_index in macro_indices.into_iter().rev() {
        actions.extend_from_slice(&config.macros[macro_index].actions);
    }
    if let Some((macro_index, action_count)) = trailing {
        actions.extend_from_slice(&config.macros[macro_index].actions[..action_count]);
    }
    debug_assert_eq!(actions.len(), total_ticks);
    actions
}

fn resolve_target<'a>(
    initial: &'a Simulation,
    target: &SearchTarget,
) -> Result<ResolvedTarget<'a>, TargetSolveError> {
    match target {
        SearchTarget::AnyExit => Ok(ResolvedTarget::AnyExit),
        SearchTarget::Exit(id) => {
            if id.trim().is_empty() {
                return Err(TargetSolveError::EmptyExitId);
            }
            initial
                .room()
                .exits()
                .iter()
                .find(|exit| exit.id == *id)
                .map(|exit| ResolvedTarget::Exit {
                    id: exit.id.as_str(),
                    bounds: exit.bounds,
                })
                .ok_or_else(|| TargetSolveError::ExitNotDefined {
                    exit_id: id.clone(),
                })
        }
        SearchTarget::Door(id) => {
            if id.trim().is_empty() {
                return Err(TargetSolveError::EmptyDoorId);
            }
            initial
                .room()
                .doors()
                .iter()
                .find(|door| door.id == *id)
                .map(|door| ResolvedTarget::Door {
                    id: door.id.as_str(),
                    bounds: door.trigger_bounds,
                    side: door.side,
                })
                .ok_or_else(|| TargetSolveError::DoorNotDefined {
                    door_id: id.clone(),
                })
        }
        SearchTarget::Pickup(id) => {
            if id.trim().is_empty() {
                return Err(TargetSolveError::EmptyPickupId);
            }
            initial
                .room()
                .pickups()
                .iter()
                .enumerate()
                .find(|(_, pickup)| pickup.id() == id)
                .map(|(index, pickup)| ResolvedTarget::Pickup {
                    id: pickup.id(),
                    index,
                    bounds: pickup.bounds(),
                })
                .ok_or_else(|| TargetSolveError::PickupNotDefined {
                    pickup_id: id.clone(),
                })
        }
    }
}

fn reached_target(simulation: &Simulation, target: ResolvedTarget<'_>) -> Option<ReachedTarget> {
    match target {
        ResolvedTarget::AnyExit => simulation.reached_exit().map(|id| {
            if simulation.room().doors().iter().any(|door| door.id == id) {
                ReachedTarget::Door(id.to_owned())
            } else {
                ReachedTarget::Exit(id.to_owned())
            }
        }),
        ResolvedTarget::Exit { id, .. } => {
            (simulation.reached_exit() == Some(id)).then(|| ReachedTarget::Exit(id.to_owned()))
        }
        ResolvedTarget::Door { id, .. } => {
            (simulation.reached_exit() == Some(id)).then(|| ReachedTarget::Door(id.to_owned()))
        }
        ResolvedTarget::Pickup { id, index, .. } => simulation
            .pickup_is_collected(index)
            .unwrap_or(false)
            .then(|| ReachedTarget::Pickup(id.to_owned())),
    }
}

fn target_bounds(initial: &Simulation, target: ResolvedTarget<'_>) -> Vec<Rect> {
    match target {
        ResolvedTarget::AnyExit => initial
            .room()
            .exits()
            .iter()
            .map(|exit| exit.bounds)
            .chain(
                initial
                    .room()
                    .doors()
                    .iter()
                    .map(|door| door.trigger_bounds),
            )
            .collect(),
        ResolvedTarget::Exit { bounds, .. }
        | ResolvedTarget::Door { bounds, .. }
        | ResolvedTarget::Pickup { bounds, .. } => {
            vec![bounds]
        }
    }
}

fn score(simulation: &Simulation, target: ResolvedTarget<'_>, path_ticks: usize) -> i64 {
    distance_to_target(simulation, target) + i64::try_from(path_ticks / 4).unwrap_or(i64::MAX / 4)
}

fn distance_to_target(simulation: &Simulation, target: ResolvedTarget<'_>) -> i64 {
    let player = simulation.player().bounds();
    target_bounds(simulation, target)
        .into_iter()
        .map(|bounds| rectangle_distance(player, bounds))
        .min()
        .unwrap_or(i64::MAX / 4)
}

fn distance_to_batch_targets(simulation: &Simulation, batch: &BatchSearch<'_>) -> i64 {
    batch
        .unresolved_indices()
        .map(|index| distance_to_target(simulation, batch.resolved[index]))
        .min()
        .unwrap_or(0)
}

fn rectangle_distance(left: Rect, right: Rect) -> i64 {
    let horizontal = if left.right() < right.x {
        right.x - left.right()
    } else if right.right() < left.x {
        left.x - right.right()
    } else {
        0
    };
    let vertical = if left.bottom() < right.y {
        right.y - left.bottom()
    } else if right.bottom() < left.y {
        left.y - right.bottom()
    } else {
        0
    };
    i64::from(horizontal) + i64::from(vertical)
}

const fn target_inconclusive(reason: InconclusiveReason, stats: SearchStats) -> TargetSolveOutcome {
    TargetSolveOutcome::Inconclusive { reason, stats }
}

#[cfg(test)]
mod tests {
    use downwards_core::{Door, Exit, Pickup, Point, Room, Tile, TimedHazard};

    use super::*;

    fn state_key_room(with_objects: bool) -> Room {
        let width = 32_usize;
        let height = 18_usize;
        let mut tiles = vec![Tile::Empty; width * height];
        for x in 0..width {
            tiles[16 * width + x] = Tile::Solid;
        }
        let room = Room::new(
            "state-key",
            "State key",
            width as u16,
            height as u16,
            10,
            tiles,
            Point::new(20, 148),
            vec![Exit {
                id: "east".into(),
                bounds: Rect::new(300, 140, 10, 20),
                destination: None,
                destination_entrance: None,
            }],
        )
        .unwrap();
        if with_objects {
            room.with_objects(
                vec![TimedHazard::new(Rect::new(250, 20, 8, 8), 10, 2, 0).unwrap()],
                vec![Pickup::new("spawn-pickup", Rect::new(20, 148, 8, 12)).unwrap()],
            )
            .unwrap()
        } else {
            room
        }
    }

    fn buffered_wall_lip_room() -> Room {
        let width = 32_usize;
        let height = 18_usize;
        let mut tiles = vec![Tile::Empty; width * height];
        for tile in &mut tiles[17 * width..18 * width] {
            *tile = Tile::Solid;
        }
        for row in 13..18 {
            tiles[row * width + 6] = Tile::Solid;
        }
        for column in 6..31 {
            tiles[13 * width + column] = Tile::Solid;
        }
        Room::new(
            "buffered-wall-lip",
            "Buffered wall lip",
            width as u16,
            height as u16,
            10,
            tiles,
            Point::new(22, 158),
            vec![],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".into(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 148, 12, 22),
                arrival: Point::new(22, 158),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".into(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(308, 108, 12, 22),
                arrival: Point::new(290, 118),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    fn input_transitions(replay: &Replay) -> usize {
        let mut previous = Action::default();
        replay
            .actions()
            .map(|action| {
                let changed = usize::from(action != previous);
                previous = action;
                changed
            })
            .sum()
    }

    #[test]
    fn direct_probe_audit_keeps_hard_first_and_easy_later_witnesses() {
        let initial = Simulation::new(state_key_room(false));
        let targets = vec![SearchTarget::exit("east")];
        let resolved = vec![resolve_target(&initial, &targets[0]).unwrap()];
        let probes = vec![
            AuditedRouteProbe {
                probe: RouteProbe {
                    move_x: 1,
                    policy: DirectProbePolicy::PeriodicJump {
                        period: 18,
                        hold_ticks: 5,
                        phase: 0,
                    },
                },
                target_indices: vec![0],
            },
            AuditedRouteProbe {
                probe: RouteProbe {
                    move_x: 1,
                    policy: DirectProbePolicy::Run,
                },
                target_indices: vec![0],
            },
        ];
        let audit = run_direct_probe_audit(
            &initial,
            &targets,
            &resolved,
            &SolverConfig::default(),
            &probes,
        );

        assert_eq!(audit.status, DirectProbeAuditStatus::Complete);
        assert_eq!(audit.stats.expanded_nodes, 2);
        assert_eq!(audit.witnesses.len(), 2);
        assert_eq!(audit.witnesses[0].probes[0].ordinal, 0);
        assert!(matches!(
            audit.witnesses[0].probes[0].policy,
            DirectProbePolicy::PeriodicJump { .. }
        ));
        assert_eq!(audit.witnesses[1].probes[0].ordinal, 1);
        assert_eq!(audit.witnesses[1].probes[0].policy, DirectProbePolicy::Run);
        assert!(
            input_transitions(&audit.witnesses[0].replay)
                > input_transitions(&audit.witnesses[1].replay),
            "the later run witness is the easier controller trace"
        );
        for witness in &audit.witnesses {
            assert_eq!(
                witness
                    .replay
                    .verify(&initial)
                    .unwrap()
                    .reached_exit
                    .as_deref(),
                Some("east")
            );
        }
    }

    #[test]
    fn direct_probe_audit_deduplicates_action_traces_and_keeps_probe_aliases() {
        let initial = Simulation::new(state_key_room(false));
        let targets = vec![SearchTarget::exit("east")];
        let resolved = vec![resolve_target(&initial, &targets[0]).unwrap()];
        let run = AuditedRouteProbe {
            probe: RouteProbe {
                move_x: 1,
                policy: DirectProbePolicy::Run,
            },
            target_indices: vec![0],
        };
        let audit = run_direct_probe_audit(
            &initial,
            &targets,
            &resolved,
            &SolverConfig::default(),
            &[run.clone(), run],
        );

        assert_eq!(audit.witnesses.len(), 1);
        assert_eq!(
            audit.witnesses[0]
                .probes
                .iter()
                .map(|probe| probe.ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn targeted_probe_interleave_preserves_per_target_vocabularies() {
        let shared = RouteProbe {
            move_x: 1,
            policy: DirectProbePolicy::Run,
        };
        let first_only = RouteProbe {
            move_x: -1,
            policy: DirectProbePolicy::DropThrough,
        };
        let second_only = RouteProbe {
            move_x: 0,
            policy: DirectProbePolicy::DropThrough,
        };
        let probes =
            interleave_targeted_probes(&[vec![shared, first_only], vec![shared, second_only]]);

        assert_eq!(
            probes,
            vec![
                AuditedRouteProbe {
                    probe: shared,
                    target_indices: vec![0, 1],
                },
                AuditedRouteProbe {
                    probe: first_only,
                    target_indices: vec![0],
                },
                AuditedRouteProbe {
                    probe: second_only,
                    target_indices: vec![1],
                },
            ]
        );
    }

    #[test]
    fn public_direct_probe_audit_is_repeatable_and_reports_budget_limits() {
        let initial = Simulation::new(state_key_room(false));
        let targets = vec![SearchTarget::exit("east")];
        let config = SolverConfig::default();
        let first = audit_direct_controller_probes(&initial, &targets, &config).unwrap();
        let second = audit_direct_controller_probes(&initial, &targets, &config).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.status, DirectProbeAuditStatus::Complete);
        assert!(first.witnesses.len() >= 2);

        let mut bounded = config;
        bounded.max_expanded_nodes = 1;
        let limited = audit_direct_controller_probes(&initial, &targets, &bounded).unwrap();
        assert_eq!(
            limited.status,
            DirectProbeAuditStatus::BudgetLimited(DirectProbeBudgetLimit::ExpandedNodes)
        );
        assert_eq!(limited.stats.expanded_nodes, 1);
        assert_eq!(limited.stats.generated_nodes, 1);
        assert!(!limited.witnesses.is_empty());
    }

    #[test]
    fn future_state_key_tracks_timed_phase_and_exact_pickup_bits() {
        let config = SolverConfig::default();
        let mut simulation = Simulation::new(state_key_room(true));
        let before = state_key(&simulation, Action::default(), &config);
        assert_eq!(before.room_tick, Some(0));
        assert_eq!(&*before.collected_pickups, &[false]);

        simulation.step(Action::default());
        let after = state_key(&simulation, Action::default(), &config);
        assert_eq!(after.room_tick, Some(1));
        assert_eq!(&*after.collected_pickups, &[true]);
        assert_ne!(before, after);

        let mut static_simulation = Simulation::new(state_key_room(false));
        let static_before = state_key(&static_simulation, Action::default(), &config);
        static_simulation.step(Action::default());
        let static_after = state_key(&static_simulation, Action::default(), &config);
        assert_eq!(static_before.room_tick, None);
        assert_eq!(static_after.room_tick, None);
        assert!(static_before.collected_pickups.is_empty());
    }

    #[test]
    fn buffered_wall_climb_probe_mounts_a_lip_and_records_an_exact_door_replay() {
        let abilities = AbilitySet::new(true, false);
        let initial = Simulation::enter_via_door(buffered_wall_lip_room(), abilities, "west")
            .expect("west arrival is valid");
        let target = resolve_target(&initial, &SearchTarget::door("east")).unwrap();
        let route_probe = RouteProbe {
            move_x: 1,
            policy: DirectProbePolicy::BufferedWallClimb,
        };
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        let mut wall_jumps = 0_usize;

        for probe_tick in 0..600 {
            let previous_action = actions.last().copied().unwrap_or_default();
            let action = route_probe.action(&simulation, previous_action, probe_tick);
            let report = simulation.step(action);
            actions.push(action);
            wall_jumps += report
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        SimulationEvent::Jumped(downwards_core::JumpKind::Wall { .. })
                    )
                })
                .count();
            if reached_target(&simulation, target).is_some() {
                break;
            }
        }

        assert_eq!(simulation.reached_exit(), Some("east"));
        assert!(wall_jumps >= 1, "the lip route must exercise wall jumping");
        assert!(
            actions.len() < 600,
            "direct lip probe reached the path horizon at {} ticks",
            actions.len()
        );
        assert_eq!(
            Replay::record(&initial, actions)
                .verify(&initial)
                .unwrap()
                .reached_exit
                .as_deref(),
            Some("east")
        );
    }
}
