# First-room vertical slice

## Purpose

The first vertical slice exists to answer two questions before building a dungeon around the
wrong foundations:

1. Is moving through one screen enjoyable and legible?
2. Can the same deterministic simulation support automated solvability and difficulty checks?

It is one authored platforming room with a fixed camera. It is not a small version of every
eventual system. Procedural dungeon generation, metaprogression, a run economy, final art and
audio, narrative, save files, and a large trap catalogue are explicitly deferred.

## Current implementation checkpoint

The mechanics lab is playable. Rust owns fixed-step integer-subpixel movement, collision, coyote
time, jump buffering, wall slide/jump, optional eight-direction dash, static and periodic hazards,
one-way platforms, pickups, retries, boundary-door entry, and stable state hashes. The authored
First Steps development room is a typed Rust fixture. Generator v6 creates compositional
two-to-four-door candidates for four explicit loadouts, and both content paths construct the same
validated core room model.

The generation work has moved beyond the historical v5 whole-room families. V6 compares cyclic
graph rewriting, bottom-up reachability growth, and movement-rhythm weaving behind exact
seed/strategy/intent/ability keys. Every accepted room is entered from each boundary door; the AI
must replay-verify every ordered route to every other door and every pickup from every entrance.
One shared multi-target search per source door avoids repeating common exploration without
weakening any certificate.

The Macroquad client retains its continuous scrolling browser, cached miniature previews, debug
overlays, semantic human-attempt recording, replay playback, visible ability locks, and session-only
statistics. Its normal generated list uses four nine-room offline-curated manifests rather than
treating a raw 0–999 seed range as playable content. Each curated row is one representative
source-to-target route with a unique three-word name and an exact v6 regeneration key. Explicit
raw seeds remain available only for uncurated development inspection.

The curation harness rejects incomplete route/pickup matrices, fragile witnesses, exact visual
duplicates, and unusable socket inventories before selecting across quality-diversity niches.
Gentle/Standard/Technical is a fixed, route-specific heuristic pending human calibration. See
[`level-validation.md`](level-validation.md) and the
[`v6 validation and curation report`](../validation/generator-v6-2026-08-14.md). The
[`v5 report`](../validation/generator-v5-2026-08-14.md) is retained only as historical
single-exit evidence. Dungeon assembly and run systems remain later milestones.

## Design target

The player enters from a safe boundary door, reads the room, and selects a route to another door.
The room may connect walls, ceiling, and floor in several directions, and the reverse traversal is
a separate challenge. A selected browser entry highlights one source/target pair for focused
playtesting; other doors remain part of the room's eventual dungeon topology. Optional pickups
give us another way to compare safe and expressive routes from every possible entrance.

Failure should be clear and recovery should be fast: touch a lethal obstacle, show a very short
response, and reset to the entrance without a loading transition. There is no combat. Pressure
comes from geometry, timing, route choice, and the controlled use of traversal abilities.

### Confirmed prototype direction

- A 320×180 logical canvas with nearest-neighbour scaling.
- A fixed 60 Hz simulation tick.
- Desktop keyboard controls first, while simulation input remains device-independent.
- Run and variable-height jump form the baseline: release Jump early for a low jump and hold it
  for a high jump. Explicit loadouts add wall slide/jump and dash. Stamina is excluded for now,
  while active climbing can be evaluated later.
- Dash and later traversal abilities are tested through explicit loadouts. Their acquisition UI
  and placement are outside the room prototype.
- Death resets the current room with infinite retries during mechanics development.
- Rust owns generated and authored content, mechanisms, physics, and authoritative state. There is
  no runtime scripting engine.

Movement values remain subject to playtesting. In the eventual run structure, deaths will reduce
a health bar and reaching zero will end the run; success and/or pickups may restore health. That
economy is deliberately absent from the room prototype.

## Room contents and acceptance criteria

The room should contain the smallest vocabulary capable of testing the movement:

