# Procedural-generation research program

Status: active experiment, 2026-08-14

> **Current direction:** The first 36-entry v6 catalogue exposed serious weaknesses in its
> difficulty-band selection: harder witnesses could hide simple routes, ability-bearing routes
> were not necessarily ability-required, and visible complexity did not guarantee useful terrain.
> The active next-phase plan is the
> [500–1,000-room corpus research plan](corpus-plan.md). It supersedes the band-quota curation
> described below while preserving the compositional generator, multi-door contract, positive
> replay certification, and quality-diversity research foundation. The remainder of this document
> records the v6 program and its rationale.

The historical v5 generator was not a sufficient foundation for the game. Its four whole-room
builders had small finite shape spaces: the observed 1,000-seed catalogues contained only 251,
134, 141, and 24 static layouts for the four traversal kits. Changing the random number generator
or varying trap timers could not fix that structural ceiling.

Generator v6 replaces those builders with compositional generation behind an exact
seed/strategy/intent/ability key. Three approaches now emit the same multi-door room artifact and
are evaluated through the same authoritative simulation. Construction is deliberately separate
from acceptance: raw candidates become normal playable content only through offline AI
certification and quality-diversity selection.

## Prior art and the adaptation being tested

- Unexplored separates an abstract cyclic graph, progressive resolution, spatial embedding, and
  final terrain/object passes. Its main cycle supplies two routes between entrance and goal, while
  minor cycles add detours and complications. BorisTheBrave's detailed reconstruction also makes
  an important limitation explicit: the result depends on a large authored rule vocabulary, not
  on randomness alone. [Dungeon Generation in Unexplored][unexplored]
- Dormans and Bakkes separate mission structure from spatial realization through graph rewriting.
  [Generating Missions and Spaces for Adaptable Play Experiences][missions-spaces]
- Launchpad generates movement rhythm before geometry; Tanagra combines hierarchical planning
  with numerical movement constraints. Those ideas suggest that jumps, rests, reversals, gates,
  and route junctions should exist before tiles do. [Rhythm-Based Level Generation][launchpad]
  [Tanagra][tanagra]
- Precomputed platformer movement models show how actual physics transitions can constrain
  generation instead of relying only on geometric guesses. [Precomputing Player Movement][moves]
- MAP-Elites and PCG quality-diversity research optimize a collection across behavior niches,
  rather than converging on one high-scoring shape. [MAP-Elites][map-elites]
  [Procedural Content Generation via Quality Diversity][pcg-qd]
- Agent action-trajectory distance measures gameplay diversity that tile Hamming distance misses;
  search effort is useful operational evidence but a questionable stand-in for human difficulty.
  [Towards Objective Metrics for Procedurally Generated Levels][objective-metrics]

Applying cyclic graph rewriting *inside one fixed-screen platforming room* is a Downwards design
hypothesis, not a result claimed by those sources.

## Foundational room contract

A generated room is a composable dungeon tile, not a single start-to-exit challenge.

- A room has two to four boundary doors on walls, ceiling, or floor.
- Each door records an inward, safe arrival state.
- The abstract route graph connects door nodes, junctions, landing/rest zones, ability gates, and
  optional rewards.
- For the room's declared ability set, the game-playing AI must produce a positive replay witness
  for every ordered door pair.
- Optional pickups are validated independently from every compatible entrance.
- A lower ability set may lack some routes. That is recorded as bounded inconclusive evidence, not
  as a proof of impossibility; structural gate dominance supplies the generator's negative claim.
- Door positions and route annotations survive into later dungeon-graph assembly, where adjacent
  rooms are matched by opposite side plus the same boundary offset and aperture span. Trigger
  depth and inward arrival position remain room-local implementation details.

## Implemented v6 pipeline

```text
boundary ports + challenge intent
    -> abstract traversal graph / movement rhythm
    -> compositional rewrites (subdivide, fork, rejoin, gate, recovery)
    -> movement-constrained spatial embedding
    -> rasterization and accidental-shortcut audit
    -> hazards, pickups, and visual treatment
    -> cheap structural rejection
    -> authoritative all-pairs solver ensemble
    -> replay, robustness, difficulty, and diversity observations
    -> quality-diversity archive
    -> versioned curated catalogue
```

