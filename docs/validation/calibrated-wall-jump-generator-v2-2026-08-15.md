# Calibrated WallJump generator v2 — 2026-08-15

## Correction

Human inspection of generated seed 03 found that its paired causeway spikes were mechanically
useless. V1 placed `HazardDown` above `HazardUp`, so both lethal tips pointed into the inaccessible
seam between two solid hazard tiles. Approaching from either playable side reached a nonlethal
back face.

Generation v2 places `HazardUp` above `HazardDown`. Their solid bases meet without a visual gap and
their lethal tips face the playable spaces above and below. A structural regression scans every
causeway and low-bridge key for outward `Up/Down` pairs and rejects any inward `Down/Up` pair.

Making the hazard functional exposed a second hidden issue: the v1 shaft opening had erased its
upper contact sequence, and the solver compensated with repeated same-wall hops. V2 restores the
human-validated five-contact climb before the opening. It also shifts low-bridge shafts within a
safe range instead of reflecting their directional finish.

## Regenerated evidence

The version-bound artifact was regenerated mechanically with:

```sh
cargo run --release -p downwards-content --example retune_calibrated_generator
```

All 12 playtest keys pass again under generation version 2 and player movement policy version 3:

- clean exact completion, with the target first reached on the final tick;
- every jump press accepted and no Dash/death/reset input or event;
- 3–5 accepted wall jumps and at most one adjacent repeated wall side;
- 47–139 ticks and 9–46 semantic action spans;
- climb-plus-traverse rooms now require 2–3 accepted ordinary jumps;
- complete baseline direct-controller audit with zero positives;
- ordinary bounded baseline solve inconclusive with `PathHorizon` for every key.

The simple WallJump controller portfolio still solves the climb-only rooms but no longer solves the
four functional traverse rooms. This is controller-coverage evidence, not a difficulty score. In
particular, seed 09's 46-span retained witness should be checked by a human rather than interpreted
as proof that its room is difficult.

## Scope

V2 remains an isolated 12-room playtest generator. It does not alter corpus enumeration, source
policy, selection, or the rejected legacy difficulty metrics. Seed geometry changed, so the exact
generation version was bumped rather than silently rewriting v1 identities. Run
`cargo run -- --calibrated 0` and use brackets to cycle the regenerated seeds.
