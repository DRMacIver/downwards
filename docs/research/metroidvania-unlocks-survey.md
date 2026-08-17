# Metroidvania Ability/Traversal Unlocks: A Survey

Research baseline for downwards' unlock vocabulary. Downwards currently has two
unlocks — climbing gloves (wall-jump) and winged boots (8-way dash) — with
gating that must be *geometric* (room shapes impassable without the ability),
never ability-keyed doors. This document surveys what the genre does, what's
unusual, and what fits our constraints. It makes no implementation decisions.

Games surveyed: Super Metroid, Metroid Dread, Castlevania: Symphony of the
Night (SotN), Hollow Knight and Silksong, Ori and the Blind Forest / Will of
the Wisps, Celeste, Environmental Station Alpha (ESA), Axiom Verge 1/2,
La-Mulana, Animal Well, Pseudoregalia, Guacamelee 1/2, Iconoclasts, Dead
Cells, Rogue Legacy 1/2, plus design writeups (Mark Brown's *Boss Keys* /
GMTK, misc. devlogs and analyses; links at the end).

---

## 1. The canonical unlock set

Roughly in descending order of ubiquity. "Body-length" (BL) = one character
height; in downwards terms think in tiles of the 32x18 grid.

### Double jump — near-universal
- **Games:** SotN (Leap Stone), Hollow Knight (Monarch Wings), Ori (both),
  Metroid's Space Jump is the infinite-jump extreme, Guacamelee, Rogue Legacy,
  Dead Cells (baseline kit), Silksong, countless others. Widely cited as *the*
  most ubiquitous metroidvania upgrade.
- **Gates:** ledges 1.5–2x max jump height; gaps just beyond single-jump
  distance; escape from pits deeper than jump height with no walls to climb.
  The classic geometric gate is a smooth-walled pit or a ledge one jump-arc
  too high.
- **Design note:** it's usually a *mid-to-late* unlock precisely because it
  retroactively softens hundreds of earlier gates — it is the great
  gate-dissolver.

### Wall jump / wall climb — near-universal
- **Games:** Super Metroid (walljump is a technique, not an item), Metroid
  Dread (Spider Magnet), Hollow Knight (Mantis Claw), Ori (Wall Jump/Climb),
  Celeste (baseline climb + stamina), Dead Cells (Spider Rune), Pseudoregalia
  (Cling Gem), downwards (climbing gloves).