Small mechanic affordances such as a support, wall-jump zone, or dash transfer are legitimate
primitives. A fixed arrangement of those primitives covering an entire room is not.

## Constructive strategies

All prototypes consume the same seed, ability set, and challenge intent, and emit the same
boundary-port, route-graph, and room representation. This experiment lets the later dungeon
assembler choose from a socket-indexed curated catalogue. Directly embedding an externally
requested socket set is a later alternative, not an implemented claim of these prototypes.

1. **Cyclic graph** starts with parallel arcs between boundary ports, then repeatedly applies small
   graph rewrites. The hypothesis is that explicit fork/rejoin structure gives the best route
   diversity and is the cleanest precursor to dungeon-scale cyclic generation.
2. **Reachability growth** incrementally places supports reachable under a conservative movement
   model, retaining placements that increase connectivity, cycles, or useful vertical span. The
   hypothesis is that bottom-up emergence produces less regular silhouettes and useful accidental
   affordances, at a higher rejection rate.
3. **Rhythm weave** samples movement beats and pacing first, embeds two or more compatible landing
   traces, and weaves them at junctions. The hypothesis is that player-action diversity and pacing
   improve when the generator's primary representation is movement rather than tiles.

The v6 facade retains all three strategies so curation can prefer a mixture when they occupy
complementary behavior niches. `generate_uncurated` rotates the nine strategy/intent combinations
for development convenience, but it performs no AI validation and is never the normal browser's
content source.

### Ability-gate experiment

Rhythm weave includes a variable-width wall shaft whose upward graph cut requires wall movement
and a dash transfer placed above the conservative baseline jump envelope. The combined loadout
serializes distinct wall and dash cuts leading to distinct dominated ports. Structural tests check
route-graph dominance. The final selected representatives must provide catalogue-level evidence:
wall-jump coverage for the wall kit, dash coverage for the dash kit, and both across the combined
kit, with a preference for demonstrating both on one route. A bounded failure with a lower kit
remains inconclusive rather than a proof of impossibility.

## Measurements

Hard feasibility:

- core room and door invariants;
- exact replay/state/event verification;
- every ordered door pair certified under the intended kit;
- every pickup reachable from every compatible entrance;
- intended wall/dash gates structurally dominate the relevant routes and appear as accepted events
  in positive witnesses;
- solver use remains below a fixed budget-headroom ceiling.

Expressive range:

- exact static visual descriptors, with timer schedules excluded;
- tile-field and collision-topology uniqueness;
- route-plan signatures and graph cycle/branch distributions;
- interior tile Hamming nearest-neighbour distribution;
- traversal-path and semantic-action edit distances;
- quality-diversity cell coverage over verticality, route topology, traversal verbs, reversals, and
  hazard pressure.

Challenge and fairness are kept separate:

- challenge: completion time, input transitions per second, accepted movement verbs, vertical
  travel, reversals, route length, and hazard exposure along the witness;
- fairness: perturbation robustness, landing margin, hazard clearance, recovery opportunities, and
  success across multiple solver personas;
- operational confidence: solver nodes/ticks and budget headroom. This is not labelled player
  difficulty.

The existing Gentle/Standard/Technical component score is retained as one diagnostic, not used as
the sole curation objective. The current route-band policy is fixed, not an empirical within-kit
distribution. It subtracts the capped solver-effort component from the AI component score, then
maps 0–4 to Gentle, 5–10 to Standard, and 11+ to Technical. This keeps implementation search cost
out of the presented challenge band, but it remains an uncalibrated heuristic. Human session data
must eventually calibrate or replace it.

Difficulty is attached to an **ordered door route**, not asserted as one scalar property of a
multi-door room. A room can have a gentle west-to-floor traverse and a technical reverse climb.
The room manifest therefore stores the complete route matrix; the standalone level browser may
choose a particular entrance/target pair, while dungeon assembly can reason about the routes its
topology actually asks the player to use. Room-level summaries (minimum, median, maximum, and
spread) are selection features, never substitutes for the per-route observations.

