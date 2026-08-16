use std::{error::Error, fmt};

use downwards_ai::{Replay, ReplayDivergence, Solution};
use downwards_core::{
    Action, DashDirection, DeathReason, JumpKind, Simulation, SimulationEvent, WallSide,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TraversalGrid {
    columns: u16,
    rows: u16,
}

impl TraversalGrid {
    pub fn new(columns: u16, rows: u16) -> Result<Self, TraversalGridError> {
        if columns == 0 || rows == 0 {
            return Err(TraversalGridError { columns, rows });
        }
        Ok(Self { columns, rows })
    }

    #[must_use]
    pub const fn columns(self) -> u16 {
        self.columns
    }

    #[must_use]
    pub const fn rows(self) -> u16 {
        self.rows
    }
}

impl Default for TraversalGrid {
    fn default() -> Self {
        Self {
            columns: 16,
            rows: 9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraversalGridError {
    pub columns: u16,
    pub rows: u16,
}

impl fmt::Display for TraversalGridError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "traversal grid dimensions must be positive, got {}x{}",
            self.columns, self.rows
        )
    }
}

impl Error for TraversalGridError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraversalCell {
    pub x: u16,
    pub y: u16,
}

/// A run of identical coarse positions. `samples` counts authoritative
/// positions, not simulation actions; the first span includes the tick-zero
/// position and the whole trace therefore has `completion_ticks + 1` samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TraversalSpan {
    pub cell: TraversalCell,
    pub samples: usize,
}

/// Coarse behavior-space trace of a successful replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraversalTrace {
    pub grid: TraversalGrid,
    pub sample_count: usize,
    pub spans: Box<[TraversalSpan]>,
    /// Canonical row-major set of cells visited at least once.
    pub visited_cells: Box<[TraversalCell]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticAction {
    pub move_x: i8,
    pub move_y: i8,
    pub jump_held: bool,
    pub dash_held: bool,
    pub restart: bool,
}

impl From<Action> for SemanticAction {
    fn from(action: Action) -> Self {
        Self {
            move_x: action.move_x.clamp(-1, 1),
            move_y: action.move_y.clamp(-1, 1),
            jump_held: action.jump,
            dash_held: action.dash,
            restart: action.restart,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActionSpan {
    pub action: SemanticAction,
    pub ticks: usize,
}

/// IDs and exact hazard coordinates are intentionally omitted: this describes
/// what happened in traversal terms, not which content record emitted it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticEvent {
    GroundJump,
    CoyoteJump,
    BufferedJump,
    WallJump(WallSide),
    Dash(DashDirection),
    Land,
    DeathFromStaticHazard,
    DeathFromTimedHazard,
    Pickup,
    Reset,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SemanticEventAt {
    /// One-based simulation tick within the successful replay prefix.
    pub tick: usize,
    pub event: SemanticEvent,
}

/// Run-length encoded semantic input plus authoritative traversal events.
///
/// Press counts include attempted input edges. Successful traversal verbs are
/// counted separately from authoritative simulation events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticActionTrace {
    pub total_ticks: usize,
    pub spans: Box<[ActionSpan]>,
    pub events: Box<[SemanticEventAt]>,
    pub jump_presses: usize,
    pub dash_presses: usize,
    pub restart_presses: usize,
    pub successful_jumps: usize,
    pub successful_wall_jumps: usize,
    pub successful_dashes: usize,
    pub deaths: usize,
    pub pickups_collected: usize,
}

/// Multiple independent observations of one verified successful AI witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuccessfulWitnessObservation {
    pub reached_exit_id: String,
    /// Number of replay frames through first exit contact. Any recorded input
    /// after completion is verified for integrity but omitted from behavior.
    pub completion_ticks: usize,
    pub traversal: TraversalTrace,
    pub actions: SemanticActionTrace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WitnessObservationError {
    ReplayDiverged(ReplayDivergence),
    DidNotReachExit,
    ReachedUnexpectedExit {
        expected_exit_id: String,
        actual_exit_id: String,
    },
}

impl fmt::Display for WitnessObservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReplayDiverged(error) => write!(formatter, "cannot observe replay: {error}"),
            Self::DidNotReachExit => write!(formatter, "replay does not reach an exit"),
            Self::ReachedUnexpectedExit {
                expected_exit_id,
                actual_exit_id,
            } => write!(
                formatter,
                "solution claims exit {expected_exit_id:?}, but replay reaches {actual_exit_id:?}"
            ),
        }
    }
}