- **Gates:** vertical shafts wider than a jump but with facing walls; single
  tall walls (climb variants); shafts with only one wall gate climb but not
  wall-jump. Distinguish *wall-jump* (needs two facing walls or repeated
  single-wall kicks) from *cling/climb* (any one wall, often stamina-limited —
  Celeste's stamina makes wall height itself a gate).

### Dash / air-dash — near-universal in platform-heavy entries
- **Games:** Hollow Knight (Mothwing Cloak), Celeste (baseline, the whole
  game is dash-geometry), Ori WotW, Pseudoregalia (Solar Wind), Dead Cells,
  downwards (winged boots, 8-way).
- **Gates:** horizontal gaps ~1.5–2x jump distance; timed hazard windows
  (crushers, flame jets) that walking speed can't beat; 8-way dashes
  additionally gate diagonal reaches and mid-air redirects (Celeste's core
  vocabulary). An 8-way dash is much more powerful than horizontal-only:
  it is a mini double-jump, a fast-fall, and a gap-crosser at once.

### High jump — classic, now less fashionable
- **Games:** Super Metroid (Hi-Jump Boots), La-Mulana, Rogue Legacy 2.
- **Gates:** single ledges between one and two jump heights. Largely
  superseded by double jump in modern design because it's the same gate with
  less expressive mid-air control; games rarely ship both.

### Glide / slow-fall
- **Games:** Ori (Feather/Glide), SotN (Gravity Boots-adjacent forms, Mist),
  Silksong-adjacent tools, Hollow Knight mods; Yoshi/Kirby lineage.
- **Gates:** horizontal distance *while descending* — gaps crossable only by
  launching from height; safe descent past hazard gauntlets. Naturally
  descent-flavored: it gates "get across while falling", not "get up".

### Grapple / hookshot
- **Games:** Super Metroid (Grapple Beam — specific blocks only), ESA
  (Hookshot — fires at 45°, attaches to almost any surface, ~3-block range),
  Ori WotW (Grapple — designated points), Axiom Verge (Grapple/drone
  teleport), La-Mulana (Grapple Claw), Iconoclasts (wrench-as-grapple on
  bolts).
- **Gates:** two flavors. *Point-anchored* (Super Metroid, Ori): gates are
  wherever the designer placed anchors — effectively a geometric key that's
  easy to reason about. *Any-surface* (ESA): gates almost nothing hard,
  becomes a general mobility multiplier and sequence-break engine. The
  point-anchored version is far more controllable.

### Ground pound / downward strike
- **Games:** Ori (Stomp), Guacamelee (Slam), Silksong tools, Wario/Mario
  lineage; Hollow Knight's Desolate Dive doubles as this.
- **Gates:** cracked floors (destructible-tile gating), pressure plates,
  descending through breakable strata. In a descent game this is literally
  "unlock the way down."

### Swim / liquid traversal
- **Games:** Super Metroid (Gravity Suit — un-gimps movement in water/lava),
  Hollow Knight (Isma's Tear — acid becomes swimmable), Ori (Water Breath /
  Swim Dash), Animal Well (bubble/underwater sections), La-Mulana.
- **Gates:** liquid pools as terrain: pre-unlock they're either lethal
  (acid) or movement-crippling (deep water). The Metroid version is elegant:
  water is *passable but your kit is halved*, so the same room is a gate or
  a corridor depending on the suit.

### Morph / shrink / shape-change
- **Games:** Super Metroid (Morph Ball — 1-tile-tall tunnels), Metroid Dread,
  Axiom Verge (remote drone squeezes where you can't), Animal Well (you're
  small already), Guacamelee (Chicken form fits low passages).
- **Gates:** corridors shorter than standing height — the purest geometric
  gate in the genre: a 1-tile-high passage in a 2-tile-tall world. Almost
  every Metroid-like has *some* answer to "passage too small."

### Block-breaking (dedicated traversal reading)
- **Games:** Super Metroid (bombs/missiles/Screw Attack/Speed Booster each
  break a different block type), Guacamelee (color-coded blocks per move),
  Axiom Verge (Address Disruptor "glitches" corrupted terrain passable).
- **Gates:** destructible tiles keyed to the tool. Note: this is the closest
  the genre comes to ability-keyed *doors* — a colored block is a lock with a
  texture. Downwards' "geometric gating only" rule explicitly excludes the
  naive version; the interesting variants are ones where the *physics* of the
  break move (Speed Booster needs a long runway; bombs need you to survive
  next to them) is itself geometric.

### Momentum / charge moves
- **Games:** Super Metroid (Speed Booster + Shinespark: needs a long flat
  runway, then converts stored speed into a straight-line flight),
  Pseudoregalia (Solar Wind slide-jump bunnyhopping), Celeste (wavedash /
  hyper tech, unofficial but geometry-relevant).
- **Gates:** runway length is the gate — "you can only cross/ascend this if
  the approach room gives you N tiles of flat ground." Deeply geometric and
  composes across rooms, which is exactly why speedrunners love it and why
  it's hard to procgen safely.

### Teleport / phase
- **Games:** SotN (Mist form passes grates; Bat flies), Axiom Verge (drone
  teleport, passcode-glitch walls), Dead Cells (Teleportation Rune at
  designated sarcophagi), Animal Well (well-hidden endgame item).
- **Gates:** grates/porous barriers (mist), designated teleport nodes.
  Node-anchored teleport is controllable; free phase is a gate-dissolver.

### Frequency summary
Near-universal: double jump, wall jump/climb, dash. Very common: swim,
morph/small-passage answer, glide, grapple, ground pound, block-break.
Common-but-declining: high jump. Distinctive: momentum/charge, teleport,
projectile-platform tools (§2).

---

## 2. Unusual and distinctive unlocks

- **Ori's Bash** (Blind Forest, refined in WotW): grab a projectile/enemy/
  lantern mid-air, aim, launch yourself one way and the object the other.
  Turns *hazards into anchors*. Gates: any airspace with a bashable object in
  it. Widely considered one of the best traversal verbs ever shipped; needs
  entities in rooms, not just tiles.
- **Pseudoregalia's kit**: Sun Greaves (three air *kicks* that also scale
  walls — a quantized triple wall-kick), Sunsetter (downward pound that
  converts into a backflip height gain), Solar Wind (slide-jump momentum),
  Cling Gem (wall run/climb). Notable because every gate is pure geometry in
  a low-poly 3D castle — the closest existing game to downwards' "geometry
  only, no keyed doors" rule, and proof a small kit (about 6 real movement
  unlocks) can gate a whole castle.
