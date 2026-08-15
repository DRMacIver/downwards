# Demo dungeon vertical slice — 2026-08-15

This slice tests the run-level seams that single-screen generation cannot: reciprocal boundary
doors, room-to-room entry points, a persistent ability unlock, a loop in the room graph, and an
item-defined terminal goal.

Run it with:

```sh
cargo run -- --dungeon
```

The seven room shells are deterministic `downwards-gen` palette outputs. Their graph is currently
assembled by trusted content rather than by a general topology generator:

```text
Threshold ─ Crossroads ─ Wall Gallery ─ Gale Chasm ─ Crown Sanctum
               │              │
          Boots Vault ─── Underpass
```

The run begins with Wall Jump and without Dash. The Winged Boots are a room-local pickup whose
stable ID is interpreted by client run state; collecting them grants Dash immediately and omits
the pickup on later visits. The Crown similarly persists and is the only terminal goal. Ordinary
door exits change rooms and are deliberately not counted as whole-level victories.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, persistent item
omission, additive mid-run Dash state, client traversal of the intended loop, and authoritative
solver positives for every leg of the intended route. A WallJump-only search misses the chasm
under the same bounded search budget while the post-boots loadout succeeds. That miss is evidence
for this vertical slice, not a proof of physical impossibility.

This does not claim a strong dungeon generator, calibrated whole-run difficulty, or durable
save-game persistence. It is a playable integration prototype intended to expose graph, pacing,
unlock, and room-transition problems before generalising the generator.
