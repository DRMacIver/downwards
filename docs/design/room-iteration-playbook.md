# Room iteration playbook

How to redesign a dungeon room and validate it with the automated analysis
tools. Written for both humans and design agents.

## The grid

Each room is `crates/downwards-gen/rooms/<slug>.txt`: 18 lines of 32
characters, one per 10px tile.

- `#` solid, `.` empty, `-` one-way platform (passable rising, solid landing;
  press down to drop through)
- `^ v < >` spikes pointing up/down/left/right. Lethal on contact from every
  face **except the back** (the face opposite the point), which behaves as
  solid. Exception: the left/right flanks of `^`/`v` block movement like
  obstacles (not lethal) but give **no** wall surface — you cannot wall-jump
  or dash-carry off them.
- Door mouths are the gaps in the boundary rows/columns (left/right walls rows
  13–16, ceiling/floor cols 14–17). **Never move or reseal a door mouth.**

## Physics facts that matter for layout

- Player is 8×10px wide × 12px tall (8px tall while dashing horizontally, so a
  1-tile gap under a wall is a dash-only crawl; a 2-tile gap is walkable).
- Full jump rises ~40px (4 tiles) at the limit. Use **2-tile (20px) rises**
  for comfortable platforming; 3 tiles is demanding; 4 tiles reads as unfair.
- Wall jumps need alternating opposing walls; a single wall cannot be climbed.
- One-way platforms need **2 empty tiles directly above** every `-` tile
  (standing headroom); a test enforces this. Stagger ladder rungs into
  disjoint column bands when they are 2 rows apart.
- Dash travels ~40px; up-dash chains with a jump for ~80px of height.
- Dying resets to the room's entry door with abilities/coins kept, so
  "entry → door solvable" is the unit of traversability.

## Design principles (from the 2026-08-16 design review)

- Every room needs a job: teach, test, reward, decide, or breathe. If the
  mandatory route is walk-plus-one-jump, the room needs a reason to exist.
- Put the coin at the far end of the room's built geometry so the structure is
  the test; never at the door mouth.
- Precision should have stakes: a timing-sensitive climb with no hazard is
  retry-friction, not challenge. Conversely, hazards should punish overshoot
  and commitment, not undershoot (falling short should cost a retry, not a
  death, wherever possible).
- Give routes convergence points (wall-abutting ledges, floors to stand on)
  between precision segments; blind-jitter robustness comes from geometry the
  player can resynchronise against.
- Retreat must always be possible: every entry door must reach every other
  door — and itself — with the loadout a player could hold there.

## Tools

Build once (`cargo build --release -p downwards-content --examples`), then run
the prebuilt binaries. Always set `DOWNWARDS_ROOM_GRID_DIR` so edited grids
are read from disk without recompiling:

```sh
export DOWNWARDS_ROOM_GRID_DIR=crates/downwards-gen/rooms

# Solve one (entry -> exit) pair. entry is a door name or `spawn`; loadout is
# <wall><dash> as two 0/1 digits. Prints `solved in N ticks` or `inconclusive`.
./target/release/examples/audit_dungeon_traversal --pair <slug> west east 10

# Full route + robustness study for the room's analysis target (usually its
# coin). Prints candidate observations (ticks, spans, jumps, wall jumps,
# dashes, reversals) and strength-one shaky curves `family successes trials
# deaths`. Worst family ≥ 48/64 (75%) is the robustness bar; deaths > 0 means
# hazards genuinely threaten the route.
./target/release/examples/retune_demo_dungeon -- --route demo-dungeon.<slug>
```

Interpretation:

- `--pair ... inconclusive` = the bounded solver found no route. Treat as
  failure for mandatory pairs; it is not a proof of impossibility, but design
  so the solver succeeds.
- Validate **every ordered door pair including door→same-door** (the sealed
  gate bounce) at the weakest loadout a player can hold in the room, and
  re-check that pairs that previously solved at weaker loadouts still do.
- `--route` targets the room's registered coin/target; if you intend to move
  the coin, the shaky study still measures the old position — note that and
  rely on `--pair` plus judgement for the new placement.

Do **not** run the artifact-writing tools (`retune_demo_dungeon` without
`--route`, `audit_dungeon_traversal` without `--pair`, `export_room_grids`)
during parallel iteration; integration reruns them once at the end. Coin
positions live in `demo_dungeon.rs` (`room_coin_specs`, pixel rects) and are
applied at integration time — propose moves, don't edit Rust.
