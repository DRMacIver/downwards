//! Seeded constrained-walk melody generation (§6.5 of the soundtrack spec).
//!
//! Deterministic: the only randomness is xorshift64* seeded with
//! `fnv1a64(slug)`, consumed in a fixed order — the sixteen loop downbeats
//! first, then fills bar by bar. Changing a later rule can therefore never
//! reshuffle earlier choices.

use crate::{
    theory::{Key, XorShift64Star, fnv1a64},
    track::{NoteEvent, TempoGrid},
};

/// Which room boundaries have doors; drives the contour bias (§6.5 rule 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DoorSet {
    pub west: bool,
    pub east: bool,
    pub ceiling: bool,
    pub floor: bool,
}

/// Melody range: C4..=C6 as scale-lattice bounds are enforced in MIDI space.
const RANGE_LOW_MIDI: u8 = 60;
const RANGE_HIGH_MIDI: u8 = 84;

const LOOP_BARS: u32 = 16;

/// Downbeat and fill velocities (tuning constants live here; see
/// `compose::defaults` for the mixer gains).
const DOWNBEAT_VEL: u8 = 12;
const FILL_VEL: u8 = 9;

struct Walk<'a> {
    key: Key,
    rng: XorShift64Star,
    grid: &'a TempoGrid,
    progression: [u8; 4],
    coin_count: u32,
    doors: DoorSet,
}

/// Generate the full 16-bar lead-voice pattern: a constrained walk whose
/// downbeats are chord tones of each bar's actual chord, fills tethered
/// between neighbouring downbeats, and the final note forced to the tonic.
#[must_use]
pub fn generate_melody(
    slug: &str,
    key: Key,
    grid: &TempoGrid,
    progression: [u8; 4],
    coin_count: u32,
    doors: DoorSet,
    voice_name: &str,
) -> Vec<NoteEvent> {
    let mut walk = Walk {
        key,
        rng: XorShift64Star::new(fnv1a64(slug.as_bytes())),
        grid,
        progression,
        coin_count,
        doors,
    };

    // Phase 1: the sixteen loop downbeats (scale-lattice indices), each a
    // chord tone of its *loop* bar's chord. (Redesigned 2026-08: the original
    // 8-bar phrase repeated over the 16-bar progression put the second pass's
    // downbeats on the wrong chords — audibly dissonant against the
    // accompaniment, part of the designer's "annoying" complaint.)
    let mut downbeats = [0_i32; LOOP_BARS as usize];
    for bar in 1..LOOP_BARS as usize {
        // Bar 0 starts on the tonic in octave 4 (index 0).
        downbeats[bar] = walk.choose_downbeat(bar, downbeats[bar - 1]);
    }

    // Phase 2: fills, bar by bar. Each fill is tethered to its own downbeat
    // and the *next* downbeat (wrapping), so no consecutive interval anywhere
    // — including across the barline — exceeds a fifth (designer feedback
    // 2026-08: gentler, more stepwise motion).
    let mut fills: Vec<Vec<(u32, i32)>> = Vec::with_capacity(LOOP_BARS as usize);
    for bar in 0..LOOP_BARS as usize {
        let next = downbeats[(bar + 1) % LOOP_BARS as usize];
        fills.push(walk.fill_bar(downbeats[bar], next));
    }

    // Assemble the 16-bar loop.
    let bar_steps = grid.bar_steps();
    let mut placed: Vec<(u32, i32)> = Vec::new();
    for loop_bar in 0..LOOP_BARS {
        let base_step = loop_bar * bar_steps;
        placed.push((base_step, downbeats[loop_bar as usize]));
        for &(offset, index) in &fills[loop_bar as usize] {
            placed.push((base_step + offset, index));
        }
    }
    // Force the final note of the loop to the tonic (rule 6).
    if let Some(last) = placed.last_mut() {
        last.1 = 0;
    }

    // Note lengths: sustain to the next onset, capped at one beat.
    let loop_steps = grid.loop_steps();
    let mut events = Vec::with_capacity(placed.len());
    for (position, &(step, index)) in placed.iter().enumerate() {
        let next_start = placed
            .get(position + 1)
            .map_or(loop_steps, |&(next, _)| next);
        let len = (next_start - step).min(grid.beat_steps).max(1);
        let on_downbeat = step.is_multiple_of(bar_steps);
        events.push(NoteEvent {
            voice: voice_name.to_owned(),
            start_step: step,
            len_steps: len,
            pitch: Some(key.scale_pitch(index)),
            vel: if on_downbeat { DOWNBEAT_VEL } else { FILL_VEL },
        });
    }
    events
}

