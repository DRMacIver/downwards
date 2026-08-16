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

Individual floors can be calibrated without mutating that checkpoint:

```sh
cargo run -- --dungeon-floor 26
cargo run -- --dungeon-floor gale-chasm
cargo run -- --dungeon-floor demo-dungeon.void-pass
```

This floor lab is explicitly non-persistent. It reconstructs the selected route coordinate with
its exact authored entry, inventory, abilities, and target; `V` consumes the policy- and
palette-bound checked witness artifact. Human attempt rows retain current dungeon provenance, but
no room transition, coin, unlock, or Crown state is saved.

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

Palette generation v21 contains eleven evidence-driven room replacements. Persistent history recorded
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

Vacuum Gallery is the third replacement. Its v12 route reached a late mandatory coin using three
ordinary jumps and neither unlocked traversal method. The v13 room instead composes a cap-safe
alternating wall shaft with a grounded recovery and a one-tile Dash tunnel. The mechanically
retained trace performs six Wall Jumps before its first Dash, then four accepted Dashes; all four
strength-one shaky-hand families retain nonzero successes. Separate same-budget Wall-Jump-only and
Dash-only searches find no coin. Targeted retuning prints exact events, positions, velocities, and
inputs so this is inspected as behavior rather than inferred from a scalar score.

Lunar Cache is the fourth replacement in v14. Its prior coin route contained two incidental Wall
Jumps, no Dash, and a buffered-jump-heavy controller trace. The replacement preserves the ceiling
branch socket but separates three visible acts: a three-landing non-lethal descent, three horizontal
Dashes through a ten-pixel passage, and a cap-safe shaft ascent with five accepted Wall Jumps. The
route tool evaluates both its staged candidate and the unconstrained faster solve, then retains the
staged route because it does not spend a diagonal Dash before the descent and uses fewer Dashes and
reversals in the climb. Same-budget Wall-Jump-only and Dash-only searches have no positive. A
separate exact solve returns from the collected coin to the ceiling exit, and all four strength-one
perturbation families retain observed successes. These bounded misses and noisy-controller results
remain descriptive evidence, not impossibility or human-difficulty claims.

Comet Run is the fifth replacement in v15. The old mandatory coin route was a monotone broad-shelf
staircase with four ordinary jumps and two incidental Dashes. The replacement exposes an
up-up-down-up contour over lethal floor with three two-tile recovery platforms and a ceiling hazard
that punishes an unbroken upward line. The route authoring tool solves each visible platform leg
with the ordinary Dash vocabulary, walks to the next launch edge, and exact-replays the combined
result; it does not prescribe per-tick actions. The retained behavior has three launch jumps, four
accepted Dashes, four short braking reversals, and no failed jump or Wall-Jump-grace events. The
unconstrained global solve is behaviorally busier and is rejected. A same-budget no-Dash search has
no positive, a post-coin solve reaches the east door, and all four strength-one perturbation
families retain at least 44/64 observed successes.

Meteor Run is the sixth replacement in v16. Its old mandatory late route consisted of five
repeated full-height jumps along broad shelves and used neither unlocked traversal method. The
replacement is a flat, legible timing course: three full-height room-clock shutters remain active
for seventy of every ninety-six ticks, and their twenty-six-tick inactive windows advance east at
thirty-two-tick intervals. Safe bays between shutters make the next state visible and provide
braking room. The retained authored policy launches exactly one horizontal Dash through each
window, with no jumps and only two braking reversals. The unconstrained solver's eleven-Dash trace
is rejected as behaviorally noisy. A same-budget no-Dash search has no positive, and all four
strength-one perturbation families retain at least 39/64 observed successes. This is robustness
evidence for the timing controller, not a claim that the room is difficult for a human.

Moon Vault is the seventh replacement in v17. The old mandatory coin branch retained a
178-tick/30-span trace with five scattered Dashes, nine reversals, rejected jump presses, and only
3/64 successes in its weakest perturbation family. The replacement is a visible clockwise orbit
beneath the ceiling entry: drop through the entrance shelf, recover low in the centre, Dash beneath
a solid separator, rise through two broad right-hand supports, and kick once from the boundary back
to the coin. A paired up/down ceiling hazard closes the safe-backed-spike shortcut exposed during
authoring. The staged route exact-replays in 111 ticks with 17 spans, three accepted Dashes, one
final Wall Jump, no horizontal reversals, and at least 40/64 successes in every strength-one family.
A same-budget no-Dash search has no positive; Dash-only remains possible and is not misrepresented
as a two-method gate. A separate exact solve returns from the collected coin to the ceiling door.

