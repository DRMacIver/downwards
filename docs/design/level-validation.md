# Individual-room v6 generation and validation contract

This document defines what “ready for level playtesting” means for a compositional v6 room. It is
an engineering acceptance contract, not a claim that automated metrics can replace human taste.

## Validation scenario

A generated room is evaluated as a future dungeon tile, not as a single spawn-to-exit level. A
scenario records:

- the exact v6 `CompositionalKey`: seed, constructive strategy, generation intent, and ability set;
- the validated room definition and all safe boundary-door arrivals;
- every promised pickup;
- deterministic timing state for periodic hazards; and
- the versioned solver and difficulty configurations used to make positive reachability claims.

For a room with `n` doors and `p` pickups, full acceptance requires `n × (n - 1)` ordered
door-route certificates and `n × p` pickup-from-door certificates. Direction matters: proving
west-to-floor does not prove floor-to-west. Every objective starts from a fresh
`Simulation::enter_via_door` state with the scenario's exact loadout.

`SearchTarget::Door` and `SearchTarget::Pickup` name their goals explicitly. A successful door
certificate binds the source door, target door, complete generated provenance, exact replay,
route-specific difficulty report, and versioned witness fingerprint. A pickup certificate binds
its source and exact pickup independently; reaching some door or another pickup cannot be
relabeled as success.

The solver makes positive claims only. Exhausting a bounded search is `inconclusive`, never proof
that a lower ability set cannot traverse a route. Intended ability restrictions therefore also
need a structural gate argument from the generator.

## Door and dungeon-tiling contract

Each v6 room has two to four doors on `Left`, `Right`, `Ceiling`, or `Floor` boundaries. A door
owns a trigger immediately inside the room and a safe inward arrival position. Its canonical
`DoorSocket` contains only:

- the boundary side;
- the aperture offset along that side; and
- the aperture span.

Two rooms can be aligned when their sockets have opposite sides and identical offsets and spans.
Trigger depth and safe arrival geometry remain local to each room. The curated catalogue must be
socket-closed: every socket exposed by a selected room has at least one mate somewhere in the
same ability catalogue. This does not yet assemble or validate a multi-room dungeon; it makes the
room inventory composable when that layer is added.

## Mechanics vocabulary

The playtestable generator may use:

- baseline run and variable-height jump (release Jump early for low, hold it for high);
- wall slide and wall jump when enabled;
- a rechargeable eight-direction dash when enabled;
- solid and one-way platform tiles;
- static lethal tiles and deterministic periodic hazards;
- two to four safe boundary doors; and
- pickups on optional, intentionally more demanding routes.

Every mechanic and room definition is owned by Rust. Procedural generators and the authored First
Steps fixture construct the same validated core types; neither path can implement alternate
physics outside the headless simulation.

## Constructive strategies and ability gates

Generator v6 exposes an exact strategy/intent profile rather than selecting one of a few complete
room templates. The three current strategies share one route-plan and boundary-port vocabulary:

1. **Cyclic graph** rewrites abstract paths into fork/rejoin cycles before spatial embedding.
2. **Reachability growth** builds a reversible support network from movement-reachable placements,
   retaining cross-links and useful vertical span.
3. **Rhythm weave** describes action beats first, embeds compatible landing traces, and joins them
   at shared junctions.

Wall-jump and dash transfers are local constructive primitives, not fixed whole-room layouts.
Rhythm-weave profiles build graph cuts dominated by the corresponding transfer; the combined kit
serializes distinct wall and dash cuts. Structural tests check that the intended edge dominates a
boundary port. Curation then requires ability-event coverage across the selected representatives:
at least one wall-jump route for the wall kit, at least one dash route for the dash kit, and both
events across the combined catalogue, preferring a single route that demonstrates both. Geometry
alone is never substituted for a positive full-physics replay.

## Acceptance and shared search

Before a v6 candidate can enter a playable catalogue:

1. Core validation checks dimensions, object bounds, unique IDs, door triggers, safe arrivals, and
   content references.
2. The route plan and physical room agree on two to four boundary ports.
3. From each source door, one shared `solve_targets` exploration requests every other door first
   and every declared pickup afterward.
4. Every requested target returns a successful certificate; any generation, topology, search, or
   replay failure rejects the room.
5. Replaying every witness reproduces each per-tick state digest and ordered event-stream digest.
6. Every door route receives its own difficulty and one-/two-tick input-boundary perturbation
   report.
7. The curation fairness floor requires zero deaths in each accepted route and at least one quarter
   of applicable perturbations to succeed. A witness with no applicable transitions passes that
   particular floor.
8. Each ability catalogue's complete selected set provides the required representative traversal
   coverage; individual rooms need not all force every unlocked ability.

