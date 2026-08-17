# Research Log

Experiments, metrics, and findings on level generation and difficulty.
Primary experiment writeups live in `docs/research/` and dated reports in
`docs/validation/`; this log is the narrative index of what was learned.

## AI-testing bias: the solver thrashes

- The designer's observation — "the game-playing AI tends to jump around a
  lot... There are a lot of places where it could just walk and instead it
  hops around like a mad thing" — was confirmed as a measurement defect: the
  solver scores only target distance and elapsed ticks, never penalizing
  jumps, reversals, or drops; normalization only trims trailing frames.
- Case studies: Still Flame Hall's stored witness used 25 jump presses and 40
  reversals where the structure needs 9 transfers and 4 reversals (the room
  is trivial). Gale Chasm's 43-span, 21-jump route reflected confusing
  onboarding, not difficulty — replaced with a readable 9-span route. Vacuum
  Gallery's poor 29-span witness traced to the authoring controller, not
  geometry.
- Generalization: inspect event traces, not aggregate scores. Corollary:
  readable, deliberately paced routes can be *more* robust than minimal ones —
  Astral Seal's settle-on-each-wall route improved its worst family from 3/64
  to 23/64, and the shortest solver result was deliberately not shipped.
- "Don't treat simple scores (77 ticks, 5 reversals) as definitive difficulty
  claims... Route metrics are symptoms to audit, not proof."

## Shaky-hand robustness testing

- Deterministic perturbation evaluator: seeded timing jitter, delayed/early
  presses, dropped inputs, in four families (BoundaryTiming,
  CorrelatedTiming, HoldRelease, DropRepeatFrame), reporting success vs noise
  strength. Known limitation: recovery is blind replay continuation — no
  replanning.
- Deliberately deferred while levels were too easy (2026-08-14), then adopted
  as a hard gate (2026-08-15): worst family ≥48/64 (75%) on mandatory routes.
- Rooms passing *exactly* at 48/64 (e.g. Pressure Fork) are fragile to any
  physics/tuning change; recommendation: 56/64+ for core content, 48/64 for
  peripheral rooms.
- **Deaths and robustness are separate signals.** Hazards must produce real
  deaths (proof of threat): ~10–20% death rate reads "moderate", 50% reads
  trial-and-error. A room can be FRAGILE with deaths, or dull (0% deaths)
  while passing robustness — both rejected, for different reasons. Timeouts
  (desync) versus deaths distinguish fiddly execution from genuine hazard
  threat; dash-chain desync FRAGILE flags on dash-heavy rooms are expected
  in-family, with deaths > 0 the real criterion.
- CorrelatedTiming FRAGILE flags are often marginal and reversible by 1–2
  tile geometry adjustments — check the actual worst-family percentage before
  rejecting.
- Trial counts: 16 for pilots, 64 for final validation, 128+ for deep
  follow-up.

## Geometry–difficulty relationships (measured)

- Widening a drop slot from 4 to 6 tiles and shifting flanking spikes
  eliminated ~40px BoundaryTiming failure windows while keeping readability.
- Spike-wall chutes need ≥2 tiles of interior width to be climbable.
- Alternating safe-face spike walls ("read the wall") beat symmetric or
  fixed-safe-side layouts on both teachability and robustness.
- Crawl-height geometry caps robustness: one tile of crawl ceiling capped a
  room at 46/64; a 4–5 tile crawl dropped it to 24–37/64.
- Dual spike-gap layouts underperform single gaps (~15 two-gap layouts swept;
  all topped out at 40–46/64 vs 48/64 for a single 2-tile gap).
- **Takeoff geometry drives timing sensitivity more than landing geometry**:
  extending landings changed nothing; adding flat run-up before the jump-off
  was the effective fix.
- DropRepeatFrame noise resists platform-softening fixes that cure the other
  families.
- Row clearance can hide "invisible dash-gates" (10px clearance vs 12px
  player height is passable only while dashing) — always test pre-ability
  loadouts.
- Solid strata create commitment puzzles; one-way platforms shift the puzzle
  to momentum management. Wall-jump chutes between opposing columns (2–3
  tiles) are a reusable, tunable optional-reward mechanic.
- **Jump count is nearly irrelevant to difficulty**; per-transfer control
  demand and reversal cadence dominate (Two-Tile Turn is hard with two jumps
  because its reversals are 9 ticks apart). Passive/recovery geometry makes
  long rooms trivial. Visual threat and execution difficulty are independent
  axes (Three Pins; Broken Causeway's narrow supports are easy given the
  5-tick coyote window).
- Completion ticks, transition counts, and hazard-clearance counts all
  systematically over-rank easy rooms.

