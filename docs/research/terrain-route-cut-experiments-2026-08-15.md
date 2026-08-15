# Terrain Route-Cut Experiments (2026-08-15)

Status: frozen mechanic experiments. Do not promote either mapping into the
production catalogue or use the fixed shelf-return grammar as a corpus source.

## Diagnosis

`RoomDraft::new` creates a continuous solid boundary floor. The three v6
compositional strategies mainly used static floor hazards to make their
authored elevated routes necessary. Terrain-only staging removes those
hazards and exposes a route beneath the elaborate terrain.

In a 21-room complete-kit pilot, 89 easiest-known direct positives contained
32 run-only routes, 38 monotone-simple-only routes, and only 19 other
controllers. This confirms that existence of an elaborate alternate route is
not useful difficulty evidence when an easier floor bypass remains.

## Grounded route pier v1

API: `generate_terrain_constrained_candidate` with
`TerrainConstraintExperiment::GroundedRoutePierV1`. The transform removes both
hazard layers and grounds one tile of an existing central route support. It is
defined only for cyclic-graph and rhythm-weave. Reachability-growth is rejected
explicitly: a high pier regressed construction reachability, while low hurdles
did not robustly improve the easiest route.

Four-seed commands:

```text
cargo run --release --offline --quiet --manifest-path crates/downwards-research/Cargo.toml --bin terrain_constraint_experiment -- 0 4 baseline cyclic
cargo run --release --offline --quiet --manifest-path crates/downwards-research/Cargo.toml --bin terrain_constraint_experiment -- 0 4 baseline rhythm
```

Both variants retained all positive construction and complete-kit gates.

| Strategy | Variant | Direct known | Run | Monotone only | Other | Reversals | Interior terrain | Uncorroborated tiles |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Cyclic | legacy terrain-only | 60 | 23 | 31 | 6 | 5 | 740 | 0 |
| Cyclic | grounded pier v1 | 60 | 21 | 28 | 11 | 9 | 782 | 8 |
| Rhythm | legacy terrain-only | 101 | 47 | 34 | 20 | 14 | 708 | 27 |
| Rhythm | grounded pier v1 | 98 | 40 | 38 | 20 | 19 | 761 | 43 |

Cyclic improved modestly. Rhythm did not increase genuinely-other easiest
controllers, and both strategies added terrain without positive corroboration.
This is useful negative evidence, not a promotable rewrite.

## Shelf return v1

API: `SwitchbackCutKey::regenerate` or `generate_switchback_cut`. The complete
typed key records:

- source seed;
- exact construction abilities;
- challenge intent;
- frozen grammar (`ShelfReturnV1`);
- explicit embedding-attempt identity.

Generation never silently retries. Unsupported attempt identities fail. Room
IDs and metadata use an independent switchback namespace rather than v6 IDs.

The physical mechanic is one long solid shelf joined to the entry-side wall.
The lower entry and upper route are behind the shelf, while its only transfer
is outside the shelf span. The player must travel out through the opening and
reverse back over the shelf. All later rises are at most two rows. Every
interior terrain tile belongs to an explicit route support; there is no accent
or decorative terrain.

Several stricter prototypes were rejected before freezing this version:

- four and two alternating cuts were complete-kit positive but baseline
  searches hit the path horizon;
- wider/full-height grounded transforms increased unused terrain;
- three-row transfer rises were rejected because they repeat the old
  sub-pixel-clearance defect and are not baseline-conservative.

One-seed, all-construction-loadout command:

```text
cargo run --release --offline --quiet --manifest-path crates/downwards-research/Cargo.toml --bin switchback_cut_experiment -- 0 1 all 0
```

Results across 12 rooms (four construction loadouts by three intents):

- 12/12 structurally constructed and 12/12 complete-kit all-target positive;
- complete-kit evidence: 80/80 ordered doors and 36/36 pickup-from-door routes;
- 7/12 also passed the exact construction-loadout gate: wall-jump Gentle,
  every Dash intent, and every Both intent;
- Baseline passed 17/20 door routes and 6/9 pickup routes, but the three
  bottom-to-ceiling and bottom-to-pickup compositions remained bounded
  `PathHorizon` inconclusive;
- the exact baseline direct vocabulary was complete with no bottom-to-ceiling
  positive (293 probes, 148,733 simulated ticks, deepest path 600 on Gentle);
- a complete-kit exact bottom-to-ceiling witness used one horizontal reversal,
  23 vertical decisions, 402 ticks, six wall jumps, and no dashes.

The baseline result is not an unreachability claim. Individual bottom-to-shelf
and shelf-to-ceiling segments have baseline positives. A separate solver-only
investigation owns the remaining composition/search gap.

### Solver-composition follow-up

The gap was a bounded-search vocabulary defect, not an impossible route. A
deterministic wider search first found and exactly replayed a 363-tick baseline
bottom-to-ceiling witness. A new generic elevated-target detour/homing climb
probe then found a 232-tick witness with one horizontal reversal, 18 ordinary
jumps, and no wall jump or dash. It also reaches the pickup. Under solver
policy v3/direct-probe-audit v2, the default construction and complete-kit
hard gates now pass for all three frozen baseline intents. This strengthens
the mechanic result without changing the frozen room geometry.

Four-seed complete-kit command:

```text
cargo run --release --offline --quiet --manifest-path crates/downwards-research/Cargo.toml --bin switchback_cut_experiment -- 0 4 both 0
```

Results across 12 rooms:

- dual hard gate: 12/12 rooms;
- authoritative positives: 80/80 ordered doors and 36/36 pickups;
- easiest exact direct positives: 66/80, classified as 16 run-only, 8
  monotone-simple-only, and 42 genuinely other; the other 14 are complete
  finite-vocabulary misses, not negative reachability evidence;
- canonical bottom-to-ceiling: 12/12 positives and exactly one horizontal
  reversal in every witness, with 202 vertical decisions and 3,596 ticks total;
- canonical ceiling-to-bottom: 12/12 positives and exactly one reversal in
  every witness, with 59 vertical decisions and 3,844 ticks total;
- all 40 unordered door pairs differed directionally in measured canonical
  demand; total absolute duration difference was 3,708 ticks (maximum 280);
- terrain: 441 interior tiles, all 441 route-attributed and positively
  corroborated, with zero uncorroborated components or tiles.

This is substantially stronger route-cut evidence than the pier transform: a
real collision cut survives wall-jump and dash, the easiest witnessed main
route reverses, and less terrain is used. It remains intentionally low
expressivity: the 12-room batch produced only 9 route signatures, 9 static
visuals, and 9 simulation geometries. The fixed grammar varies only mirror,
one cut endpoint, and port inventory.

## Consequence

The shelf-return experiment validates a mechanic seam, not a room template.
Follow-up work should extract an `insert route cut` rewrite into a genuinely
compositional generator with variable cut count, rows, orientation sequence,
route order, and branch structure. Every realized candidate must retain exact
construction-loadout and complete-kit all-pairs/pickup positives, measure the
easiest exact-loadout controller rather than a harder alternate, and reject
unattributed terrain.
