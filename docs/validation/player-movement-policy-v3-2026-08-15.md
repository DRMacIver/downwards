# Player movement policy v3

Status: current game-wide policy on 2026-08-15.

Policy v3 inherits the human-selected movement tuning and jump-input behavior from policy v2. Its
semantic change is the static spike contract.

## Explicit directional hazards

Static spike direction is authored directly in the room tile field:

| Authoring character | Tile | Pointed face |
| --- | --- | --- |
| `^` | `HazardUp` | top |
| `v` | `HazardDown` | bottom |
| `<` | `HazardLeft` | left |
| `>` | `HazardRight` | right |

There is no adjacency, support, corridor, or route-based direction inference. Rendering, physics,
room hashing, visual descriptors, research artifacts, and textual diagnostics preserve the exact
tile variant.

The pointed face is lethal. The rear face and both perpendicular faces are ordinary blocking
surfaces, so approaching a spike from behind does not kill the player or permit passage through
the tile. This is a physics change, not merely an art change, and therefore advances
`PLAYER_MOVEMENT_POLICY_VERSION` from 2 to 3.

A two-sided lethal barrier is authored as a back-to-back pair, such as `^` directly above `v`.
Low Clearance, Low Bridge, and the hard challenge use this explicit pairing above their downward
ceiling banks, preventing the safe rear face from becoming an unintended walkable bypass. The
client draws an opaque shared base seam for paired vertical or horizontal spikes.

The existing numeric tile identities remain stable for `Empty`, `Solid`, upward spikes, and
`OneWay`; the three new directions use additive identities. Historical artifact wire data still
maps its `hazard` spelling to an upward spike.

## Authored content and witnesses

Handcrafted wall spikes were rewritten as explicit inward-facing `>`/`<` tiles. Floor spikes stay
`^`; the Low Clearance, Low Bridge, and hard-challenge ceiling banks use `v`. The mechanically
generated gallery witness artifact was regenerated under policy v3 and is checked with:

```sh
cargo run -p downwards-content --example retune_gallery -- --check
```

This refresh is performed by the existing generator/replay process, not by copying new tick counts
into tests.

## Visual collision contract

The v2 environment atlas makes every spike sprite fill the complete 24x24 collision cell. The
one-way bridge begins at the exact top collision surface. These invariants are enforced by
`docs/art/build_environment_atlas.py`; the art provenance and rebuild command are recorded in
`docs/art/README.md`.

Core regressions verify an upward spike's rear behaves as a nonlethal ceiling and a horizontal
spike blocks from behind while killing from its authored pointed face. Content and client tests
verify the explicit Low Clearance directions and replay every gallery route under policy v3.
