#!/usr/bin/env python3
"""Seeded end-to-end dungeon layout generator.

Wraps the skeleton-search + signature-fill pipeline from
tools/dungeon_layout_solver.py into a single deterministic run:

  seed + params  ->  complete candidate layout file

The layout follows every rule dungeon_layout_metrics.py enforces by
construction where possible (grid-exact embedding, exit room on the
top row with its ceiling door unwired, geometric gating only, coin
arithmetic) and relies on tools/dungeon_check.py rejection sampling
for the sim-dependent rules (no absorbing traps, act coherence,
difficulty band).

Shape of every dungeon (the validated v2 family, parameterized):
  * a roof row (the exit room's ceiling is the escape), spawn directly
    under the exit room (descent theme: spawn high),
  * a full block of W x H rooms,
  * a partial bottom row ending east in keep-crown-sanctum (crown low),
    guarded by the loadout's GUARD room — the one crossing into the
    crown cul-de-sac that demands the dungeon's traversal items — and
    carrying the single coin gate on its east door.
  * the loadout's pickups placed on rooms that are a peak-so-far
    challenge to reach at the band's quantile (verified by
    dungeon_check.py).

LOADOUT AXIS (designer requirement, 2026-08-17): the traversal items a
dungeon contains are a first-class parameter. --loadout both places
glove+boots and guards the crown with keep-astral-seal; wall places
the glove only and guards with keep-wall-gate; dash places the boots
only and guards with keep-meteor-run; none places no traversal item at
all and the crown's only key is the coin gate. Difficulty is meant to
be independent of the loadout: the band is hit by room-demand
targeting (below) plus footprint size, not by ability count, so a bare
dungeon can sit in the high band.

BAND TARGETING: every vocabulary room carries a `demand` rating in
[0, 1] blended from its mean transfer solve ticks and its shaky-hand
robustness deficit. The fill step samples rooms around the band's
demand target, which moves the intensive terms of the difficulty score
(peak/mean transfer ticks, per-coin control demand) instead of only
the extensive ones. Before this, band control came from footprint size
and coin fraction alone and every generated dungeon scored within a
few points of every other.

Usage:
  python3 tools/dungeon_generator.py --seed N [--size small|medium|large]
      [--band low|mid|high] [--loadout both|wall|dash|none]
      [--acts 3] [--out FILE]

Determinism: same seed+params -> byte-identical layout.
Exit code 0 on success (layout written), 2 if this seed found no
consistent fill (callers should just move to the next seed).
"""

import argparse
import itertools
import json
import pathlib
import random
import sys
from collections import Counter

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from dungeon_layout_solver import (  # noqa: E402
    DIRNAME, OPP, analyse, shape_of, vocabulary,
)
from dungeon_difficulty import (  # noqa: E402
    Assessor, one_way_states, reachable_states,
)
from dungeon_layout_metrics import Layout  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
ROOMS = ROOT / "crates" / "downwards-gen" / "rooms-v2"
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"

# Rooms a concurrent agent owns: do not select them into new layouts.
EXCLUDED = {"eaves-walk-a", "strata-sort-b"}

# The crown cul-de-sac's guard, per loadout: the room whose west->east
# crossing is exactly the traversal demand the dungeon is built around.
# `sig` is the door signature the skeleton must hand that cell.
LOADOUTS = {
    "both": {"pickups": ("glove", "boots"), "guard": "keep-astral-seal",
             "sig": frozenset("cew"), "final": "both"},
    "wall": {"pickups": ("glove",), "guard": "keep-wall-gate",
             "sig": frozenset("we"), "final": "wall"},
    "dash": {"pickups": ("boots",), "guard": "keep-meteor-run",
             "sig": frozenset("we"), "final": "dash"},
    "none": {"pickups": (), "guard": None,
             "sig": frozenset("we"), "final": "none"},
}
# Rooms only ever placed by a pin (crown, or a loadout's guard).
PIN_ONLY = {"keep-crown-sanctum"} | {
    cfg["guard"] for cfg in LOADOUTS.values() if cfg["guard"]
}

