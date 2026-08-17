//! Tempo-grid derivation (§6.2 of the soundtrack spec).
//!
//! Pure integer math: given a room's timed-hazard periods and phases, choose a
//! musical step length `u` (in 60 Hz ticks), a beat subdivision `b`, and a
//! grid offset `φ` such that every rising edge of every *locked* hazard lands
//! exactly on a grid step. No tolerances anywhere — exact modular arithmetic.

use crate::track::TempoGrid;

/// Ticks per second of the simulation clock (`downwards_core::TICKS_PER_SECOND`).
pub const TICKS_PER_SECOND: u32 = 60;

/// The client's amber wind-up telegraph length in ticks. Mirrors
/// `TIMED_HAZARD_WARNING_TICKS` in `downwards-client/src/main.rs`; the client
/// `debug_assert_eq!`s the two constants when it constructs the audio engine.
pub const WARNING_TICKS: u32 = 24;

/// Loop length in bars; meter is always 4/4.
pub const LOOP_BARS: u32 = 16;

/// The timing triple of one timed hazard, copied from
/// `downwards_core::TimedHazard` accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HazardTiming {
    pub period: u32,
    pub active: u32,
    pub phase: u32,
}

impl HazardTiming {
    /// The first room tick at which the hazard's activation rises; rising
    /// edges then repeat every `period` ticks. Matches
    /// `TimedHazard::is_active_at` (`downwards-core/src/room.rs:124`).
    #[must_use]
    pub const fn fire_tick(self) -> u32 {
        (self.period - self.phase) % self.period
    }

    /// Mirror of `TimedHazard::is_active_at` for a u64 room tick.
    #[must_use]
    pub const fn is_active_at(self, room_tick: u64) -> bool {
        let period = self.period as u64;
        (room_tick % period + self.phase as u64) % period < self.active as u64
    }
}

/// Difficulty axis of the composition mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Target BPM used to score tempo candidates (§6.2 step 3).
    #[must_use]
    pub const fn target_bpm(self) -> u32 {
        match self {
            Self::Easy => 96,
            Self::Medium => 114,
            Self::Hard => 136,
        }
    }

    /// Hazard-free / empty-locked-set fallback grid (§6.2).
    #[must_use]
    pub const fn default_grid(self) -> (u32, u32) {
        match self {
            Self::Easy => (10, 4),
            Self::Medium => (8, 4),
            Self::Hard => (7, 4),
        }
    }
}

/// Result of the grid derivation: the tempo grid plus the indices of the
/// hazards whose rising edges provably land on grid steps ("locked").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridChoice {
    pub grid: TempoGrid,
    pub locked: Vec<usize>,
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

/// Alignment gcd `G(S)` of a hazard subset: gcd of every member period and of
/// every pairwise fire-tick difference. `G({i}) = period_i`.
fn alignment_gcd(hazards: &[HazardTiming], subset: &[usize]) -> u32 {
    let mut value = 0;
    for &index in subset {
        value = gcd(value, hazards[index].period);
    }
    for (position, &left) in subset.iter().enumerate() {
        for &right in &subset[position + 1..] {
            let difference =
                hazards[left].fire_tick().abs_diff(hazards[right].fire_tick());
            value = gcd(value, difference);
        }
    }
    value
}

/// All musical candidates for an alignment gcd: step `u` divides `G`, `u >= 3`,
/// beat length `u * b` in `[24, 42]` ticks with `b` in `{2, 4, 8}`.
fn candidates(alignment: u32) -> Vec<(u32, u32)> {
    let mut pairs = Vec::new();
    for step in 3..=alignment {
        if !alignment.is_multiple_of(step) {
            continue;
        }
        for beat_steps in [2_u32, 4, 8] {
            let beat_ticks = step * beat_steps;
            if (24..=42).contains(&beat_ticks) {
                pairs.push((step, beat_steps));
            }
        }
    }
    pairs
}

