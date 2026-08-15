//! Exact landing geometry observed along a replay.
//!
//! This is deliberately a narrow measurement. A small edge margin or support
//! overlap is evidence that the recorded landing used little geometric room;
//! it is not proof that the input window was small, that the landing was
//! mandatory, or that a person would find the route difficult.

use std::{error::Error, fmt};

use downwards_ai::{Replay, ReplayDivergence};
use downwards_core::{Rect, Simulation, SimulationEvent, Tile};

/// Schema version of [`LandingPrecisionReport`].
pub const LANDING_PRECISION_VERSION: u32 = 1;

/// Interpretation boundary for exact landing measurements.
pub const LANDING_PRECISION_DISCLAIMER: &str = "landing samples describe exact replay geometry only; a narrow observed margin is not proof of a narrow input window or a mandatory route, an unmeasured landing is not assigned zero margin, and human reproducibility belongs in perturbation evidence";

/// Collision material supporting a recorded landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LandingSupportKind {
    Solid,
    OneWay,
    Mixed,
}

/// One authoritative `Landed` event whose post-step surface could be
/// reconstructed exactly from the room tile field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandingSample {
    /// One-based replay tick.
    pub replay_tick: usize,
    pub player_bounds: Rect,
    /// Pixel y-coordinate of the supporting surface.
    pub surface_y: i32,
    /// Maximal contiguous collision surface containing the tile(s) directly
    /// below the player's footprint. Half-open pixel interval.
    pub support_left: i32,
    pub support_right: i32,
    pub support_kind: LandingSupportKind,
    /// Horizontal intersection of the player's footprint and support span.
    pub footprint_overlap_pixels: u32,
    /// Signed space from each player edge to the corresponding support edge.
    /// Negative means that edge overhangs the surface.
    pub left_edge_margin_pixels: i32,
    pub right_edge_margin_pixels: i32,
}

impl LandingSample {
    #[must_use]
    pub const fn support_width_pixels(self) -> u32 {
        self.support_right.saturating_sub(self.support_left) as u32
    }

    #[must_use]
    pub const fn minimum_edge_margin_pixels(self) -> i32 {
        if self.left_edge_margin_pixels < self.right_edge_margin_pixels {
            self.left_edge_margin_pixels
        } else {
            self.right_edge_margin_pixels
        }
    }

    #[must_use]
    pub const fn has_edge_overhang(self) -> bool {
        self.minimum_edge_margin_pixels() < 0
    }
}

/// Aggregate exact landing geometry through a caller-selected replay prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandingPrecisionReport {
    pub version: u32,
    pub inspected_ticks: usize,
    pub landing_event_count: usize,
    pub samples: Box<[LandingSample]>,
    /// One-based ticks where `Landed` was authoritative but the same step
    /// reset the room or no exact supporting tile surface could be recovered.
    pub unmeasured_landing_ticks: Box<[usize]>,
    pub minimum_footprint_overlap_pixels: Option<u32>,
    pub minimum_edge_margin_pixels: Option<i32>,
    pub narrowest_support_width_pixels: Option<u32>,
    pub edge_overhang_landings: usize,
    pub one_way_or_mixed_landings: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LandingPrecisionError {
    ReplayDiverged(ReplayDivergence),
    PrefixLongerThanReplay {
        requested_ticks: usize,
        replay_ticks: usize,
    },
}

impl fmt::Display for LandingPrecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReplayDiverged(source) => {
                write!(formatter, "cannot inspect landing geometry: {source}")
            }
            Self::PrefixLongerThanReplay {
                requested_ticks,
                replay_ticks,
            } => write!(
                formatter,
                "landing prefix requests {requested_ticks} ticks from a {replay_ticks}-tick replay"
            ),
        }
    }
}

impl Error for LandingPrecisionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReplayDiverged(source) => Some(source),
            Self::PrefixLongerThanReplay { .. } => None,
        }
    }
}

impl From<ReplayDivergence> for LandingPrecisionError {
    fn from(value: ReplayDivergence) -> Self {
        Self::ReplayDiverged(value)
    }
}

