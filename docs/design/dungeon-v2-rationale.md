# Dungeon v2 — layout rationale (rev 2, post-critique)

Rev 2 is a rebuild of the braided keep in response to the 2026-08-16 critique
(15 confirmed findings: 5 blockers, 5 majors, 5 minors). The headline changes:
the ability arc is now mandatory (the crown sits behind a room that is
physically impassable without both abilities), the crown coin door is a real
toll (58 > the 55 coins bankable below loadout `both`), the spawn and the
antechamber have unique grids, and keep-wall-gate sits on the mainline.

Metrics (tools/dungeon_layout_metrics.py): verdict ok; 27 rooms, 31 edges,
cycle-rank 5, all-unlocked-diameter 8, dead-end-rooms 1 (crown),
same-class-adjacent-edges 0, backtrack-unlock-events 5,
goal-reachable-at none/wall/dash = False, at both = True, no absorbing traps
at any loadout.

## Shape

Three themed columns joined by the spawn row on top and a basement row on the
bottom, plus a dash-era deep wing:

- **West column (climbs):** greed-loop-a -> strata-sort-a (spawn) ->
  tide-shaft-a -> switchback-spine-a. Everything here climbs bare in both
  directions; it is the safety spine every retreat funnels into.
- **Mid column (shutter cascade):** keep-boots-vault -> shutter-chute-a ->
  one-way-loop-c -> shutter-chute-b -> one-way-loop-b -> low-ceiling-arena-c.
  Alternates timed-aperture drops with one-way ring rooms; lateral exits at
  one-way-loop-c (to the airlock braid) and one-way-loop-b (to the greed ring)
  break the fall every two rooms.
- **East stack (vaults):** greed-loop-b -> strata-sort-b -> tide-shaft-b ->
  strata-sort-c. The mainline crosses its head; its foot is the basement
  gatehouse.
- **Deep wing (dash era):** keyhole-vault-a, low-ceiling-arena-a,
  one-way-loop-b (2nd copy), tide-shaft-c, greed-loop-c, keyhole-vault-b —
  the only part of the map that needs boots, and the only place a grid
  repeats.

**Mainline:** s1 -> ob (glove) -> wg (keep-wall-gate) -> gb -> sb -> asl
(keep-astral-seal) -> crown — six edges, and every route to the crown crosses
both wg (wall-only geometry, plus `gate ob east ability wall`) and asl
(passable west->east only at loadout `both`). The coin door
`gate asl east coins 58` is the single coin gate in the dungeon.

## Acts

**Act 1 — bare prologue (8 rooms).** The glove is one easy room from spawn:
s1 (easy) -> keep-observatory, passable bare in both directions. Before
grabbing it the player can lap the ga/met coin ring above spawn and the
t1-swa-tcb basement loop below, and sees three locked doors bare: ob east
(wall), met west (wall), kvb west (wall). Every bare state retreats to spawn:
the west column climbs at `none` end to end.

**Act 2 — wall (22 rooms).** The glove opens the mainline (ob east -> wg,
the wall showcase, crossed head-on) and the boots wing (met west -> bv:
**boots**). The east stack (gb, sb, tb, ssc) and the whole mid cascade open,
including the wall-era shortcut lcc -> kvb -> tcb -> swa that loops the
basement back to the west column. Three locked doors are seen during this
act: ssc floor (dash), gc floor (dash), and the asl coin door.

