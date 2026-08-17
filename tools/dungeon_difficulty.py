#!/usr/bin/env python3
"""Programmatic dungeon difficulty assessment.

Reads a layout file (dungeon_layout_metrics.py format) plus
docs/design/rooms-v2-passability.json, computes the EASIEST MANDATORY
ROUTE — spawn -> first pickup -> second pickup -> coins-to-gate ->
crown -> climb back to the exit room's ceiling — respecting gates and
loadout-dependent crossability, and aggregates per-room challenge
along it.

Challenge signals per crossing (entry door -> exit door of a room):
  * solve ticks at the route loadout (the auditor's bounded-search
    solve time; hazard waits and control demand are embedded in it)
  * worst-family shaky-hand robustness where the passability table
    carries it (currently populated for coin routes only; when a
    room's pairs gain shaky data the tool picks it up automatically)

Aggregates: traversal ticks, coin-collection ticks, crossing count,
peak/mean crossing ticks (per-transfer control demand), vertical
reversals (reversal cadence), ascent crossings, shaky robustness.

Scalar score (dimensionless, open-ended; dungeon-v2 sits low on it),
scale v2 — see SCALE NOTE below:
  score =   4.0 * (traverse_ticks / 3600)      # route time, coins excluded
          + 4.0 * (peak_crossing_ticks / 600)  # hardest single transfer
          + 4.0 * (mean_crossing_ticks / 120)  # sustained transfer demand
          + 0.10 * crossings                   # route length (deliberately small)
          + 0.30 * reversals                   # reversal cadence
          + 3.0 * (coin_ticks_mean / 120)      # per-coin collection demand
          + 8.0 * coin_shaky_mean              # per-coin control demand

SCALE NOTE (2026-08-17, iteration 2). Scale v1 summed the shaky-hand
deficit over every collected coin and put total_ticks (which includes
coin collection) at the top of the score. Once the passability table
gained shaky data for the whole vocabulary rather than dungeon-v2's
rooms only, both terms turned into restatements of "how many coins
does the gate demand": dungeon-v2 jumped 27 -> 91 and generated
layouts all landed 78-99, i.e. the instrument measured coin count,
exactly the "jump count" driver the designer rejected. Scale v2
separates traversal from collection, charges collection per coin
(mean ticks, mean shaky deficit) rather than per route, and pays for
reversal cadence and per-transfer demand explicitly. Every term is
now intensive except the small `crossings` length term.

Calibration on scale v2: docs/design/dungeon-v2-layout.txt (the
shipped baseline) scores 25.68. Shaky coverage is now complete —
every room carrying coins has shaky data — so scores are comparable
across the portfolio; regenerate with tools/room_passability.py
--shaky if new coin rooms are authored.

Usage:
  python3 tools/dungeon_difficulty.py <layout.txt> [--json]

--json prints the full breakdown as JSON (score included) and nothing
else. Exit code 0 on a scored route, 1 if no mandatory route exists.
"""

import heapq
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from dungeon_layout_metrics import Layout  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"

ALLOWED = {
    "none": ["none"],
    "wall": ["none", "wall"],
    "dash": ["none", "dash"],
    "both": ["none", "wall", "dash", "both"],
}

OPTIMISTIC_COINS = 10 ** 9

WEIGHTS = {
    "traverse_ticks": 4.0 / 3600.0,
    "peak_crossing_ticks": 4.0 / 600.0,
    "mean_crossing_ticks": 4.0 / 120.0,
    "crossings": 0.10,
    "reversals": 0.30,
    "coin_ticks_mean": 3.0 / 120.0,
    "coin_shaky_mean": 8.0,
}


