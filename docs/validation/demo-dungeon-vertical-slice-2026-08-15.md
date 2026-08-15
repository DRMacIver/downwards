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
                    Rafter Mint          Needle Belfry
                         │                    │
Threshold ─ Three-Way Hall ─ Climbers' Gallery ─ Gale Chasm ─ Six-Coin Gate ─ Crown
                         │                    │
                 Winged Vault ───── Rootbound Underpass ─ Three-Coin Treasury
```

The run begins with Wall Jump and without Dash. The Winged Boots are a room-local pickup whose
stable ID is interpreted by client run state. They are placed after a partitioned, multi-jump
switchback rather than beside the entrance; collecting them grants Dash immediately and omits the
pickup on later visits. Horizontal Dash uses a low posture in the current movement policy, so an
eight-pixel body can traverse a ten-pixel tunnel that the standing twelve-pixel body cannot walk
through, then expands automatically once headroom returns.

The Winged Boots and Crown use a dedicated nearest-neighbour 16-pixel pickup sheet rather than
the earlier debug rectangles. The Crown trigger has no generic exit frame drawn over it, so the
item itself remains the final room's visual goal.

Ten stable coin IDs are distributed across the graph. Their collection mask persists across room
reconstruction and is shown in both HUD rails. The east Underpass door rejects the player until
three coins have been collected; the final Gatehouse door requires six. A rejected door returns
the player to its validated interior arrival without resetting room-local progress. The Crown
similarly persists and is the only terminal goal. Ordinary door exits change rooms and are
deliberately not counted as whole-level victories.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, full standing
headroom over every authored one-way surface, unique persistent coins, both gate requirements,
persistent item omission, additive mid-run Dash state, gate rejection without room reset, client
traversal of the intended loop, and authoritative solver positives for every critical leg and coin
branch. The retained boots route contains a genuine multi-jump climb. A WallJump-only search misses
the chasm under the same bounded search budget while the post-boots loadout succeeds. That miss is
evidence for this vertical slice, not a proof of physical impossibility.

This does not claim a strong dungeon generator, calibrated whole-run difficulty, or durable
save-game persistence. It is a playable integration prototype intended to expose graph, pacing,
unlock, and room-transition problems before generalising the generator.