SIZES = {
    "small": {"w": 5, "h": 3, "diameter": 7, "rank": (4, 7)},
    "medium": {"w": 6, "h": 4, "diameter": 8, "rank": (5, 9)},
    "large": {"w": 7, "h": 4, "diameter": 9, "rank": (6, 10)},
}
# Coin gate as a fraction of the dungeon's collectable coins, the
# quantile of approach cost used when placing pickups, and the room
# demand rating the fill aims at (see module docstring).
BANDS = {
    "low": {"coin_frac": 0.5, "quantile": 0.5, "demand": 0.15},
    "mid": {"coin_frac": 0.72, "quantile": 0.75, "demand": 0.5},
    "high": {"coin_frac": 0.95, "quantile": 1.0, "demand": 0.9},
}
# Width of the demand-sampling kernel: small enough to bite, wide
# enough that the fill still has choices and layouts stay varied.
DEMAND_SIGMA = 0.22
# Empirical per-loadout calibration (2026-08-17 sweep): at equal band and
# size, glove-only dungeons scored ~5 points above both-item ones — the
# wall gate is an expensive room and the routes it forces are long — so
# their demand target is pulled down to keep the bands comparable across
# the loadout axis, which is the whole point of the axis.
LOADOUT_DEMAND_SHIFT = {"both": 0.0, "wall": -0.15, "dash": -0.05, "none": 0.0}
# Relative sampling weight of a room that is one-way at some feasible
# loadout, on a cell where the skeleton offers a way back round, and how
# many such rooms a dungeon may earn through validated swaps.
ONE_WAY_WEIGHT = 0.3
# How many single-room repairs a candidate fill may be given before it is
# abandoned for a fresh one.
REPAIR_STEPS = 60

# Pickup approaches must never be free (designer rule: peak-so-far
# challenge). Candidate rooms cheaper than this to reach are skipped.
MIN_PICKUP_APPROACH_TICKS = 150

# Coin-optimistic exploration (as the metrics tool does when it asks
# whether a state can get home): gates are about abilities here.
OPTIMISTIC_COINS = 10 ** 9

ALL_LOADOUTS = ("none", "wall", "dash", "both")
ALLOWED = {
    "none": ("none",),
    "wall": ("none", "wall"),
    "dash": ("none", "dash"),
    "both": ALL_LOADOUTS,
}


class InlineText:
    """A stand-in for a pathlib.Path holding a layout: the repair pass
    scores hundreds of candidate fills, and none of them deserve a trip
    through the filesystem."""

    def __init__(self, text):
        self.text = text

    def read_text(self):
        return self.text


def passable(entry, doors, entry_door, exit_door, loadout):
    slot = entry.get("pairs", {}).get(f"{entry_door}->{exit_door}", {})
    return any(slot.get(l) is not None for l in ALLOWED[loadout])


def collectable_coins(entry, loadout):
    routes = entry.get("coin_routes")
    if not routes:
        return entry.get("coins", 0)
    return sum(
        1 for slot in routes.values()
        if any(slot.get(l) is not None for l in ALLOWED[loadout])
    )


def room_demand(table):
    """slug -> difficulty demand in [0, 1].

    Blends the room's mean transfer solve ticks (per-transfer control
    demand) with its shaky-hand robustness deficit (how much a wobbly
    input costs you), both min-max normalised over the vocabulary.
    Same-door turnarounds are ignored: they are not transfers."""
    raw = {}
    for slug, entry in table.items():
        ticks = []
        for pair, slot in entry.get("pairs", {}).items():
            source, target = pair.split("->")
            if source == target:
                continue
            values = [v for v in slot.values() if v is not None]
            if values:
                ticks.append(min(values))
        mean_ticks = sum(ticks) / len(ticks) if ticks else 0.0
        shaky = entry.get("shaky", {})
        deficit = (
            sum(1.0 - s["worst"] for s in shaky.values()) / len(shaky)
            if shaky else 0.5
        )
        raw[slug] = (mean_ticks, deficit)
    lo = min(t for t, _ in raw.values() if t > 0)
    hi = max(t for t, _ in raw.values())
    demand = {}
    for slug, (mean_ticks, deficit) in raw.items():
        if mean_ticks <= 0:
            demand[slug] = 0.0
            continue
        scaled = (mean_ticks - lo) / max(1e-9, hi - lo)
        demand[slug] = round(0.65 * scaled + 0.35 * deficit, 4)
    return demand