/// Pick the candidate minimizing `|bpm − target|`; tiebreak larger `u`
/// (coarser grid), then larger `b`. Comparisons are exact integer arithmetic:
/// `|3600/(u·b) − T|` compares as `|3600 − T·u·b| / (u·b)` cross-multiplied.
fn best_candidate(pairs: &[(u32, u32)], target_bpm: u32) -> Option<(u32, u32)> {
    pairs.iter().copied().max_by(|&(u_a, b_a), &(u_b, b_b)| {
        let beat_a = u64::from(u_a * b_a);
        let beat_b = u64::from(u_b * b_b);
        // |3600/beat − T| = |3600 − T·beat| / beat; compare a/b vs c/d as a·d vs c·b.
        let numerator_a = (3600_i64 - i64::from(target_bpm) * beat_a as i64).unsigned_abs();
        let numerator_b = (3600_i64 - i64::from(target_bpm) * beat_b as i64).unsigned_abs();
        let left = u128::from(numerator_a) * u128::from(beat_b);
        let right = u128::from(numerator_b) * u128::from(beat_a);
        // Smaller distance wins, so reverse the distance comparison for max_by.
        right
            .cmp(&left)
            .then(u_a.cmp(&u_b))
            .then(b_a.cmp(&b_b))
    })
}

/// The member of a subset with the smallest `(period, phase, index)` triple.
fn first_member(hazards: &[HazardTiming], subset: &[usize]) -> usize {
    *subset
        .iter()
        .min_by_key(|&&index| (hazards[index].period, hazards[index].phase, index))
        .expect("subset is non-empty")
}

/// Derive the tempo grid and locked hazard set for a room (§6.2).
#[must_use]
pub fn derive_grid(hazards: &[HazardTiming], difficulty: Difficulty) -> GridChoice {
    let target = difficulty.target_bpm();

    let fallback = |locked: Vec<usize>| {
        let (step_ticks, beat_steps) = difficulty.default_grid();
        GridChoice {
            grid: TempoGrid {
                step_ticks,
                beat_steps,
                grid_offset: 0,
                loop_bars: LOOP_BARS,
            },
            locked,
        }
    };

    if hazards.is_empty() {
        return fallback(Vec::new());
    }

    let all: Vec<usize> = (0..hazards.len()).collect();
    let full_gcd = alignment_gcd(hazards, &all);
    if let Some((step_ticks, beat_steps)) = best_candidate(&candidates(full_gcd), target) {
        let phi = hazards[first_member(hazards, &all)].fire_tick() % step_ticks;
        return GridChoice {
            grid: TempoGrid {
                step_ticks,
                beat_steps,
                grid_offset: phi,
                loop_bars: LOOP_BARS,
            },
            locked: all,
        };
    }

    // Partition: enumerate non-empty subsets as bitmasks counting up; keep
    // subsets admitting at least one candidate; choose max size, then min sum
    // of indices, then first in enumeration order.
    struct BestSubset {
        size: usize,
        index_sum: usize,
        mask: u64,
        pair: (u32, u32),
        subset: Vec<usize>,
    }
    let count = hazards.len().min(24);
    let mut best: Option<BestSubset> = None;
    for mask in 1_u64..(1 << count) {
        let subset: Vec<usize> = (0..count).filter(|&bit| mask & (1 << bit) != 0).collect();
        let subset_gcd = alignment_gcd(hazards, &subset);
        let Some(pair) = best_candidate(&candidates(subset_gcd), target) else {
            continue;
        };
        let size = subset.len();
        let index_sum: usize = subset.iter().sum();
        let better = match &best {
            None => true,
            Some(current) => {
                size > current.size
                    || (size == current.size
                        && (index_sum < current.index_sum
                            || (index_sum == current.index_sum && mask < current.mask)))
            }
        };
        if better {
            best = Some(BestSubset {
                size,
                index_sum,
                mask,
                pair,
                subset,
            });
        }
    }

    match best {
        Some(BestSubset {
            pair: (step_ticks, beat_steps),
            subset,
            ..
        }) => {
            let phi = hazards[first_member(hazards, &subset)].fire_tick() % step_ticks;
            GridChoice {
                grid: TempoGrid {
                    step_ticks,
                    beat_steps,
                    grid_offset: phi,
                    loop_bars: LOOP_BARS,
                },
                locked: subset,
            }
        }
        None => fallback(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_and_alignment_gcd_are_exact() {
        assert_eq!(gcd(90, 200), 10);
        let hazards = [
            HazardTiming { period: 180, active: 30, phase: 120 },
            HazardTiming { period: 180, active: 30, phase: 30 },
        ];
        assert_eq!(hazards[0].fire_tick(), 60);
        assert_eq!(hazards[1].fire_tick(), 150);
        assert_eq!(alignment_gcd(&hazards, &[0, 1]), 90);
        assert_eq!(alignment_gcd(&hazards, &[0]), 180);
    }
}
