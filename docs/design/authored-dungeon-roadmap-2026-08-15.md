# Authored dungeon roadmap — 2026-08-15

The active target is a hand-authored dungeon of at least 100 playable floors. The current playable
build has 21. The 21 are not being relabelled as completion: they are the first two progression
regions and the integration testbed for the authoring/runtime contracts needed by the full game.

## Current progression

- Floors 1–10: Rootworks, ordinary movement, six coins, Climbing Gloves unlock Wall Jump.
- Floors 11–21: the earlier vertical slice, six more coins, Winged Boots unlock Dash, four final
  coins, then the Crown.
- Crown ingress requires all 16 current coins and both traversal methods. This is deliberately
  stronger than the final contract's minimum of one third of all dungeon coins.

The generator palette is scaffolding for geometry, not an authority on quality. Every floor has an
explicit stable content identity, title, graph position, pickup placement, and progression role.
Geometry promoted from a palette still needs route inspection and human feedback before it counts
as finished authoring.

## Authoritative content contract

`downwards-content::AuthoredDungeonDefinition` is the scaling seam. It validates:

- a declared minimum floor count (100 for the finished dungeon);
- unique floor IDs, keys, doors, coin indices, and traversal unlocks;
- exact reciprocal graph connections;
- up to 128 persistent coins;
- monotone reachability of every floor and unlock;
- every Crown ingress requiring at least one third of the coins and every current traversal method.

The client consumes the same typed door requirements used by the validator. A flaw in physical
geometry therefore cannot silently turn a method gate into an unguarded Crown shortcut.

## Evidence policy

An authored floor is not accepted merely because a search returns a number. Each required leg needs:

1. an exact authoritative replay reaching the intended door or pickup;
2. replay inspection for deaths, resets, accepted movement events, repeated-wall hopping, and
   obvious controller thrashing;
3. explicit reduced-loadout or simple-controller bypass checks where a traversal method is claimed;
4. deterministic noisy-input trials recorded by family, without converting blind-continuation
   success into a human difficulty score;
5. eventual human playtest feedback, especially for the late-game floors.

The current Rootworks routes have exact positives and observed strength-one noisy successes. This is
early robustness evidence only. Replanning under perturbation, durable per-floor witness artifacts,
and full-dungeon route verification remain required work.

## Next authored regions

The remaining 79+ floors will be added in bounded regions rather than as generated filler:

1. a Wall-Jump region that revisits Rootworks spaces from new routes;
2. a Dash region with optional coin vaults and mandatory method checks;
3. mixed-method traversal and navigation puzzles;
4. a late Crown Citadel using the calibrated hard-gallery execution envelope.

Each region should introduce new authored geometry and puzzle vocabulary. Reusing a palette shell is
acceptable for a first draft, but repeated shells do not satisfy the final hand-authored requirement.
