//! Opt-in diagnostic search for exact grounded support waypoints.
//!
//! This policy is deliberately separate from ordinary exit, door, and pickup
//! search.  In particular, it neither adds a `SearchTarget` variant nor
//! changes `SOLVER_POLICY_VERSION` or the witness returned by `solve*`.

use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt,
    rc::Rc,
};

use downwards_core::{Action, PLAYER_WIDTH, Simulation, SimulationEvent};

use crate::{
    InconclusiveReason, Replay, SearchStats, SolverConfig, SolverConfigError,
    solver::{state_key, validate_config},
};

/// Version of waypoint reach semantics, scoring, pruning, and tie-breaking.
///
/// This identity is intentionally independent of `SOLVER_POLICY_VERSION`.
pub const WAYPOINT_DIAGNOSTIC_POLICY_VERSION: u32 = 1;

/// One inclusive interval of admissible player top-left x coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroundedStandingRegion {
    min_player_x: i32,
    max_player_x: i32,
}

impl GroundedStandingRegion {
    pub fn new(min_player_x: i32, max_player_x: i32) -> Result<Self, GroundedSupportTargetError> {
        if min_player_x > max_player_x {
            return Err(GroundedSupportTargetError::ReversedStandingRegion {
                min_player_x,
                max_player_x,
            });
        }
        Ok(Self {
            min_player_x,
            max_player_x,
        })
    }

    #[must_use]
    pub const fn min_player_x(self) -> i32 {
        self.min_player_x
    }

    #[must_use]
    pub const fn max_player_x(self) -> i32 {
        self.max_player_x
    }

    const fn contains(self, player_x: i32) -> bool {
        self.min_player_x <= player_x && player_x <= self.max_player_x
    }
}

/// Exact horizontal support surface and its validated standing regions.
///
/// `surface_left` and `surface_right` are an exclusive pixel interval.  A
/// reached player must fit fully inside it, have its bottom exactly at
/// `surface_y`, and have an integer top-left x in one of `standing_regions`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroundedSupportTarget {
    surface_left: i32,
    surface_right: i32,
    surface_y: i32,
    standing_regions: Box<[GroundedStandingRegion]>,
}