def usable_rooms(table, loadout):
    """Slugs whose doors all reach each other at this loadout.

    A room that cannot be crossed with the kit the dungeon contains is a
    wall with doors painted on it: place enough of them and no skeleton
    survives. At loadout `none` only 29 of the 51 selectable grids pass —
    which is why the skeleton search has to know the loadout before it
    decides how many four-door junctions to ask for."""
    usable = set()
    for slug, entry in table.items():
        doors = entry.get("doors", [])
        if not doors:
            continue
        ok = True
        for source in doors:
            seen = {source}
            stack = [source]
            while stack:
                here = stack.pop()
                for target in doors:
                    if target not in seen and passable(
                            entry, doors, here, target, loadout):
                        seen.add(target)
                        stack.append(target)
            if len(seen) < len(doors):
                ok = False
            if len(doors) == 1 and not passable(
                    entry, doors, doors[0], doors[0], loadout):
                ok = False
        if ok:
            usable.add(slug)
    return usable


def feasible_loadouts(loadout):
    """The loadouts a player can actually hold in a dungeon built for this
    one. With no boots in the dungeon, 'dash' and 'both' never happen."""
    if loadout == "both":
        return ALL_LOADOUTS
    if loadout == "none":
        return ("none",)
    return ("none", loadout)


def reversible_rooms(table, loadouts=ALL_LOADOUTS):
    """Slugs you can always walk back out of, at every feasible loadout:
    every door has a turnaround (enter and leave by the same door) and
    every crossing that works one way works the other way too.

    This is the trap-free primitive. If every room in a dungeon is
    reversible then every reachable state can retreat — turn round in the
    room you are in, step back through the door you came by, turn round
    there — so no fill of reversible rooms can produce an absorbing trap,
    whatever the skeleton looks like. One-way rooms are still wanted (the
    one-way loop is a design staple), but they go in afterwards, one
    validated swap at a time, where the graph offers a way back round."""
    safe = set()
    for slug, entry in table.items():
        doors = entry.get("doors", [])
        ok = True
        for loadout in loadouts:
            for a in doors:
                if not passable(entry, doors, a, a, loadout):
                    ok = False
                for b in doors:
                    if a == b:
                        continue
                    if passable(entry, doors, a, b, loadout) and not passable(
                            entry, doors, b, a, loadout):
                        ok = False
        if ok:
            safe.add(slug)
    return safe


def bridge_cells(cells, edges):
    """Cells incident to a bridge (an edge whose removal disconnects the
    skeleton). Retreat through such a cell has no alternative route."""
    adj = {c: [] for c in cells}
    for index, (a, b) in enumerate(edges):
        adj[a].append((b, index))
        adj[b].append((a, index))
    disc, low = {}, {}
    timer = [0]
    fragile = set()

    def walk(node, via):
        disc[node] = low[node] = timer[0]
        timer[0] += 1
        for nxt, index in adj[node]:
            if index == via:
                continue
            if nxt in disc:
                low[node] = min(low[node], disc[nxt])
                continue
            walk(nxt, index)
            low[node] = min(low[node], low[nxt])
            if low[nxt] > disc[node]:
                fragile.add(node)
                fragile.add(nxt)

    for cell in cells:
        if cell not in disc:
            walk(cell, -1)
    return fragile


def build_footprint(rng, size, loadout):
    cfg = SIZES[size]
    w, h = cfg["w"], cfg["h"]
    block = [(x, y) for y in range(1, h + 1) for x in range(1, w + 1)]
    roof_len = rng.randint(2, 3)
    roof_x = rng.randint(1, w - roof_len + 1)
    roof = [(x, 0) for x in range(roof_x, roof_x + roof_len)]
    bottom_len = rng.randint(3, 4)
    crown_x = rng.choice([w - 1, w])
    bottom_x = crown_x - bottom_len + 1
    if bottom_x < 1:
        bottom_x = 1
        bottom_len = crown_x - bottom_x + 1
    bottom = [(x, h + 1) for x in range(bottom_x, crown_x + 1)]
    crown = (crown_x, h + 1)
    seal = (crown_x - 1, h + 1)
    exit_cell = rng.choice(roof)
    spawn = (exit_cell[0], 1)
    return {
        "cells": roof + block + bottom,
        "crown": crown,
        "seal": seal,
        "seal_sig": LOADOUTS[loadout]["sig"],
        "exit": exit_cell,
        "spawn": spawn,
        "diameter": cfg["diameter"],
        "rank": cfg["rank"],
    }