**Act 3 — both (27 rooms).** Boots + glove open four rooms never seen
before — kva, lca, tsc, ow2 — plus the crossing of asl itself: the deep wing
is a braid of dash crawls (lca teaches the low-lid rule at the easy tier
before lcc's 1-tile pinches are required), the tsc/gc hard ring, and the two
keyhole junctions. Its 11 coins are what push the purse over the crown door.
The finale ramps hard: ssc (hard) -> kva -> ... -> asl (both-ability
gauntlet) -> crown.

## Coin arithmetic

Total coins on the map: **66**. Bankable by fixed loadout (rooms reachable
per the metrics tool):

| loadout | rooms | coins bankable |
|---|---|---|
| none | 8 | 22 |
| wall | 22 | 55 |
| dash | 8 | 22 |
| both | 27 | 66 |

Crown door: **58 coins**. 58 > 55, so no sub-`both` loadout can bank the
toll even sweeping everything it can reach; at `both` the margin is 8 coins
(sweep ~88%), so the door demands the deep wing's 11 coins, not a full-clear.
There is no small "teaching" coin door — coin doors appear exactly once, at
the crown, so the vocabulary is not diluted (finding 7).

## Backtrack payoffs

| Locked door seen | Opens with | Payoff |
|---|---|---|
| ob east (Act 1) | glove | the entire mainline east of spawn |
| met west (Act 1) | glove | boots vault |
| kvb west (Act 1) | glove | basement shortcut into the mid cascade |
| ssc floor (Act 2) | boots | deep-wing junction (kva) and the wing braid |
| gc floor (Act 2) | boots | tsc/ow2/lca dash ring and its coins |
| asl east (Act 2) | 58 coins + both | crown |

Metric: backtrack-unlock-events 5.

## Retreat safety

The metrics tool's absorbing-trap check passes at all four loadouts: every
reachable (room, entry) state can walk back to spawn at its own loadout.
Load-bearing choices: the west column and switchback/tcb basement climb bare
both ways; one-way descents (owc, owb, shb) all sit behind wall gates, and
each has a same-loadout return (owc floor->west at none, owb east->ceiling
and shb floor->ceiling at wall); the dash-sealed wing is entered only at
`both`, at which every wing pair needed for the walk home solves. Rooms whose
grids cannot be re-ascended bare (kva, kvb interiors) are unreachable below
the loadout that escapes them, and every gate bounce has a bare self-pair.

## Room table

| id | grid | difficulty | role |
|---|---|---|---|
| s1 | strata-sort-a | easy | spawn hub (unique grid — no duplicate) |
| ga | greed-loop-a | easy | coin ring above spawn |
| met | metronome-gallery-a | easy | boots-vault approach |
| bv | keep-boots-vault | keep | boots pickup |
| ob | keep-observatory | keep | glove pickup, 1 room from spawn |
| wg | keep-wall-gate | keep | wall showcase, on the mainline |
| gb | greed-loop-b | medium | east stack head |
| sb | strata-sort-b | medium | east crossroads, crown antechamber approach |
| tb | tide-shaft-b | medium | east stack shaft |
| ssc | strata-sort-c | hard | basement gatehouse, dash seal |
| ap | antiphase-airlock-a | easy | braid corridor ssc <-> owc |
| asl | keep-astral-seal | keep | final gauntlet, both-only, coin door |
| crown | keep-crown-sanctum | keep | goal (only dead end) |
| t1 | tide-shaft-a | easy | west climb shaft |
| swa | switchback-spine-a | easy | west basement climber |
| tcb | two-clock-fork-b | medium | basement timing corridor |
| sha | shutter-chute-a | easy | mid cascade aperture |
| owc | one-way-loop-c | hard | mid cascade junction (wall-up ring) |
| shb | shutter-chute-b | medium | mid cascade aperture |
| owb | one-way-loop-b | medium | mid cascade ring — duplicate #1, optional wing |
| lcc | low-ceiling-arena-c | hard | deep crawl junction |
| kva | keyhole-vault-a | easy | deep wing junction (Act 3 only) |
| kvb | keyhole-vault-b | medium | wing/basement junction |
| lca | low-ceiling-arena-a | easy | dash teaching corridor (Act 3 only) |
| gc | greed-loop-c | hard | dash coin ring (Act 3 only) |
| tsc | tide-shaft-c | hard | deep wing shaft (Act 3 only) |
| ow2 | one-way-loop-b | medium | deep wing ring — duplicate #2, optional wing |

Class census (27 rooms): strata x3, tide x3, greed x3, one-way x3 (two of
them the deliberate duplicate pair), shutter x2, keyhole x2, low-ceiling x2,
metronome, antiphase, switchback, two-clock x1 each, keeps x6. The two
largest classes together supply 6/27 = 22% of the dungeon.

## Critique disposition (15 confirmed findings)

1. **Ability arc optional (blocker).** Fixed. keep-astral-seal sits on the
   only door into the crown and is impassable below `both`;
   goal-reachable-at none/wall/dash all report False.
2. **30-coin door not a toll (blocker).** Fixed. Door raised to 58 against
   55 bankable below `both` (table above); the 6-coin toll is gone, so this
   is the only coin door and it forces the Act 3 sweep.
3. **Glove only via a hard room (major).** Fixed. The glove room
   (keep-observatory) is adjacent to the easy spawn room and passable bare
   both ways; no hard room stands before the first ability.
4. **Act 4 fizzles, no new endgame rooms (major).** Fixed. Loadout `both`
   newly opens kva, lca, gc, tsc, ow2 and the asl crossing — five rooms plus
   the finale that no earlier act can touch — and the difficulty rises into
   the crown (ssc hard -> wing -> asl gauntlet).
5. **Duplicate grids at landmark junctions (blocker).** Fixed. Spawn and
   antechamber grids are unique; the only duplicated grid is one-way-loop-b,
   both copies in optional wing/cascade positions off the mainline.
6. **Columns are class permutations (major).** Fixed. West = bare climbs
   (tide/switchback), mid = shutter cascade with one-way rings, east =
   greed/strata vault stack, deep wing = keyhole/low-ceiling braid; no two
   columns share their sequence, and greed-loops no longer head 4/5 shafts.
7. **6-coin toll dilutes coin-door vocabulary (minor).** Fixed by deletion:
   exactly one coin door exists (the crown's), so the vocabulary is taught
   once, where it matters.
8. **Shard vault dead-end with no payoff (minor).** Fixed by removal. The
   dungeon's only dead end is the crown; the dash treasure role moved to the
   gc ring, which is a loop, not a cul-de-sac.
9. **Absorbing trap {kv,tc,s5,o4} (blocker).** Fixed. The new topology
   passes the tool's absorbing-trap check at every loadout (see Retreat
   safety); the old kv funnel no longer exists.
10. **Crown is a bare six-room walk (blocker).** Fixed. The bare-reachable
    set is 8 rooms and does not contain wg's far side, let alone the crown;
    both wall (wg) and both (asl) stand on the mainline.
11. **Six consecutive mid-column descents (major).** Fixed. The mainline has
    no consecutive descents; the optional cascade alternates shutter and
    one-way verbs with lateral exits every two rooms, and the two remaining
    strata siblings (sb, ssc) sit in different roles two rooms apart with a
    tide shaft between them, in the east stack rather than a single chute.
12. **Two classes supply a third of the dungeon (major).** Fixed. Largest
    two classes are 6/27 = 22% (was 9/28 = 32%); seven classes appear once.
13. **keep-wall-gate in an optional wing (major).** Fixed. wg is mainline
    room three; every spawn->crown route crosses it head-on right after the
    glove.
14. **Long braid has no difficulty ramp (minor).** Fixed. Both long routes
    ramp: mainline easy (s1/ob) -> keep/medium (wg/gb/sb) -> both-gauntlet
    (asl); the basement braid runs easy west rooms into hard lcc/ssc before
    rejoining, and Act 3's wing is uniformly late and hard except its
    deliberate easy teaching rooms.
15. **low-ceiling-arena has no teaching instance (minor).** Fixed.
    low-ceiling-arena-a (easy) is the wing's entry corridor, met with boots
    in hand before lcc's hard crawls are ever required.
