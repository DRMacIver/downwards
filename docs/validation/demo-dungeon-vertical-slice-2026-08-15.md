# Demo dungeon vertical slice — 2026-08-15

This slice tests the run-level seams that single-screen generation cannot: reciprocal boundary
doors, room-to-room entry points, persistent ability unlocks, loops and required branches in the
room graph, coin-gated progression, and an item-defined terminal goal.

Run it with:

```sh
cargo run -- --dungeon
```

The default launch resumes the version-bound atomic checkpoint at
`playtest-history/demo-dungeon-save-v1.json`. `--dungeon-new` archives that file before starting a
fresh run, and `--dungeon-save PATH` isolates a separate playthrough. Every persistent-state change
is read back after publication. Unknown schemas, stale authored-dungeon or palette versions,
duplicate/out-of-range coins, invalid unlock order, and unknown room/entry-door coordinates are
rejected rather than silently reset. The existing human-history JSONL also receives typed records
for coins, unlocks, sealed-door attempts, room transitions, and the Crown; deaths and full input
traces remain in its ordinary attempt records. Progress records use
`downwards-dungeon-progress-v2`; attempts use `downwards-human-attempt-v2`. Both bind the exact
dungeon-definition ID and palette generation.

The 101 room shells are deterministic `downwards-gen` palette outputs. Their graph is currently
assembled by trusted content rather than by a general topology generator:

```text
Hollow Landing ─ Moss Walk ─ Split Root ─ Broken Aqueduct ─ Old Lift ─ Lantern Gallery ─ Sluice ─ Climber's Reliquary
                              │                                      │
                         Root Cellar                           Sunken Watchpost
                                                                                         │
Stone Lessons ─ Broad Chimney ─ Bell Switchback ─ Evening Measure ─ Split Spire ─ Landing Chain ─ Needle Turn ─ Climber's Gate
                                │                                  │
                          The Bell Niche                       Rafter Shrine
                                                                                         │
                  Winged Vault ───── Rootbound Underpass ─ The Deep Treasury
                       │                       │
Threshold ─ Three-Way Hall ───────── Climbers' Gallery ─ Gale Chasm ─ Gale Landing ─ Low Passage ─ Forked Current
                       │                       │
                  Rafter Mint            Needle Belfry
                                                                                                      │
Pulse Gallery ─ Storm Split ─ Relay Chasm ─ Brake Tower ─ Dash Seal ─ Aerial Foundry (20 floors) ─ Glassworks (20 floors) ─ Astral Keep (20 floors) ─ Crown Gate ─ Crown
                    │
                Storm Cache
```

The run begins with ordinary movement and no traversal unlocks. Six coins distributed across both
Rootworks branches open the Climber's Reliquary; collecting the Climbing Gloves there enables Wall
Jump. The next ten floors are mandatory: two side branches hold coins, the Climber's Gate needs all
six regional coins, and its physical route has an observed multi-wall positive while an equivalent
bounded baseline search has no positive. The Winged Boots are another
room-local pickup whose stable ID is interpreted by client run state. Eighteen coins open only the
lower route. The Underpass coin is required for the Treasury, and both Treasury coins are required
before either Winged Vault entrance opens. The reward sits above a tightened two-tile version of
the gallery-calibrated alternating wall pattern: safe wall bands separated by inward-facing hazard
bands. Collecting the
boots grants Dash immediately and omits the pickup on later visits. Horizontal Dash uses a low
posture in the current movement policy, so an eight-pixel body can traverse a ten-pixel tunnel that
the standing twelve-pixel body cannot walk through, then expands automatically once headroom
returns. The post-Treasury path is now a mandatory ten-floor Dash course rather than a short route
to the Crown. Its two side branches contribute required coins; the final Dash Seal requires all six
regional coins and has a physical low-passage witness with an accepted Dash.

The Winged Boots and Crown use a dedicated nearest-neighbour 16-pixel pickup sheet rather than
the earlier debug rectangles. The Crown trigger has no generic exit frame drawn over it, so the
item itself remains the final room's visual goal.

Sixty-four stable coin IDs are distributed across the graph. Their 128-bit collection mask persists
across room reconstruction and is shown in both HUD rails. Exactly six are available before the
Climbing Gloves. Six more are distributed through the mandatory Wall-Jump course, including both
branches; all twelve are needed to leave it. Another six are available in the Threshold, Three-Way
Hall, Rafter Mint, Climbers' Gallery, and Needle Belfry; collecting all eighteen is required to
enter the lower loop. The Underpass contributes coin nineteen and opens the Treasury. Its two coins
bring the inventory to twenty-one and unseal the Winged Vault; the vault's final pre-Dash coin makes
twenty-two. Thus every floor is mechanically critical: removing any one floor prevents the Crown
inventory and traversal contract from being satisfied. All six Dash-region coins are then required
at the Dash Seal, producing the 28-coin Foundry entry inventory. Twelve Foundry coins produce the
40-coin Foundry-exit inventory. The mandatory Glassworks then adds three more branches and twelve
coins, producing its 52-coin exit inventory. The final mandatory Astral Keep adds another three
branches and twelve coins, producing the full 64-coin Crown inventory. A rejected door
returns the player to its validated interior arrival without resetting room-local progress. The
Crown similarly persists and is the only terminal goal. Ordinary door exits change rooms and are
deliberately not counted as whole-level victories.