def required_sigs(fp, edges):
    """cell -> required door signature given current edges (exit cell's
    signature gains an unwired ceiling door)."""
    result = analyse(fp["cells"], edges)
    if result is None:
        return None
    _, adj = result
    sigs = {c: shape_of(c, adj) for c in fp["cells"]}
    sigs[fp["exit"]] = sigs[fp["exit"]] | {"c"}
    return sigs, adj


def penalty(fp, edges, supply):
    got = required_sigs(fp, edges)
    if got is None:
        return 1000
    sigs, adj = got
    score = 0
    result = analyse(fp["cells"], edges)
    diameter, _ = result
    if diameter > fp["diameter"]:
        score += (diameter - fp["diameter"]) * 20
    rank = len(edges) - len(fp["cells"]) + 1
    lo, hi = fp["rank"]
    if rank < lo:
        score += (lo - rank) * 8
    if rank > hi:
        score += (rank - hi) * 3
    # Dead ends are cul-de-sacs, and a cul-de-sac cell needs a room with
    # exactly one door. The crown is always one; anything beyond that
    # depends on what the vocabulary can seat at this loadout, so the
    # skeleton is allowed a pocket rather than forced into one.
    dead = [c for c in fp["cells"] if len(adj[c]) == 1]
    score += 6 * max(0, 1 - len(dead)) + 6 * max(0, len(dead) - fp["dead_max"])
    if sigs[fp["crown"]] != frozenset("w"):
        score += 25
    if sigs[fp["seal"]] != fp["seal_sig"]:
        score += 25
    if "f" not in sigs[fp["exit"]]:
        score += 25
    counts = Counter(sigs.values())
    counts[frozenset("w")] -= 1  # crown pin
    counts[fp["seal_sig"]] -= 1  # guard pin
    for sig, n in counts.items():
        cap = supply.get(sig, 0)
        if n > cap:
            score += 6 * (n - cap)
    # single-class signatures must not sit adjacent to themselves
    singles = [s for s, cap in supply.items() if cap <= 3]
    for sig in singles:
        placed = [c for c, s in sigs.items() if s == sig]
        for a, b in itertools.combinations(placed, 2):
            if abs(a[0] - b[0]) + abs(a[1] - b[1]) == 1:
                score += 5
    return score


def anneal(fp, rng, supply, iterations=15000):
    cells = set(fp["cells"])
    crown, seal = fp["crown"], fp["seal"]
    forced = [
        (seal, crown),
        ((seal[0] - 1, seal[1]), seal),
    ]
    if "c" in fp["seal_sig"]:
        forced.append(((seal[0], seal[1] - 1), seal))
    else:
        # The guard has no ceiling door, so the bottom row hangs off the
        # block through the cell west of it instead: without this the
        # crown wing can float free of the dungeon.
        forced.append(((seal[0] - 1, seal[1] - 1), (seal[0] - 1, seal[1])))
    forced.append((fp["exit"], fp["spawn"]))
    forced = [e for e in forced if e[0] in cells and e[1] in cells]
    # The guard cell's signature is pinned, so no optional edge may touch
    # it, and nothing may reach the crown except through the guard.
    frozen = {crown, seal}
    optional = [
        ((x, y), (x + dx, y + dy))
        for (x, y) in fp["cells"]
        for dx, dy in ((1, 0), (0, 1))
        if (x + dx, y + dy) in cells
        and ((x, y), (x + dx, y + dy)) not in forced
        and (x + dx, y + dy) not in frozen and (x, y) not in frozen
    ]
    # The diameter cap must be achievable for this footprint: adding edges
    # only shrinks the diameter, so the all-edges diameter is the floor.
    full_result = analyse(fp["cells"], forced + optional)
    if full_result is not None:
        fp["diameter"] = max(fp["diameter"], full_result[0] + 1)
    current = forced + [e for e in optional if rng.random() < 0.7]
    best_score = penalty(fp, current, supply)
    best = list(current)
    temperature = 8.0
    for _ in range(iterations):
        edge = rng.choice(optional)
        if edge in current:
            trial = [e for e in current if e != edge]
        else:
            trial = current + [edge]
        trial_score = penalty(fp, trial, supply)
        if trial_score <= best_score or rng.random() < pow(2.718, -(trial_score - best_score) / temperature):
            current = trial
            if trial_score < best_score:
                best_score, best = trial_score, list(trial)
        temperature *= 0.9995
        if best_score == 0:
            break
    return best_score, best