class Assessor:
    def __init__(self, layout: Layout, table: dict):
        self.layout = layout
        self.table = table
        self.step = {}
        for a, da, b, db in layout.edges:
            self.step[(a, da)] = (b, db)
            self.step[(b, db)] = (a, da)

    def doors(self, room):
        return self.table.get(self.layout.rooms[room], {}).get("doors", [])

    def cross_ticks(self, room, entry, exit_door, loadout):
        slug = self.layout.rooms[room]
        slot = self.table.get(slug, {}).get("pairs", {}).get(f"{entry}->{exit_door}", {})
        values = [slot.get(l) for l in ALLOWED[loadout] if slot.get(l) is not None]
        return min(values) if values else None

    def gate_ok(self, room, door, loadout, coins):
        gate = self.layout.gates.get((room, door), {})
        need = gate.get("ability")
        if need == "wall" and loadout not in ("wall", "both"):
            return False
        if need == "dash" and loadout not in ("dash", "both"):
            return False
        if need == "both" and loadout != "both":
            return False
        return gate.get("coins", 0) <= coins

    def dijkstra(self, starts, loadout, coins, targets, target_states=None):
        """starts: [((room, door), cost)]; targets: set of rooms, or
        target_states: set of (room, door) states when arriving by the
        right door matters (the climb-out has to reach the exit room by
        a door it can actually climb out of).
        Returns (cost, end_state, crossings list) for the cheapest
        qualifying arrival, or None."""
        dist = {}
        parent = {}
        heap = []
        for state, cost in starts:
            if dist.get(state, float("inf")) > cost:
                dist[state] = cost
                parent[state] = None
                heapq.heappush(heap, (cost, state))
        best = None
        while heap:
            cost, state = heapq.heappop(heap)
            if cost > dist.get(state, float("inf")):
                continue
            room, entry = state
            if state in target_states if target_states is not None else room in targets:
                best = state
                break
            for exit_door in self.doors(room):
                ticks = self.cross_ticks(room, entry, exit_door, loadout)
                if ticks is None:
                    continue
                if not self.gate_ok(room, exit_door, loadout, coins):
                    continue
                nxt = self.step.get((room, exit_door))
                if nxt is None:
                    continue
                new_cost = cost + ticks
                if new_cost < dist.get(nxt, float("inf")):
                    dist[nxt] = new_cost
                    parent[nxt] = (state, {
                        "room": room,
                        "slug": self.layout.rooms[room],
                        "pair": f"{entry}->{exit_door}",
                        "ticks": ticks,
                        "loadout": loadout,
                    })
                    heapq.heappush(heap, (new_cost, nxt))
        if best is None:
            return None
        crossings = []
        state = best
        while parent[state] is not None:
            state, record = parent[state]
            crossings.append(record)
        crossings.reverse()
        return dist[best], best, crossings

    def spawn_starts(self):
        return [((self.layout.spawn, door), 0) for door in self.doors(self.layout.spawn)]

    def coin_ticks(self, room, loadout="both"):
        """[(ticks, index)] for each coin in the room that THIS loadout can
        actually collect. A coin whose only route needs the dash is not
        money in a dungeon that has no boots in it."""
        slug = self.layout.rooms[room]
        out = []
        for index, slot in self.table.get(slug, {}).get("coin_routes", {}).items():
            values = [slot.get(l) for l in ALLOWED[loadout] if slot.get(l) is not None]
            if values:
                out.append((min(values), index))
        return sorted(out)

    def coin_shaky(self, room, index):
        slug = self.layout.rooms[room]
        slot = self.table.get(slug, {}).get("shaky", {}).get(index)
        return slot["worst"] if slot else None


def reachable_states(assessor, starts, loadout, coins=0):
    """(room, entry) states reachable from `starts` at a fixed loadout."""
    visited = set()
    frontier = [state for state, _ in starts]
    while frontier:
        state = frontier.pop()
        if state in visited:
            continue
        visited.add(state)
        room, entry = state
        for exit_door in assessor.doors(room):
            if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                continue
            if not assessor.gate_ok(room, exit_door, loadout, coins):
                continue
            nxt = assessor.step.get((room, exit_door))
            if nxt and nxt not in visited:
                frontier.append(nxt)
    return visited


def one_way_states(assessor, loadout, coins=OPTIMISTIC_COINS):
    """States you can get to but not get back from, at a fixed loadout.

    Retreating to the spawn ROOM (what the metrics tool checks) is not
    enough for a dungeon that asks you to fetch things: arriving at the
    spawn by its floor door and needing to leave by its west one is a
    different state, and a route that cannot make that turn is a route
    the difficulty tool will fail to find. The invariant is that every
    state reachable from the spawn lies in one strongly connected
    component: anywhere can get to anywhere, given the dungeon's kit."""
    starts = [state for state, _ in assessor.spawn_starts()]
    if not starts:
        return set()
    # Anchor on ONE spawn state: everything the player can reach must be
    # reachable from it and able to return to it, which is what makes the
    # reachable set a single strongly connected component.
    anchor = starts[0]
    forward = reachable_states(assessor, [(anchor, 0)], loadout, coins)
    everything = reachable_states(
        assessor, [(s, 0) for s in starts], loadout, coins)
    back = {}
    for state in everything:
        room, entry = state
        for exit_door in assessor.doors(room):
            if assessor.cross_ticks(room, entry, exit_door, loadout) is None:
                continue
            if not assessor.gate_ok(room, exit_door, loadout, coins):
                continue
            nxt = assessor.step.get((room, exit_door))
            if nxt:
                back.setdefault(nxt, []).append(state)
    seen = {anchor}
    stack = [anchor]
    while stack:
        state = stack.pop()
        for previous in back.get(state, ()):
            if previous not in seen:
                seen.add(previous)
                stack.append(previous)
    return (everything - forward) | (everything - seen)



