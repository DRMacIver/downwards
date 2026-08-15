# Compositional ability edge rewrite v1 — 2026-08-15

Status: **graph-certified; physical v2 promotes two exact experimental sources, not a production
corpus source**.

The graph-only results below remain version-1 evidence. The separately versioned
[`physical mapping v2`](compositional-ability-physical-v2-2026-08-15.md) now embeds and
authoritatively validates exact WallJump seed-0 and Dash seed-0 sources. The exact combined seed-1
request retains a typed bounded construction deficit and is not promoted.

This slice composes local traversal gates onto the coordinate-free
`CompositionalRouteCut` mission. It does not add a whole-room family or a fixed room template, and
it does not change the baseline route-cut v2 derivation, embedding, keys, or generated rooms.

## Exact graph API

`CompositionalAbilityEdgeRewriteKey` retains the complete baseline mission key, one of the
`WallJump`, `Dash`, or `Both` profiles, and an explicit finite rewrite-attempt index. The public
entry point accepts an already-derived `DerivedMission`; it never silently re-derives a different
mission, chooses another attempt, or falls back to another geometry.

The rewritten plan has exactly one `DirectedMissionEdge` for each original mission edge. Ordinary
edges are baseline-traversable in both directions. A selected critical spine edge is replaced by
an ability requirement in the authored source-to-sink direction and remains baseline-traversable
in reverse. That forward direction is a requested physical ascent, not yet a replay-backed
reachability claim.

Only baseline-derived missions are accepted. This prevents the old ability bits—which affected a
random stream without authoring a gate—from being repackaged as capability provenance.

## Selection and certificates

The finite selector considers only interior spine edges which:

- have exactly one critical spine-edge owner;
- do not attach a boundary port or a pre-existing collision-cut shelf;
- disconnect source and sink when that exact undirected edge is deleted.

The deletion check exhaustively traverses the complete finite mission graph, including every
fork/rejoin edge. It therefore rejects a visually plausible edge whenever an alternative graph
path bypasses it. Combined profiles select two different non-adjacent bridges and assign wall then
dash or dash then wall with a deterministic seed-varied order.

Every accepted rewrite retains three sorted reachable-node sets per gate:

1. source reachability after deleting the selected edge, which must exclude the sink;
2. source reachability with only that gate's ability removed, which must exclude the sink;
3. sink reachability with the baseline kit, which must include the source through the reverse
   descent.

The intended complete profile must also reach source to sink in the rewritten abstract graph.
These are graph certificates only. Physical promotion still requires accepted replay events and a
reduced-loadout bypass audit; a bounded replay miss will remain inconclusive.

The rewritten topology signature binds the base canonical topology, ordered spine positions,
ability requirements, and reverse-baseline contracts. The exact rewrite signature additionally
binds the base derivation history, profile, explicit attempt, original mission-edge identities, and
ordered rewrite records.

## Fixed seed block

Attempt zero over Standard baseline-derived seeds 0–511 produced:

| Profile | Successful graph rewrites | Distinct rewritten topology signatures | Insufficient bridges | No separated pair |
| --- | ---: | ---: | ---: | ---: |
| Wall jump | 503 / 512 | 499 | 9 | 0 |
| Dash | 503 / 512 | 501 | 9 | 0 |
| Both | 449 / 512 | 448 | 47 | 16 |

Both combined orders occurred in the fixed block. Every non-success above is a typed finite
construction result, not a hidden retry and not evidence about physical reachability.

Focused tests also insert a direct source-to-sink bypass and verify that no spine edge is then
eligible. An attempt exactly equal to the finite arrangement count returns
`ArrangementAttemptExhausted`. The full `downwards-gen` suite passed 77/77 tests after module
registration (75 unit tests, two integration tests), including all baseline route-cut v2
regressions. Strict generator linting passed with warnings denied.

## Smallest safe embedding seam

The graph output carries an `AbilityGateEmbeddingContract` and an explicit pending evidence state.
The safe seam in the existing v2 embedder is the coupled spine-row and support-domain constraint
stage, before any tile rasterization:

1. `embed_mission` must accept an already-rewritten mission instead of deriving a baseline plan
   internally. The historical baseline entry point should keep calling the same code with no gates,
   preserving its exact mapping.
2. `embed_spine_rows` / `complete_rhythm` must treat a gate edge as one atomic special rise. Wall
   shafts reserve at least four rows; dash-rise transfers reserve five. Ordinary edges retain the
   frozen baseline rhythm domain.
3. `spine_support_domains` and `complete_spine_supports` must assign the lower landing, upper
   landing, and reserved volume as a coupled candidate. Independent endpoint domains cannot prove
   that the shaft/transfer remains clear. The reservation must participate in cut, port, standing,
   fork-incident-edge, headroom, and future-feasibility checks.
4. `validate_support_transition_contract` must require the named local structural contract in the
   ascent direction and the conservative baseline transition in reverse. It must not call
   `edge_verb` and infer an ability from distance alone.
5. The route-edge construction must emit `WallClimb` or `DashUp` only from the retained gate
   provenance. For dash gates, the reserved ascent volume excludes usable wall contacts so the
   combined profile cannot wall-jump around the dash requirement.
6. Wall columns are rasterized from the accepted reservation; dash transfers need no decorative
   bridge. `validate_rasterized_route_contract` then checks the exact reserved collision and empty
   cells. No solid may be cleared or repaired after rasterization.
7. Only authoritative replay may advance the current typed pending state. Promotion requires a
   positive intended-kit witness containing the expected ability event and a complete recorded
   reduced-loadout audit; bounded non-success remains inconclusive.

This seam is deliberately not implemented *inside* v1 of the rewrite. Retrofitting a shaft after
independent support search would invalidate cut ownership and could create unmodelled shortcuts.
Physical mapping v2 implements the seam as a separate key/generator path without changing
baseline v2 bytes or production corpus enumeration; its exact evidence and combined-profile
deficit are recorded in
[`compositional-ability-physical-v2-2026-08-15.md`](compositional-ability-physical-v2-2026-08-15.md).
