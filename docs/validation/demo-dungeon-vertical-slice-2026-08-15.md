# Demo dungeon vertical slice — 2026-08-15

This slice tests the run-level seams that single-screen generation cannot: reciprocal boundary
doors, room-to-room entry points, persistent ability unlocks, loops and required branches in the
room graph, coin-gated progression, and an item-defined terminal goal.

Run it with:

```sh
cargo run -- --dungeon
```

The 31 room shells are deterministic `downwards-gen` palette outputs. Their graph is currently
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
Threshold ─ Three-Way Hall ───────── Climbers' Gallery ─ Gale Chasm ─ The Crown Gate ─ Crown
                       │                       │
                  Rafter Mint            Needle Belfry
```

The run begins with ordinary movement and no traversal unlocks. Six coins distributed across both
Rootworks branches open the Climber's Reliquary; collecting the Climbing Gloves there enables Wall
Jump. The next ten floors are mandatory: two side branches hold coins, the Climber's Gate needs all
six regional coins, and its physical route has an observed multi-wall positive while an equivalent
bounded baseline search has no positive. The Winged Boots are another
room-local pickup whose stable ID is interpreted by client run state. Their floor door is sealed
until eighteen coins have been collected, and the reward sits above the gallery-calibrated “Even
Tempo” pattern: alternating safe wall bands separated by inward-facing hazard bands. Collecting the
boots grants Dash immediately and omits the pickup on later visits. Horizontal Dash uses a low
posture in the current movement policy, so an eight-pixel body can traverse a ten-pixel tunnel that
the standing twelve-pixel body cannot walk through, then expands automatically once headroom
returns.

The Winged Boots and Crown use a dedicated nearest-neighbour 16-pixel pickup sheet rather than
the earlier debug rectangles. The Crown trigger has no generic exit frame drawn over it, so the
item itself remains the final room's visual goal.

Twenty-two stable coin IDs are distributed across the graph. Their 128-bit collection mask persists
across room reconstruction and is shown in both HUD rails. Exactly six are available before the
Climbing Gloves. Six more are distributed through the mandatory Wall-Jump course, including both
branches; all twelve are needed to leave it. Another six are available in the Threshold, Three-Way
Hall, Rafter Mint, Climbers' Gallery, and Needle Belfry; collecting all eighteen is therefore
required to enter the Winged Vault or lower loop. The vault and Underpass contribute the nineteenth
and twentieth, opening the Treasury. Its final two coins open the 22-coin Crown Gate. A rejected door
returns the player to its validated interior arrival without resetting room-local progress. The
Crown similarly persists and is the only terminal goal. Ordinary door exits change rooms and are
deliberately not counted as whole-level victories.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, full standing
headroom over every authored one-way surface, unique persistent coins, all coin and method gates,
persistent item omission, additive mid-run Wall Jump and Dash state, gate rejection without room reset, client
traversal of the intended loop, and authoritative solver positives for every critical leg and coin
branch. The retained boots route contains at least four alternating wall jumps and no repeated-side
wall hops. A WallJump-only search misses the chasm under the same bounded search budget while the
post-boots loadout succeeds. That miss is evidence for this vertical slice, not a proof of physical
impossibility. The ten opening-region representative routes also retain at least one observed
success in every applicable strength-one blind input-perturbation family under a small deterministic
study. Those observations are controller diagnostics, not a scalar difficulty or human-robustness
claim.

This does not claim a strong dungeon generator, calibrated whole-run difficulty, or durable
save-game persistence. It is a playable integration prototype intended to expose graph, pacing,
unlock, and room-transition problems before generalising the generator.