def fill(fp, edges, vocab, rng, guard, demand, safe, target, seal_pool=None,
         reversible_only=False):
    got = required_sigs(fp, edges)
    if got is None:
        return None
    sigs, adj = got
    pins = {fp["crown"]: "keep-crown-sanctum"}
    if guard:
        pins[fp["seal"]] = guard
    fragile = bridge_cells(fp["cells"], edges)
    by_sig = {}
    for slug, info in vocab.items():
        if slug in EXCLUDED or slug in PIN_ONLY:
            continue
        by_sig.setdefault(info["doors"], []).append(slug)
    order = sorted(fp["cells"], key=lambda c: len(by_sig.get(sigs[c], [])))
    assignment = {}

    def ranked(cell):
        """Candidates for a cell, sampled without replacement with weight
        concentrated on the band's demand target — so the fill decides how
        hard the dungeon plays, not just how big it is."""
        pool = sorted(by_sig.get(sigs[cell], []))
        if cell == fp["seal"] and seal_pool is not None:
            pool = [s for s in pool if s in seal_pool]
        if reversible_only or cell in fragile:
            pool = [s for s in pool if s in safe] or pool
        weights = [
            pow(2.718, -((demand.get(s, 0.5) - target) ** 2)
                / (2 * DEMAND_SIGMA * DEMAND_SIGMA))
            # One-way rooms are wanted, but a dungeon full of them is a
            # dungeon full of absorbing traps: keep them a minority so the
            # trap check is a formality rather than the main rejection.
            * (1.0 if s in safe else ONE_WAY_WEIGHT)
            for s in pool
        ]
        picked = []
        while pool:
            total = sum(weights)
            roll = rng.random() * total
            for index, weight in enumerate(weights):
                roll -= weight
                if roll <= 0:
                    break
            picked.append(pool.pop(index))
            weights.pop(index)
        return picked

    def rec(i):
        if i == len(order):
            return True
        cell = order[i]
        if cell in pins:
            options = [pins[cell]]
        else:
            options = ranked(cell)
        used = set(assignment.values())
        for slug in options:
            if vocab[slug]["doors"] != sigs[cell]:
                continue
            # No grid twice in a dungeon: a repeated room reads as filler
            # (and the metrics tool rejects it outright).
            if slug in used:
                continue
            cls = vocab[slug]["class"]
            if any(
                vocab[assignment[n]]["class"] == cls
                for n in adj[cell]
                if n in assignment
            ):
                continue
            assignment[cell] = slug
            if rec(i + 1):
                return True
            del assignment[cell]
        return False

    return assignment if rec(0) else None


def pickup_spot(slug):
    """Deterministic standable open tile in the room grid: open air with
    solid ground below, no spikes, near mid-room. Returns pixel coords."""
    grid = (ROOMS / f"{slug}.txt").read_text().splitlines()
    candidates = []
    for ty in range(1, len(grid) - 1):
        for tx in range(1, len(grid[ty]) - 1):
            if grid[ty][tx] != ".":
                continue
            below = grid[ty + 1][tx]
            if below not in "#-":
                continue
            if "^" in (grid[ty - 1][tx], grid[ty][tx - 1], grid[ty][tx + 1]):
                continue
            candidates.append((abs(tx - 16) + abs(ty - 9), tx, ty))
    if not candidates:
        return 160, 90
    _, tx, ty = min(candidates)
    return tx * 10, ty * 10


def choose_pickup(assessor, starts, loadout, candidates, quantile, rng):
    """Rank candidate rooms by cheapest approach cost at the loadout and
    pick at the band's quantile. Returns (room, cost) or None."""
    costs = []
    for room in sorted(candidates):
        leg = assessor.dijkstra(starts, loadout, 0, {room})
        if leg is not None and leg[0] >= MIN_PICKUP_APPROACH_TICKS:
            costs.append((leg[0], room))
    if not costs:
        return None
    costs.sort()
    index = min(len(costs) - 1, int(round(quantile * (len(costs) - 1))))
    return costs[index][1], costs[index][0]


