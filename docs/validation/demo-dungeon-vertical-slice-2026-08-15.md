# Demo dungeon vertical slice — 2026-08-15

This slice tests the run-level seams that single-screen generation cannot: reciprocal boundary
doors, room-to-room entry points, a persistent ability unlock, loops and optional branches in the
room graph, coin-gated progression, and an item-defined terminal goal.

Run it with:

```sh
cargo run -- --dungeon
```

The eleven room shells are deterministic `downwards-gen` palette outputs. Their graph is currently
assembled by trusted content rather than by a general topology generator:

```text
                  Winged Vault ───── Rootbound Underpass ─ Eight-Coin Treasury
                       │                       │
Threshold ─ Three-Way Hall ───────── Climbers' Gallery ─ Gale Chasm ─ Ten-Coin Gate ─ Crown
                       │                       │
                  Rafter Mint            Needle Belfry
```

The run begins with Wall Jump and without Dash. The Winged Boots are a room-local pickup whose
stable ID is interpreted by client run state. Their floor door is sealed until six coins have been
collected, and the reward sits above the gallery-calibrated “Even Tempo” pattern: alternating safe
wall bands separated by inward-facing hazard bands. Collecting the boots grants Dash immediately
and omits the pickup on later visits. Horizontal Dash uses a low posture in the current movement
policy, so an eight-pixel body can traverse a ten-pixel tunnel that the standing twelve-pixel body
cannot walk through, then expands automatically once headroom returns.

The Winged Boots and Crown use a dedicated nearest-neighbour 16-pixel pickup sheet rather than
the earlier debug rectangles. The Crown trigger has no generic exit frame drawn over it, so the
item itself remains the final room's visual goal.

Ten stable coin IDs are distributed across the graph. Their collection mask persists across room
reconstruction and is shown in both HUD rails. Exactly six are available in the Threshold,
Three-Way Hall, Rafter Mint, Climbers' Gallery, and Needle Belfry; collecting all six is therefore
required to enter the Winged Vault or lower loop. The vault and Underpass contribute the seventh
and eighth, opening the Treasury. Its final two coins open the ten-coin Gatehouse. A rejected door
returns the player to its validated interior arrival without resetting room-local progress. The
Crown similarly persists and is the only terminal goal. Ordinary door exits change rooms and are
deliberately not counted as whole-level victories.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, full standing
headroom over every authored one-way surface, unique persistent coins, all four gate requirements,
persistent item omission, additive mid-run Dash state, gate rejection without room reset, client
traversal of the intended loop, and authoritative solver positives for every critical leg and coin
branch. The retained boots route contains at least four alternating wall jumps and no repeated-side
wall hops. A WallJump-only search misses the chasm under the same bounded search budget while the
post-boots loadout succeeds. That miss is evidence for this vertical slice, not a proof of physical
impossibility.

This does not claim a strong dungeon generator, calibrated whole-run difficulty, or durable
save-game persistence. It is a playable integration prototype intended to expose graph, pacing,
unlock, and room-transition problems before generalising the generator.