- axis-aligned solid collision;
- one-way platforms if they materially improve the test layout;
- spikes or a generic lethal volume;
- one dynamic timing element, initially either a moving platform or a periodic hazard;
- two to four safe boundary doors with tileable apertures;
- an optional harder route or collectible; and
- no camera scrolling.

The vertical slice is complete when all of the following are true:

1. A player can finish representative door routes and optional pickup routes with readable,
   responsive controls.
2. Restarting after death feels immediate and never spawns the player in collision or danger.
3. The game can record an input stream and replay it from a known initial state.
4. Replaying the same seed and inputs produces the same simulation hashes.
5. The headless solver finds every ordered door pair and every promised pickup from every door,
   saving each as a separate replay witness.
6. The graphical client successfully plays door-route and pickup witnesses through the normal
   simulation path.
7. A test fails if geometry or movement changes make the required or optional route unsolvable.
8. Debug views expose collision shapes, player state, current input, tick, and seed.

## Mechanics sequence

Each phase should end with focused tests and a small authored movement exercise. Constants should
be tuned after the behaviour is correct, not embedded throughout the code.

### 1. Deterministic room loop

Implement a fixed-step state transition, abstract input actions, tile collision, a safe entry,
boundary-door triggers, death, and reset. Rendering observes simulation state; it does not
determine it. Audio and particles respond to events and are not part of authoritative state.

### 2. Ground movement and jump feel

Add horizontal acceleration, deceleration, maximum speed, air control, gravity, terminal speed,
variable jump height (release Jump early for low, hold it for high), coyote time, and jump
buffering. Test exact edge cases such as jumping on the final coyote tick and landing while a
buffered jump is active.

Human jump-input policy v3 maps a physical release within 100 ms to one semantic low-jump gesture
and a continued hold to the ordinary variable-height input. Buffered taps remain one intent until
accepted. The wall-clock threshold is an input affordance, not a unit that level geometry or AI
certification may depend upon.

This phase is the first real playtest gate. Geometry should remain simple until running and
jumping are satisfying.

### 3. Wall interaction

Add wall detection, wall slide, and wall jump, including explicit rules for input lockout or
steering after a wall jump. Active climbing is deferred for later evaluation, and stamina is not
part of the current design.

### 4. Dash

Prototype a single directional dash with documented duration, speed, steering rules, and
recharge conditions. Test cardinal and diagonal directions, collisions during a dash, and
whether jump buffering applies at dash end. Including dash here is an evaluation choice, not a
commitment that it belongs in the starting ability set.

### 5. Environmental challenge

Add lethal volumes and one deterministic moving or periodic obstacle. Then author the main and
optional routes. Avoid adding more trap types until both humans and the solver can complete this
version reliably.

### 6. Replay and solver integration

Record normalized per-tick actions, the content version or room identity, the initial seed, and
enough metadata to reject incompatible replays. Run the solver against the same `step` operation
used by the client, then render its successful witness in the client.

### 7. After the movement slice

Boundary doors, capability-aware room generation, and graph-level validation are now implemented
for isolated rooms. Next, add actual room-to-room transitions and a dungeon graph assembled from
matching sockets. Switches, springs, crumble blocks, checkpoints, and further traversal abilities
should still arrive one at a time with new solver vocabulary and focused regression rooms.

## Architecture boundaries

The scaffold is split so that game rules can be exercised without a window:

