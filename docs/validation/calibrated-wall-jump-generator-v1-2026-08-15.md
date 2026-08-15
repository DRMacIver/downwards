# Calibrated WallJump generator v1 — 2026-08-15

> Superseded by generation v2 before corpus integration. Human inspection found that the paired
> causeway bank pointed both lethal spike faces into the inaccessible seam between its tiles. See
> the [v2 correction report](calibrated-wall-jump-generator-v2-2026-08-15.md).

## Outcome

A new, isolated generator now produces a twelve-room WallJump-only/no-Dash human-playtest batch.
It is intentionally narrower than the corpus generators and is not wired into corpus enumeration,
selection, or difficulty ranking.

Run it in game with:

```sh
cargo run -- --calibrated 0
```

Use `[` and `]` to cycle seeds 0–11. `V` plays the exact retained witness.

## Human-calibrated grammar

Generation version 1 selects one of five burdens:

- `short-turns`: three staged contact bands and a recovery shelf;
- `even-tempo`: five regular alternating contact bands;
- `recovery-ascent`: the same rhythm with one intermediate one-way shelf;
- `causeway`: a readable climb followed by a rising three-landing traverse;
- `low-bridge`: the causeway with a paired directional-spike bank that distinguishes a short
  release from a full-height jump.

The shaft position and safe reflections vary deterministically by seed. Contact bands are three
tiles wide in v1. This deliberately avoids reproducing the original high-end Needle's Eye burden
or claiming that generated geometry interpolates human difficulty linearly.

## Mechanical acceptance

The checked-in artifact is produced by:

```sh
cargo run --release -p downwards-content --example retune_calibrated_generator
```

For each seed the tool:

1. exact-regenerates the room under current player movement;
2. runs the ordinary authoritative WallJump solver and the complete finite direct-controller
   portfolio;
3. greedily removes action chunks and individual inputs to a fixed point;
4. chooses lexicographically by invalid/range state, repeated wall sides, jump presses, action
   spans, ticks, then exact action bytes;
5. requires a clean final-tick completion with every jump press accepted, 3–7 wall jumps, and at
   most one adjacent repeated wall side;
6. runs the complete baseline direct-controller portfolio and requires zero positives;
7. runs the ordinary bounded baseline solver and requires an inconclusive result.

The first frozen batch passes 12/12. Retained witnesses contain 3–5 accepted wall jumps, 47–127
ticks, and 9–22 semantic action spans. Climb-plus-traverse rooms add accepted ordinary jumps. Every
baseline bounded solve ended `PathHorizon`; these finite misses are no-known-bypass evidence, not
unreachability proofs. The WallJump direct portfolio found one or more routes in every room, which
is useful coverage evidence and is not scored as difficulty.

The replay artifact binds generator version 1 and player movement policy version 3. Content and
client tests regenerate every room, replay every action, require no Dash/death/reset, require the
target first on the final tick, and verify the retained input-quality bounds. Route lengths are
read from the generated artifact rather than copied into hand-maintained expectations.

## Defects caught while tuning

Two candidate shapes failed before the accepted batch was frozen:

- reflecting the low-bridge bank produced a tractable but visibly thrashy retained route with two
  adjacent same-wall hops;
- reflecting the recovery shelf produced a replay-certified baseline solution. The complete
  direct-controller portfolio had missed this bypass, so the full bounded ability-removal solve
  was retained as an independent gate.

V1 keeps both shapes directional and varies their shaft position instead. The rejection was not
weakened to preserve yield.

## Limits and next decision

This is a small grammar trial, not evidence that the generator can cover the desired long-term
range. Symmetry and shaft translation account for part of the twelve-room diversity. Structural
counts, solver effort, and retained replay length are not presented as human difficulty metrics.
The next decision is based on direct play feedback per seed: which rooms are trivial, satisfying,
awkward, or outside the target range, and which geometric axis caused that result. Only after that
feedback should the grammar expand or feed a larger evaluation run.