Star Threshold is the eighth replacement in v18. Its old mandatory coin route was a generic shelf
staircase: four accepted ordinary jumps, no Wall Jumps, and no Dashes. The replacement serializes a
single low Dash aperture through a full-height backing wall immediately followed by a broad cap-safe
alternating shaft. The route authoring tool deliberately separates those acts: it searches a
one-Dash grounded entry and then disables Dash in the suffix vocabulary so the displayed route
cannot substitute vertical Dashes for the wall rhythm. The retained 94-tick replay uses one Dash
before six accepted Wall Jumps and four horizontal reversals. It cleanly collects the coin, a
separate exact solve reaches the east door afterward, and every strength-one perturbation family
retains at least 28/64 successes. Same-budget Wall-Jump-only and Dash-only searches have no positive.
Those bounded misses certify no known bypass in the current vocabulary; they do not prove physical
impossibility or human difficulty.

Nova Niche is the ninth replacement in v19. Its old required branch reached the coin in 48 ticks
with three Dashes and one incidental Wall Jump. The replacement starts at the floor-door sill
inside a cap-safe alternating core, exposes a broad upper launch shelf, and places the coin across
a sixty-pixel corona gap above lethal floor. The authoring tool composes an exact Wall-Jump-only
support solve with a deliberately small jump-and-Dash transfer vocabulary; the selected 102-tick
route has 19 spans, four accepted Wall Jumps, one Dash after the climb, and five reversals. It
returns to the floor door after collection and retains at least 35/64 successes in every
strength-one perturbation family. Current movement semantics also admit a noisier Wall-Jump-only
corona leap and a Dash-only ascent using wall-momentum carry. Both are exact retained evidence:
this room explores the interaction between methods and does not falsely claim to gate either one.

Constellation Hall is the tenth replacement in v20. The old mandatory coin route was another broad
diagonal staircase and exact-replayed in 112 ticks with five Dashes, no Wall Jumps, and no
horizontal reversal. The replacement is a single readable under-over-under silhouette: recover
beneath a ceiling pillar, climb over a floor-anchored centre, then descend beneath a second ceiling
pillar to the coin. Lethal floor closes the low bypass while four broad shelves preserve deliberate
braking. The unconstrained first positive took 189 ticks, six Dashes, three Wall Jumps, and 12
reversals. A three-waypoint composition exact-replays in 165 ticks with four Dashes, two Wall Jumps,
and eight reversals, and lands on all three authored recovery heights. A separate exact solve
continues east after the coin, same-budget baseline search has no positive, and each strength-one
perturbation family retains at least 27/64 successes. The route-shape comparison, not its raw event
count, is the reason the composed witness is retained.

Shadow Duct is the eleventh replacement in v21. Its old mandatory coin route took 85 ticks across
three broad shelves, with one incidental Wall Jump, no Dash, and five braking reversals. The
replacement exposes three acts instead: settle in a one-way start bay and Dash left through a
ten-pixel aperture; use a Dash assist and three alternating Wall Jumps to climb the cap-safe shaft;
then make one coyote-assisted Dash across the seventy-pixel reward gap. The retained composition is
136 ticks and 16 spans, cleanly collects the coin, and retains at least 40/64 successes in every
strength-one perturbation family. A separate exact return reaches the one-way start bay and floor
door, proving the branch is not a one-way trap. Wall-Jump-only
search has no positive. Dash-only search does find a 489-tick, ten-Dash wall-ascent-carry route, so
the documented claim is a readable mixed route with a costly expert alternate—not a strict
two-method gate or a scalar human-difficulty score.

The Observatory is the twelfth replacement in v22. Its old mandatory route was a monotone
up-right staircase: 143 ticks, 15 spans, three Dashes, one incidental Wall Jump, and no horizontal
reversal. The replacement keeps the entire lower approach safe but seals its ceiling against a
shortcut. The player runs to a far-right observatory tower, climbs three deliberately broad
alternating contact bands, then reverses west across a middle lens and final coin roof. The staged
route exact-replays in 276 ticks with 17 spans, one climb-assist Dash, three accepted Wall Jumps,
and two roof-crossing Dashes. It records every authored recovery landing, and an independent exact
solve continues from the coin to the east door. Its four strength-one blind-continuation families
retain 32, 36, 37, and 50 successes out of 64. Same-budget Wall-Jump-only and Dash-only searches
have no positive. Those finite searches and perturbation counts validate the intended structure;
they are not a scalar human-difficulty score or a proof that no unknown bypass exists.

