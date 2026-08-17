# Dungeon portfolio

A growing portfolio (target: ~100) of assessed dungeon layouts, with
the shipped dungeon-v2 at the low end of the final difficulty scale.
Dungeons live here as `NNN-<slug>.txt` in the layout format that
`tools/dungeon_layout_metrics.py` reads (room / edge / gate / pickup /
spawn / goal / exit directives, grid-consistent embedding).
`index.json` records, per dungeon: file, name, seed + generator
params, difficulty score + breakdown, room list, and date. Entry 000
is dungeon-v2 itself, referenced in place at
`docs/design/dungeon-v2-layout.txt` (never copied here).

## Toolchain

All commands run from the repo root with python3.

### 1. Assess difficulty — `tools/dungeon_difficulty.py`

    python3 tools/dungeon_difficulty.py <layout.txt> [--json]

Computes the easiest mandatory route (spawn -> glove -> boots ->
coins-to-gate -> crown -> climb out through the exit room's ceiling),
respecting gates and loadout-dependent crossability, from the
per-pair solve ticks in `docs/design/rooms-v2-passability.json`. It
prints a scalar score plus a breakdown (total/peak/mean crossing
ticks, ascent crossings, coin-collection ticks, shaky-hand penalty,
per-leg detail). `--json` emits the full breakdown as JSON.

The scale is open-ended. Baseline calibration at the time of writing:
dungeon-v2 scores ~22 from solve ticks alone and **27.16** with the
shaky-hand data its rooms currently carry. IMPORTANT: shaky data only
exists for rooms that have been audited with `--shaky`; scores are
strictly comparable only at equal coverage. Before recalibrating
bands, regenerate the table over the whole vocabulary:

    python3 tools/room_passability.py --shaky

### 2. Generate a candidate — `tools/dungeon_generator.py`

    python3 tools/dungeon_generator.py --seed N \
        [--size small|medium|large] [--band low|mid|high] \
        [--acts 3] [--out FILE]

Deterministic: same seed + params -> byte-identical layout. Pipeline:
footprint (roof row + block + partial bottom row) -> annealed skeleton
(connectivity, diameter, cycle rank, door-signature supply) -> exact
door-signature room fill (no same-class adjacency; crown pinned to
keep-crown-sanctum at the bottom-east dead end; keep-astral-seal
pinned in front of it carrying the single coin gate; exit room on the
top row with a free ceiling door and the spawn directly beneath it) ->
absorbing-trap pre-check at every fixed loadout -> pickup placement
(glove reachable bare, boots in territory the wall ability opens,
approach cost at the band's quantile and never below 150 ticks).
Rooms owned by concurrent agents (`EXCLUDED` in the script) are never
selected. Exit code 2 means the seed exhausted its retries — skip to
the next seed; that is expected rejection-sampling behaviour.

### 3. Accept or reject — `tools/dungeon_check.py`

    python3 tools/dungeon_check.py <layout.txt> [--band low|mid|high] [--json]

Exit 0 = accept. Runs the metrics tool (verdict of record: embedding,
exit-on-top, geometric gating, no absorbing traps), scores the layout
and requires the score inside the band, and adds structural sanity:
no same-grid or same-class adjacency, pickups a peak-so-far challenge
(approach leg >= 100 ticks and containing a crossing >= 60% of the
route peak so far), and act coherence (goal unreachable bare,
reachable with both, each ability strictly expands the map, at least
one backtrack unlock). Band defaults to the generator's header
comment, else `low`.

Score bands (recalibrate after full-coverage shaky regeneration):

    low  [12, 20)    mid  [20, 28)    high [28, 120)

## Adding a dungeon

1. Generate (or hand-edit) a candidate; iterate seeds until
   `dungeon_check.py` accepts it in the intended band.
2. Name it `NNN-<slug>.txt` (next free NNN) in this directory.
3. Append its entry to `index.json` (score + breakdown from
   `dungeon_difficulty.py --json`, seed/params from the header).
4. The session owner commits.

## Current acceptance data (bootstrap, 2026-08-17)

Over seeds 0-19 (low->medium, mid->medium/large, high->large):
18/20 accepted (90%). Scores: low 12.3-17.7, mid 20.8-24.6,
high 28.9-35.4. Both rejections were seeds that exhausted the
generator's 24 anneal/fill retries, almost entirely on the
absorbing-trap pre-check (~85% of trap-free fills is the dominant
loss; pickup placement occasionally fails on the survivor). Top
open critiques for the next loop iteration:

* absorbing-trap rate: the fill step should bias room choice against
  trap-prone one-way rooms instead of rejecting after the fact
* shaky coverage asymmetry (see above) inflates v2 relative to
  generated layouts until the table is regenerated in full
* high band depends on the large footprint plus a 0.95 coin fraction;
  deeper footprints (H=5) currently fill for only ~1/3 of seeds
* small footprints score below the low band floor (~9-11); either
  raise their coin fraction or drop the size
