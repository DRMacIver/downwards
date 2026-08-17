# Vision and Inspirations

## The pitch

The founding brief, in the designer's words:

> "I'd like you to build me a game, in rust... roguelike metroidvania... descend
> into a procedurally generated dungeon with multiple paths. Traps, obstacles,
> no combat. Inspiration: Celeste, Moneyseize, Super Meatboy for platformer
> mechanics; Unexplored for dungeon generation. Focus on single level code.
> Minimize aesthetic — Moneyseize pixel-art level."

Restated consistently across sessions:

> "It's a roguelike metroidvania. You descend into a procedurally generated
> dungeon, with multiple paths out of most rooms. There are traps, obstacles,
> etc. but no combat in the game. Individual levels fit on a screen with no
> scrolling."

Key constraints that follow directly from the pitch:

- **No combat.** All challenge is traversal: precision movement, hazards, and
  route choice.
- **One screen per room, no scrolling.** Rooms are 320×180 (16:9), a 32×18
  grid of 10px tiles.
- **Multiple paths out of most rooms.**
- **Minimalist pixel art** at roughly Moneyseize's level of fidelity.
- **Desktop, keyboard-first** (an explicit designer preference, reaffirmed
  multiple times).
- Early development focus: get a *single* level's code, AI testing, and
  solvability/difficulty validation right before scaling up.

The pitch originally included Lua scripting; Lua was later removed entirely
(see [decision-log.md](decision-log.md#lua-adopted-then-removed) and ADR 0003).

## The name is the brief: descent

> "The reason this game is called 'downwards' was meant to be that you start at
> the top and the crown is at the bottom." — the designer

This is the founding structural conceit, not a flavour detail, and it was
stated explicitly only mid-project — earlier dungeon layouts ignored it and
had to be restructured (layout rev 3, "THE DESCENT"). Its consequences:

- Spawn sits near the top of the dungeon grid; the goal (the crown) sits on
  the bottom row. The current run goes Hollow Landing (spawn, top) → Crown
  (bottom).
- Falling is free; climbing costs effort. Forward progress is committal and
  retreat is effortful. Traversal abilities (wall-jump, dash) are the tools
  that fight gravity back upward.
- Difficulty arcs easy near the top to hard near the bottom.
- In the descent layout, depth is the progress bar, strata are regions, and
  the geography itself should answer "where am I?".

## Reference games

- **Celeste** — precision platformer movement: run, variable-height jump, wall
  jump, 8-way dash. Also the top-end difficulty anchor: the designer asked for
  a level at "late game Celeste levels of difficulty, with no dash" as a
  calibration reference, and Celeste's "responsive baseline with selective
  momentum" plus its forgiveness philosophy (a difficult game should still
  "want you to succeed") informed movement tuning research.
- **Moneyseize** — 2D platformer mechanics and the minimalist pixel-art
  aesthetic bar.
- **Super Meatboy** — 2D platformer mechanics.
- **Unexplored** — procedural dungeon generation. Boris the Brave's Unexplored
  writeup (unexplored.com/2021/04/10) is the core external reference for the
  generator program, cited repeatedly. Two Unexplored mechanics are marked as
  core future directions rather than immediate features (see
  [open-questions-and-todo.md](open-questions-and-todo.md)):
  1. **Keyed cul-de-sacs** — descend into a dead-end pocket you cannot climb
     back out of until you collect the item at its bottom. Deliberately breaks
     the strict "every reachable state can retreat" rule; a no-return pocket
     is good design iff its escape is obtainable inside it.
  2. **Get-out-alive victory** — like Unexplored's amulet: grabbing the crown
     is not the end; you must climb back to the top to win, under some
     pressure mechanism (candidates: countdown timer, escalating hazards,
     rising lava — to be found by experimentation, not decided a priori).

The designer also noted "you won't find too many procedurally generated
platformers" — the generation problem is a genuine research gap, which is why
the project runs its own generation research program rather than adapting
prior art directly.

## Feel goals

- **Jumping is the centre.** "Jumping 'feeling good' is probably the most
  central part of a platformer" — the designer commissioned comparative
  research (Celeste, Moneyseize, Hollow Knight, Super Meatboy, Dustforce,
  N++) and personally A/B tested movement models. The selected model is
  "Retained Snap" (see [decision-log.md](decision-log.md#movement-feel)).
- **Robust, human-performable control.** What matters is "robust and usable
  gameplay", not implementation details like tick counts. AI-viability proofs
  are never sufficient: routes must be reproducibly performable by a human
  with a shaky hand.
- **Hazards punish commitment, not jitter.** Hazards should punish overshoot
  or commitment, never single-frame timing noise; failures should cost a
  retry, not feel unfair; spikes should be readable.
- **Difficulty is the primary fun metric**, route diversity through a level is
  second, with a variety of other metrics tracked for later use.
- **Asymmetric difficulty is a feature.** It is explicitly fine — desirable —
  for a route to be much harder in reverse: "you might want a branch that you
  have to descend to the bottom of, get some traversal ability, and only then
  be able to easily go back up."
- **Skill-based sequence breaks are easter eggs, not bugs.** "If there's a
  super challenging route without the traversal item, that's a fun easter egg
  for extremely skilled players, not a bug." Easy or moderate bypasses of
  intended gates, by contrast, are design failures.

## Dungeon structure

- **Multi-door tiling.** Rooms have 2–4 boundary doors (west/east walls,
  ceiling, floor) rather than a single exit, with the constraint that any door
  is reachable from any other door (possibly requiring traversal abilities).
  This lets rooms tile together into a seamless dungeon with no hand-authored
  junctions. Door mouths are standardized: west/east on rows 13–16 at columns
  0/31; ceiling/floor on columns 14–17 at rows 0/17.
- **Geometric gating only.** Traversal abilities gate progress via room
  geometry that is impassable (or nearly so) without them — never via door
  locks keyed to ability ownership.
- **Coin gates** are the mainline progression mechanism: coins unlock sealed
  doors at escalating thresholds, culminating (in the current design) in a
  64-coin Crown gate that also requires both Wall Jump and Dash.
- The current authored dungeon is a 101-room multi-act descent with mandatory
  trial regions (a Wall-Jump region and a Dash region), each with required
  branches and validated return routes.
- Progression model: abilities acquired per-run with some metaprogression; a
  health bar for full runs (death drains it, zero ends the run) with infinite
  retries retained for individual-level prototyping; stamina explicitly
  skipped.

## Room design grid (quick reference)

- 32×18 tiles, 10px per tile; player hitbox 8×12px.
- Jump: ~40px max rise, 20px comfortable. Dash: 40px, 8-way.
- One-way platforms require exactly 2 empty tiles directly above (hard rule).
- Max 3 coins per room; hazards specified as rect + period/active/phase ticks
  at 60Hz.

See `docs/design/room-iteration-playbook.md` for the authoritative authoring
playbook, and [research-log.md](research-log.md) for the measured
geometry-difficulty relationships behind these numbers.