## Input precision and accessibility

- **One-tick input gates are inaccessible to humans regardless of intent.**
  cal-05 needed a 1–3 tick (17–50ms) release window; cal-11 2–5 ticks; Low
  Clearance's single-frame jump-cut window was diagnosed as a *content
  defect* and fixed by moving the ceiling hazard to widen the window to
  ~100ms.
- The variable-height jump (7px tap vs 30px hold) existed in code but was
  undocumented until the designer asked "What's the intended mechanism for a
  short jump?"
- A buffered-jump bug produced fixed ~17px hops from early-buffered taps —
  an input bug masquerading as difficulty.
- Stored witnesses can contain pathfinding artifacts (Open Shaft's spurious
  4px y-reversal); replace artifact witnesses before treating a room as
  validated hard content.
- Showing the AI witness before a human's first attempt anchors them on
  brittle shortcuts — hide it until after the first attempt.
- Landing-geometry precision metrics are useful evidence but do not stand in
  for human difficulty; shaky-hand curves remain the reproduction test.

## Comparative jump-mechanics research

Commissioned by the designer. Compared Celeste, Hollow Knight, Super Meat
Boy, Dustforce, N++, MoneySeize. Celeste: responsive baseline with selective
momentum (0.16s wall-direction retention, 0.2s variable jump height) and a
forgiveness philosophy — check for walls beyond contact, allow wall jumps
from a small distance, widen windows. Hollow Knight: direct authority,
minimal momentum. Recommendation adopted: treat buffering, coyote time,
variable height, and acceleration as independently tunable parameters.
Calibration data point: the "50% easier" variant of the hard probe overshot
well past 50% easier — difficulty-adjustment intuition errs easy.

## The 101-room dungeon audit

- 27/101 rooms were filler (0–2 inputs, zero deaths, unused geometry),
  concentrated in the opening ten rooms, dead-template junctions, and
  door-mouth coin caches. Quality tiers: Q1×9, Q2×37, Q3×36, Q4×19, Q5×0.
- Nine archetypes with clear over/under-representation; four clusters
  (spiked wall-jump chimneys 18, flat spike-floor corridors 19, dead-end coin
  vaults 18, flat drop-slot junctions 10) drove the "samey" complaint.
- Under-explored dimensions: timed/dynamic hazards in 1/101 rooms; no moving
  platforms, crumble, wind, or enemies; ~0 genuine multi-route rooms despite
  21 topological junctions; descent-as-challenge in ~3 rooms; ~75 rooms with
  dead space.
- Keep-list of signature rooms (the aspirational model): astral-seal,
  aurora-spire, boots-vault, crown-sanctum, meteor-run, observatory,
  shard-vault, skybridge, wall-gate, void-pass, zenith-shaft (plus strong
  seconds). Climax rooms 87–101 validated strong (30–60% death rates under
  perturbation).
- Difficulty curve: flat-zero through room 10, spike at ~18, sawtooth middle,
  strong sustained climax from ~87. Some mid-game rooms are timing-sensitive
  but consequence-free — retry friction, not challenge.
- Coin economy: top coins already strawberry-grade; with gate slack the
  hardest coins are genuinely optional. Note: cache-descent coins' real cost
  (the climb out) is unmeasured by collection-only route studies.
- Reachability tracing found the first ability (Glove) gated behind a
  HARD-tier room — exactly the spike the tier system was meant to prevent.
  Door-level graph BFS over `rooms-v2-passability.json` revealed the
  {kv, tc, s5, o4} absorbing trap; a reported keep-observatory softlock was a
  search-budget timeout, not a gate (documented in
  `docs/design/dungeon-v2-rationale.md`).
- Braided Crossing demonstrated **profile gates**: 10px spike-ceiling
  clearance fits only the 8px dashing profile — an unambiguous geometric gate
  without large gaps.
- Audit method: three critique lenses (variety/identity, soft-locks/retreat,
  difficulty arc), each producing tool-evidenced findings.

## Timed-hazard experiments

Timed hazard curtains were supported but used in only one room (Meteor Run).
Prototyped new families: Tide Shaft and Metronome Gallery (phase-marching
curtains), Shutter Chute (three full-width bands at 33% duty, cascading
phases), Antiphase Airlock (four partition gates forming a travelling wave).
Timing-based play is a mechanically distinct late-game axis the dungeon
barely exercises. Constraint discovered: long hazard periods blow the solver
budget (period 90 inconclusive, period 40 fine) — hazard period is a
provability constraint, not just feel.

## Solver capabilities and limits

