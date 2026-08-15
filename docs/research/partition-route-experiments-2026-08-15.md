# Recursive partition-route experiment — 2026-08-15

Status: experimental corpus source, not promoted. Construction and bounded AI
evidence below are reproducible checkpoints, not human-difficulty claims.
Every solver non-success remains inconclusive; none is reported as
unreachable.

## Hypothesis and identity

`downwards_gen::experimental::PartitionRouteKey` identifies a graph-first,
terrain-only recursive partition candidate by source seed, exact construction
abilities, challenge intent, profile, and explicit bounded embedding attempt.
There is no fallback geometry. The derivation distinguishes:

- pure graph topology signature (rewrite ancestry/axes, forks, port sides,
  pickup attachment);
- coordinate-free route/rhythm signature (topology plus movement beats,
  reversals, and cadence);
- full normalized derivation fingerprint (adds split/gate embedding choices);
- coordinate-bearing `RoutePlanSummary::signature`.

The generator builds two to four connected traversal chambers. Its root
vertical collision cut closes the continuous floor run and leaves an elevated
opening. Horizontal partitions contribute zero to two fork/rejoin cycles.
Version 3 intentionally makes only conservative baseline terrain claims for
all construction profiles: every authored edge changes at most two rows and
no edge is labelled WallClimb or Dash.

## Version 1 evidence, retained historically

The initial mapping constructed all 128 MixedBsp/Standard/baseline seeds. It
produced 127 distinct static tile maps, 42 pure graph topologies, 128
coordinate-free route derivations, and 128 full derivation fingerprints.

The first authoritative smoke used seeds 0–3, MixedBsp, all three intents, and
baseline construction abilities (12 rooms):

```text
attempted=12 constructed=12 hard-gate-positive=12
construction: rooms 12/12, doors 80/80, pickups 31/31
Both: rooms 12/12, doors 80/80, pickups 31/31
construction direct: 57/80 exact, 23 complete-finite-vocabulary without a direct positive, 0 bounded
classes: 13 run, 35 monotone-only, 9 other
Both direct: 57/80 exact, 23 complete-finite-vocabulary without a direct positive, 0 bounded
classes: 13 run, 34 monotone-only, 10 other
directional: 40 bidirectional canonical pairs, 37 asymmetric
terrain: 850 interior tiles, 566 statically attributed, 773 positively corroborated,
         1 component / 77 tiles without positive corroboration
floor smoke: 4 ports, 4 safe entries, 0 immediate source retriggers, 4 source rows
```

The eight-seed MixedBsp breadth checkpoint (24 rooms) also passed all
construction and Both matrices: 160/160 directed door routes and 63/63 pickup
routes under each loadout. It had 80 canonical bidirectional pairs, 75 with
measured directional asymmetry.

Version 1 nevertheless failed the dungeon composition contract. Across the
exact 72 breadth keys (seeds 0–7 × three profiles × three intents), only 4 of
20 distinct sockets had an opposite mate; 16 distinct sockets and 59 socket
occurrences had no mate. Lateral sockets matched, but unconstrained ceiling
offsets did not share the small floor inventory. This correctness failure
stopped the breadth solver run and forced a mapping version bump. The positive
room-local solver evidence above must not obscure that failure.

## Version 2 socket contract

Version 2 chooses one of a frozen three-slot ceiling/floor grid from seed,
profile, construction loadout, and retry. Intent is deliberately excluded:
the Standard ceiling and Technical ceiling/floor therefore share the exact
slot for every paired identity. A shallow authored ceiling aperture keeps the
fixed trigger and arrival clear; the root partition reserves the corresponding
floor aperture while preserving its elevated collision cut.

Exact construction-only audit for seeds 0–7 × three profiles × three intents:

```text
attempted=72 constructed=72 sockets=216 distinct-sockets=8
distinct-missing-mates=0 occurrences-missing-mates=0
port counts: 2×24 rooms, 3×24 rooms, 4×24 rooms
side combinations: L+R×24, L+R+Ceiling×24, L+R+Ceiling+Floor×24
left@150×72 <-> right@150×72
ceiling@50×18  <-> floor@50×9
ceiling@150×14 <-> floor@150×7
ceiling@250×16 <-> floor@250×8
```

The generator has an inline regression over those exact 72 keys; all 216
socket occurrences must find an opposite mate. The 128-seed v2 construction
checkpoint is 128/128 distinct static maps, 42 pure graph topologies, 128
route/rhythm signatures, and 128 full fingerprints.