Sharing search work is an optimization, not a relaxation. Certificates and replays remain separate
per target, while the manifest records the aggregate solver work once per source door.

## Route-specific challenge bands

Difficulty belongs to an ordered door route, not to a room. The same geometry may be Gentle in
one direction and Technical in the reverse direction. The current AI reports capped components
for completion time, input transitions, traversal verbs, deaths, temporal fragility, and bounded
solver effort.

The curation policy deliberately removes the solver-effort component before assigning a catalogue
band, because implementation search cost is operational confidence rather than player challenge:

```text
curation score = AI component score - solver-effort component

0..=4   Gentle
5..=10  Standard
11+     Technical
```

These fixed thresholds are heuristic. They are not empirical within-kit percentiles, objective
difficulty ratings, or promises about player skill. The constructive Gentle/Standard/Technical
*intent* is also only a generation input; the measured route score decides catalogue eligibility.
Human session data must calibrate or replace this policy.

## Offline overgeneration and selection

The normal browser does not solve a raw seed range at startup. Offline curation:

1. Overgenerates every seed across all three strategies and all three challenge intents for one
   exact ability kit.
2. Rejects construction failures, exact static-visual duplicates, incomplete door/pickup matrices,
   fragile routes, and candidates whose sockets cannot be covered by the pool.
3. Places certified rooms in quality-diversity strata using strategy, intent, port count, route
   cycle rank, and vertical-span bins.
4. Favors strategy and catalogue-level ability coverage, vertical ports, intent and stratum
   coverage, and socket feasibility. Within those constraints it ranks routes deterministically by
   ability contribution, perturbation robustness, hazard clearance, and lower simulated
   ticks/nodes, then favors farthest separation over static visuals, collision topology, traversal
   traces, and semantic action traces.
5. Assigns distinct rooms to requested route-band quotas and enforces socket-mate closure across
   the final set.
6. Fails with an explicit deficit or search-budget report rather than weakening a quota, fairness
   floor, ability requirement, or band label.

The line-oriented manifest retains every room's exact regeneration key, doors and sockets, route
plan summary, complete route and pickup matrices, source-search effort, policy versions,
fingerprints, and compact representative route/pickup action streams. Runtime loading treats the
manifest as untrusted data: it checks the document checksum and stable ordering, version and tier
consistency, unique IDs/keys/visuals, route/source identities, socket closure, regenerated v6 room
geometry, visual fingerprints, and representative replay outcomes.

Raw seeds remain an explicitly uncurated developer facility. Construction alone does not imply
all-pairs reachability, fairness, difficulty, or catalogue acceptance.

## Batch gates and evidence

The raw expressive-range gate samples seeds 0–999 for every ability kit. It independently measures
static preview descriptors, complete tile fields, and collision topology so moving a pickup or
changing a timer cannot hide repeated platform geometry. V6 currently produces 1,000/1,000 unique
values for all three descriptors in every kit. Exact counts and limits are in the
[`generator v6 report`](../validation/generator-v6-2026-08-14.md).

The checked-catalogue gate additionally requires:

- all requested band quotas with distinct rooms;
- every ordered door pair and every pickup from every door;
- exact replay verification and stable policy identities;
- catalogue-level representative traversal-ability coverage where required;
- fairness floors and solver headroom;
- useful quality-diversity and strategy coverage;
- at least one vertical port where required by the selection policy;
- socket-mate closure; and
- byte-for-byte deterministic manifest regeneration.

Exact catalogue counts, coverage, rejection classes, policy identities, and repeated manifest
fingerprints are recorded in the [`generator v6 report`](../validation/generator-v6-2026-08-14.md).
Generator [`v5`](../validation/generator-v5-2026-08-14.md) and
[`v4`](../validation/generator-v4-2026-08-14.md) reports are historical single-exit gates and do
not satisfy this contract.

## Playtest client gate

The desktop client must let a tester:

- continuously scroll through the Rust-authored First Steps room and the checked entries for each
  loadout;
- jump with Page Up/Page Down/Home/End without page-mode navigation;
- inspect a cached room preview, three-word name, strategy, intent, route band, source/target
  doors, and socket information before starting;
- enter through the selected source and clearly distinguish the target from other valid doors;
- restart instantly and advance to the next curated room without restarting the process;
- toggle collision, hazard, door, and movement-state debug drawing;
- record human input through the same semantic `Action` stream used by AI;
- watch stored representative door and pickup witnesses through the normal simulation path; and
- inspect session-only human attempts, deaths, completions, coins, and best times that exclude AI
  and recorded-replay playback.

Reaching the wrong door must not count as completing the selected ordered route. HUD chrome must
remain outside playable geometry. Explicit raw-seed access belongs to developer tooling rather
than the normal catalogue browser.
