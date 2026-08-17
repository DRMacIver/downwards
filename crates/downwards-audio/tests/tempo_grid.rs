//! §10.1: exact unit tests of the tempo-grid derivation, plus the brute-force
//! locked-edge invariant.

use downwards_audio::{
    Difficulty, HazardTiming, derive_grid,
    tempo::{LOOP_BARS, TICKS_PER_SECOND},
};

const fn hazard(period: u32, active: u32, phase: u32) -> HazardTiming {
    HazardTiming {
        period,
        active,
        phase,
    }
}

fn lcm(a: u64, b: u64) -> u64 {
    fn gcd(mut a: u64, mut b: u64) -> u64 {
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a
    }
    a / gcd(a, b) * b
}

/// Every rising edge of every locked hazard must satisfy
/// `tick ≡ grid_offset (mod step_ticks)`, verified by brute-force walk.
fn assert_locked_edges_on_grid(hazards: &[HazardTiming], difficulty: Difficulty) {
    let choice = derive_grid(hazards, difficulty);
    let step = u64::from(choice.grid.step_ticks);
    let offset = u64::from(choice.grid.grid_offset);
    let horizon = hazards
        .iter()
        .fold(1_u64, |acc, h| lcm(acc, u64::from(h.period)))
        .min(100_000);
    for &index in &choice.locked {
        let h = hazards[index];
        let mut previous_active = h.is_active_at(0);
        // Tick 0 can itself be a rising edge (fire tick 0).
        if u64::from(h.fire_tick()) == 0 {
            assert_eq!(offset % step, 0, "edge at tick 0 must sit on the grid");
        }
        for tick in 1..horizon {
            let active = h.is_active_at(tick);
            if active && !previous_active {
                assert_eq!(
                    tick % step,
                    offset % step,
                    "locked hazard {index} rising edge at {tick} misses grid (step {step}, offset {offset})"
                );
            }
            previous_active = active;
        }
    }
}

#[test]
fn single_hazard_locks_with_its_period_grid() {
    let hazards = [hazard(96, 20, 45)];
    let choice = derive_grid(&hazards, Difficulty::Medium);
    assert_eq!(choice.locked, vec![0]);
    assert!(96_u32.is_multiple_of(choice.grid.step_ticks));
    assert_eq!(
        choice.grid.grid_offset,
        hazards[0].fire_tick() % choice.grid.step_ticks
    );
    assert_locked_edges_on_grid(&hazards, Difficulty::Medium);
}

#[test]
fn equal_periods_with_phase_offsets_pick_the_medium_grid() {
    // keep-meteor-run flavour: 96/{45,13,77} → fire ticks {51,83,19}, G = 32.
    let hazards = [hazard(96, 20, 45), hazard(96, 20, 13), hazard(96, 20, 77)];
    let choice = derive_grid(&hazards, Difficulty::Medium);
    // All of (4,8), (8,4), (16,2) give bpm 112.5; the normative tiebreak
    // (larger u, then larger b) selects the coarsest step.
    assert_eq!(
        (choice.grid.step_ticks, choice.grid.beat_steps),
        (16, 2),
        "closest bpm 112.5 tie resolves to the coarsest grid"
    );
    assert!((choice.grid.bpm() - 112.5).abs() < 1e-9);
    assert_eq!(choice.grid.grid_offset, 51 % 16);
    assert_eq!(choice.grid.grid_offset, 3);
    assert_eq!(choice.locked, vec![0, 1, 2]);
    assert_locked_edges_on_grid(&hazards, Difficulty::Medium);
}

#[test]
fn antiphase_airlock_example_picks_100_bpm_with_larger_step() {
    // antiphase-airlock-a: 180/{120,30} → f = {60,150}, G = 90; easy target
    // 96 → bpm 100 candidates (9,4) and (18,2); tiebreak larger u → (18,2).
    let hazards = [hazard(180, 30, 120), hazard(180, 30, 30)];
    let choice = derive_grid(&hazards, Difficulty::Easy);
    assert_eq!((choice.grid.step_ticks, choice.grid.beat_steps), (18, 2));
    assert!((choice.grid.bpm() - 100.0).abs() < 1e-9);
    assert_eq!(choice.grid.grid_offset, 6);
    assert_eq!(choice.locked, vec![0, 1]);
    assert_locked_edges_on_grid(&hazards, Difficulty::Easy);
}