impl GroundedSupportTarget {
    pub fn new(
        surface_left: i32,
        surface_right: i32,
        surface_y: i32,
        standing_regions: impl IntoIterator<Item = GroundedStandingRegion>,
    ) -> Result<Self, GroundedSupportTargetError> {
        if surface_left >= surface_right {
            return Err(GroundedSupportTargetError::EmptySupportSurface {
                surface_left,
                surface_right,
            });
        }
        let standing_regions = standing_regions.into_iter().collect::<Vec<_>>();
        if standing_regions.is_empty() {
            return Err(GroundedSupportTargetError::NoStandingRegions);
        }
        let mut previous = None::<GroundedStandingRegion>;
        for (index, region) in standing_regions.iter().copied().enumerate() {
            if region.min_player_x < surface_left
                || region.max_player_x.saturating_add(PLAYER_WIDTH) > surface_right
            {
                return Err(GroundedSupportTargetError::StandingRegionOutsideSurface {
                    index,
                    region,
                    surface_left,
                    surface_right,
                });
            }
            if let Some(previous) = previous
                && previous.max_player_x.saturating_add(1) >= region.min_player_x
            {
                return Err(GroundedSupportTargetError::NonCanonicalStandingRegions {
                    previous_index: index - 1,
                    next_index: index,
                });
            }
            previous = Some(region);
        }
        Ok(Self {
            surface_left,
            surface_right,
            surface_y,
            standing_regions: standing_regions.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn surface_left(&self) -> i32 {
        self.surface_left
    }

    #[must_use]
    pub const fn surface_right(&self) -> i32 {
        self.surface_right
    }

    #[must_use]
    pub const fn surface_y(&self) -> i32 {
        self.surface_y
    }

    #[must_use]
    pub fn standing_regions(&self) -> &[GroundedStandingRegion] {
        &self.standing_regions
    }

    /// Whether this is the exact stable grounded state certified by policy v1.
    #[must_use]
    pub fn is_reached(&self, simulation: &Simulation) -> bool {
        if simulation.reached_exit().is_some() {
            return false;
        }
        let player = simulation.player();
        let bounds = player.bounds();
        let velocity = player.velocity_subpixels();
        player.grounded()
            && velocity.x == 0
            && velocity.y == 0
            && player.dash_ticks_remaining() == 0
            && player.one_way_drop_ticks_remaining() == 0
            && bounds.bottom() == self.surface_y
            && bounds.x >= self.surface_left
            && bounds.right() <= self.surface_right
            && self
                .standing_regions
                .iter()
                .any(|region| region.contains(bounds.x))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundedSupportTargetError {
    EmptySupportSurface {
        surface_left: i32,
        surface_right: i32,
    },
    ReversedStandingRegion {
        min_player_x: i32,
        max_player_x: i32,
    },
    NoStandingRegions,
    StandingRegionOutsideSurface {
        index: usize,
        region: GroundedStandingRegion,
        surface_left: i32,
        surface_right: i32,
    },
    NonCanonicalStandingRegions {
        previous_index: usize,
        next_index: usize,
    },
}

impl fmt::Display for GroundedSupportTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySupportSurface {
                surface_left,
                surface_right,
            } => write!(
                formatter,
                "support surface must have positive width, got {surface_left}..{surface_right}"
            ),
            Self::ReversedStandingRegion {
                min_player_x,
                max_player_x,
            } => write!(
                formatter,
                "standing region is reversed: {min_player_x}..={max_player_x}"
            ),
            Self::NoStandingRegions => write!(formatter, "support target has no standing region"),
            Self::StandingRegionOutsideSurface {
                index,
                region,
                surface_left,
                surface_right,
            } => write!(
                formatter,
                "standing region {index} ({region:?}) does not fit player width inside support surface {surface_left}..{surface_right}"
            ),
            Self::NonCanonicalStandingRegions {
                previous_index,
                next_index,
            } => write!(
                formatter,
                "standing regions {previous_index} and {next_index} are not strictly ordered with a gap"
            ),
        }
    }
}

impl Error for GroundedSupportTargetError {}

/// Exact positive evidence for one support waypoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroundedSupportSolution {
    pub target: GroundedSupportTarget,
    pub replay: Replay,
    pub stats: SearchStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundedSupportSolveOutcome {
    Solved(GroundedSupportSolution),
    /// Beam pruning and finite budgets make every non-success inconclusive.
    Inconclusive {
        reason: InconclusiveReason,
        stats: SearchStats,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundedSupportSolveError {
    SolverConfiguration(SolverConfigError),
}

impl fmt::Display for GroundedSupportSolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SolverConfiguration(error) => {
                write!(formatter, "invalid waypoint solver configuration: {error}")
            }
        }
    }
}

impl Error for GroundedSupportSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SolverConfiguration(error) => Some(error),
        }
    }
}