impl Error for WitnessObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReplayDiverged(error) => Some(error),
            Self::DidNotReachExit | Self::ReachedUnexpectedExit { .. } => None,
        }
    }
}

impl From<ReplayDivergence> for WitnessObservationError {
    fn from(value: ReplayDivergence) -> Self {
        Self::ReplayDiverged(value)
    }
}

/// Observe a solver result and additionally require the replay to reach the
/// exit named by the solution.
pub fn observe_solution(
    initial: &Simulation,
    solution: &Solution,
    grid: TraversalGrid,
) -> Result<SuccessfulWitnessObservation, WitnessObservationError> {
    let observation = observe_successful_replay(initial, &solution.replay, grid)?;
    if observation.reached_exit_id != solution.exit_id {
        return Err(WitnessObservationError::ReachedUnexpectedExit {
            expected_exit_id: solution.exit_id.clone(),
            actual_exit_id: observation.reached_exit_id,
        });
    }
    Ok(observation)
}

/// Verify and observe the prefix through first exit contact.
///
/// Replay verification happens before measurement, so a forged or stale
/// witness cannot silently contribute misleading behavior features.
pub fn observe_successful_replay(
    initial: &Simulation,
    replay: &Replay,
    grid: TraversalGrid,
) -> Result<SuccessfulWitnessObservation, WitnessObservationError> {
    replay.verify(initial)?;

    let room_width = i32::from(initial.room().width()) * initial.room().tile_size();
    let room_height = i32::from(initial.room().height()) * initial.room().tile_size();
    let mut simulation = initial.clone();
    let initial_cell = traversal_cell(&simulation, grid, room_width, room_height);
    let mut traversal_spans = vec![TraversalSpan {
        cell: initial_cell,
        samples: 1,
    }];
    let mut visited = vec![false; usize::from(grid.columns) * usize::from(grid.rows)];
    mark_visited(&mut visited, grid, initial_cell);

    let mut action_spans = Vec::<ActionSpan>::new();
    let mut semantic_events = Vec::new();
    let mut previous = SemanticAction::default();
    let mut jump_presses = 0;
    let mut dash_presses = 0;
    let mut restart_presses = 0;
    let mut successful_jumps = 0;
    let mut successful_wall_jumps = 0;
    let mut successful_dashes = 0;
    let mut deaths = 0;
    let mut pickups_collected = 0;
    let mut reached_exit_id = simulation.reached_exit().map(str::to_owned);
    let mut completion_ticks = 0;

    for frame in &replay.frames {
        if reached_exit_id.is_some() {
            break;
        }
        let action = SemanticAction::from(frame.action);
        if action.jump_held && !previous.jump_held {
            jump_presses += 1;
        }
        if action.dash_held && !previous.dash_held {
            dash_presses += 1;
        }
        if action.restart && !previous.restart {
            restart_presses += 1;
        }
        push_action_span(&mut action_spans, action);

        let report = simulation.step(frame.action);
        completion_ticks += 1;
        for event in report.events {
            let semantic = match event {
                SimulationEvent::Jumped(JumpKind::Grounded) => {
                    successful_jumps += 1;
                    SemanticEvent::GroundJump
                }
                SimulationEvent::Jumped(JumpKind::Coyote) => {
                    successful_jumps += 1;
                    SemanticEvent::CoyoteJump
                }
                SimulationEvent::Jumped(JumpKind::Buffered) => {
                    successful_jumps += 1;
                    SemanticEvent::BufferedJump
                }
                SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                    successful_jumps += 1;
                    successful_wall_jumps += 1;
                    SemanticEvent::WallJump(side)
                }
                SimulationEvent::Dashed { direction } => {
                    successful_dashes += 1;
                    SemanticEvent::Dash(direction)
                }
                SimulationEvent::Landed => SemanticEvent::Land,
                SimulationEvent::Died(DeathReason::Hazard { .. }) => {
                    deaths += 1;
                    SemanticEvent::DeathFromStaticHazard
                }
                SimulationEvent::Died(DeathReason::TimedHazard { .. }) => {
                    deaths += 1;
                    SemanticEvent::DeathFromTimedHazard
                }
                SimulationEvent::PickupTouched { .. } => continue,
                SimulationEvent::PickupCollected { .. } => {
                    pickups_collected += 1;
                    SemanticEvent::Pickup
                }
                SimulationEvent::Reset => SemanticEvent::Reset,
                SimulationEvent::ExitReached { id } => {
                    reached_exit_id = Some(id);
                    SemanticEvent::Exit
                }
            };
            semantic_events.push(SemanticEventAt {
                tick: completion_ticks,
                event: semantic,
            });
        }

        let cell = traversal_cell(&simulation, grid, room_width, room_height);
        push_traversal_span(&mut traversal_spans, cell);
        mark_visited(&mut visited, grid, cell);
        previous = if semantic_events.last().is_some_and(|event| {
            event.tick == completion_ticks && event.event == SemanticEvent::Reset
        }) {
            SemanticAction::default()
        } else {
            action
        };

        if reached_exit_id.is_some() {
            break;
        }
    }

    let reached_exit_id = reached_exit_id.ok_or(WitnessObservationError::DidNotReachExit)?;
    let visited_cells = visited
        .into_iter()
        .enumerate()
        .filter_map(|(index, visited)| {
            visited.then_some(TraversalCell {
                x: (index % usize::from(grid.columns)) as u16,
                y: (index / usize::from(grid.columns)) as u16,
            })
        })
        .collect();

    Ok(SuccessfulWitnessObservation {
        reached_exit_id,
        completion_ticks,
        traversal: TraversalTrace {
            grid,
            sample_count: completion_ticks + 1,
            spans: traversal_spans.into_boxed_slice(),
            visited_cells,
        },
        actions: SemanticActionTrace {
            total_ticks: completion_ticks,
            spans: action_spans.into_boxed_slice(),
            events: semantic_events.into_boxed_slice(),
            jump_presses,
            dash_presses,
            restart_presses,
            successful_jumps,
            successful_wall_jumps,
            successful_dashes,
            deaths,
            pickups_collected,
        },
    })
}

