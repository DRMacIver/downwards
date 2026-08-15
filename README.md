# Downwards

Downwards is an early-stage, no-combat roguelike metroidvania platformer. The player
descends through a procedurally assembled dungeon of single-screen rooms, choosing among
multiple routes and overcoming movement challenges, traps, and environmental obstacles.

The intended feel draws on the precise, quick-reset platforming of *Celeste*, *MoneySeize*,
and *Super Meat Boy*, while the eventual dungeon structure takes inspiration from
*Unexplored*: authored rules and meaningful loops first, procedural arrangement second.
Those games are references for design principles, not sources for art or other assets.

## Project status

The repository contains a playable single-screen mechanics and procedural-generation lab. Its
deterministic Rust simulation implements run/jump movement, coyote time and buffering, wall
slide/jump, an optional eight-direction dash, solid and one-way collision, static and periodic
hazards, collectibles, boundary doors, and instant canonical retries. Generator v6 produces
fixed-screen rooms for four explicit ability loadouts without selecting from whole-room
templates. The manually designed First Steps mechanics room is also constructed directly in
Rust, so generated and authored content use the same validated core types.

A v6 room is a tile in the future dungeon rather than a challenge with one privileged exit. It
has two to four doors on its walls, ceiling, or floor. Validation enters through each door and
requires a replay-verified route to every other door, in both directions, plus a route to every
pickup from every entrance. Matching `DoorSocket` values—opposite boundary sides with the same
offset and aperture span—make the room inventory suitable for later dungeon tiling.

Three independent constructive strategies are under comparison: cyclic-graph rewriting,
bottom-up reachability growth, and movement-rhythm weaving. Wall-jump and dash profiles contain
structural ability gates. At catalogue level, the selected representative routes must cover wall
jump for T2, dash for T3, and both abilities for T4; T4 prefers one route that demonstrates both.
The AI shares one multi-target exploration for all door and pickup goals from a source door, then
replay-verifies every positive certificate against the authoritative simulation.

The normal desktop browser now uses four small, checked, offline-curated catalogues instead of the
legacy 1,000-seed list. Each kit has nine rooms—three per route band—selected after overgeneration
across seeds, strategies, and challenge intents. Curation rejects infeasible, duplicate, fragile,
or unmatchable candidates and selects a varied set across quality-diversity niches. Each browser
row represents one ordered source-door-to-target-door challenge. Explicit raw-seed generation
remains available as uncurated developer content, not as a certification claim.

This remains a prototype rather than a complete game. The raw v6 diversity gate found 1,000
distinct static visuals, tile fields, collision topologies, and route-plan signatures in seeds
0–999 for each of the four loadouts, with no repeated descriptor. Pairwise and nearest-neighbour
tile-distance gates guard against superficial one-cell variation. This is an expressive-range
result, not a solvability claim; only rooms admitted to a checked manifest are playable catalogue
content. See the
[`v6 validation and curation report`](docs/validation/generator-v6-2026-08-14.md). The
[`v5`](docs/validation/generator-v5-2026-08-14.md) and
[`v4`](docs/validation/generator-v4-2026-08-14.md) reports remain as explicitly historical
single-exit evidence.

The final v6 manifests contain 36 keyed room challenges—three per heuristic band in each kit—and
31 distinct static layouts across kits, with all three strategies represented. Their 164 ordered
door routes and 93 pickup-from-door routes are certified, every selected socket has a mate,
runtime regeneration/replay checks pass, and a second curation run reproduced every manifest byte.
They remain explicitly versioned solver-policy-v2 fixtures after the solver's v3 route-composition
improvement: the loader accepts that historical identity and still regenerates and exactly replays
the stored demonstrations, but does not relabel the checksum-bound offline matrix evidence as v3.

Session-only human statistics, health/run economy, metaprogression, multi-room dungeon assembly,
final art/audio, and human calibration of the difficulty heuristic remain separate work. Current
Gentle/Standard/Technical labels describe a deterministic route-ranking heuristic, not measured
human difficulty.

See [the vertical-slice design](docs/design/vertical-slice.md) for the milestone, implementation
order, solver plan, invariants, and open product questions.

The active procedural-generation work is specified in the
[diverse room corpus research plan](docs/research/corpus-plan.md). It targets 500–1,000 distinct
multi-door rooms, evaluates difficulty per directed route and ability loadout, adds reproducible
imperfect-input testing, prioritizes route diversity, and stages obstacle complexity only after
the existing vocabulary is placed convincingly. It also records the correctness blockers that
must be resolved before the current small catalogue is replaced.