/// Run the opt-in waypoint search without changing ordinary solver behavior.
pub fn solve_grounded_support(
    initial: &Simulation,
    target: &GroundedSupportTarget,
    config: &SolverConfig,
) -> Result<GroundedSupportSolveOutcome, GroundedSupportSolveError> {
    validate_config(config).map_err(GroundedSupportSolveError::SolverConfiguration)?;
    let mut stats = SearchStats::default();
    if target.is_reached(initial) {
        return Ok(GroundedSupportSolveOutcome::Solved(
            GroundedSupportSolution {
                target: target.clone(),
                replay: Replay::record(initial, []),
                stats,
            },
        ));
    }
    if config.max_expanded_nodes == 0 {
        return Ok(inconclusive(InconclusiveReason::ExpandedNodeBudget, stats));
    }
    if config.max_simulated_ticks == 0 {
        return Ok(inconclusive(InconclusiveReason::SimulatedTickBudget, stats));
    }

    let initial_action = Action::default();
    let initial_node = WaypointNode {
        simulation: initial.clone(),
        path: None,
        path_ticks: 0,
        previous_action: initial_action,
        score: target_distance(initial, target),
        serial: 0,
    };
    let mut visited = HashMap::from([(
        state_key(&initial_node.simulation, initial_action, config),
        0_usize,
    )]);
    let mut frontier = vec![initial_node];
    let mut serial = 1_u64;
    let mut reached_horizon = false;

    while !frontier.is_empty() {
        let mut candidates = Vec::new();
        for node in frontier {
            if stats.expanded_nodes >= config.max_expanded_nodes {
                return Ok(inconclusive(InconclusiveReason::ExpandedNodeBudget, stats));
            }
            stats.expanded_nodes += 1;

            for (macro_index, action_macro) in config.macros.iter().enumerate() {
                if node.path_ticks + action_macro.actions.len() > config.max_ticks_per_path {
                    reached_horizon = true;
                    continue;
                }
                let mut child_simulation = node.simulation.clone();
                let mut terminal = false;
                let mut applied_actions = 0_usize;
                for (action_index, &action) in action_macro.actions.iter().enumerate() {
                    if stats.simulated_ticks >= config.max_simulated_ticks {
                        return Ok(inconclusive(InconclusiveReason::SimulatedTickBudget, stats));
                    }
                    let report = child_simulation.step(action);
                    stats.simulated_ticks += 1;
                    applied_actions = action_index + 1;
                    let path_ticks = node.path_ticks + applied_actions;
                    stats.deepest_path_ticks = stats.deepest_path_ticks.max(path_ticks);
                    if report
                        .events
                        .iter()
                        .any(|event| matches!(event, SimulationEvent::Died(_)))
                        || child_simulation.reached_exit().is_some()
                    {
                        terminal = true;
                        break;
                    }
                    if target.is_reached(&child_simulation) {
                        stats.generated_nodes += 1;
                        let actions = reconstruct_actions(
                            node.path.as_ref(),
                            config,
                            Some((macro_index, applied_actions)),
                            path_ticks,
                        );
                        return Ok(GroundedSupportSolveOutcome::Solved(
                            GroundedSupportSolution {
                                target: target.clone(),
                                replay: Replay::record(initial, actions),
                                stats,
                            },
                        ));
                    }
                }
                stats.generated_nodes += 1;
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
                candidates.push(WaypointNode {
                    score: waypoint_score(&child_simulation, target, path_ticks),
                    simulation: child_simulation,
                    path: Some(Rc::new(WaypointPathSegment {
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
        frontier = select_frontier(candidates, config.beam_width);
    }

    Ok(inconclusive(
        if reached_horizon {
            InconclusiveReason::PathHorizon
        } else {
            InconclusiveReason::FrontierExhausted
        },
        stats,
    ))
}

struct WaypointNode {
    simulation: Simulation,
    path: Option<Rc<WaypointPathSegment>>,
    path_ticks: usize,
    previous_action: Action,
    score: i64,
    serial: u64,
}

struct WaypointPathSegment {
    parent: Option<Rc<Self>>,
    macro_index: usize,
}

fn reconstruct_actions(
    path: Option<&Rc<WaypointPathSegment>>,
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

fn waypoint_score(
    simulation: &Simulation,
    target: &GroundedSupportTarget,
    path_ticks: usize,
) -> i64 {
    target_distance(simulation, target)
        .saturating_add(i64::try_from(path_ticks / 4).unwrap_or(i64::MAX / 4))
}

fn target_distance(simulation: &Simulation, target: &GroundedSupportTarget) -> i64 {
    let player = simulation.player();
    let bounds = player.bounds();
    let horizontal = target
        .standing_regions()
        .iter()
        .map(|region| {
            if bounds.x < region.min_player_x() {
                region.min_player_x() - bounds.x
            } else if bounds.x > region.max_player_x() {
                bounds.x - region.max_player_x()
            } else {
                0
            }
        })
        .min()
        .unwrap_or(i32::MAX);
    let vertical = bounds.bottom().abs_diff(target.surface_y());
    let velocity = player.velocity_subpixels();
    i64::from(horizontal)
        .saturating_add(i64::from(vertical))
        .saturating_add(i64::from(velocity.x.unsigned_abs() / 256))
        .saturating_add(i64::from(velocity.y.unsigned_abs() / 256))
        .saturating_add(if player.grounded() { 0 } else { 8 })
}

fn select_frontier(mut candidates: Vec<WaypointNode>, beam_width: usize) -> Vec<WaypointNode> {
    candidates.sort_unstable_by_key(|node| (node.score, node.path_ticks, node.serial));
    if candidates.len() <= beam_width {
        return candidates;
    }

    const REGION_PIXELS: i32 = 16;
    let mut region_indices = HashMap::<(i32, i32), usize>::new();
    let mut regions = Vec::<VecDeque<WaypointNode>>::new();
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

const fn inconclusive(
    reason: InconclusiveReason,
    stats: SearchStats,
) -> GroundedSupportSolveOutcome {
    GroundedSupportSolveOutcome::Inconclusive { reason, stats }
}

#[cfg(test)]
mod tests {
    use downwards_core::{Point, Room, Tile};

    use super::*;

    const WIDTH: usize = 32;
    const HEIGHT: usize = 18;

    fn platform_room() -> Room {
        let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        for x in 0..WIDTH {
            tiles[16 * WIDTH + x] = Tile::Solid;
        }
        for x in 12..18 {
            tiles[12 * WIDTH + x] = Tile::OneWay;
        }
        Room::new(
            "waypoint",
            "Waypoint",
            WIDTH as u16,
            HEIGHT as u16,
            10,
            tiles,
            Point::new(60, 148),
            vec![],
        )
        .unwrap()
    }

    fn target_for_platform() -> GroundedSupportTarget {
        GroundedSupportTarget::new(
            120,
            180,
            120,
            [GroundedStandingRegion::new(120, 172).unwrap()],
        )
        .unwrap()
    }

    fn target_for_absent_surface() -> GroundedSupportTarget {
        GroundedSupportTarget::new(40, 100, 80, [GroundedStandingRegion::new(40, 92).unwrap()])
            .unwrap()
    }

    #[test]
    fn target_requires_exact_stationary_ground_contact() {
        let initial = Simulation::new(platform_room());
        let target =
            GroundedSupportTarget::new(0, 320, 160, [GroundedStandingRegion::new(0, 312).unwrap()])
                .unwrap();
        assert!(!target.is_reached(&initial));

        let outcome = solve_grounded_support(&initial, &target, &SolverConfig::default()).unwrap();
        let GroundedSupportSolveOutcome::Solved(solution) = outcome else {
            panic!("floor support should be certified: {outcome:?}");
        };
        assert!(solution.replay.verify(&initial).is_ok());
        let mut final_state = initial;
        for action in solution.replay.actions() {
            final_state.step(action);
        }
        assert!(target.is_reached(&final_state));
    }

    #[test]
    fn bounded_physically_absent_waypoint_is_never_called_unreachable() {
        let initial = Simulation::new(platform_room());
        let config = SolverConfig {
            max_expanded_nodes: 1,
            max_simulated_ticks: 32,
            max_ticks_per_path: 16,
            beam_width: 4,
            ..SolverConfig::default()
        };
        let outcome =
            solve_grounded_support(&initial, &target_for_absent_surface(), &config).unwrap();
        assert!(matches!(
            outcome,
            GroundedSupportSolveOutcome::Inconclusive { .. }
        ));
    }

    #[test]
    fn waypoint_search_is_deterministic() {
        let initial = Simulation::new(platform_room());
        let target = target_for_platform();
        let config = SolverConfig {
            max_expanded_nodes: 20_000,
            max_simulated_ticks: 500_000,
            max_ticks_per_path: 300,
            beam_width: 128,
            ..SolverConfig::default()
        };
        let first = solve_grounded_support(&initial, &target, &config).unwrap();
        let second = solve_grounded_support(&initial, &target, &config).unwrap();
        assert_eq!(first, second);
    }
}