#[test]
fn multi_period_commensurate_hazards_lock_together() {
    // Periods {100, 140}: gcd 20 with equal fire residues → all locked.
    let hazards = [hazard(100, 30, 0), hazard(140, 30, 0)];
    let choice = derive_grid(&hazards, Difficulty::Easy);
    assert_eq!(choice.locked, vec![0, 1]);
    assert!(20_u32.is_multiple_of(choice.grid.step_ticks));
    assert_locked_edges_on_grid(&hazards, Difficulty::Easy);
}

#[test]
fn phase_driven_gcd_reduction_forces_a_32nd_grid() {
    // Eight hazards, period 120, phases with pairwise differences of gcd 4:
    // G = 4, so the only candidate is (4, 8) — step is a 32nd.
    let phases = [0, 40, 80, 72, 48, 24, 84, 4];
    let hazards: Vec<HazardTiming> =
        phases.iter().map(|&phase| hazard(120, 12, phase)).collect();
    let choice = derive_grid(&hazards, Difficulty::Medium);
    assert_eq!((choice.grid.step_ticks, choice.grid.beat_steps), (4, 8));
    assert!((choice.grid.bpm() - 112.5).abs() < 1e-9);
    assert_eq!(choice.locked.len(), hazards.len());
    assert_locked_edges_on_grid(&hazards, Difficulty::Medium);
}

#[test]
fn coprime_periods_partition_and_lock_the_larger_subset() {
    // two-clock-fork-c flavour: periods {70, 157} are coprime → partition.
    // Subset {70} admits candidates; subset {157} (prime, out of range) does
    // not, so the 70-hazard locks and the 157-hazard is unlocked.
    let hazards = [hazard(70, 20, 0), hazard(157, 20, 0)];
    let choice = derive_grid(&hazards, Difficulty::Hard);
    assert_eq!(choice.locked, vec![0]);
    assert!(70_u32.is_multiple_of(choice.grid.step_ticks));
    assert!((choice.grid.bpm() - 3600.0 / 28.0).abs() < 1e-9);
    assert_locked_edges_on_grid(&hazards, Difficulty::Hard);
}

#[test]
fn all_prime_periods_fall_back_to_difficulty_defaults() {
    let hazards = [hazard(157, 20, 0), hazard(149, 20, 0)];
    let choice = derive_grid(&hazards, Difficulty::Hard);
    assert!(choice.locked.is_empty());
    assert_eq!((choice.grid.step_ticks, choice.grid.beat_steps), (7, 4));
    assert_eq!(choice.grid.grid_offset, 0);
}

#[test]
fn hazard_free_rooms_use_difficulty_defaults() {
    for (difficulty, expected_step, expected_beat, expected_bpm) in [
        (Difficulty::Easy, 10, 4, 90.0),
        (Difficulty::Medium, 8, 4, 112.5),
        (Difficulty::Hard, 7, 4, 3600.0 / 28.0),
    ] {
        let choice = derive_grid(&[], difficulty);
        assert_eq!(
            (choice.grid.step_ticks, choice.grid.beat_steps),
            (expected_step, expected_beat)
        );
        assert!((choice.grid.bpm() - expected_bpm).abs() < 1e-9);
        assert_eq!(choice.grid.grid_offset, 0);
        assert_eq!(choice.grid.loop_bars, LOOP_BARS);
        assert!(choice.locked.is_empty());
    }
    assert_eq!(TICKS_PER_SECOND, 60);
}

#[test]
fn grid_offset_is_always_below_the_step() {
    for phase in 0..96 {
        let hazards = [hazard(96, 10, phase)];
        let choice = derive_grid(&hazards, Difficulty::Easy);
        assert!(choice.grid.grid_offset < choice.grid.step_ticks);
        assert_locked_edges_on_grid(&hazards, Difficulty::Easy);
    }
}
