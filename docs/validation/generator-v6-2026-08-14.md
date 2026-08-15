# Generator v6 validation and curation report — 2026-08-14

Status: raw expressive-range, curation, runtime, and repeatability gates passed.

This report separates two claims that the historical generator reports combined:

1. **Raw expressive range:** deterministic construction produces a large variety of genuinely
   different rooms rather than cosmetic variations of a small template set.
2. **Playable catalogue acceptance:** an offline curator admits only rooms whose complete ordered
   door and pickup matrices are replay-certified, sufficiently robust, diverse as a set, and
   compatible by boundary socket.

Both claims now have exact results below. Every final manifest was regenerated in a second run and
matched byte for byte; no preliminary or failed-run fingerprint is treated as a release result.

Generator v6 supersedes the historical [v5](generator-v5-2026-08-14.md) and
[v4](generator-v4-2026-08-14.md) single-exit evidence.

## Architecture under test

A v6 regeneration key contains an unchanged seed plus an exact ability set, constructive strategy,
and challenge intent. The strategies are cyclic-graph rewriting, reachability growth, and rhythm
weave. They produce a shared route plan and a room with two to four boundary doors; v6 rooms do not
use a legacy goal exit.

The raw developer selector rotates all nine strategy/intent profiles in deterministic seed order
and returns the first structurally valid composition. It performs no AI assessment. Offline
curation instead evaluates every requested seed/profile combination independently.

Rooms use tile-aligned, 20-pixel door apertures. A socket is the boundary side, offset, and span;
its mate has the opposite side and the same offset/span. Construction regressions check that the
generated inventory can supply mates. Final catalogue selection imposes the stronger postcondition
that every socket in the selected set has a selected mate.

## Raw expressive-range gate

The command was:

```sh
cargo test -p downwards-gen --test diversity \
  v6_uncurated_pool_has_substantial_static_visual_diversity -- --nocapture
```

For each loadout it constructs raw v6 seeds 0–999 and measures four independent identities:

- a static preview descriptor containing tile contents, spawn, doors, pickups, and hazard bounds,
  but excluding IDs, destinations, and periodic-hazard schedules;
- the complete tile field;
- collision topology, where hazard paint is treated as empty so repainting a platform does not
  create false structural diversity; and
- the generated route-plan signature.

Exact uniqueness can still hide 1,000 one-tile nudges. The gate therefore also samples 10,000
deterministic distinct-seed pairs across the full set and measures tile-field Hamming distance. An
independent nearest-neighbour audit computes every pair among the 256-seed prefix. Percentiles use
the deterministic lower-rank convention implemented by the test.

### Hard floors

| Metric | Required |
| --- | ---: |
| Unique static visuals | at least 900/1,000 |
| Unique tile fields | at least 850/1,000 |
| Unique collision topologies | at least 750/1,000 |
| Unique route-plan signatures | at least 900/1,000 |
| Largest exact visual repetition | at most 4 |
| Variable tile cells | at least 100 |
| Sampled distinct-pair tile Hamming, p10 / median | at least 60 / 90 |
| Prefix nearest-neighbour tile Hamming, p10 / median | at least 20 / 35 |

### Results

| Loadout | Static visuals | Tile fields | Collision topologies | Route signatures | Max repeat | Variable cells | Pair Hamming p10 / median | Nearest Hamming p10 / median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline | 1,000 | 1,000 | 1,000 | 1,000 | 1 | 542 | 86 / 117 | 35 / 56 |
| Wall jump | 1,000 | 1,000 | 1,000 | 1,000 | 1 | 535 | 87 / 120 | 36 / 56 |
| Dash | 1,000 | 1,000 | 1,000 | 1,000 | 1 | 544 | 85 / 117 | 31 / 55 |
| Wall jump + dash | 1,000 | 1,000 | 1,000 | 1,000 | 1 | 544 | 89 / 121 | 33 / 57 |