- Beam search over macro-actions (not frame-perfect): PRECISION_TICKS=4 at
  60Hz, beam width 96, 2px position quantum, 0.5px/tick velocity quantum,
  ~600-tick path horizon, ~14,550-node budget. The budget is a hard ceiling
  independent of geometry validity; *any* hazard anywhere in a room shrinks
  effective reach (a clean 243-tick route went inconclusive after adding one
  off-route hazard).
- 2-tile shafts aren't recognized as wall-jumpable; 3 tiles with a one-way
  rung at the mouth works. Solid nubs at 20px rises fail "none"-loadout
  validation where one-way shelves at 30px pass — platform type interacts
  with solver reachability non-obviously.
- Solver route choice can diverge from intent acceptably (Glass Threshold's
  fast risky coin line doubles as a speedrun incentive). Mild nondeterminism
  on threshold-boundary pairs was observed and flagged for confirmation at
  integration.
- Finite-vocabulary direct audits can miss solutions the full solver finds
  (Wall0's Dash-only bypass) — gate logic must trust deep-solver positives.
- BufferedWallClimb probe (buffer jumps during descent, steer into walls)
  raised wall-room traversal success dramatically (e.g. Gentle 25/40→40/40).
- Generator v4 baseline: 4,000/4,000 scenarios certified across four
  loadouts, identical on repeat.

## Generator diversity crisis and remediation

- v5 templates: 251 distinct T1 layouts / 24 T4 layouts per 1,000 seeds;
  Hamming analysis showed every distinct shape had a neighbour at distance 1
  — near-duplication explained the "samey" feel. Root cause: two mostly-fixed
  templates per tier with one-tile nudges.
- Remediation: compositional generators (cyclic-graph, reachability-growth,
  rhythm-weave; later graph-first Compositional Route Cut — coordinate-free
  topology signatures, exact one-to-one embedding, one room or a typed
  failure, no retries; partition v3 passed 566/576 broad keys without
  geometry repair).
- v6 diversity audit added tile-field Hamming, nearest-neighbour distances,
  and route-signature counts to catch nudge-passing; results: 10th-percentile
  random-pair difference 85–89 tiles (median 117–121). Regression tests pin
  1,000/1,000 unique visuals/collision fields/route signatures; target
  floors: 900 unique visuals / 850 tile-fields / 750 topologies per 1,000
  seeds, with v5 deliberately left failing the regression as a marker.
- Both cyclic_graph and rhythm_weave were extended to 2–4 genuine boundary
  doors with carved openings and reversible links, enabling door-to-door
  certification and tiling.

## Difficulty-scoring defects and corrective gates

- Length/repetition masqueraded as skill: all 12 sampled "Technical" routes
  were reversal-free horizontal crossings; mean perturbation robustness 98% —
  the selector was optimizing *safety* inside a nominally hard band.
- Corrective gates adopted: controller-simplicity requirements per tier; a
  route-difficulty gate requiring 2+ concrete demands (proven ability use,
  reversal, 60px+ vertical, ≤15% margin, ≤20px landing, ≤4px clearance, timed
  decision, ±3–4 tick window); explicit ability gates; ≥25% robustness floor;
  challenge-ranked selection; endurance separated from execution.
- **Room difficulty is not a scalar**: the same room ranges Gentle→Technical
  by entrance; certify specific entrance/target routes.
- Coin reachability was originally never verified (exit-only certification) —
  caught by the designer in play; every pickup became an independent solver
  target with its own witness, from every entrance. Related: a T1 coin route
  required two 30px rises against a 30.94px max jump — sub-pixel margins on a
  beginner collectible; optional-route margins loosened to 10–20px.

## Evidence-pipeline and performance findings

- Cache serialization drift (histogram key representation, float text vs
  IEEE-754 bits) had silently corrupted cached difficulty data; canonical
  representations fixed it. Shard verification reduced from 29 to 6 passes
  without weakening fail-closed guarantees.
- Profiling (designer-prompted) found: ≥513 simulations per route/loadout
  cell in default shaky analysis; ~8 replays per fusion candidate before
  dedup; the same room resolved ~7 times per cache row; 14 minutes of startup
  verification before the first row. A QD-selector fix (triangular pairwise
  distance cache replacing a cubic rescan) cut 703-room evaluation from
  57.9M to 246.7K distance calculations. First pass net: 5.5% speedup with
  byte-identical output; final 596-room corpus build ~17 minutes (acceptable
  for finals, too slow for design iteration).
- Operational hazards that invalidated runs (details in
  [engineering-notes.md](engineering-notes.md)): the `retune_demo_dungeon --`
  bug, shared-worktree file reverts, background grid regeneration, and the
  stale-coin-position measurement gap (route studies measure the *compiled*
  coin position, so unapplied coin moves are silently unmeasured).
