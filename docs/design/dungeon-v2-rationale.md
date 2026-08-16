# Dungeon v2 — layout rationale

Layout file: `docs/design/dungeon-v2-layout.txt` (28 rooms, 32 edges, verdict ok).

## Shape

Three vertical columns of shafts, tied together by horizontal corridors at
several depths, so the map reads as a braided keep rather than a corridor:

- **West column:** greed-loop-a → strata-sort-a (spawn) → one-way-loop-b →
  tide-shaft-a → keyhole-vault-a.
- **Mid column:** boots-vault → shutter-chute-a → strata-sort-b →
  one-way-loop-b → strata-sort-c → shard-vault.
- **East column:** greed-loop-b → tide-shaft-b → one-way-loop-c →
  strata-sort-b (antechamber) → switchback-spine-a.
- A **far spur** (greed-loop-a → strata-sort-a → low-ceiling-arena-b, with
  greed-loop-c → low-ceiling-arena-c hanging off it) closes two more loops.

Cycle rank 5, all-unlocked diameter 7, zero same-class adjacencies, and the
only dead ends are the two vault rooms (shard vault, crown sanctum).

## The player journey

**Act 1 — bare-handed (no abilities).** Spawn in `s1` (strata-sort-a), an
easy three-door sorting hub. Everything nearby is easy: greed-loop-a above,
antiphase-airlock-a east, metronome-gallery-a on the top corridor. Roaming
bare, the player can reach most of the map's spine and passes **five visibly
locked doors**: two wall seals on the boots vault (metronome west door, the
shutter-chute ceiling), the wall seal on the wall-gate wing (`o4` east), the
dash seal on the keyhole vault's east door, and the dash-locked shard-vault
hatch under strata-sort-c. The **glove** sits in `ob` (keep-observatory), on
the mid corridor two rooms from the mid column — mid-early, but the player
has already walked past wall-locked doors to find it.

**Act 2 — glove.** The three wall seals open (3 backtrack-unlock events).
The boots vault is now reachable from either side (metronome corridor above,
shutter-chute climb below), and the wall-gate wing (`wg` → low-ceiling-arena-b)
opens as a loop back to the far spur. `bv` (keep-boots-vault) holds the
**boots** — so dash is deliberately two steps behind wall.

**Act 3 — boots.** The dash doors seen in Act 1 pay off (2 more events):
the keyhole vault's east door opens into low-ceiling-arena-c's dash pinches,
closing the big southern loop, and the shard-vault hatch under strata-sort-c
opens. The dash gate on the hatch doubles as soft-lock protection: the shard
vault's only exit is a dash/wall re-climb of its own chimney.

**Act 4 — the crown.** The antechamber `sb2` (strata-sort-b) is reached from
two independent directions, and its east door is a **30-coin door** (optimistic
route total is ~67 coins, so a player who has been grabbing easy coins walks
through; a rusher backtracks to greed rooms). Behind it: keep-crown-sanctum.

## Multiple routes

- Two fully edge-disjoint routes from spawn to the antechamber:
  1. `s1 → ap → gb → t2 → oc → sb2` (the east column drop; the `gb` floor
     door carries a small 6-coin toll, retreat west is always free).
  2. `s1 → o2 → br → ga2 → sa2 → gc → lcc → swb → sb2` (the far spur and
     the southern dash loop).
  A third braid runs `s1 → o2 → t1 → kv → lcc → swb → sb2`.
- The tool's `edge-disjoint-spawn-goal-routes` metric reads 1 and cannot read
  higher for this goal: keep-crown-sanctum has exactly one door, so every
  route shares the final `sb2–crown` edge by construction. Route multiplicity
  is therefore engineered (and verified by hand) up to the antechamber.
- Retreat safety: no door is coin-gated in both directions; every gated room's
  self-pairs solve bare, so bouncing off any locked door never strands you.

## Backtrack unlocks (6 events)

| Locked door seen bare | Opens with | Payoff |
|---|---|---|
| `met` west (wall) | glove | boots vault from above |
| `sh` ceiling (wall) | glove | boots vault from below |
| `o4` east (wall) | glove | wall-gate wing loop |
| `kv` east (dash) | boots | southern loop via low-ceiling-arena-c |
| `s5` floor (dash) | boots | shard vault treasure |
| `gb` floor / `sb2` east (coins 6 / 30) | coins | east shaft shortcut; crown door |

## Room table

| id | grid | class | difficulty | role |
|---|---|---|---|---|
| s1 | strata-sort-a | strata-sort | easy | spawn hub |
| ga | greed-loop-a | greed-loop | easy | top-west coin ring |
| met | metronome-gallery-a | metronome-gallery | easy | top corridor, wall seal |
| bv | keep-boots-vault | keep-boots-vault | keep | boots pickup |
| sh | shutter-chute-a | shutter-chute | easy | mid column shaft, wall seal |
| s3 | strata-sort-b | strata-sort | medium | mid crossroads |
| o4 | one-way-loop-b | one-way-loop | medium | mid column, wall-gated east door |
| s5 | strata-sort-c | strata-sort | hard | shard-vault gatehouse |
| sv | keep-shard-vault | keep-shard-vault | keep | dead-end treasure |
| ob | keep-observatory | keep-observatory | keep | glove pickup |
| ap | antiphase-airlock-a | antiphase-airlock | easy | spawn corridor |
| gb | greed-loop-b | greed-loop | medium | coin-toll turn into east column |
| t2 | tide-shaft-b | tide-shaft | medium | east column shaft |
| oc | one-way-loop-c | one-way-loop | hard | east junction over antechamber |
| sb2 | strata-sort-b | strata-sort | medium | crown antechamber, coin door |
| swb | switchback-spine-a | switchback-spine | easy | southern re-ascent into antechamber |
| crown | keep-crown-sanctum | keep-crown-sanctum | keep | goal |
| o2 | one-way-loop-b | one-way-loop | medium | west column junction |
| t1 | tide-shaft-a | tide-shaft | easy | west column shaft |
| kv | keyhole-vault-a | keyhole-vault | easy | west column foot, dash-gated east |
| br | braided-crossing-a | braided-crossing | easy | corridor to far spur |
| ga2 | greed-loop-a | greed-loop | easy | far spur entry |
| sa2 | strata-sort-a | strata-sort | easy | far spur junction |
| lcb | low-ceiling-arena-b | low-ceiling-arena | medium | wall-gate wing loop closer |
| wg | keep-wall-gate | keep-wall-gate | keep | wall-ability set piece |
| tc | two-clock-fork-b | two-clock-fork | medium | deep corridor s5→kv |
| lcc | low-ceiling-arena-c | low-ceiling-arena | hard | southern dash loop |
| gc | greed-loop-c | greed-loop | hard | optional dash coin branch |

Duplicated grids (one duplicate each, forced by the door-count arithmetic
needed to reach cycle rank 5): strata-sort-a, strata-sort-b, one-way-loop-b,
greed-loop-a. No duplicate is adjacent to its twin or to any same-class room.