| Layer | Owns | Must not own |
| --- | --- | --- |
| `downwards-core` | Authoritative state, fixed-step rules, collision, normalized input, room model, mechanisms, events, snapshots, and hashes | Rendering, audio, wall-clock time, OS input |
| `downwards-content` | Trusted built-in Rust fixtures such as First Steps | Runtime parsing, alternate game rules, mutable process-global state |
| `downwards-gen` | Exact v6 regeneration keys, route plans, constructive strategies, spatial embedding, and ability-gate structure | Treating construction as proof of playability |
| `downwards-ai` | Search states, action macros, reachability, replay witnesses, difficulty observations | A second approximation of player physics |
| `downwards-validation` | Exact generated-scenario objectives, positive acceptance certificates, retained witnesses and metrics | Treating an inconclusive bounded search as proof of impossibility |
| `downwards-catalogue` | Strict manifest parsing, v6 regeneration checks, socket closure, and representative replay verification | Generating, repairing, or silently accepting manifest content at runtime |
| `downwards-client` | Window, input mapping, presentation, debug overlays, replay viewing | Alternate gameplay rules |
| `downwards-tools` | Batch validation and human-readable diagnostics | Rules that differ from the client or solver |

The standalone `downwards-lab` and `downwards-research` crates own canonical diversity
descriptors and offline experimental/curation policy respectively. They are not runtime authority.

The central seam should be conceptually small:

```text
initial state + normalized input frame -> one simulation tick -> new state + events
```

The implemented seam is `Simulation::step(Action) -> StepReport`. `Simulation` is cheaply
cloneable, `Action` contains semantic held input rather than keyboard keys, and `StepReport`
contains authoritative events plus a stable state digest. `Action` already carries movement,
jump, dash, and restart intent and can grow alongside later traversal mechanics.

### Content and mechanism boundary

Procedural generators and manually designed fixtures construct the same validated Rust `Room`
types. First Steps is an authored Rust fixture rather than a second runtime content path. The
checked catalogue manifests remain external, versioned curation artifacts: they select exact v6
regeneration keys and retain certificates, but they do not define or execute gameplay behaviour.

Room-specific mechanisms should begin as typed Rust definitions paired with explicit,
cloneable simulation state. Every value that can affect a future tick must participate in reset,
state hashing, replay verification, and the solver's state equivalence. This keeps traps and
event-driven behaviour inside the same deterministic authority as movement and collision.

There is no general data-loading or scripting API in the prototype. If a future requirement such
as non-programmer authoring, hot reload, or modding makes one worthwhile, add a versioned adapter
that validates into the Rust model and decide separately whether executable callbacks are needed.
Do not reserve a runtime or schema before that requirement exists.

## Solver roadmap

The solver is part of the vertical slice, not a post-generation cleanup tool. Its first job is to
detect impossible rooms and produce a concrete replay that explains success.

### Stage 1: deterministic replay harness — implemented

Before search, record a human input stream and confirm that it replays identically in both the
headless runner and graphical client. Compare state hashes at intervals and report the first
divergent tick with useful state differences.

### Stage 2: input-space search — implemented and extended to multiple targets

Use best-first search or A* over short action macros rather than branching over every possible
input on every frame. Example macros include holding left, right, or neutral for a small number
of ticks; pressing or releasing jump; and dashing in one of the supported directions.

Search keys include all future-relevant state:

- quantized position and velocity;
- grounded, wall-contact, and movement-mode state;
- jump-buffer, coyote, dash, and other resource state;
- room flags and collected items relevant to the route; and
- time modulo the cycle of deterministic moving hazards.

Pruning may merge equivalent or dominated states, but every pruning rule needs a regression test:
an over-aggressive equivalence relation can incorrectly label a valid room impossible. A success
result always contains the complete normalized input replay. `SearchTarget::Door` and
`SearchTarget::Pickup` name goals explicitly. `solve_targets` shares exploration among all targets
from one source door, then retains an independently replay-verifiable result for each goal.

### Stage 3: reachability graph

As the movement vocabulary stabilizes, discover useful surfaces, ledges, and interaction points.
Simulate movement primitives between them to build a room-level reachability graph. This should
make repeated validation faster and give better explanations than raw frame search while still
using core physics as its authority.

### Stage 4: useful difficulty estimates — first heuristic implemented