## Technical direction

- Rust 1.92, edition 2024
- [Macroquad 0.4.15](https://docs.rs/macroquad/0.4.15/macroquad/) for the playable client
- Rust-owned procedural generation, authored fixtures, and deterministic mechanisms
- A deterministic, fixed-step simulation that can run without graphics or audio
- A small logical resolution scaled up with nearest-neighbour rendering for minimalist pixel art

The current prototype uses a 320×180 logical room and a 60 Hz integer-subpixel simulation. Those
choices are deliberately easy to tune after movement playtests. There is no runtime scripting
engine: First Steps and generated rooms construct the same validated Rust room model. A data or
scripting boundary can be introduced later if concrete authoring or modding requirements justify
its schema, determinism, and maintenance costs. This reversal is recorded in
[ADR 0003](docs/decisions/0003-rust-owned-content.md).

## Workspace map

| Crate | Responsibility | Current state |
| --- | --- | --- |
| `downwards-core` | Renderer-independent room state, fixed-step movement/collision, semantic input, events, and stable hashes | Current movement and room-object vocabulary implemented |
| `downwards-content` | Trusted built-in Rust rooms shared by the client, tools, and regression tests | First Steps plus a fifteen-room authored no-Dash calibration and movement gallery |
| `downwards-gen` | Stable v6 compositional generation for explicit ability loadouts | Cyclic graph, reachability growth, and rhythm weave strategies implemented behind exact regeneration keys |
| `downwards-ai` | Headless solving, shared multi-target search, replay validation, and route observations | Door/pickup targets, event-verified witnesses, and provisional heuristic metrics implemented |
| `downwards-validation` | Typed objectives and owned positive certificates | Every ordered door pair and every pickup-from-door objective can be certified in one shared search per entrance |
| `downwards-catalogue` | Strict runtime loading of offline-curated manifests | Checks versions, checksum, identities, sockets, regenerated geometry, fingerprints, and representative replays |
| `downwards-client` | Macroquad window, rendering, human input, debug overlays, recording, and replay viewing | Curated-room browser integration and the mechanics lab |
| `downwards-tools` | CLI entry point for built-in and generated-room diagnostics | First Steps solving plus reproducible v4/v5 developer commands |

`downwards-lab` and `downwards-research` are standalone support crates. The lab defines canonical
visual, collision, traversal, and semantic-action descriptors; the research harness runs sweeps
and deterministic offline curation without becoming a runtime dependency.

The important dependency rule is that `downwards-core` stays independent of rendering, audio,
and windowing. Both a human-controlled client and the AI drive the same Rust simulation through
the same per-tick input type. Generated rooms, authored fixtures, and future mechanisms must
preserve that cloneable, hashable authority boundary.

## Development

The pinned toolchain is selected automatically by `rustup` when commands are run in the
repository.

Commands that work with the current scaffold:

```sh
cargo run
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

To launch either fixed, hand-authored WallJump-only challenge with Dash locked off:

```sh
cargo run -- --challenge hard
cargo run -- --challenge tutorial
```

Bare `--challenge` remains an alias for `--challenge hard`; `--challenge medium` remains a
compatibility alias for the tutorial. Both challenges are deliberately outside the generated
corpus and catalogues. Press `V` to view the selected room's exact tractability witness. Their
labels come from human playtesting; exact replays and finite no-known-bypass audits are not human
difficulty scores. The motivation, evidence, and limits are recorded in the
[`no-Dash human-calibration report`](docs/validation/hard-no-dash-human-calibration-2026-08-15.md).

To open the fifteen-room authored calibration and movement gallery:

```sh
cargo run -- --gallery
```

The gallery browser shows stable `cal-01` through `cal-15` IDs, titles, and the mechanic varied by
each room; the IDs are not a difficulty ranking. Use the arrows or WASD to select a room, Enter to
play it, or `V` to play its mechanically generated exact tractability witness. During play, `[` and `]` move
directly to the previous or next gallery room and `M` returns to the gallery browser. Dash is
locked off for every entry. Jump height is variable: tap or release Jump early for a low jump and
hold it for a high jump. Human jump-input policy v4 classifies an ordinary ground-jump release
within 100 ms as one low-jump gesture, while wall-jump presses respond immediately. A live wall
jump remembers recent contact, commits its initial ascent, and briefly preserves its push away
from the wall. The final three entries are non-lethal movement courses for horizontal momentum,
wall-jump rhythm, and mixed transitions; missed jumps fall onto recovery terrain. The low-ceiling gallery
rooms are calibration experiments, not endorsed examples of robust clearance yet. The witness
uses the same movement and held-Jump input available to a human player. Record results in the
[`human calibration gallery feedback plan`](docs/research/human-calibration-gallery-plan.md).

The first resumed generator pilot is deliberately separate from both that authored gallery and
the production corpus. It contains twelve deterministic WallJump-only/no-Dash rooms built from
the human-calibrated grammar: short turns, even-tempo climbs, recovery ascents, rising causeways,
and visible low-bridge jump cuts. Launch seed 0 (or any seed 0–11) with:

```sh
cargo run -- --calibrated 0
```

During play, `[` and `]` cycle the twelve generated keys, `V` plays the mechanically simplified
exact witness, and `M` opens the ordinary level browser. Regenerate the witness artifact after a
movement-policy or generator change with:

```sh
cargo run --release -p downwards-content --example retune_calibrated_generator
```

That process performs the solver/simplifier and ability-removal checks; tests consume its output
rather than hard-coding route lengths. The retained route facts are acceptance evidence, not a
difficulty score. See the
[`calibrated WallJump generator v2 report`](docs/validation/calibrated-wall-jump-generator-v2-2026-08-15.md).

To play the first multi-room dungeon vertical slice:

```sh
cargo run -- --dungeon
```

This is an expanding hand-authored dungeon built from deterministic room-palette starting points,
not a claim that a general dungeon generator can design the finished game. It currently contains
81 connected floors and 52 persistent coins. The opening Rootworks region begins without traversal
powers: both coin branches are required to enter the Climber's Reliquary, whose pickup unlocks Wall
Jump. A mandatory ten-floor Wall-Jump region follows, with two required coin branches and a final
physical Wall-Jump gate; its six coins are needed before the older halls can be entered. The next
six branch coins no longer open the Winged Vault directly: eighteen coins expose the lower route,
the Underpass coin opens the Treasury, and both Treasury coins are required to unseal either vault
entrance. The vault's final pre-Dash coin and two-tile alternating hazard-band climb then award
Dash. This makes every pre-Dash branch part of the critical path. A mandatory ten-floor Dash region
with two required branches follows. Six regional coins open a physical low-posture Dash seal and
the twenty-floor Aerial Foundry beyond it. The Foundry has three required coin branches, twelve new
coins, mixed Wall-Jump/Dash rooms, and a regional seal whose known positive uses both methods. A
mandatory twenty-floor Glassworks follows, adding three more required branches, twelve coins, and
a second mixed-method seal. All fifty-two coins are required at that seal and again at the Crown
gate. The Crown ingress explicitly
requires both unlocked traversal
methods as well as the coins. Deaths and restarts return to the door used to enter the current room
without discarding gloves, boots, Crown, or coins. `V` demonstrates the next intended room-local
objective; it is not a whole-dungeon route.

Dungeon route evidence is regenerated rather than edited into tests by hand:

```sh
cargo run -p downwards-content --example retune_demo_dungeon
cargo run -p downwards-content --example retune_demo_dungeon -- --check
```

The checked-in artifact contains one exact route and 64-trial strength-one input-perturbation
observations for every floor under the current movement and palette policies. These are
tractability and controller-behaviour records, not difficulty scores.

In any human-controlled room, press `F2` to open the movement-tuning menu. It directly adjusts top
speed in pixels/second, acceleration and braking response in milliseconds, and wall-momentum
behavior along three axes: Wall Ascent converts a rising wall impact into additional upward speed,
Wall-Jump Boost reflects incoming speed into the outward launch, and Wall Memory controls the
post-contact grace duration. The starting 110 px/s, 72 ms acceleration, and 203 ms braking values
are the latest human-selected settings; both wall percentages start at a visible 50%. Applying a change restarts the room so attempts
do not mix physics policies. Every applied change is
appended immediately as a `downwards-movement-tuning-v1` event in the human-attempt JSONL history,
and subsequent attempt rows include the same exact values. These defaults apply game-wide to live
play and new AI solves. The selected tuning and current calibration caveats are recorded in the
historical [`player movement policy v2 report`](docs/validation/player-movement-policy-v2-2026-08-15.md);
the current explicit directional-spike contract is recorded in the
[`player movement policy v3 report`](docs/validation/player-movement-policy-v3-2026-08-15.md).

Gallery witnesses are generated rather than copied into hand-maintained tests. After a movement or
authored-room change, update and verify the single policy-bound artifact with:

```sh
cargo run -p downwards-content --example retune_gallery
cargo run -p downwards-content --example retune_gallery -- --check
```

`cargo run` opens the level browser. Its normal generated entries come from four separately
checked manifests, one per ability loadout, rather than from an arbitrary range of raw seeds. The
Rust-authored First Steps mechanics room remains a normal row in the same continuously scrolling
list. Every curated entry has a stable, unique three-word playtest name and retains its exact v6
regeneration key: seed, strategy, generation intent, and ability set.

The selected row has a cached miniature preview. Its detail panel identifies the curated
Gentle/Standard/Technical band, strategy and generation intent, door count and sockets, and the
ordered entrance/target pair being tested. The band belongs to that route: another direction
through the same multi-door room can have a different score.

Level-browser controls:

- Up/Down or W/S moves through the continuous list one room at a time;
- Page Up/Page Down jumps by one visible window, and Home/End moves to the first/last entry;
- Enter starts the selected room;
- 1–4 changes the ability loadout before starting;
- C starts the selected room and plays its representative pickup witness; and
- M or Escape closes the browser without changing the currently loaded room.

During play, A/D or arrows move and aim, Space/Z/Up jumps, and R retries. Release Jump early (or
tap it) for a low jump; keep holding Jump for a high jump. M or Escape reopens the level browser.
The live-input adapter preserves press/release order across render frames and normalizes a physical
tap into one semantic low-jump gesture. Ground jumps wait for either release within the 100 ms tap
window or a continued hold, so frame scheduling does not turn a quick tap into a taller jump.
Wall-jump presses are immediate: recent wall contact remains eligible briefly, a quick tap receives
a useful minimum launch, and steering back toward the old wall cannot instantly cancel the outward
kick. Holding continues to expose the full variable-height window.
The HUD explicitly reports which traversal abilities the selected kit provides:
T1 is baseline movement, T2 adds wall jump, T3 adds dash, and T4 enables both. To wall-jump in T2
or T4, push into a wall and jump. In T3 or T4, X/Shift plus a held direction produces a short
eight-direction burst that overrides normal movement; landing recharges it. Locked abilities are
labelled explicitly, so T1 correctly ignores dash and wall-jump input. After a human-controlled
completion of the selected target route, Enter advances directly to the next curated level in the
same kit. Reaching a different boundary door does not silently count as completing the selected
route. The HUD and room-identity footer use rails outside the 320×180 room instead of covering
ceiling or floor doors, hazards, or the player.

The playtest lab also retains these expert shortcuts:

- `[`/`]` moves to the previous/next curated entry, 1–4 changes the explicit ability loadout, and
  Tab switches between curated content and the Rust-authored First Steps room. In explicit
  developer mode, `[`/`]` changes the raw seed;
- F1 shows debug geometry and movement state;
- V runs the AI to the selected target under the current movement policy;
- C runs the AI to the selected coin under the current movement policy;
- during replay, P pauses, N frame-steps, and Escape returns to human control; and
- H replays the latest successful (or latest completed) recorded human attempt.

The browser preview and clear panel show session-only human statistics for each room and kit:
completed runs, deaths, clears, coins collected, and fastest clear. Manual resets count as
completed runs, but AI witnesses and playback of recorded human attempts never increment these
figures. These summary counters remain session-only.

Completed human attempts are also appended persistently to
`playtest-history/human-attempts-v1.jsonl` (override with `--history PATH`). Each JSONL row contains
the room and outcome, player-movement policy version and exact tuning, per-tick actions and
state/event digests, accepted jump events, raw
Space/Z/Up press samples, and the contiguous Jump spans delivered to physics. Raw durations use
render-frame sampling because the window library does not expose operating-system key-event
timestamps: `sampled_duration_us` and `sampled_frames_held` describe what the UI observed, while
`delivered_jump_spans[].ticks` records what the fixed-step simulation actually received. The file
is flushed after every death, manual reset, success, or wrong-door completion.

Raw v6 seed generation is intentionally a developer-only path. It tries the nine
strategy/intent compositions in deterministic order but performs no reachability or difficulty
assessment. A raw result must never be presented as curated merely because it constructed.
Launch options are `--seed <u64>` for that explicit developer mode,
`--tier <1|2|3|4>`, `--development`, `--generated`, and `--history PATH` to override the persistent
human-attempt log location.

### Playing the room corpus

The corpus selection has a separate, strict playtest bridge because its rooms come from several
native generator families rather than the historical v6 manifest format. Generate the compact
manifest once from the final-recomputed selection, then launch the ordinary game client:

```sh
crates/downwards-research/target/release/corpus_v3_offline_selection \
  export-playtest content/corpora/v1/cache content/corpora/v1/shards \
  content/corpora/v1/selection-final content/corpora/v1/playtest.manifest
cargo run -- --corpus content/corpora/v1/playtest.manifest
```

The default export begins with the first 64 rooms in deterministic quality-diversity selection
order, then adds only the rooms needed to close their boundary sockets. Each row keeps its full
native regeneration key, socket inventory, construction loadout, and a replay-certified
source-to-target witness; it does not copy room geometry into another format. The client checks
the manifest fingerprint, exact-regenerates every row, checks sockets and the entry-state digest,
replays the representative positive, and compares its per-frame runtime checksum before exposing
the list. Normal export first reruns the full final-selection verifier; it does not trust the
selection's publication label alone.

`export-playtest` rejects provisional selections. For an explicitly non-production inspection,
use `export-playtest-dev-provisional` and launch with `--allow-provisional-corpus`; both opt-ins are
required, so a provisional operational cache cannot silently become normal playable content.

The checked-in production manifest currently contains 68 rooms (64 requested plus four
different-room socket mates), all with authored route witnesses. Its SHA-256 is
`a32163af520fc77d512f875bc6d5463ddbe340b84d9cb8cd42f7af44038bace5`.

The current headless commands are:

```sh
# Search the Rust-authored First Steps room and verify its in-memory replay witness.
cargo run -p downwards-tools -- solve-first-steps

# Print and solve one generated candidate for a loadout.
cargo run -p downwards-tools -- generate 42 wall

# Validate a consecutive batch. Every room must have a verified exit witness and a
# separate verified witness for every pickup; the tool prints aggregate fingerprints.
cargo run -p downwards-tools -- validate-seeds 0 100 baseline

# Compare v6 strategies under the all-pairs door contract.
cargo run --manifest-path crates/downwards-research/Cargo.toml --release -- \
  sweep 0 100 all baseline all

# Overgenerate and emit one strict manifest. This fails rather than relaxing a quota.
cargo run --manifest-path crates/downwards-research/Cargo.toml --release -- \
  curate 0 16 3 baseline
```

The `downwards-tools` generated commands above exercise the historical single-exit generator and
remain useful for reproducing v4/v5 reports. The research commands exercise v6.

## First milestone

The first vertical slice is deliberately one screen rather than a miniature full game. It
should provide:

- responsive running and variable-height jumping—release Jump early for low, hold it for
  high—including input buffering and coyote time;
- wall slide/jump and a rechargeable directional dash when the selected loadout enables them;
- solid tiles, one-way platforms, spikes or kill volumes, and one timing obstacle;
- two to four safe boundary doors and an optional harder route or collectible;
- near-instant deterministic room reset after failure;
- collision, movement-state, seed, and input debug overlays;
- input recording and deterministic replay; and
- a headless solver that finds every ordered door-to-door route and every promised pickup from
  every door, producing replays the client can play back.

Dungeon generation, metaprogression, final art, narrative, broad content production, and run
economy are outside this milestone.

## Confirmed product direction

| Area | Direction |
| --- | --- |
| Platforms and input | Desktop and keyboard first. Browser and polished controller support are deferred. |
| Traversal abilities | Abilities such as dash are discovered during a run. Mechanics tests provide an explicit ability loadout instead of simulating acquisition. |
| Wall movement | Begin with wall slide and wall jump. Do not add stamina yet; active climbing can be evaluated later. |
| Prototype failure | Infinite, near-instant room retries while movement and level design are being developed. |
| Eventual run failure | Room deaths reduce a health bar; reaching zero ends the run. Success and/or pickups may restore health, with exact values deferred. |
| Progression | Traversal abilities are acquired per run. Metaprogression may control which abilities can appear early in a run. |
| Content and mechanisms | Procedural rooms, authored fixtures, traps, and authoritative behaviour live in Rust. Add an external data or scripting boundary only in response to concrete authoring or modding needs. |
| Presentation | The working target is a 320×180, 16:9 logical canvas under the working title *Downwards*. |
