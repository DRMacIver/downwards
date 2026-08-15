# `downwards-ai`

Headless deterministic solving, replay verification, and evaluation metrics
for Downwards rooms.

## Shaky-hand studies

`record_shaky_hand_study` creates a replay-bound, versioned set of seeded
perturbation schedules. `evaluate_recorded_shaky_hand_study` executes those
schedules one semantic action per authoritative `Simulation::step` tick.
`evaluate_shaky_hand` is the convenience form that does both.

Studies require a canonical successful `TargetSolution` for one exact exit,
door, or pickup. The zero-noise curve verifies the complete replay and must
reach the declared target on its final frame. Non-zero curves are reported
separately for:

- individual input-boundary shifts of 1, 2, and 4 ticks;
- correlated early/late shifts across consecutive boundaries at the same
  strengths;
- one-frame semantic hold/release errors;
- one dropped or repeated semantic input frame.

Each curve retains trials, successes, deaths, wrong exits, other doors,
timeouts, first-divergence/failure diagnostics, and exact same-tick state
convergence where it is observed. Replanning from a perturbed state is
explicitly unsupported: the first policy slice only measures blind
continuation of the exact recorded controller. A failed trial describes that
schedule and never proves the room or target unreachable.

The root seed, per-trial derived seed, policy version, config version/config
digest, replay fingerprint, and concrete edits are all retained so a study
can be persisted and reproduced exactly.