/// Verify a replay, then inspect every authoritative landing through exactly
/// `inspected_ticks` frames.
///
/// Callers measuring a target witness should pass its first-contact tick, not
/// necessarily the entire stored replay length. No target interpretation is
/// performed here, which keeps this usable for doors, exits, and pickups.
pub fn analyze_replay_landings(
    initial: &Simulation,
    replay: &Replay,
    inspected_ticks: usize,
) -> Result<LandingPrecisionReport, LandingPrecisionError> {
    if inspected_ticks > replay.frames.len() {
        return Err(LandingPrecisionError::PrefixLongerThanReplay {
            requested_ticks: inspected_ticks,
            replay_ticks: replay.frames.len(),
        });
    }
    replay.verify(initial)?;

    let mut simulation = initial.clone();
    let mut samples = Vec::new();
    let mut unmeasured = Vec::new();
    let mut landing_event_count = 0;
    for (frame_index, frame) in replay.frames[..inspected_ticks].iter().enumerate() {
        let report = simulation.step(frame.action);
        if !report.events.contains(&SimulationEvent::Landed) {
            continue;
        }
        landing_event_count += 1;
        let replay_tick = frame_index + 1;
        // Death/restart restores a different position before the post-step
        // state can be inspected, so never attribute its spawn support to the
        // earlier landing event.
        if report.events.contains(&SimulationEvent::Reset) {
            unmeasured.push(replay_tick);
            continue;
        }
        match landing_sample(&simulation, replay_tick) {
            Some(sample) => samples.push(sample),
            None => unmeasured.push(replay_tick),
        }
    }

    let minimum_footprint_overlap_pixels = samples
        .iter()
        .map(|sample| sample.footprint_overlap_pixels)
        .min();
    let minimum_edge_margin_pixels = samples
        .iter()
        .map(|sample| sample.minimum_edge_margin_pixels())
        .min();
    let narrowest_support_width_pixels = samples
        .iter()
        .map(|sample| sample.support_width_pixels())
        .min();
    let edge_overhang_landings = samples
        .iter()
        .filter(|sample| sample.has_edge_overhang())
        .count();
    let one_way_or_mixed_landings = samples
        .iter()
        .filter(|sample| sample.support_kind != LandingSupportKind::Solid)
        .count();

    Ok(LandingPrecisionReport {
        version: LANDING_PRECISION_VERSION,
        inspected_ticks,
        landing_event_count,
        samples: samples.into_boxed_slice(),
        unmeasured_landing_ticks: unmeasured.into_boxed_slice(),
        minimum_footprint_overlap_pixels,
        minimum_edge_margin_pixels,
        narrowest_support_width_pixels,
        edge_overhang_landings,
        one_way_or_mixed_landings,
    })
}

fn landing_sample(simulation: &Simulation, replay_tick: usize) -> Option<LandingSample> {
    let player = simulation.player().bounds();
    let room = simulation.room();
    let tile_size = room.tile_size();
    if player.bottom().rem_euclid(tile_size) != 0 {
        return None;
    }
    let row = player.bottom().div_euclid(tile_size);
    if row < 0 || row >= i32::from(room.height()) {
        return None;
    }
    let row = u16::try_from(row).ok()?;

    let first_column = player.x.div_euclid(tile_size).max(0);
    let last_column = (player.right() - 1)
        .div_euclid(tile_size)
        .min(i32::from(room.width()) - 1);
    let supporting = (first_column..=last_column)
        .filter_map(|column| {
            let column = u16::try_from(column).ok()?;
            let tile = room.tile(column, row)?;
            is_support(tile).then_some((column, tile))
        })
        .filter(|(column, _)| {
            let bounds = room.tile_bounds(*column, row);
            player.x < bounds.right() && player.right() > bounds.x
        })
        .collect::<Vec<_>>();
    let first = supporting.first()?.0;
    let last = supporting.last()?.0;

    let mut left = first;
    while left > 0 && room.tile(left - 1, row).is_some_and(is_support) {
        left -= 1;
    }
    let mut right = last;
    while right + 1 < room.width() && room.tile(right + 1, row).is_some_and(is_support) {
        right += 1;
    }

    let support_left = i32::from(left) * tile_size;
    let support_right = (i32::from(right) + 1) * tile_size;
    let overlap_left = player.x.max(support_left);
    let overlap_right = player.right().min(support_right);
    let footprint_overlap_pixels = overlap_right.saturating_sub(overlap_left) as u32;
    let has_solid = (left..=right).any(|column| room.tile(column, row) == Some(Tile::Solid));
    let has_one_way = (left..=right).any(|column| room.tile(column, row) == Some(Tile::OneWay));
    let support_kind = match (has_solid, has_one_way) {
        (true, false) => LandingSupportKind::Solid,
        (false, true) => LandingSupportKind::OneWay,
        (true, true) => LandingSupportKind::Mixed,
        (false, false) => return None,
    };

    Some(LandingSample {
        replay_tick,
        player_bounds: player,
        surface_y: player.bottom(),
        support_left,
        support_right,
        support_kind,
        footprint_overlap_pixels,
        left_edge_margin_pixels: player.x - support_left,
        right_edge_margin_pixels: support_right - player.right(),
    })
}