Gravity Lift is the thirteenth replacement in v23. Its previous ceiling route took 77 ticks and
14 spans, fired five Dashes, accepted no Wall Jump, and crossed a loose diagonal set of one-way
shelves. The replacement is a deliberately non-lethal right-left-right lift: three solid baffles
span almost the full room width, alternating the only upward opening while turning each completed
rise into a safe recovery floor. A three-waypoint composition avoids the global solver's repeated
horizontal Dash spam. Its 525-tick route has only 18 semantic spans, three accepted Dashes, two
accepted Wall Jumps, and three horizontal reversals. It lands on all three authored baffles and
reaches the ceiling branch cleanly. An independent exact solve enters from that ceiling branch and
descends to the east corridor, while a same-budget baseline search has no positive. Every
strength-one blind-continuation family retains at least 20/64 successes. The duration records a
long traversal, not a claim that time or solver effort equals human difficulty.

Aurora Spire is the fourteenth replacement in v24. Its prior mandatory coin route was another
loose diagonal staircase: 143 ticks, 15 spans, three Dashes, one incidental Wall Jump, and no
horizontal reversal. The replacement uses an out-and-over silhouette. The player enters a
cap-safe alternating core, reaches a broad crown shelf, then crosses a lethal horizontal light
sheet to the upper-right coin; a three-height recovery cascade makes the onward descent safe and
visually explicit. The staged route exact-replays in 200 ticks and 20 spans with four accepted Wall
Jumps, one Dash after the climb, and eight horizontal reversals. Its four strength-one
blind-continuation families retain 31, 42, 44, and 63 successes out of 64. An independent exact
solve continues from the coin to the east door. Ordinary search also retains a clean Dash-free
Wall-Jump route, so the evidence describes a readable preferred mixed route rather than a strict
method gate.

The Skybridge is the fifteenth replacement in v25. Its previous mandatory coin route took 107
ticks and 18 spans while firing six Dashes monotonically right, with no accepted Wall Jump or
horizontal reversal. The replacement makes the bridge itself a readable over-under puzzle. A
lower recovery island leads into a narrow floor-anchored mast; three alternating Wall Jumps reach
its broad roof. The route then walks to the far edge, drops along a ceiling-hung mast, uses one
downward Dash through a thirty-pixel aperture, and lands on the lower coin deck. The staged witness
exact-replays in 182 ticks and 18 spans with five accepted jumps, three Wall Jumps, one Dash, and
two horizontal reversals. An independent exact solve continues east after collection, while a
same-budget baseline search has no positive. Its strength-one blind-continuation families retain
6, 24, 25, and 37 successes out of 64; that lower tail is recorded as a genuinely less forgiving
late-game route, not converted into a scalar human-difficulty score.

The Empty Throne is the sixteenth replacement in v26 and the first intentionally authored final
exam. Its former Crown route was a monotone five-shelf ascent: 202 ticks, 11 spans, two Wall Jumps,
three Dashes, and no horizontal reversal. The replacement makes a W-shaped three-act silhouette.
The west core rises to a full roof recovery; a safe central landing faces a ten-pixel passage that
only the Dash posture can cross; the taller east core then rises to the Crown dais. The composed
witness exact-replays in 295 ticks and 26 spans with nine accepted jump presses, seven Wall Jumps,
one Dash between the climbs, and four reversals. It lands after both major acts, collects the Crown
before reaching the separately placed terminal trigger, and retains 20, 24, 39, and 58 successes
out of 64 across the four strength-one blind-continuation families, with no deaths in those rows.
Same-budget Wall-Jump-only and Dash-only searches both have no positive. Those bounded negatives
support the physical two-method contract; the independent all-64-coin/all-method door and dungeon
audits remain authoritative for progression.

The Crown Gate is the seventeenth replacement in v27. The previous route used seven Dash presses,
no accepted jump, and no reversal before the final door. The replacement has a visibly open bottom
entrance into a paired-wall shaft, a full upper staging cap, and a ceiling-anchored lintel whose
ten-pixel passage cannot be bypassed over the top. The composed witness exact-replays in 212 ticks
and 12 spans with four accepted jump presses, three strictly alternating Wall Jumps, one horizontal
Dash through the keyhole, and four reversals. Its four strength-one families retain 25, 21, 40, and
62 successes out of 64 with no deaths. Same-budget Wall-Jump-only and Dash-only searches both have
no positive. This room-local evidence complements rather than replaces the independent 64-coin and
all-method ingress gate.

Validation covers exact reciprocal room/door IDs, opposite socket geometry, full standing
headroom over every authored one-way surface, unique persistent coins, all coin and method gates,
persistent item omission, additive mid-run Wall Jump and Dash state, gate rejection without room reset, client
traversal of the intended loop, and authoritative solver positives for every critical leg and coin
branch. The retained boots route now takes 76 ticks, five accepted jumps, four wall jumps, and at
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
a scalar difficulty or human-robustness claim. Targeted retuning additionally prints the selected
route's event, position, velocity, and input trace for behavior-level inspection.

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