Solver effort alone is not a trustworthy measure of human difficulty. Collect several signals:

- duration and number of meaningful actions in the shortest known solution;
- number and diversity of viable routes;
- narrowest valid timing window;
- minimum clearance from lethal geometry;
- required consecutive precision actions or resource commitments;
- recovery opportunities after imperfect movement; and
- success when replay inputs are perturbed by one or two ticks.

The current curation score sums capped components for completion, input transitions, traversal
verbs, deaths, and temporal fragility. It subtracts the capped solver-effort component before using
fixed thresholds: 0–4 Gentle, 5–10 Standard, and 11+ Technical. This prevents search cost from
masquerading as player challenge, but the result still requires calibration against human attempts.
Until then the bands are diagnostics and selection strata, not claims about player experience.

### Stage 5: procedural validation

Build the eventual dungeon graph from room sockets and capability constraints: cycles, required
and optional paths, later keys or switches, and available ability sets. Instantiate each node from
the socket-indexed curated inventory, connect only opposite sides with identical aperture offsets
and spans, then validate transitions and graph objectives. Generated rooms retain exact
seed/strategy/intent/ability keys and replay witnesses so failures are reproducible.

## Testable invariants

These rules should become automated tests as their systems appear.

### Determinism and state

- The same content version, seed, initial state, and normalized inputs produce identical state
  hashes.
- Headless and rendered execution call the same simulation step and produce the same outcome.
- Reset restores an explicitly documented canonical room state. Any persistent flags are listed,
  not accidental.
- Seed and tick are included in crash, generation, validation, and solver diagnostics.

### Movement and collision

- The player does not tunnel through solid tiles at any legal velocity.
- Collision resolution never leaves the player embedded in solids.
- Spawn and transition destinations are neither solid nor immediately lethal.
- Jump buffer, coyote time, dash duration, and dash recharge obey exact tick boundaries.
- One-way platform behaviour is consistent while rising, falling, standing, and dropping through.

### Rooms and routes

- Every ordered pair of distinct doors is solver-reachable using the declared ability set.
- Every promised generated pickup is independently solver-reachable from a fresh state entered
  through every door using the declared ability set.
- A route intended for a later ability has a structural capability gate. A bounded unsuccessful
  search with a lower kit is never presented as proof of impossibility.
- Every ordered route has its own difficulty report and perturbation evidence; a room-level summary
  cannot replace the route matrix.
- Every selected socket has a geometrically matching mate in the same ability catalogue.
- All gameplay-relevant collision remains within the fixed room bounds.
- Every accepted generated room retains separate event-verified witnesses for all ordered door
  routes and pickup-from-door routes, plus compact representative replays for the client.

### Content construction

- Constructing First Steps repeatedly produces the same validated room definition and content
  digest.
- Regenerating an exact v6 key produces the same validated room definition and content digest.
- Invalid entity references and out-of-bounds geometry fail at the typed Rust construction
  boundary with useful context.
- Every mechanism field that affects future behaviour is represented in cloneable simulation
  state and included in reset and hashing semantics.

## Progression and validation contract

Traversal abilities are acquired during a run, but room mechanics must be testable before any
acquisition system exists. Each implemented validation scenario therefore pairs a room with an
explicit ability loadout. Human playtests and the solver use the same loadout and authoritative
Rust simulation. The same geometry can be validated several times—for example with baseline
movement, with wall jump, and with wall jump plus dash. Door and pickup targets are implemented,
including complete ordered-route matrices for generated rooms. Lower-kit failures remain
inconclusive unless backed by the generator's structural gate analysis.

Metaprogression may change which abilities are eligible to appear in early dungeon regions. The
future dungeon generator must treat that availability tier separately from the abilities already
held by a particular run.

Still-deferred design details include the health cost of a room death, how much success or pickups
restore, the precise metaprogression tiers, and the fiction/reward that motivates descent. None of
these blocks the current movement work.
