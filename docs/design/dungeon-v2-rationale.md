# Dungeon v2 rationale (rev 3): the descent

## Geometry: the dungeon is a descent, and the map is exact

The game is called *downwards*, and the layout now says so. The whole
dungeon embeds on a 2D grid with no displacement: every edge joins two
rooms in adjacent cells via matching door directions, no two rooms share a
cell, and the in-game map can draw the dungeon exactly as it plays -
connection lines never cross and never lie. The footprint is a compact
6-wide, 6-deep block (30 rooms, 37 edges, cycle rank 8, all-unlocked
diameter exactly 8).

The spawn (`s1`) sits on the top row of rooms, directly under the
greed-loop roof-cap; the crown is the east end of the bottom row, behind
the astral seal. The mainline trends downward the whole run: roof, spawn
column, upper halls, the sandglass trapdoors, the deep, the seal. Upward
travel is what retreat and ability-powered backtracking look like - tide
shafts and strata sorts climb bare, shutterless chimneys climb only with
wall-kicks - so gravity is the geography and the abilities are the tools
that fight it.

## No ability door locks: geometry gates everything

Traversal abilities never lock doors. The five old `gate ... ability`
lines are gone; ability progression is enforced by rooms whose door-pair
crossings genuinely require the ability, each verified by the room auditor
(`audit_room_grid`, all door pairs x loadouts, including retreats):

- **Wall act (north-west quarter).** `chimney-lock-a` is the gate: its
  west door sits behind a full-height partition flanked by sheer wall-kick
  bays (crossable at wall/both only, in both directions - dash gets no
  height there). The boots vault (`bv`) hangs at the roof's west corner
  behind `keep-west-postern` (the same bay mechanism) and above a
  `one-way-loop-c` under-stair whose climbs need wall. At bare or
  dash-only loadouts the quarter, and the boots, are sealed.
- **Dash act (the deep).** `sandglass-drop-a/b` are trapdoor rooms: their
  floor doors sit behind a broad timed sand-curtain that only a dash
  crosses (both ways), and the sealed bulb (`sd1`) additionally hides its
  ceiling behind a wall-kick bay, so the wall quarter's vault cannot leak
  back up at dash (floor->ceiling there is both-only). `keep-meteor-run`
  and the `low-ceiling-arena-a` crawl gate the deep's side doors with
  dash-symmetric crossings.
- **Both (the seal).** `keep-astral-seal` was re-cut with a ceiling door:
  the seal chute and west door share one chamber, and the east door lies
  beyond a dash curtain *and* a wall-kick partition in sequence - every
  crossing to the east door is both-only, in either direction. The crown
  is reachable exactly at `both` and at no lesser loadout.

Every reachable state at every fixed loadout can retreat to spawn (the
metrics tool verifies this); the gates are symmetric at the loadout that
opens them, so nothing one-ways a player into a pocket they cannot leave.

## Acts, backtracking, loops

Bare: the roof walk, spawn column, upper halls and east gallery (glove one
door east of spawn in the observatory; the wall-gate keep on the gallery
loop shows a wall lock in the first minutes). Wall: back up and west
through the chimney lock into the gable quarter - boots at the roof corner
via the postern. Dash: down the sandglass trapdoors into the deep vaults.
Both: the astral seal, then the crown. Fourteen-plus geometric locks are
seen before they open, so the backtrack-unlock metric stays comfortably
above target; cycle rank 8 keeps two-to-three live loops per act.

## Crown coin gate arithmetic

Coins bankable at fixed loadouts (per the regenerated passability table):
bare 36, wall 50 (the wall quarter's 14 coins arrive with the glove), dash
57 (the deep's vaults arrive with the boots), both 69. The crown door
costs **58**: strictly more than anything bankable below `both` (max 57,
at dash), and 11 under the full-clear total, so the seal always demands
the complete descent but never demands perfection.

## New vocabulary rooms

The room set gained six reusable grids and one keep re-cut, all authored
at rooms-v2 standard (32x18 grid + spec, audited across all door pairs x
loadouts): `gable-run-a` (east+floor roof corner), `eaves-walk-a`
(east+west+floor walk), `lantern-cross-a` (the four-door crossing),
`chimney-lock-a` (three-door wall gate), `keep-west-postern` (roof wall
gate), `sandglass-drop-a/b` (dash trapdoors), and the re-cut
`keep-astral-seal` (three-door both gate). Rooms of rev 2 that the new
embedding does not seat (`strata-sort-c`, `tide-shaft-b/c`,
`shutter-chute-a/b`, `keyhole-vault-a/b`, `one-way-loop-b`,
`antiphase-airlock-a`, `metronome-gallery-a`, `two-clock-fork-b`,
`braided-crossing-*`) remain in the vocabulary for the layout generator.

Remaining door-signature coverage gaps (commissioning targets): only one
`east+floor` room (gable-run-a) and one four-door room (lantern-cross-a)
exist; there is no `ceiling+east+west` room with bare crossings besides
the wall-gated chimney-lock, and no wall-symmetric `ceiling+floor` shaft
(a wall-only descent seems physically awkward - dash or fall always
leaks); a `west+floor` room outside the greed-loop family would also help
roof-caps vary.

## Layout tooling

`tools/dungeon_layout_solver.py` holds the generator-shaped pipeline used
to produce this layout: skeleton search (annealed grid edge-sets scored on
diameter, cycle rank, dead-ends and door-shape supply), room selection per
cell from the vocabulary by door signature and class-adjacency, then
verdict via `tools/dungeon_layout_metrics.py`. Layouts are meant to be
generated; rooms stay authored.
