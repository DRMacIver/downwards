# Player movement policy v2

Status: promoted as the game-wide player-facing default on 2026-08-15.

## Selected values

The values below are the final state left in the in-game tuning menu after human playtesting:

| Control | Default |
| --- | ---: |
| Top speed | 110 px/s |
| Acceleration response | 72 ms |
| Braking/reversal response | 203 ms |
| Rising wall-impact carry | 50% |
| Wall-jump speed carry | 50% |
| Recent-wall memory | 250 ms |

`MovementTuning::GAMEPLAY_DEFAULT` is the single source of these values. Player-facing rooms and
new AI solves install it through `Simulation::enable_current_player_movement`. Low-level
`Simulation` constructors remain unconfigured so old replay and corpus identities are not silently
reinterpreted.

## Human calibration note

The first broad gallery playthrough under this policy felt substantially easier than the prior
movement, probably mainly because of the higher speed. Small-platform landings felt slightly
harder with the selected braking response, but still acceptable. A fractionally shorter braking
time is an open follow-up hypothesis, not a change in this policy; 203 ms remains the selected
default until another controlled playtest changes it.

This reinforces two evaluation rules:

- any corpus difficulty/easiness result produced under an older movement policy is historical;
- controller route complexity is not a human-difficulty score, especially when a faster movement
  policy opens simpler human routes.

The current-policy comparison audit also found a concrete geometry consequence: `cal-06` Safe
Harbor has a replay-certified baseline `AutoJump` controller route even though its retained
WallJump-loadout witness uses two wall jumps. The room may still demonstrate the intended gesture,
but it is not WallJump-required under policy v2. This matches the human report that it is trivial
and is retained as calibration evidence rather than hidden behind the old no-bypass assertion.

## Mechanical gallery retuning

Gallery route actions are no longer duplicated as hand-edited Rust arrays or exact tick assertions.
The checked artifact is:

`crates/downwards-content/generated/calibration-witnesses-v2.txt`

It binds the player-movement policy version, all six tuning values, all 15 stable gallery IDs, and
canonical run-length encoded actions. Regenerate it mechanically with the current game-playing AI:

```sh
cargo run -p downwards-content --example retune_gallery
```

Check that a committed artifact is current without rewriting it:

```sh
cargo run -p downwards-content --example retune_gallery -- --check
```

The generator considers both the previous valid route and a fresh bounded AI solve, replay-verifies
them under the exact player policy, greedily removes redundant input, rejects death/Dash/restart or
unaccepted jump presses, preserves the declared minimum wall-jump burden, and writes deterministic
bytes. Ordinary tests parse that one artifact and validate clean completion plus authored geometry
and mechanic invariants. They do not copy generated tick counts or contact coordinates into source.

## Compatibility boundary

The historical v6 catalogue and corpus-v3 playtest manifests were evaluated before player movement
policy v2 and do not carry a compatible movement-policy identity. Their stored actions remain
available to strict artifact verification, but the game client does not play them as current-policy
routes. V and C solve the selected target afresh from the live configured simulation. A future
corpus regeneration must bind `PLAYER_MOVEMENT_POLICY_VERSION` and the exact tuning before its
stored witnesses can become player-facing again.

Persistent human history rows record the policy version, the `gameplay-v2`/`custom` profile, and all
six values. Applied F2 changes are also appended immediately as tuning-change records, so later
calibration can reconstruct the actual controls used rather than infer them from the build date.