def reachable_rooms(assessor, starts, loadout):
    seen = set()
    dist = {}
    import heapq
    heap = [(c, s) for s, c in starts]
    heapq.heapify(heap)
    while heap:
        cost, state = heapq.heappop(heap)
        if state in dist:
            continue
        dist[state] = cost
        room, entry = state
        seen.add(room)
        for exit_door in assessor.doors(room):
            ticks = assessor.cross_ticks(room, entry, exit_door, loadout)
            if ticks is None:
                continue
            nxt = assessor.step.get((room, exit_door))
            if nxt and nxt not in dist:
                heapq.heappush(heap, (cost + ticks, nxt))
    return seen


def trapped_states(assessor, loadouts=ALL_LOADOUTS):
    """(loadout, room, entry) states that cannot retreat to spawn — the
    absorbing traps dungeon_layout_metrics rejects, listed rather than
    merely counted so the fill can be repaired where it actually fails."""
    layout = assessor.layout
    trapped = set()
    for loadout in loadouts:
        visited = set()
        frontier = [s for s, _ in assessor.spawn_starts()]
        while frontier:
            state = frontier.pop()
            if state in visited:
                continue
            visited.add(state)
            room, entry = state
            for exit_door in assessor.doors(room):
                if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                    continue
                nxt = assessor.step.get((room, exit_door))
                if nxt and nxt not in visited:
                    frontier.append(nxt)
        for start in visited:
            seen = set()
            stack = [start]
            retreated = False
            while stack:
                room, entry = stack.pop()
                if room == layout.spawn:
                    retreated = True
                    break
                if (room, entry) in seen:
                    continue
                seen.add((room, entry))
                for exit_door in assessor.doors(room):
                    if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                        continue
                    nxt = assessor.step.get((room, exit_door))
                    if nxt:
                        stack.append(nxt)
            if not retreated:
                trapped.add((loadout,) + start)
    return trapped


def has_absorbing_trap(assessor, loadouts=ALL_LOADOUTS):
    """Mirror of dungeon_layout_metrics: at every FEASIBLE fixed loadout,
    every reachable (room, entry) state must be able to retreat to spawn
    (coin gates treated as open, as the metrics tool does)."""
    layout = assessor.layout
    for loadout in loadouts:
        visited = set()
        frontier = list(assessor.spawn_starts())
        frontier = [s for s, _ in frontier]
        while frontier:
            state = frontier.pop()
            if state in visited:
                continue
            visited.add(state)
            room, entry = state
            for exit_door in assessor.doors(room):
                if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                    continue
                nxt = assessor.step.get((room, exit_door))
                if nxt and nxt not in visited:
                    frontier.append(nxt)
        for start in visited:
            seen = set()
            stack = [start]
            retreated = False
            while stack:
                room, entry = stack.pop()
                if room == layout.spawn:
                    retreated = True
                    break
                if (room, entry) in seen:
                    continue
                seen.add((room, entry))
                for exit_door in assessor.doors(room):
                    if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                        continue
                    nxt = assessor.step.get((room, exit_door))
                    if nxt:
                        stack.append(nxt)
            if not retreated:
                return True
    return False