impl Walk<'_> {
    fn in_range(&self, index: i32) -> bool {
        let midi = self.key.scale_pitch(index).midi();
        (RANGE_LOW_MIDI..=RANGE_HIGH_MIDI).contains(&midi)
    }

    /// Chord (1-based scale degree root) for a loop bar: one chord per four
    /// bars of the 16-bar loop, matching the accompaniment exactly.
    fn bar_chord(&self, bar: usize) -> u8 {
        self.progression[bar / 4]
    }

    /// Rule 3 + rule 7: a chord tone within a fifth (7 semitones) of the
    /// previous downbeat, biased downward with a floor door (or upward with
    /// only a ceiling door) in the loop's second half.
    fn choose_downbeat(&mut self, bar: usize, previous: i32) -> i32 {
        let chord = self.bar_chord(bar);
        let chord_indices = Key::triad_scale_indices(chord);
        let previous_midi = i32::from(self.key.scale_pitch(previous).midi());
        let mut candidates: Vec<i32> = Vec::new();
        for octave in -3..=3_i32 {
            for base in chord_indices {
                let index = base + octave * 7;
                if !self.in_range(index) {
                    continue;
                }
                let midi = i32::from(self.key.scale_pitch(index).midi());
                if (midi - previous_midi).abs() > 7 {
                    continue;
                }
                candidates.push(index);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        if bar >= LOOP_BARS as usize / 2 {
            if self.doors.floor {
                candidates.retain(|&index| index <= previous);
            } else if self.doors.ceiling {
                candidates.retain(|&index| index >= previous);
            }
        }
        // The final phrase downbeat cadences home: stay within a fifth of
        // the tonic so the forced tonic ending (rule 6) never leaps.
        if bar == LOOP_BARS as usize - 1 {
            let tonic_midi = i32::from(self.key.scale_pitch(0).midi());
            candidates.retain(|&index| {
                (i32::from(self.key.scale_pitch(index).midi()) - tonic_midi).abs() <= 7
            });
        }
        if candidates.is_empty() {
            // Deterministic fallback: nearest *chord tone* to the previous
            // downbeat, no RNG consumed.
            return (-21..=21)
                .map(|offset| previous + offset)
                .filter(|&index| {
                    self.in_range(index)
                        && chord_indices
                            .iter()
                            .any(|&chord| chord.rem_euclid(7) == index.rem_euclid(7))
                })
                .min_by_key(|&index| {
                    let midi = i32::from(self.key.scale_pitch(index).midi());
                    ((midi - previous_midi).abs(), index)
                })
                .unwrap_or(previous);
        }
        let pick = self.rng.below(candidates.len() as u64) as usize;
        candidates[pick]
    }

    /// Fill positions for one bar as step offsets from the bar start, in the
    /// density model of §6.5 rule 5. On 32nd grids, "16th positions" means
    /// every 2nd step; the melody never uses 32nds.
    fn fill_positions(&self) -> Vec<(u32, bool)> {
        let b = self.grid.beat_steps;
        let bar_steps = self.grid.bar_steps();
        let eighth = b / 2; // 1, 2, or 4 steps
        let sixteenth = b / 4; // 0 (b=2), 1, or 2 steps
        let mut positions = Vec::new();
        let mut step = eighth;
        while step < bar_steps {
            positions.push((step, false));
            step += eighth;
        }
        if self.coin_count >= 2 && sixteenth > 0 {
            let mut step = sixteenth;
            while step < bar_steps {
                if !step.is_multiple_of(eighth) {
                    positions.push((step, true));
                }
                step += sixteenth * 2;
            }
        }
        positions.sort_unstable();
        positions
    }

    /// Fill one bar with a constrained walk starting from the bar's downbeat,
    /// tethered within a fifth of both the bar's downbeat and the next bar's
    /// downbeat so no interval — including across the barline — leaps wide.
    fn fill_bar(&mut self, downbeat: i32, next_downbeat: i32) -> Vec<(u32, i32)> {
        let sixteenth_percent = match self.coin_count {
            0 | 1 => 0,
            2 => 15,
            _ => 25,
        };
        let mut notes = Vec::new();
        let mut current = downbeat;
        // 0 = previous move was a step (or none); ±1 = a leap in that direction.
        let mut leap_direction = 0_i32;
        for (offset, is_sixteenth) in self.fill_positions() {
            let percent = if is_sixteenth { sixteenth_percent } else { 50 };
            if !self.rng.percent(percent) {
                continue;
            }
            let (next, new_leap_direction) =
                self.walk_step(current, leap_direction, downbeat, next_downbeat);
            current = next;
            leap_direction = new_leap_direction;
            notes.push((offset, current));
        }
        notes
    }

    /// Whether MIDI distance from `index` to `anchor` is within a fifth.
    fn near(&self, index: i32, anchor: i32) -> bool {
        let a = i32::from(self.key.scale_pitch(index).midi());
        let b = i32::from(self.key.scale_pitch(anchor).midi());
        (a - b).abs() <= 7
    }

    /// Rule 4, gentled (designer feedback 2026-08): ±1/±2 scale steps
    /// (weights 6:2) or a leap of a 4th (±3 scale steps, weight 1) — wider
    /// leaps removed. A leap MUST be followed by a step in the opposite
    /// direction. Every candidate must stay within a fifth of both tether
    /// anchors so consecutive sounded intervals never exceed a fifth.
    fn walk_step(
        &mut self,
        current: i32,
        leap_direction: i32,
        anchor: i32,
        next_anchor: i32,
    ) -> (i32, i32) {
        let after_up_leap: [(i32, u32); 2] = [(-1, 6), (-2, 2)];
        let after_down_leap: [(i32, u32); 2] = [(1, 6), (2, 2)];
        let free: [(i32, u32); 6] = [
            (1, 6),
            (-1, 6),
            (2, 2),
            (-2, 2),
            (3, 1),
            (-3, 1),
        ];
        let moves: &[(i32, u32)] = match leap_direction {
            1 => &after_up_leap,
            -1 => &after_down_leap,
            _ => &free,
        };
        let legal: Vec<(i32, u32)> = moves
            .iter()
            .copied()
            .filter(|&(delta, _)| {
                let next = current + delta;
                self.in_range(next) && self.near(next, anchor) && self.near(next, next_anchor)
            })
            .collect();
        if legal.is_empty() {
            // Cornered (range edge or tether): move one scale step toward the
            // next anchor without consuming RNG; standing still is legal too.
            let towards = (next_anchor - current).signum();
            let candidate = current + towards;
            if towards != 0
                && self.in_range(candidate)
                && self.near(candidate, anchor)
                && self.near(candidate, next_anchor)
            {
                return (candidate, 0);
            }
            return (current, 0);
        }
        let delta = self.rng.pick_weighted(&legal);
        let new_leap_direction = if delta.abs() >= 3 { delta.signum() } else { 0 };
        (current + delta, new_leap_direction)
    }
}