fn traversal_cell(
    simulation: &Simulation,
    grid: TraversalGrid,
    room_width: i32,
    room_height: i32,
) -> TraversalCell {
    let bounds = simulation.player().bounds();
    let center_x = (bounds.x + bounds.width / 2).clamp(0, room_width - 1);
    let center_y = (bounds.y + bounds.height / 2).clamp(0, room_height - 1);
    TraversalCell {
        x: ((i64::from(center_x) * i64::from(grid.columns)) / i64::from(room_width)) as u16,
        y: ((i64::from(center_y) * i64::from(grid.rows)) / i64::from(room_height)) as u16,
    }
}

fn mark_visited(visited: &mut [bool], grid: TraversalGrid, cell: TraversalCell) {
    visited[usize::from(cell.y) * usize::from(grid.columns) + usize::from(cell.x)] = true;
}

fn push_traversal_span(spans: &mut Vec<TraversalSpan>, cell: TraversalCell) {
    if let Some(last) = spans.last_mut()
        && last.cell == cell
    {
        last.samples += 1;
    } else {
        spans.push(TraversalSpan { cell, samples: 1 });
    }
}

fn push_action_span(spans: &mut Vec<ActionSpan>, action: SemanticAction) {
    if let Some(last) = spans.last_mut()
        && last.action == action
    {
        last.ticks += 1;
    } else {
        spans.push(ActionSpan { action, ticks: 1 });
    }
}