Palette generation v12 contains two evidence-driven room replacements. Persistent history recorded
24 Gale Chasm attempts, all ending on spikes, while its old AI witness used 43 action spans and 11
horizontal reversals. As the first post-Boots room, it now teaches the action as two readable
sixty-pixel Dash gaps separated by a three-tile full-recovery island. Its mechanically selected
route has nine spans, two jumps, two accepted Dashes, and no reversals; a bounded Wall-Jump-only
solve has no positive. The Astral Keep's former Void Pass copy is now a distinct authored
wall-rhythm course. Four alternating wall-contact bands have upward-lethal top caps: the vertical
faces remain valid Wall-Jump contacts, while the horizontal surfaces cannot refill Dash. Its frozen
route reaches the east door in 224 ticks with five accepted Wall Jumps and seven horizontal
reversals. A bounded Dash-only solve under the same policy has no positive. These are no-known-
bypass audits, not claims of physical impossibility.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, full standing
headroom over every authored one-way surface, unique persistent coins, all coin and method gates,
persistent item omission, additive mid-run Wall Jump and Dash state, gate rejection without room reset, client
traversal of the intended loop, and authoritative solver positives for every critical leg and coin
branch. The retained boots route now takes 111 ticks, eight accepted jumps, four wall jumps, and at
least two rapid wall-side changes through two-tile contact windows. A WallJump-only search misses
the chasm under the same bounded search budget while the
post-boots loadout succeeds. That miss is evidence for this vertical slice, not a proof of physical
impossibility. The ten opening-region representative routes also retain at least one observed
success in every applicable strength-one blind input-perturbation family under a small deterministic
study. Beyond the Dash Seal, the mandatory twenty-floor Aerial Foundry adds three coin branches and
twelve unique coins. Its layouts deliberately mix the two unlocked methods; the Foundry Seal's
known exact route contains accepted Wall Jumps and Dashes, while a Wall-Jump-only solve under the
same bounded policy has no positive. All forty coins are required before leaving this act. The
following mandatory twenty-floor Glassworks repeats that contract with three distinct branches and
twelve further coins. Its Glass Seal exact route also contains both accepted Wall Jumps and Dashes;
the equivalent Wall-Jump-only bounded solve has no positive. All fifty-two coins are required
before leaving the Glassworks. The final mandatory twenty-floor Astral Keep has three further coin
branches and twelve coins. Its Astral Seal exact route again contains both accepted Wall Jumps and
Dashes, while the equivalent Wall-Jump-only bounded solve has no positive. All sixty-four coins are
required there and again at the Crown gate.

The mechanically generated per-floor witness artifact records exact routes and every
applicable strength-one outcome, including explicit zero-success blind-continuation families rather
than hiding them. The final Dash Seal's exact positive uses Dash while an
equivalent WallJump-only search has no positive. Those observations are controller diagnostics, not
a scalar difficulty or human-robustness claim.

The perturbation seed is derived from stable room identity, so inserting a floor cannot silently
change earlier observations. The current policy uses 64 trials per curve point rather than the
original eight. This does not turn blind continuation into adaptive play, and nonzero success is
not a human difficulty score.

This does not claim a strong dungeon generator or calibrated whole-run difficulty. It is a
playable integration prototype intended to expose graph, pacing,
unlock, and room-transition problems before generalising the generator.

Regenerate or verify the exact route artifact with:

```sh
cargo run -p downwards-content --example retune_demo_dungeon
cargo run -p downwards-content --example retune_demo_dungeon -- --check
cargo run -p downwards-content --example retune_demo_dungeon -- --route demo-dungeon.void-pass
```

The authoring tool considers the previous checked-in witness, finite direct-controller positives,
and explicitly registered segmented route candidates. Every candidate is replay-verified and
mechanically simplified before action shape is compared lexicographically. A replacement is only
selected when all four applicable strength-one perturbation families retain a success; otherwise
the previous exact robust witness remains eligible. This prevents a shorter but newly brittle AI
trace from silently becoming the dungeon's displayed route. The targeted `--route` form prints the
candidate observations without publishing a partial 101-floor artifact.

For iterative playtesting, render a cheap descriptive audit without rerunning the solvers:

```sh
cargo run -p downwards-content --example audit_demo_dungeon
```

The audit consumes the frozen witness artifact and
`playtest-history/human-attempts-v1.jsonl` (or one explicitly supplied JSONL path). For every floor
it keeps AI action shape, strength-one shaky outcomes, human attempts, and nearest tile geometry as
separate columns. `AI-BUSY`, `AI-REVERSING`, `HUMAN-NO-SUCCESS`, and tile-copy labels only nominate
rooms for inspection; they are intentionally not combined into a difficulty scalar. This boundary
exists because earlier route-complexity and blind-continuation metrics substantially overstated the
difficulty of trivial human routes. Human attempts join the current room only when both the
movement-policy version and initial state digest match. Old geometry/policy attempts and unbound v1
progress rows remain counted under `STALE-HUMAN`, but cannot produce current-room success or failure
flags.