All four loadouts passed every floor. In particular, collision topology and the distance
distributions rule out timer-only variation or a catalogue of otherwise identical rooms with one
moved object. This is strong evidence that the sampled v6 mapping has removed v5's finite-template
ceiling. It is not a reachability result: raw rooms are deliberately uncurated.

## Multi-door certification contract

A playable candidate with `n` doors and `p` pickups must retain:

- `n × (n - 1)` positive certificates, one for every ordered pair of distinct doors;
- `n × p` positive certificates, one for every pickup from every source door; and
- one aggregate source-search record per door.

The validation layer enters a fresh simulation through each source door and calls the shared
multi-target solver once for all of that source's door and pickup targets. Every result still has
its own exact input witness, state/event replay verification, objective identity, and fingerprint.
Bounded exhaustion is an inconclusive rejection, never proof of impossibility.

A focused regression obtains the same target semantics with 389 simulated ticks in the shared
search versus 922 ticks when certifying the fixture's targets independently, a 57.8% reduction.
This is a fixture result rather than a promised speedup; the manifest records actual aggregate
effort per source so later regressions remain observable.

Each door route receives a separate difficulty report. The curation band uses the AI component
score with its solver-effort component removed, then applies fixed thresholds: 0–4 Gentle, 5–10
Standard, and 11+ Technical. These labels are explicitly heuristic and have not been calibrated
against people. Generation intent and assessed route band are separate fields.

The fairness filter requires zero witness deaths and at least one quarter of applicable one-/two-
tick input-boundary perturbations to succeed. Ability evidence is a catalogue-level coverage rule,
not a requirement that every selected room force every unlocked mechanic: the wall catalogue must
contain a representative wall-jump event, the dash catalogue a representative dash, and the
combined catalogue both across its selected representatives. Selection prefers demonstrating both
on one combined-kit route. Structural graph-dominance tests independently guard the wall and dash
cuts.

## Offline quality-diversity selection

For each ability kit, curation overgenerates all three strategies × all three intents × the
requested seeds. It removes exact static duplicates, then rejects incomplete all-pairs/pickup
matrices and routes below the fairness floor. The selector requires distinct rooms across bands
and uses:

- strata over strategy, intent, port count, cycle-rank bin, and vertical-span bin;
- coverage priority for strategy, required ability events, vertical ports, intent, and new strata;
- deterministic representative/room quality ordering by ability contribution, perturbation
  robustness, hazard clearance, and lower simulated ticks and expanded nodes;
- farthest separation over static visuals, collision topology, traversal traces, and semantic
  action traces; and
- socket-mate closure over the complete selected set.

Missing quotas or an exhausted deterministic selection budget fail the run. The curator does not
silently weaken a band, ability gate, robustness floor, uniqueness rule, or socket constraint.

## Production catalogue results

Every run used start seed 0, 16 seeds for each of the nine strategy/intent profiles, and a quota of
three distinct rooms per route band. All 144 candidates constructed in every kit. Each pool also
had 144 distinct raw static visuals and zero exact duplicates.

| Loadout | Attempted / constructed | Certified unique | Socket-coverable | Selected G / S / T | Strategies cyclic / growth / rhythm | Manifest fingerprint |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Baseline | 144 / 144 | 113 | 90 | 3 / 3 / 3 | 1 / 4 / 4 | `downwards-curation-manifest-v2-629fb51d5e0160ed` |
| Wall jump | 144 / 144 | 119 | 99 | 3 / 3 / 3 | 2 / 3 / 4 | `downwards-curation-manifest-v2-445203c1009b1fbf` |
| Dash | 144 / 144 | 134 | 127 | 3 / 3 / 3 | 1 / 6 / 2 | `downwards-curation-manifest-v2-dff255535faac9f6` |
| Wall jump + dash | 144 / 144 | 134 | 110 | 3 / 3 / 3 | 2 / 6 / 1 | `downwards-curation-manifest-v2-3436d699a91b1858` |

All selected sets contain nine distinct rooms, all three constructive strategies, and a closed
socket inventory. More detailed coverage is:

Across the four kits, the 36 keyed challenges contain 31 distinct static visual layouts. Four
layouts recur across ability kits (the largest cross-kit group has three entries); no layout
repeats within a kit. Those entries still have different ability/loadout identities and selected
route challenges, but they should not be counted as 36 visually different rooms.

| Loadout | Intents G / S / T | QD strata | Vertical-port rooms | Doors (vertical) / socket signatures | Ability reps wall / dash / both | Rejections door / pickup / robustness |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline | 6 / 3 / 0 | 8 | 4/9 | 23 (6) / 16 | 0 / 0 / 0 | 28 / 2 / 1 |
| Wall jump | 6 / 2 / 1 | 6 | 3/9 | 20 (4) / 12 | 3 / 0 / 0 | 23 / 1 / 1 |
| Dash | 6 / 3 / 0 | 9 | 6/9 | 25 (9) / 20 | 0 / 2 / 0 | 7 / 1 / 2 |
| Wall jump + dash | 5 / 1 / 3 | 9 | 7/9 | 25 (9) / 18 | 4 / 1 / 1 | 8 / 1 / 1 |

There were no generation rejections. The complete selected matrices contain:

| Loadout | Ordered door certificates | Pickup-from-door certificates | Shared source searches |
| --- | ---: | ---: | ---: |
| Baseline | 40 | 23 | 23 |
| Wall jump | 26 | 20 | 20 |
| Dash | 50 | 25 | 25 |
| Wall jump + dash | 48 | 25 | 25 |
| **Total** | **164** | **93** | **93** |

The manifests use format v2 and selection policy v3. Solver policy v2 serializes configuration
`downwards-solver-config-v1-b296884368c27dd8` for baseline/wall and
`downwards-solver-config-v1-d1a3fb805ce6a54d` for dash/both. All four use difficulty heuristic v1
with configuration `downwards-difficulty-config-v1-42f8b52bf7c7eb5a`.

Production artifacts: [baseline](../../content/catalogues/v6/baseline.manifest),
[wall jump](../../content/catalogues/v6/wall.manifest),
[dash](../../content/catalogues/v6/dash.manifest), and
[wall jump + dash](../../content/catalogues/v6/both.manifest).

The four copies bundled under `content/catalogues/v6` are byte-identical to the corresponding
research artifacts. A second curation run reproduced every manifest byte and fingerprint.
`cargo test -p downwards-catalogue` parsed all four, regenerated every v6 room, checked their policy
coverage and representative replays, and passed. Each catalogue stores nine representative route
RLE streams and nine pickup RLE streams using the same respective source doors. All 31 client
tests also pass against the checked production catalogues.

The compact representative pickup replay uses the same source door as the selected representative
door route, so `V` and `C` compare objectives from one consistent entrance.

## Runtime manifest checks

The checked line-oriented manifest is not trusted merely because it is bundled with the client.
The runtime loader verifies:

- document checksum, final newline, stable room ordering, and supported format/policy versions;
- a single exact ability tier and consistent v6 profile metadata;
- unique catalogue indices, IDs, regeneration keys, and static visual fingerprints;
- door IDs, source/target identities, socket signatures, and catalogue-wide mate closure;
- exact v6 regeneration and the regenerated room's static visual fingerprint; and
- compact representative door-route and pickup action streams by running them through the normal
  simulation and checking the promised target event.

The manifest also retains the complete ordered route matrix, pickup matrix, and per-source search
effort used by offline curation. The normal browser consumes only checked entries. Raw v6 seeds are
developer content and carry no implied certificate.

## Interpretation and limits

The raw audit covers exactly seeds 0–999 under the current deterministic uncurated selector; it
does not prove every possible seed or each strategy in isolation has the same distribution. A
positive AI witness proves that one route exists under one simulation/configuration version. It
does not prove uniqueness, optimality, fun, accessibility, or perceived difficulty.

Socket closure only proves that compatible room pieces exist in the inventory. It does not
assemble a dungeon, validate transitions between rooms, or prove graph-scale progression. Human
playtesting remains necessary for movement feel, visual legibility, route choice, and calibration
of every challenge band.