Version 2's fixed-slot connector regressed room-local correctness even though
its socket catalogue closed. In the same 12-room authoritative smoke used for
version 1, only 7/12 construction matrices and 8/12 Both matrices were fully
positive. Construction found 67/80 door routes and all 31 pickups; Both found
70/80 doors and all pickups. Every miss targeted the ceiling port (13
construction `PathHorizon` results and 10 Both `SimulatedTickBudget` results).
The simplest failure forced a long top connector from the former local anchor
at x=23..26 to the fixed x=4..8 ceiling support, and declared connector
landings had solid partition tiles in their standing clearance. This was a
plan/raster correctness failure, not evidence that a larger solver budget was
needed.

## Version 3 realized-plan and socket contract

Version 3 keeps the closed three-offset vertical socket inventory but chooses
the nearest cut-safe ceiling slot to the highest usable embedded route anchor.
Technical floor ports cycle independently across the same finite inventory,
so closure is a pool-level composition property rather than a same-seed
Standard/Technical pairing. Floor port nodes now describe the safe arrival
support, not the aperture itself.

All landing, connector-corridor, and door-arrival constraints are established
before partition rasterization. Any conflict with an authored cut produces a
deterministic typed `EmbeddingExhausted`; the generator never clears a cut
after rasterization. Every successful candidate is then audited for:

- byte-for-byte preservation of every root and non-root authored cut tile;
- exact declared support material plus two-row solid-free standing clearance;
- the shared conservative baseline transition classification in the declared
  direction and the reverse direction; and
- root separator continuity, terrain-only construction, and exact port shape.

The exact MixedBsp/Standard/baseline 128-seed checkpoint constructs 125/128
keys (97.7%). Failures are deterministic typed exhaustion at seeds 37 and 68
(no cut-safe ceiling connector) and seed 110 (a constrained edge collapses to
an identical support). Among successes there are 124 static tile maps, 40 pure
graph topologies, 125 route/rhythm signatures, and 125 full fingerprints.

The broader attempt-zero invariant audit constructs 566/576 keys (98.3%) over
seeds 0–63 × three profiles × three intents with the `Both` construction
loadout. Six failures have no cut-safe ceiling connector and four have a
constrained edge collapse; every constructed key passes all raster/support/
edge invariants above. Non-successful exact keys remain recorded failures and
are not silently retried.

The room-aware socket audit over the exact 72 baseline keys constructs 71
rooms (one deterministic Columnar/Technical/seed-4 ceiling-connector
exhaustion) and emits 212 socket occurrences. Every occurrence has an
opposite mate on a *different* generated room:

```text
attempted=72 constructed=71 distinct-sockets=8 socket-occurrences=212
distinct-missing-different-room-mates=0
occurrences-missing-different-room-mates=0
port counts: 2×24 rooms, 3×24 rooms, 4×23 rooms
side combinations: L+R×24, L+R+Ceiling×24, L+R+Ceiling+Floor×23
left@150×71 <-> right@150×71
ceiling@50×17  <-> floor@50×8
ceiling@150×17 <-> floor@150×8
ceiling@250×13 <-> floor@250×7
```

The authoritative 12-room smoke (seeds 0–3, MixedBsp, all intents, baseline
construction) is fully positive under both construction and `Both`:

```text
attempted=12 constructed=12 hard-gate-positive=12
construction: rooms 12/12, doors 80/80, pickups 31/31
Both: rooms 12/12, doors 80/80, pickups 31/31
construction direct: 62/80 exact, 18 complete-no-positive, 0 bounded
classes: 15 run, 35 monotone-only, 12 other; reversals 12, vertical decisions 338
Both direct: 59/80 exact, 21 complete-no-positive, 0 bounded
classes: 15 run, 34 monotone-only, 10 other; reversals 9, vertical decisions 311
directional: 40/40 canonical bidirectional pairs asymmetric
terrain: 102 components, 823 interior tiles, 521 statically attributed,
         721 positively corroborated, 0 uncorroborated components / 102 tiles
floor smoke: 4 ports, 4 safe entries, 0 immediate source retriggers, 4 source rows
construction inconclusive causes: none
Both inconclusive causes: none
```

## Reproduction

```sh
cargo test --offline -p downwards-gen partition_route -- --nocapture
cargo clippy --offline -p downwards-gen --all-targets -- -D warnings

cargo run --offline \
  --manifest-path crates/downwards-research/Cargo.toml \
  --bin partition_route_socket_audit -- 0 8 0

cargo run --offline \
  --manifest-path crates/downwards-research/Cargo.toml \
  --bin partition_route_experiment -- 0 4 baseline mixed all 0
```

`partition_route_experiment` validates exact matrix shape and source-door
self-target suppression, separately runs construction-loadout and Both
matrices, partitions finite direct-controller evidence into run,
monotone-simple, and other classes, measures canonical directional asymmetry,
and reports static/positive terrain attribution. It does not alter geometry
after a solver miss.