def generate(seed, size, band, acts, loadout="both"):
    rng = random.Random(f"{seed}|{size}|{band}|{acts}|{loadout}")
    table = json.loads(PASSABILITY.read_text())
    vocab = vocabulary()
    # Exact supply: since 2026-08-17 the metrics tool rejects any dungeon
    # that uses a grid twice (repeated rooms read as filler), so a
    # signature can seat exactly as many cells as there are distinct
    # grids carrying it — not the old optimistic class-count estimate.
    cfg = LOADOUTS[loadout]
    final = cfg["final"]
    crossable = usable_rooms(table, final)
    supply = Counter(
        info["doors"] for slug, info in vocab.items()
        if slug not in EXCLUDED and slug not in PIN_ONLY and slug in crossable
    )
    feasible = feasible_loadouts(final)
    demand = room_demand(table)
    # The rooms the fill leans on: crossable with this dungeon's kit and
    # impossible to get stranded in.
    safe = reversible_rooms(table, feasible) & crossable
    target = min(1.0, max(0.0, BANDS[band]["demand"] + LOADOUT_DEMAND_SHIFT[final]))
    guard = cfg["guard"]
    seal_pool = None
    if guard is None:
        # No traversal item exists, so the crown's guard is chosen for
        # the one property the cul-de-sac needs: you can walk in and you
        # can walk back out with nothing in your pockets.
        seal_pool = {
            slug for slug, entry in table.items()
            if slug not in EXCLUDED and slug not in PIN_ONLY
            and passable(entry, entry["doors"], "west", "east", final)
            and passable(entry, entry["doors"], "east", "west", final)
        }
    fp = build_footprint(rng, size, loadout)
    # One pocket per single-door room the loadout can actually get out of
    # again, on top of the crown's own dead end.
    pockets = sum(
        1 for slug, info in vocab.items()
        if slug not in EXCLUDED and slug not in PIN_ONLY
        and len(info["doors"]) == 1 and slug in crossable
    )
    fp["dead_max"] = 1 + min(2, pockets)

    names = {c: f"r{c[0]}{c[1]}" for c in fp["cells"]}

    def graph_lines(assignment, edges):
        out = []
        for cell in sorted(fp["cells"], key=lambda c: (c[1], c[0])):
            out.append(f"room {names[cell]} {assignment[cell]}")
        for a, b in sorted(edges, key=lambda e: (e[0][1], e[0][0], e[1][1], e[1][0])):
            d = {(1, 0): "e", (0, 1): "f"}[(b[0] - a[0], b[1] - a[1])]
            out.append(f"edge {names[a]} {DIRNAME[d]} {names[b]} {DIRNAME[OPP[d]]}")
        return out

    def make_layout(extra_lines):
        return Layout(InlineText("\n".join(extra_lines) + "\n"))

    endpoints = [
        f"spawn {names[fp['spawn']]}",
        f"goal {names[fp['crown']]}",
        f"exit {names[fp['exit']]}",
    ]
    def choose_pickups(assessor):
        """Place exactly the loadout's traversal items, in acquisition
        order, each on a room the previous loadout can just about reach.
        Returns {item: room} or None if this fill offers no such room."""
        reserved = {names[fp["spawn"]], names[fp["crown"]],
                    names[fp["seal"]], names[fp["exit"]]}
        starts = assessor.spawn_starts()
        quantile = BANDS[band]["quantile"]
        picked = {}
        held = "none"
        seen = reachable_rooms(assessor, starts, held) - reserved
        for item in cfg["pickups"]:
            gained = "wall" if item == "glove" else "dash"
            candidates = seen - set(picked.values())
            pick = choose_pickup(assessor, starts, held, candidates, quantile, rng)
            if pick is None:
                return None
            picked[item] = pick[0]
            held = gained if held == "none" else "both"
            opened = reachable_rooms(assessor, starts, held) - reserved
            if len(opened) <= len(seen):
                # An item that opens no new territory is a trinket, not an
                # act boundary: reject the fill rather than ship a dungeon
                # whose middle act changes nothing.
                return None
            seen = opened
        return picked

    def defects(assignment, edges):
        """(count, culprit cells, layout, assessor) for a candidate fill.

        A fill is sound when nobody can be stranded (no absorbing trap at
        any feasible loadout), every room can be entered with what the
        dungeon gives you, and the escape hatch can be climbed. The
        culprit cells are the rooms those defects live in, which is what
        makes repair possible instead of another blind re-roll."""
        layout = make_layout(graph_lines(assignment, edges) + endpoints)
        assessor = Assessor(layout, table)
        cell_of = {name: cell for cell, name in names.items()}
        culprits = set()
        trapped = trapped_states(assessor, feasible)
        for _, room, _ in trapped:
            culprits.add(cell_of[room])
        weak = set()
        for loadout in feasible:
            for room, entry in one_way_states(assessor, loadout):
                weak.add((loadout, room, entry))
                culprits.add(cell_of[room])
        unreached = set(layout.rooms) - reachable_rooms(
            assessor, assessor.spawn_starts(), final)
        for room in unreached:
            culprits.add(cell_of[room])
        # Get-out-alive: from the crown, with the gate paid, some route must
        # reach the exit room by a door it can then climb out of. Checking
        # the exit room's ceiling crossing alone is not enough — the climb
        # back up through the dungeon has to exist too.
        exit_room = layout.exit
        crown_states = [
            ((layout.goal, entry), 0) for entry in assessor.doors(layout.goal)
        ]
        climbs = any(
            room == exit_room and entry != "ceiling"
            and assessor.cross_ticks(room, entry, "ceiling", final) is not None
            for room, entry in reachable_states(
                assessor, crown_states, final, OPTIMISTIC_COINS)
        )
        if not climbs:
            culprits.update({fp["exit"], fp["spawn"]})
        count = (len(trapped) + len(weak) + 3 * len(unreached)
                 + (0 if climbs else 5))
        return count, culprits, layout, assessor

    def repair(assignment, edges, adj):
        """Hill-climb the defect count by re-rooming the cells the defects
        are in (and their neighbours — a trap is a property of a seam, not
        of one room). Rejection sampling used to throw away whole fills
        over a single badly placed one-way room; this fixes them in place,
        which is both faster and keeps the demand targeting intact."""
        count, culprits, layout, assessor = defects(assignment, edges)
        for _ in range(REPAIR_STEPS):
            if count == 0:
                break
            pool = set()
            for cell in culprits:
                pool.add(cell)
                pool.update(n for n in adj[cell])
            pool -= {fp["crown"], fp["seal"]}
            if not pool:
                break
            cell = rng.choice(sorted(pool))
            used = set(assignment.values())
            options = [
                slug for slug, info in vocab.items()
                if slug not in EXCLUDED and slug not in PIN_ONLY
                and slug not in used
                and info["doors"] == vocab[assignment[cell]]["doors"]
                and all(vocab[assignment[n]]["class"] != info["class"]
                        for n in adj[cell] if n in assignment and n != cell)
            ]
            # Prefer rooms that cannot strand anyone, then rooms on band.
            options.sort(key=lambda s: (s not in safe,
                                        abs(demand.get(s, 0.5) - target)))
            if not options:
                continue
            keep = assignment[cell]
            assignment[cell] = options[0]
            trial = defects(assignment, edges)
            if trial[0] < count:
                count, culprits, layout, assessor = trial
            else:
                assignment[cell] = keep
        return count, layout, assessor

    assignment = edges = layout = picks = None
    for _ in range(12):
        score, edges = anneal(fp, rng, supply)
        if score != 0:
            continue
        adj = required_sigs(fp, edges)[1]
        # The fill is cheap and the anneal is not: re-roll room choices
        # against the same skeleton before paying for another skeleton.
        for _ in range(6):
            assignment = fill(fp, edges, vocab, rng, guard, demand, safe,
                              target, seal_pool)
            if assignment is None:
                break
            count, layout, assessor = repair(assignment, edges, adj)
            if count:
                assignment = None
                continue
            picks = choose_pickups(assessor)
            if picks is None:
                assignment = None
                continue
            break
        if assignment is not None:
            break
    if assignment is None:
        return None
    guard_note = (
        f"behind {guard} ({loadout}) and its coin gate."
        if guard else
        "behind a coin gate alone: this dungeon carries no traversal item."
    )
    lines = [
        f"# Generated dungeon (tools/dungeon_generator.py v2)",
        f"# seed {seed} size {size} band {band} acts {acts} loadout {loadout}",
        f"# spawn under the exit roof-cap; crown at the bottom-east end",
        f"# {guard_note}",
        "",
    ]
    lines.extend(graph_lines(assignment, edges))
    lines.append("")

    # --- gates: single coin gate on the seal's east door ----------------
    # Only coins this loadout can actually pick up count as money.
    total_coins = sum(
        collectable_coins(table[assignment[c]], final) for c in fp["cells"]
    )
    requirement = min(
        max(1, round(BANDS[band]["coin_frac"] * total_coins)),
        round(0.95 * total_coins),
    )
    lines.append(f"gate {names[fp['seal']]} east coins {requirement}")
    lines.append("")

    # --- pickups (chosen inside the retry loop) --------------------------
    for item in cfg["pickups"]:
        room = picks[item]
        px, py = pickup_spot(layout.rooms[room])
        lines.append(f"pickup {item} {room} {px} {py}")
    if cfg["pickups"]:
        lines.append("")
    lines.extend(endpoints)
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--size", choices=sorted(SIZES), default="medium")
    parser.add_argument("--band", choices=["low", "mid", "high"], default="mid")
    parser.add_argument("--loadout", choices=sorted(LOADOUTS), default="both")
    parser.add_argument("--acts", type=int, default=3)
    parser.add_argument("--out", type=pathlib.Path)
    args = parser.parse_args()
    text = generate(args.seed, args.size, args.band, args.acts, args.loadout)
    if text is None:
        print(f"seed {args.seed}: no consistent layout found", file=sys.stderr)
        return 2
    if args.out:
        args.out.write_text(text)
        print(f"wrote {args.out}")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
