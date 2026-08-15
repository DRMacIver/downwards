# ADR 0002: Prototype lives, ability loadouts, and run progression

- Status: Accepted; content-language clause superseded by [ADR 0003](0003-rust-owned-content.md)
- Date: 2026-08-14

## Decision

During mechanics development, rooms have infinite near-instant retries. The prototype does not
model acquisition, health loss, or run failure yet.

Traversal abilities are acquired during a finished run rather than belonging to one universal
starting kit. Tests must not depend on an acquisition sequence: each human or automated room
scenario supplies an explicit ability loadout. This lets the same geometry be checked with
different unlock combinations and lets gated door routes state their capability requirements.

The first added wall mechanics are wall slide and wall jump. Stamina is excluded for now; active
climbing can be evaluated later. Dash is an eventual in-run unlock, but should be implemented
early enough to test rooms both with and without it.

The eventual run model has a health bar. Dying in a room reduces health; reaching zero loses the
run. Room success and/or pickups may restore some health. Exact costs, restoration amounts, and
health presentation remain open until room difficulty can be measured.

Traversal abilities are acquired per run. Metaprogression may constrain which abilities are
eligible to appear in early regions, so the dungeon model must distinguish:

- abilities currently held in this run;
- abilities available in this run's unlock pool; and
- metaprogression tiers controlling when an ability may appear.

The content-language choice originally recorded here is superseded by
[ADR 0003](0003-rust-owned-content.md). Generated rooms, authored fixtures, mechanisms, and
authoritative behaviour now live in Rust, with no runtime scripting engine.

## Consequences

- The core exposes an explicit, cloneable ability set whenever a simulation or validation
  scenario starts.
- Door-route certificates bind the ability loadout, complete generated-room provenance, exact
  source and target doors, difficulty report, and input witness. A room is accepted only after
  every ordered door pair is certified. Separate pickup certificates bind a source door, targeted
  collectible, and witness without claiming completion of a different route.
- Room generation must never assume every implemented traversal mechanic is available.
- Infinite retry remains useful for mechanics work even after health is added to the larger run
  state.
- Desktop keyboard support is the only immediate client requirement; browser and polished
  controller work are deferred.
