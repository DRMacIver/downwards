# Compositional ability physical mapping v2 — 2026-08-15

Status: **exact WallJump and Dash sources pass the experimental promotion gate; Both remains a
typed construction deficit; no production corpus integration**.

This is the separately versioned physical successor to the coordinate-free
[`CompositionalAbilityEdgeRewrite` v1](compositional-ability-edge-rewrite-v1-2026-08-15.md).
It embeds a rewritten mission during coupled row/support constraint search. It does not add a
whole-room template, patch collision after embedding, or change baseline compositional route-cut
v2 keys or generation.

## Identities and public boundary

- graph rewrite version: `COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION = 1`;
- physical mapping version: `COMPOSITIONAL_ABILITY_GENERATION_VERSION = 2`;
- finite direct-controller audit version: `DIRECT_PROBE_AUDIT_VERSION = 2`;
- exact request: `CompositionalAbilityGenerationKey`, containing the complete baseline mission
  key, profile, explicit embedding attempt, and explicit rewrite attempt;
- exact result: `CompositionalAbilityCandidate`, retaining the rewritten mission, graph
  certificates, route-plan provenance, boundary ports, cut realizations, and exact gate tile
  reservations;
- exact failure: `CompositionalAbilityGenerationError`, with typed mission, rewrite, bounded
  constraint-phase, baseline-embedding, or local gate-contract causes.

Generation performs no hidden retry. The room ID and metadata generation identity include the
physical version, profile, intent, embedding attempt, rewrite attempt, and source seed.

## Constrained physical contract

The gate mapping participates in embedding before rasterization:

1. a graph-selected spine edge receives one atomic special ascent in the row search;
2. lower and upper endpoints must be one-way supports, outside the conservative baseline ascent
   envelope but baseline-traversable in reverse;
3. a wall gate reserves the exact solid columns and clear shaft cells of a paired-wall shaft;
4. a dash gate reserves the exact clear cells and standing arrivals of a dash-rise transfer;
5. every gate owns an isolation band: no non-endpoint support surface may lie strictly between its
   upper and lower rows. This is checked in row assignment and again during spine and fork support
   search;
6. gate reservations remain pairwise disjoint and are checked against supports, route-transfer
   corridors, cut shelves, door triggers, and door arrivals;
7. accepted solid reservations are rasterized without clearing or repairing any existing tile;
8. the raster validator rechecks every support, standing arrival, reserved solid/empty cell, and
   ordinary route corridor.

`WallClimb` and `DashUp` route verbs come only from retained directed gate provenance. The
candidate remains replay-pending until the separate validation executable supplies positive
authoritative evidence and records the reduced-loadout audit.

## Frozen exact-key results

The validation executable is frozen to two Standard, attempt-zero keys. It has no seed or attempt
fallback:

| Profile | Seed | Embedding attempt | Rewrite attempt | Construction | Experimental promotion |
| --- | ---: | ---: | ---: | --- | --- |
| WallJump | 0 | 0 | 0 | positive | yes |
| Dash | 0 | 0 | 0 | positive | yes |
| Both | 1 | 0 | 0 | `Rhythm` exhausted after 242 candidates | no |

The authoritative physical-v2 evidence for each constructed key is:

| Profile | Intended directed doors | Intended door-to-pickup routes | Canonical `port-0` → `port-1` accepted events | Baseline `port-1` → `port-0` | Baseline finite direct audit |
| --- | ---: | ---: | --- | --- | --- |
| WallJump | 12 / 12 | 4 / 4 | 1 wall jump, 0 dashes | positive | `CompleteNoPositive`; 302 expanded, 111,354 simulated ticks, deepest path 600 |
| Dash | 12 / 12 | 4 / 4 | 0 wall jumps, 7 dashes | positive | `CompleteNoPositive`; 302 expanded, 30,234 simulated ticks, deepest path 600 |

Every positive matrix cell is replay-certified by the authoritative simulation from its exact
source-door arrival. Structural/raster rechecks also passed: graph edge deletion still disconnects
source and sink, reverse graph traversal remains baseline, declared route verbs match gate
provenance, exact solid/empty reservations match the room, cut shelves are unchanged, door
arrivals are clear, and every emitted socket and its mate remain in the frozen inventory.

`CompleteNoPositive` means only that the complete finite built-in direct-controller vocabulary
found no baseline positive witness. It is not a proof of physical impossibility. A future positive
baseline witness would veto the corresponding ability claim; a budget-limited miss would leave it
pending rather than proving the gate.

## Why the combined key is not promoted

Physical v1 constructed `Both` seed 1, but its canonical 117-action complete-kit replay contained
seven dashes and no wall jump. Re-recording those exact actions under dash-only authoritative
physics still reached `port-1` with the same support/event sequence. The trace never entered the
wall shaft and crossed no cut: it landed on central row-10 supports, dashed upward around x=123,
and landed directly on the wall gate's upper row-4 support, skipping the lower row-12 endpoint.
The graph bridge was correct; an undeclared physical support-to-upper-platform transfer bypassed
it.

Physical v2 fixes that correctness hole with the general authored isolation-band constraint, not
a seed-specific obstacle or solver-shaped patch. Under the same exact `Both` key, all 242 finite
row assignments are incompatible with the two serial isolated gates, so generation returns a
typed `Rhythm` exhaustion. No alternate seed, attempt, geometry fallback, or solver budget was
tried. Combined generation remains unpromoted until a later version can compose both reservations
without weakening isolation.

## Reproduction and scope

The exact authoritative report is produced by:

```text
cargo run --offline --manifest-path crates/downwards-research/Cargo.toml \
  --bin compositional_ability_gate_audit
```

The executable enumerates only WallJump seed 0 and Dash seed 0. The generator construction test
also records the exact `Both` seed-1 typed deficit. This work does not enumerate a production
corpus and does not modify corpus candidate/config selection; that requires a separate versioned
integration.