## Experiment gates

Initial raw-candidate targets over 1,000 seeds per kit and strategy:

- at least 900 exact static layouts;
- at least 850 distinct tile fields;
- at least 750 distinct collision topologies;
- no exact layout repeated more than four times;
- at least 100 screen cells vary;
- at least 100 coordinate-free route signatures;
- solver acceptance at least 85% overall and 70% per requested challenge intent.

These are research targets, not promises to weaken silently. A report records misses and the next
hypothesis. Metrics actively optimized during curation are separated from held-out audit metrics
such as pairwise action-trace distance and nearest-neighbour morphology.

A separate promoted-facade regression over raw v6 seeds 0–999 has already found 1,000 distinct
static visuals, tile fields, collision topologies, and route-plan signatures in every ability kit,
with a maximum exact repetition of one. Deterministic pairwise and nearest-neighbour tile-Hamming
audits also pass hard distance floors, so one-cell nudges cannot satisfy this gate. That proves the
old template ceiling has been removed from the sample; it does not prove all-pairs solvability. See
the
[`v6 validation and curation report`](../validation/generator-v6-2026-08-14.md).

## Offline curation

The normal browser does not solve thousands of rooms at startup.

1. Generate a much larger deterministic pool per traversal kit.
2. Cheaply reject invalid, duplicate, and structurally unsuitable candidates.
3. Certify every ordered door route and every pickup from every door with a fingerprinted solver
   configuration.
4. Assign provisional challenge bands to ordered door routes with the fixed component-score-minus-
   solver-effort thresholds, then summarize each room's route matrix.
5. Place feasible candidates into quality-diversity cells, then satisfy distinct-room band quotas,
   strategy coverage, catalogue-level ability-event coverage, vertical-port preference, and
   socket-mate closure together.
6. Rank representatives deterministically by ability contribution, perturbation robustness,
   hazard clearance, and lower simulated ticks/nodes.
7. Prefer new cells and deterministic farthest separation over morphology, collision topology,
   traversal paths, and semantic actions.
8. Check in a versioned manifest containing exact regeneration keys, door/socket layouts,
   per-route observations, complete door/pickup/source-search matrices, certificate fingerprints,
   and compact representative replay witnesses.

Raw seeds remain available as explicitly uncurated development content. The normal level browser
uses only the manifest. Regeneration must be byte-for-byte deterministic, and missing quotas fail
with a deficit report rather than quietly changing difficulty labels.

The runtime parser treats checked-in manifests as data rather than authority. It validates the
manifest checksum and stable ordering, policy and format versions, tier/profile consistency,
unique IDs/keys/visuals, source/target identities, socket-mate closure, regenerated v6 geometry and
visual fingerprints, and the outcome of representative route and pickup action streams.

The first production experiment selected nine rooms per kit—three for each route band—from 144
raw profiles per kit. All four selected sets cover the three constructive strategies and close
their socket inventories. Exact yields, rejection reasons, route/pickup totals, policy identities,
and repeated fingerprints are in the
[`v6 validation and curation report`](../validation/generator-v6-2026-08-14.md).

[unexplored]: https://www.boristhebrave.com/2021/04/10/dungeon-generation-in-unexplored/
[missions-spaces]: https://research.hva.nl/files/149264/453867_Dormans_Bakkes_-_Generating_Missions_and_Spaces_for_Adaptable_Play_Experiences.pdf
[launchpad]: https://eis.ucsc.edu/papers/smith-fdg-09.pdf
[tanagra]: https://ojs.aaai.org/index.php/AIIDE/article/view/12379
[moves]: https://ceur-ws.org/Vol-2862/paper13.pdf
[map-elites]: https://arxiv.org/abs/1504.04909
[pcg-qd]: https://www.antoniosliapis.com/papers/procedural_content_generation_via_quality_diversity.pdf
[objective-metrics]: https://arxiv.org/abs/2201.10334
