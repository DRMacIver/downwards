# Authored dungeon roadmap — 2026-08-15

The active target is a hand-authored dungeon of at least 100 playable floors. The current playable
build has 101 connected floors. That satisfies the structural scale target, but it is not being
relabeled as design completion: much of the later geometry remains palette-derived and needs the
same behavior-level inspection and human feedback already applied to the first evidence-driven
replacements.

## Current progression

- Floors 1–10: Rootworks, ordinary movement, six coins, Climbing Gloves unlock Wall Jump.
- Floors 11–20: a mandatory Wall-Jump course with two required branches and six coins.
- Floors 21–36: six remaining pre-Dash branch coins gate a multi-room Winged Vault quest; the boots
  themselves require an alternating Wall-Jump climb. A mandatory Dash region follows.
- Floors 37–76: the Aerial Foundry and Glassworks add forty mixed-method floors, six branches, and
  twenty-four coins behind regional seals.
- Floors 77–101: the Astral Keep adds twenty-five late floors, three branches, twelve coins, and
  mixed-method Crown ingress.
- Crown ingress requires all 64 coins and both traversal methods. This is deliberately stronger
  than the contract's minimum of one third of all dungeon coins.

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

The current Rootworks routes have exact positives and observed strength-one noisy successes. The
Wall-Jump and Dash regions have exact positives for every required leg, exact return routes from coin
branches, and a final physical gate whose known positive uses both walls while the same bounded
baseline search has no positive. The Dash seal likewise records an accepted Dash and no equivalent
WallJump-only positive; each Dash-region route retains observed successes in all applicable
strength-one perturbation families. This is early robustness evidence only. Replanning under
perturbation and broad human validation of the late game remain required work. A durable generated
artifact now retains one exact replay and all applicable strength-one outcomes for every floor;
course-specific retuning rejects visibly noisy routes even when they solve faster.

## Next authoring work

The floor count is no longer the bottleneck. The next bounded slices are:

1. replace weak mandatory late palette shells with distinct mixed-method and timing vocabulary;
2. collect full-run human navigation and execution feedback rather than inferring difficulty from
   AI action counts;
3. introduce any further traversal unlock only with a real revisitation loop and a physically
   audited gate;
4. retune the Crown approach against the calibrated hand-authored challenge envelope.

Each region should introduce new authored geometry and puzzle vocabulary. Reusing a palette shell is
acceptable for a first draft, but repeated shells do not satisfy the final hand-authored requirement.