- **Animal Well's toys**: Bubble Wand (blow a bubble, jump on it — a
  *player-placed temporary platform*; upgraded, infinite bubbles = free
  climb), Disc (ricochets forever, can be *ridden* like a surfboard), Yoyo
  (activates things through crooked 1-tile tunnels), Slinky, Ball, Lantern,
  Animal Flute, Top. Everything is a physical toy with emergent overlapping
  uses; almost every item has a "secret second reading" that regates the
  whole map. The design lesson: *tools that create geometry* (bubble) rather
  than tools that pass geometry.
- **La-Mulana**: mostly weapon/software unlocks plus Grapple Claw and Feather
  (double jump); its distinctive gating is *knowledge* — puzzles, tablets,
  and combined software apps gate progress more than movement. Knowledge
  gates are free for procgen (no solver support needed) but hostile to it
  (hand-authored meaning).
- **ESA's Hookshot**: any-surface 45° grapple; late game adds a *currents/
  gravity-flip* style finale. ESA is a good study in one strong analog verb
  carrying the back half of a game — and in how any-surface grapples make
  gating fuzzy.
- **Axiom Verge**: Address Disruptor (glitch terrain/enemies — corrupted
  tiles become passable, enemies transform into platforms), Remote Drone
  (a second, smaller body you pilot through drone-sized passages, then
  teleport to — morph ball *split off* from the player), passcode tool
  (rewrites the world state). The drone is the standout: "send a small
  proxy through, then join it" is a strong geometric verb.
- **Guacamelee's dimension swap**: flip between living/dead worlds where
  *platform layouts differ*; late puzzles require swapping mid-air. A binary
  world-state toggle is a discrete, solver-friendly analog of it.
- **SotN's transformation trio**: Bat (free flight — end-of-progression gate
  dissolver), Mist (pass through grates), Wolf (speed/swim). Also Gravity
  Boots' Shinespark-like vertical rocket jump.
- **Metroid Dread's Speed Booster + Spider Magnet + slide**: the slide is
  notable as a *baseline* small-passage answer given at minute one, with
  Morph Ball re-gating tighter passages later — two tiers of the same gate.
- **Iconoclasts' wrench**: turn bolts (rotating platforms/mechanisms),
  spin-glide on wires, later electrified to power rails — a single tool
  regated three times by upgrades acting on placed machinery.
- **Celeste** (fixed kit, but the reference point for our feel): climb with
  stamina, single dash refreshed on ground; every "unlock" is instead a
  *room mechanic* (springs, dream blocks, feathers, wind, moving blocks).
  Lesson: per-room environmental verbs can substitute for a large unlock set.

## 3. How unlocks compose

- **Synergies are the real content.** Hollow Knight's claw+dash+wings, Ori's
  bash-into-double-jump-into-glide chains, Pseudoregalia's slide-jump into
  wall kicks: late-game rooms gate on *combinations*, not single abilities.
  For procgen this means room metadata should record required ability *sets*
  (and ideally the frontier "passable with {A,B} but not {A} or {B}").
