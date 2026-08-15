# Hard no-Dash human-calibration challenge (2026-08-15)

## Why this fixture exists

Human playtesting falsified the corpus metrics' strongest difficulty prediction:
`STILL FLAME HALL` was reported as the most demanding room in the 68-room
playtest export, but was in fact essentially trivial for the player.

The mismatch has two independent causes:

- the room has no static or timed hazards and uses broad one-way shelves roughly
  40–110 pixels wide, so its nominally technical route does not impose precision;
- the retained solver witness thrashes. It takes 384 ticks, presses jump 25
  times, holds jump for 179 ticks, and makes 40 horizontal reversals. The
  declared route structure has only nine jump transfers and four reversals.

The fallback beam search scores target distance and elapsed ticks, not jump
presses, reversals, or semantic input changes. Replay normalization trims only
the post-target suffix. The finite direct-controller vocabulary missed the
simple human route, leaving the noisy canonical witness as the only candidate
for route fusion. The shaky-hand diagnostic then perturbed that open-loop trace:
almost all failures were timeouts, with no deaths. Those results measure brittle
AI navigation, not dangerous geometry or human execution difficulty.

Consequently, controller transition counts, reversals, and blind-continuation
failure rates must not be treated as human difficulty estimates unless a
substantially simpler successful controller has first been ruled out. Failure
to find such a controller is still bounded search evidence, never proof.

## The calibration challenge

`downwards-content::hard_no_dash_room` is a fixed, hand-authored challenge kept
outside every generated corpus, catalogue, and selection policy. Launch it with:

```sh
cargo run -- --challenge
```

The loadout is locked to WallJump-only: ordinary run/jump and wall jump are
available, while Dash is disabled in the authoritative `Simulation`. The level
contains:

- a sealed, solid-backed shaft whose inward faces are spikes except for four
  alternating 20-pixel wall-contact pads;
- roughly eight pixels of admissible vertical player position at each contact
  pad, widened in practice by the engine's buffering and wall-slide behavior;
- a top aperture and safe transfer lip;
- a ceiling spike bank where releasing Jump early produces the required low
  jump, while holding Jump produces a higher jump; and
- three small one-way islands over a spike floor leading to the finish.

The geometry is intended to require readable precision rather than deriving a
hardness label from the actions chosen by a search algorithm.

Press `V` to inspect the stored tractability witness. It is not presented as an
easy, optimal, or human-like solution. It uses the same held-Jump boolean as
human play at exact 60 Hz ticks; the AI has no secret low-jump input.

The current live keyboard adapter is human jump-input policy v4. A physical
release within 100 ms is classified as one low-jump gesture, regardless of
render-frame timing; continuing to hold selects the higher part of the ordinary
variable-height curve. Buffered low gestures remain a single intent through
acceptance. Wall jumps use a separate responsive contract: a press is delivered
immediately, recent wall contact has a short grace window, and an accepted wall
jump receives a minimum useful upward launch plus a brief outward commitment.
These are input affordances rather than level-design units. Historical stored
witnesses retain their original simulation policy; new human-playability claims
must explicitly use the live policy.

## Exact evidence and limits

The stored witness is an authoritative 182-tick replay with:

- 10 jump presses, each corresponding to an accepted jump;
- 6 wall jumps and 4 non-wall jumps;
- zero Dash inputs or Dash events;
- zero deaths, resets, or restarts; and
- exact completion at the sole `finish` exit.

A deterministic greedy deletion/neutralization pass reaches a fixed point at
the same 182 ticks, 10 presses, and 6 wall jumps. This guards against the most
obvious replay padding; it is not a global minimum-controller proof.

The complete built-in WallJump-only direct-controller portfolio evaluated 304
probes and found no positive. The Baseline portfolio evaluated 302 probes and
also found no positive. The ordinary Baseline solver exhausted its frontier
after 11,751 expanded nodes, 396,200 simulated ticks, and depth 569 without a
positive. These are **no-known-bypass** observations under exact finite policies,
not physical-unreachability proofs.

The replay proves tractability. The lethal geometry explains why specific
precision moves are intended. Neither proves that a human will find the room
late-game-hard, readable, or fun. Human playtesting is the deciding calibration
signal, and the fixture should be revised if that feedback contradicts its
intent.

## Human results: high-end and tutorial anchors

The first human playtest judged the hard fixture “quite good” and, if anything,
slightly too hard for the player's current skill. It is therefore retained
unchanged as the current high-end calibration anchor.

A second fixture was designed to target roughly half that execution burden:

```sh
cargo run -- --challenge medium
```

Compared with the hard fixture, that room has:

- 3 accepted wall jumps instead of 6;
- 4 total accepted jump presses instead of 10;
- witnessed wall-contact bands 4–6 tiles tall instead of 2;
- witnessed recovery surfaces 3–4 tiles wide instead of bottlenecks as narrow
  as 1 tile;
- one intermediate landing instead of two tiny islands; and
- no ceiling-spike check that requires releasing Jump early.

Its fixed-point simplified replay reaches the exit in 136 ticks without Dash,
death, or reset. The complete WallJump-only direct-controller portfolio found
no positive. Baseline search was bounded-inconclusive and its complete finite
portfolio found no positive.

The next human playtest rejected the intended ratio: the room was **far more
than 50% easier** than the high-end fixture and felt suitable as a good
tutorial. This is now its accepted calibration role. The canonical command is:

```sh
cargo run -- --challenge tutorial
```

`--challenge medium` remains a compatibility alias, but the client no longer
labels the room mid-high. The failed interpolation is itself useful evidence.
Halving the accepted move count while simultaneously increasing contact
windows from 2 tiles to 4–6, widening landings, and removing the release-early
ceiling check compounded nonlinearly. Geometric burden coordinates can explain
the edit, but they cannot predict a human difficulty percentage.

The current calibrated anchors are therefore a good tutorial and a slightly
too-hard high-end room. There is no calibrated midpoint yet. A future attempt
should vary one main burden at a time—such as retaining the hard room's contact
precision while shortening its chain—instead of relaxing count, timing,
landing, and jump-height constraints together.

## Follow-up metric direction

Before resuming difficulty-guided corpus generation:

1. add a fewest-decision search ordered by ability use, reversals, jump presses
   and semantic transitions, then duration;
2. greedily simplify every retained successful replay before extracting
   controller-demand features;
3. explicitly search walk, low-jump-count, reactive, and low-reversal bypass
   portfolios;
4. use route-specific lethal clearance and landing constraints as execution
   evidence; and
5. keep blind open-loop perturbation diagnostics separate from human difficulty.