def assess(layout_path: pathlib.Path):
    layout = Layout(layout_path)
    table = json.loads(PASSABILITY.read_text())
    a = Assessor(layout, table)

    glove_room = layout.pickups.get("glove")
    boots_room = layout.pickups.get("boots")
    requirement = max([g.get("coins", 0) for g in layout.gates.values()] + [0])

    orders = []
    if glove_room and boots_room:
        orders = [
            [(glove_room, "wall", "glove"), (boots_room, "both", "boots")],
            [(boots_room, "dash", "boots"), (glove_room, "both", "glove")],
        ]
    elif glove_room:
        orders = [[(glove_room, "wall", "glove")]]
    elif boots_room:
        orders = [[(boots_room, "dash", "boots")]]
    else:
        orders = [[]]

    best = None
    for order in orders:
        result = assess_order(a, order, requirement)
        if result is None:
            continue
        if best is None or result["score"] < best["score"]:
            best = result
    return best


def assess_order(a: Assessor, order, requirement):
    layout = a.layout
    legs = []
    starts = a.spawn_starts()
    loadout = "none"
    route_rooms = {layout.spawn}
    total = 0.0

    for target_room, next_loadout, name in order:
        leg = a.dijkstra(starts, loadout, 0, {target_room})
        if leg is None:
            return None
        cost, end, crossings = leg
        total += cost
        route_rooms |= {c["room"] for c in crossings} | {end[0]}
        legs.append({"leg": f"to-{name}", "loadout": loadout, "ticks": cost,
                     "crossings": crossings, "end": list(end)})
        starts = [(end, 0)]
        loadout = next_loadout

    # --- leg to the gate room (or straight to the goal) -----------------
    gate_rooms = {room for (room, _), g in layout.gates.items() if g.get("coins", 0) > 0}
    pre_goal_target = gate_rooms if gate_rooms else {layout.goal}
    leg = a.dijkstra(starts, loadout, 0, pre_goal_target)
    if leg is None:
        return None
    cost, end, crossings = leg
    total += cost
    route_rooms |= {c["room"] for c in crossings} | {end[0]}
    legs.append({"leg": "to-gate" if gate_rooms else "to-goal", "loadout": loadout,
                 "ticks": cost, "crossings": crossings, "end": list(end)})
    starts = [(end, 0)]

    # --- coins ----------------------------------------------------------
    coin_ticks_total = 0.0
    shaky_penalty = 0.0
    coins_used = []
    if requirement > 0:
        collected_rooms = set(route_rooms)
        available = []  # (ticks, room, index)

        def refill():
            available.clear()
            for room in collected_rooms:
                for ticks, index in a.coin_ticks(room, loadout):
                    available.append((ticks, room, index))
            available.sort()

        refill()
        tour_crossings = []
        while len(coins_used) < requirement:
            if len(available) > len(coins_used):
                ticks, room, index = available[len(coins_used)]
                coins_used.append({"room": room, "coin": index, "ticks": ticks})
                coin_ticks_total += ticks
                worst = a.coin_shaky(room, index)
                if worst is not None:
                    shaky_penalty += 1.0 - worst
                continue
            # need to visit more coin rooms
            targets = {
                room for room in layout.rooms
                if room not in collected_rooms and a.coin_ticks(room, loadout)
            }
            if not targets:
                return None  # coin gate unsatisfiable
            leg = a.dijkstra(starts, loadout, 0, targets)
            if leg is None:
                return None
            cost, end, crossings = leg
            total += cost
            tour_crossings.extend(crossings)
            collected_rooms |= {c["room"] for c in crossings} | {end[0]}
            route_rooms |= collected_rooms
            starts = [(end, 0)]
            refill()
        total += coin_ticks_total
        legs.append({"leg": "coin-tour", "loadout": loadout,
                     "ticks": sum(c["ticks"] for c in tour_crossings) + coin_ticks_total,
                     "crossings": tour_crossings, "coins": coins_used,
                     "end": list(starts[0][0])})

    # --- through the gate to the goal ------------------------------------
    if layout.goal not in {s[0][0] for s in starts}:
        leg = a.dijkstra(starts, loadout, requirement, {layout.goal})
        if leg is None:
            return None
        cost, end, crossings = leg
        total += cost
        route_rooms |= {c["room"] for c in crossings} | {end[0]}
        legs.append({"leg": "to-goal", "loadout": loadout, "ticks": cost,
                     "crossings": crossings, "end": list(end)})
        starts = [(end, 0)]

    # --- climb out: goal -> exit room -> its ceiling door ----------------
    # The exit room must be entered by a door its ceiling is reachable
    # from: arriving at the cheapest door and finding no way up is not a
    # climb-out, it is a detour.
    climbable = {
        (layout.exit, door) for door in a.doors(layout.exit)
        if door != "ceiling"
        and a.cross_ticks(layout.exit, door, "ceiling", loadout) is not None
    }
    if not climbable:
        return None
    leg = a.dijkstra(starts, loadout, requirement, {layout.exit},
                     target_states=climbable)
    if leg is None:
        return None
    cost, end, crossings = leg
    total += cost
    route_rooms |= {c["room"] for c in crossings}
    exit_cross = a.cross_ticks(layout.exit, end[1], "ceiling", loadout)
    if exit_cross is None:
        return None
    total += exit_cross
    crossings = crossings + [{
        "room": layout.exit, "slug": layout.rooms[layout.exit],
        "pair": f"{end[1]}->ceiling", "ticks": exit_cross, "loadout": loadout,
    }]
    legs.append({"leg": "to-exit", "loadout": loadout,
                 "ticks": cost + exit_cross, "crossings": crossings,
                 "end": [layout.exit, "ceiling"]})

    all_crossings = [c for leg in legs for c in leg["crossings"]]
    peak = max((c["ticks"] for c in all_crossings), default=0)
    ascents = sum(1 for c in all_crossings if c["pair"].endswith("->ceiling"))
    mean = sum(c["ticks"] for c in all_crossings) / max(1, len(all_crossings))
    traverse = total - coin_ticks_total
    coins_taken = max(1, len(coins_used))
    coin_ticks_mean = coin_ticks_total / coins_taken
    coin_shaky_mean = shaky_penalty / coins_taken
    breakdown = {
        "order": [name for _, _, name in order],
        "total_ticks": round(total),
        "total_seconds": round(total / 60.0, 1),
        "traverse_ticks": round(traverse),
        "crossings": len(all_crossings),
        "peak_crossing_ticks": peak,
        "mean_crossing_ticks": round(mean, 1),
        "ascent_crossings": ascents,
        "reversals": count_reversals(all_crossings),
        "coin_gate_requirement": requirement,
        "coin_collection_ticks": round(coin_ticks_total),
        "coin_ticks_mean": round(coin_ticks_mean, 1),
        "shaky_penalty": round(shaky_penalty, 3),
        "coin_shaky_mean": round(coin_shaky_mean, 3),
        "route_rooms": len(route_rooms),
        "legs": legs,
    }
    breakdown["score"] = round(
        WEIGHTS["traverse_ticks"] * traverse
        + WEIGHTS["peak_crossing_ticks"] * peak
        + WEIGHTS["mean_crossing_ticks"] * mean
        + WEIGHTS["crossings"] * len(all_crossings)
        + WEIGHTS["reversals"] * breakdown["reversals"]
        + WEIGHTS["coin_ticks_mean"] * coin_ticks_mean
        + WEIGHTS["coin_shaky_mean"] * coin_shaky_mean,
        2,
    )
    return breakdown