const fn is_support(tile: Tile) -> bool {
    matches!(tile, Tile::Solid | Tile::OneWay)
}

#[cfg(test)]
mod tests {
    use downwards_ai::Replay;
    use downwards_core::{Action, Point, Room, Tile};

    use super::*;

    fn falling_room(spawn_x: i32, support: Tile) -> Simulation {
        let width = 32_u16;
        let height = 18_u16;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        for x in 0..width {
            tiles[usize::from(x)] = Tile::Solid;
            tiles[usize::from(height - 1) * usize::from(width) + usize::from(x)] = Tile::Solid;
        }
        for y in 0..height {
            tiles[usize::from(y) * usize::from(width)] = Tile::Solid;
            tiles[usize::from(y) * usize::from(width) + usize::from(width - 1)] = Tile::Solid;
        }
        for x in 10..15 {
            tiles[10 * usize::from(width) + x] = support;
        }
        let room = Room::new(
            "landing",
            "Landing",
            width,
            height,
            10,
            tiles,
            Point::new(spawn_x, 50),
            Vec::new(),
        )
        .unwrap();
        Simulation::new(room)
    }

    fn idle_landing(spawn_x: i32, support: Tile) -> LandingPrecisionReport {
        let initial = falling_room(spawn_x, support);
        let replay = Replay::record(&initial, [Action::default(); 90]);
        analyze_replay_landings(&initial, &replay, replay.frames.len()).unwrap()
    }

    #[test]
    fn centered_and_edge_landings_have_distinct_signed_margins() {
        let centered = idle_landing(121, Tile::Solid);
        let edge = idle_landing(96, Tile::Solid);

        assert_eq!(centered.landing_event_count, 1);
        assert_eq!(centered.samples.len(), 1);
        assert_eq!(centered.samples[0].support_left, 100);
        assert_eq!(centered.samples[0].support_right, 150);
        assert_eq!(centered.minimum_footprint_overlap_pixels, Some(8));
        assert_eq!(centered.minimum_edge_margin_pixels, Some(21));
        assert_eq!(centered.edge_overhang_landings, 0);

        assert_eq!(edge.minimum_footprint_overlap_pixels, Some(4));
        assert_eq!(edge.minimum_edge_margin_pixels, Some(-4));
        assert_eq!(edge.edge_overhang_landings, 1);
    }

    #[test]
    fn one_way_support_and_empty_prefix_are_explicit() {
        let landing = idle_landing(121, Tile::OneWay);
        assert_eq!(landing.samples[0].support_kind, LandingSupportKind::OneWay);
        assert_eq!(landing.one_way_or_mixed_landings, 1);

        let initial = falling_room(121, Tile::Solid);
        let replay = Replay::record(&initial, [Action::default(); 2]);
        let before_landing = analyze_replay_landings(&initial, &replay, 2).unwrap();
        assert_eq!(before_landing.landing_event_count, 0);
        assert_eq!(before_landing.minimum_edge_margin_pixels, None);
        assert!(before_landing.samples.is_empty());
    }

    #[test]
    fn invalid_prefix_and_stale_replay_fail_closed() {
        let initial = falling_room(121, Tile::Solid);
        let replay = Replay::record(&initial, [Action::default(); 1]);
        assert!(matches!(
            analyze_replay_landings(&initial, &replay, 2),
            Err(LandingPrecisionError::PrefixLongerThanReplay { .. })
        ));

        let different = falling_room(122, Tile::Solid);
        assert!(matches!(
            analyze_replay_landings(&different, &replay, 1),
            Err(LandingPrecisionError::ReplayDiverged(_))
        ));
    }
}