- **Soft vs hard gates** (Boss Keys terminology): a *hard* gate is
  impossible without the ability (Morph Ball tunnel); a *soft* gate is
  possible-but-hard (Hollow Knight pogo-jumping spikes without wings,
  Super Metroid's early walljump/bomb-jump). The genre's best-loved worlds
  (Super Metroid, Hollow Knight) deliberately leave soft gates in as
  sanctioned sequence breaks; Boss Keys documents how HK's "canonical" order
  is really a soft critical path over an open graph. A deterministic solver
  can *classify* this: a gate is soft if the solver finds a low-probability/
  frame-perfect route without the ability. That's a feature, not a bug —
  but procgen must not accidentally place a soft gate where a hard one is
  structurally required (softlock risk).
- **Gate-dissolvers vs gate-creators.** Double jump, any-surface grapple,
  flight, and free teleport retroactively erase earlier gate types; morph
  ball, swim, and block-breaks create *new orthogonal* gate types without
  weakening old ones. Orthogonal gates are what keep a large map legible —
  and they're much friendlier to a room-vocabulary procgen system, because
  each room's requirement set stays stable as the kit grows.
- **Roguelike hybrids gate the run-graph, not the room.** Dead Cells' runes
  (Vine, Teleport, Ram, Spider) are permanent meta-unlocks that open branch
  *exits* between procedurally generated biomes; Rogue Legacy gates little by
  ability and much by stats. Dead Cells is the closest structural precedent
  for downwards: fixed inter-zone topology + shuffled rooms + permanent
  traversal unlocks marking which exits/branches exist for this save.
- **Item-randomizer culture** (Hollow Knight RandomizerMod, Super Metroid
  randomizers) is worth studying: those communities have already formalized
  "which gates does each ability set pass" as machine-checkable logic files —
  exactly the artifact downwards' solver-verified procgen needs per room.

## 4. Combat-flavored unlocks that double as traversal

Downwards has no combat, so only the traversal reading matters:

- **Pogo / down-strike bounce** (Hollow Knight, Silksong, Shovel Knight,
  Zelda II): bounce off *something* below you — enemies, spikes, switches.
  Traversal reading: designated bounceable tiles/objects that refresh your
  jump/dash mid-air. HK's spike-pogo is the community's favorite soft gate.
- **Guacamelee's colored moves** (Rooster Uppercut, Dashing Derpderp, etc.):
  each combat move breaks matching blocks *and* displaces you (uppercut =
  high jump, headbutt = horizontal dash). Traversal reading: displacement
  moves with distinct arcs; the color-keyed blocks part is exactly the
  keyed-lock pattern downwards forbids.
- **Ori's Bash**: mechanically combat (redirect projectiles) but really the
  best traversal verb in the genre — see §2. Traversal reading: launch off
  placed objects.
- **Axiom Verge's Address Disruptor**: a gun, but its traversal reading is
  "make marked terrain passable."
- **Iconoclasts' wrench / Hollow Knight's Crystal Heart** (a chest-laser
  aesthetic on a horizontal super-dash — gates long flat corridors and
  crystal-rail alignments, launched from walls too).
- Screw Attack (Metroid): attack state during spin jump that also breaks
  terrain — traversal reading is a jump that passes destructible barriers.

## 5. Assessment against downwards' constraints

Constraints: pure traversal, geometric gating on 32x18 tile rooms, 60Hz
deterministic sim, AI solver must verify routes, descent-then-climb-out
structure. Trade-offs flagged, not resolved.

**Good fits:**
- **Small-passage answer (crawl/slide/shrink)** — the purest geometric gate;
  trivially solver-checkable (passage height < standing height); orthogonal
  to the existing gloves/boots so it doesn't erode current room vocabulary.
- **Double jump** — canonical, discrete, easy to verify. Trade-off: it is a
  gate-dissolver; existing wall-jump and dash rooms may need re-auditing, and
  in a descent game it mostly matters on the climb *out* — which could be a
  feature (unlock it at the bottom, like Metroid escape sequences) or could
  flatten the descent if given early.
- **Ground pound through cracked floors** — literally descent-themed
  ("unlock the way down"); discrete; solver-friendly. Caveat: breakable tiles
  edge toward keyed locks unless the pound has real geometric physics (needs
  air height above the floor, commits you downward past hazards).
- **Glide/slow-fall** — descent-flavored (gates "cross while falling", and
  drop-safety), discrete enough if it's a fixed fall-speed toggle. Trade-off:
  makes descents *safer*, which may dilute the hazard identity of falling.
- **Node-anchored grapple or bash-style launch points** — designer-placed
  anchors are geometry, are enumerable, and the solver reasons about a
  discrete graph of anchor points. Any-surface grapple is the version to
  avoid: continuous attach angles are solver-hostile and dissolve gates.
- **Player-placed temporary platform (Animal Well bubble)** — striking,
  fits no-combat perfectly, and is discrete if placement/lifetime are
  quantized. Trade-off: state-space blowup for the solver (where was it
  placed, when), and it can dissolve vertical gates if uncapped.
- **Speed/momentum runway moves (Shinespark-like)** — gorgeous geometric
  gating (runway length is the lock) and very on-theme for a Celeste-like.
  Trade-off: composes *across* rooms, so procgen must guarantee runway rooms
  adjacent to spark rooms; solver cost rises with carried momentum state.
- **Binary world/state toggle (Guacamelee-style)** — discrete, doubles every
  room's geometry cheaply. Trade-off: authoring cost per room doubles; can
  feel like a gimmick if underused.

**Poor fits / conflicts:**
- **Flight / infinite jump (Space Jump, Bat form), free teleport** —
  dissolve verticality entirely; in a game whose name and structure *are* the
  vertical axis, these break the descent contract. If ever used, only as a
  post-crown escape mechanic.
- **Any-surface grapple (ESA-style), analog swing physics** — continuous
  attach points and pendulum dynamics are hard for a deterministic tile
  solver to verify and make "impassable without X" claims unreliable.
- **Colored/keyed destructible blocks (Guacamelee, naive Metroid bombs)** —
  explicitly against the geometric-gating rule; they're door locks wearing
  tile skins.
- **Swim** — fine in principle (liquid regions are geometric) but adds a
  second movement model to the sim and solver; the Metroid "passable but
  crippled" version also creates soft gates everywhere, which procgen must
  then reason about.
- **Knowledge gates (La-Mulana)** — incompatible with solver verification
  (the solver can't know what the player knows) and with procgen reuse.
- **Enemy-dependent verbs (pogo off enemies, Bash off projectiles)** — no
  combat means no enemies to bounce off; the salvageable reading is
  *bounceable/launchable static objects*, which are just placed anchors.

**Structural note for the descent theme:** unlocks split naturally into
*descent verbs* (ground pound, glide, fast-fall dash) and *ascent verbs*
(double jump, wall climb, bubble, grapple-up). The get-out-alive victory
suggests a deliberate asymmetry — gate the way down with descent verbs and
make the climb out demand ascent verbs found near the bottom — but soft-gate
audits matter doubly here: an ascent verb obtained early must not let the
solver (or player) skip the bottom of the map.

---

## Sources

Game mechanics: as cited by game name above (Super Metroid, Metroid Dread,
SotN, Hollow Knight/Silksong, Ori 1/2, Celeste, ESA, Axiom Verge, La-Mulana,
Animal Well, Pseudoregalia, Guacamelee, Iconoclasts, Dead Cells, Rogue
Legacy).

Design writeups:
- Mark Brown, *Boss Keys* series (GMTK) — [The World Design of Super Metroid](https://www.youtube.com/watch?v=nn2MXwplMZA); [The World Design of Hollow Knight: Silksong](https://gmtk.substack.com/p/the-world-design-of-hollow-knight)
- [Ori and the Will of the Wisps and Strength in Traversal](https://viciousundertow.wordpress.com/2020/04/07/ori-and-the-will-of-the-wisps-and-strength-in-traversal/) (Vicious Undertow)
- [The Hidden Genius Behind Hollow Knight's Terrain Design](https://ludonauta.itch.io/platformer-essentials/devlog/1084669/the-hidden-genius-behind-hollow-knights-terrain-design) (Ludonauta)
- [Combat Analysis: Guacamelee](https://www.gamedeveloper.com/design/combat-analysis-guacamelee) (Game Developer)
- [Animal Well review — Play Critically](https://playcritically.com/2024/05/26/animal-well-review/)
- [Hollow Knight RandomizerMod](https://github.com/homothetyhk/RandomizerMod) — machine-readable ability-gating logic
- [ESA Hookshot](https://environmental-station-alpha.fandom.com/wiki/Hookshot); [Pseudoregalia wiki: Movement](https://pseudoregalia.fandom.com/wiki/Movement)
- [10 Most Iconic Metroidvania Power-Ups](https://www.dualshockers.com/most-iconic-metroidvania-power-ups/) (DualShockers)
- [Dead Cells runes overview](https://www.gamerevolution.com/guides/422993-dead-cells-runes-locations) (GameRevolution)