def count_reversals(crossings):
    """Reversal cadence: how often the mandatory route flips between
    descending and ascending. Lateral crossings do not break a run, so
    a long sideways detour between a drop and a climb still counts as
    one reversal, and a straight plunge counts as none."""
    direction = 0
    reversals = 0
    for crossing in crossings:
        exit_door = crossing["pair"].split("->")[1]
        if exit_door == "floor":
            step = 1
        elif exit_door == "ceiling":
            step = -1
        else:
            continue
        if direction and step != direction:
            reversals += 1
        direction = step
    return reversals


def main() -> int:
    arguments = [a for a in sys.argv[1:] if not a.startswith("--")]
    as_json = "--json" in sys.argv
    if not arguments:
        print(__doc__)
        return 1
    breakdown = assess(pathlib.Path(arguments[0]))
    if breakdown is None:
        if as_json:
            print(json.dumps({"score": None, "error": "no mandatory route found"}))
        else:
            print("no mandatory route found (spawn->pickups->coins->goal->exit)")
        return 1
    if as_json:
        print(json.dumps(breakdown, indent=1))
        return 0
    print(f"score {breakdown['score']}")
    for key in ("order", "total_ticks", "total_seconds", "traverse_ticks",
                "crossings", "peak_crossing_ticks", "mean_crossing_ticks",
                "ascent_crossings", "reversals", "coin_gate_requirement",
                "coin_collection_ticks", "coin_ticks_mean", "shaky_penalty",
                "coin_shaky_mean", "route_rooms"):
        print(f"  {key} {breakdown[key]}")
    for leg in breakdown["legs"]:
        print(f"  leg {leg['leg']:<10} loadout {leg['loadout']:<5} "
              f"ticks {round(leg['ticks']):>6} crossings {len(leg['crossings'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
