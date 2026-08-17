#!/usr/bin/env python3
"""Assess a prototype dungeon layout against the design goals:
multiple routes, backtracking unlocks, low diameter, no soft-locks,
and archetype variety.

Usage: python3 tools/dungeon_layout_metrics.py docs/design/dungeon-v2-layout.txt

Layout format (one directive per line, # comments allowed):
  room <id> <grid-slug>          # instance of a rooms-v2 grid
  edge <idA> <doorA> <idB> <doorB>
  gate <id> <door> ability wall|dash|both     # door locked without ability
  gate <id> <door> coins <n>                  # door locked below n coins
  pickup glove <id>
  pickup boots <id>
  spawn <id>
  goal <id>
  exit <id>                      # room whose CEILING door is the escape exit

Requires docs/design/rooms-v2-passability.json (tools/room_passability.py).

Exit code 0 iff all hard checks pass. Prints `metric`/`check`/`problem` lines.
"""

import json
import pathlib
import sys
from collections import deque

ROOT = pathlib.Path(__file__).resolve().parent.parent
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"

OPPOSITE = {"west": "east", "east": "west", "ceiling": "floor", "floor": "ceiling"}
LOADOUTS = ["none", "wall", "dash", "both"]


def loadout_name(wall: bool, dash: bool) -> str:
    return {(False, False): "none", (True, False): "wall", (False, True): "dash", (True, True): "both"}[(wall, dash)]


class Layout:
    def __init__(self, path: pathlib.Path):
        self.rooms: dict[str, str] = {}
        self.edges: list[tuple[str, str, str, str]] = []
        self.gates: dict[tuple[str, str], dict] = {}
        self.pickups: dict[str, str] = {}
        self.spawn = None
        self.goal = None
        self.exit = None
        for raw in path.read_text().splitlines():
            line = raw.split("#")[0].strip()
            if not line:
                continue
            parts = line.split()
            if parts[0] == "room":
                self.rooms[parts[1]] = parts[2]
            elif parts[0] == "edge":
                self.edges.append((parts[1], parts[2], parts[3], parts[4]))
            elif parts[0] == "gate":
                gate = self.gates.setdefault((parts[1], parts[2]), {})
                if parts[3] == "ability":
                    gate["ability"] = parts[4]
                elif parts[3] == "coins":
                    gate["coins"] = int(parts[4])
            elif parts[0] == "pickup":
                self.pickups[parts[1]] = parts[2]
            elif parts[0] == "spawn":
                self.spawn = parts[1]
            elif parts[0] == "goal":
                self.goal = parts[1]
            elif parts[0] == "exit":
                self.exit = parts[1]


def main() -> int:
    layout = Layout(pathlib.Path(sys.argv[1]))
    table = json.loads(PASSABILITY.read_text())
    problems: list[str] = []

    # Design rule: traversal abilities must gate areas through room geometry
    # (crossings that genuinely need the ability), never through magic door
    # locks. Only coin gates are allowed as explicit door requirements.
    for (room, door), gate in sorted(layout.gates.items()):
        if "ability" in gate:
            problems.append(
                f"problem ability gate on {room}.{door}: use geometry, not door locks"
            )

    # --- structural checks -------------------------------------------------
    door_used: dict[tuple[str, str], int] = {}
    for a, da, b, db in layout.edges:
        for room, door in [(a, da), (b, db)]:
            if room not in layout.rooms:
                problems.append(f"problem edge references unknown room {room}")
                continue
            slug = layout.rooms[room]
            if slug not in table:
                problems.append(f"problem room {room} slug {slug} missing from passability table")
            elif door not in table[slug]["doors"]:
                problems.append(f"problem room {room} ({slug}) has no door {door}")
            door_used[(room, door)] = door_used.get((room, door), 0) + 1
        if OPPOSITE.get(da) != db:
            problems.append(f"problem edge {a}.{da} <-> {b}.{db} sides do not geometrically mate")
    for (room, door), count in door_used.items():
        if count > 1:
            problems.append(f"problem door {room}.{door} used by {count} edges")
    for room, slug in layout.rooms.items():
        if slug in table:
            for door in table[slug]["doors"]:
                if (room, door) not in door_used:
                    if room == layout.exit and door == "ceiling":
                        # The declared exit room's ceiling door is the escape
                        # out of the dungeon: it must NOT be an edge.
                        continue
                    problems.append(f"problem door {room}.{door} is not connected to anything")
    if layout.spawn is None or layout.goal is None:
        problems.append("problem layout needs both spawn and goal")
        print_report(problems, {})
        return 1
    # --- exit checks -------------------------------------------------------
    # The escape door is the declared exit room's ceiling door, and that room
    # must genuinely be at the top of the dungeon: nothing above it, and its
    # ceiling door free of edges so it leads OUT of the dungeon.
    if layout.exit is None:
        problems.append("problem layout must declare `exit <room>` (the escape room)")
    elif layout.exit not in layout.rooms:
        problems.append(f"problem exit room {layout.exit} is not a declared room")
    else:
        exit_slug = layout.rooms[layout.exit]
        if exit_slug in table and "ceiling" not in table[exit_slug]["doors"]:
            problems.append(
                f"problem exit room {layout.exit} ({exit_slug}) has no ceiling door to escape through"
            )
        for a, da, b, db in layout.edges:
            if (a, da) == (layout.exit, "ceiling") or (b, db) == (layout.exit, "ceiling"):
                problems.append(
                    f"problem exit room {layout.exit}'s ceiling door is wired to another room "
                    f"({a}.{da} <-> {b}.{db}); the escape door must lead out of the dungeon"
                )

    # --- grid embedding ----------------------------------------------------
    # The dungeon must embed on a 2D grid: every edge joins rooms in adjacent
    # cells (unit step in the door's direction) and no two rooms share a cell,
    # so the in-game map can draw the graph with no displacement and no
    # crossing or lying connection lines.
    step_of = {"west": (-1, 0), "east": (1, 0), "ceiling": (0, -1), "floor": (0, 1)}
    adjacency: dict[str, list[tuple[str, str]]] = {r: [] for r in layout.rooms}
    for a, da, b, db in layout.edges:
        if a in adjacency and b in adjacency:
            adjacency[a].append((da, b))
            adjacency[b].append((db, a))
    positions = {layout.spawn: (0, 0)}
    order = deque([layout.spawn])
    while order:
        current = order.popleft()
        for door, neighbour in adjacency[current]:
            target = (
                positions[current][0] + step_of[door][0],
                positions[current][1] + step_of[door][1],
            )
            if neighbour in positions:
                if positions[neighbour] != target:
                    problems.append(
                        f"problem grid-inconsistent edge {current}.{door} -> {neighbour}: "
                        f"expected cell {target}, placed at {positions[neighbour]}"
                    )
            else:
                positions[neighbour] = target
                order.append(neighbour)
    cell_owners: dict[tuple[int, int], list[str]] = {}
    for room, cell in positions.items():
        cell_owners.setdefault(cell, []).append(room)
    for cell, owners in sorted(cell_owners.items()):
        if len(owners) > 1:
            problems.append(f"problem grid cell {cell} shared by rooms {sorted(owners)}")
    if layout.exit in positions:
        x, y = positions[layout.exit]
        above = cell_owners.get((x, y - 1), [])
        if above:
            problems.append(
                f"problem exit room {layout.exit} is not on the top row: "
                f"room(s) {sorted(above)} sit directly above it"
            )

    # --- progression model -------------------------------------------------
    # neighbour map: (room, entry_door) --edge--> (other_room, other_entry)
    step = {}
    for a, da, b, db in layout.edges:
        step[(a, da)] = (b, db)
        step[(b, db)] = (a, da)

    def crossable(slug: str, entry: str, exit_door: str, loadout: str) -> bool:
        pairs = table.get(slug, {}).get("pairs", {})
        slot = pairs.get(f"{entry}->{exit_door}", {})
        # monotone: any smaller loadout solving implies this one works
        order = {"none": 0, "wall": 1, "dash": 2, "both": 3}
        ok_loadouts = {
            "none": ["none"],
            "wall": ["none", "wall"],
            "dash": ["none", "dash"],
            "both": LOADOUTS,
        }[loadout]
        return any(slot.get(l) is not None for l in ok_loadouts)

    def gate_open(room: str, door: str, wall: bool, dash: bool, coins: int) -> bool:
        gate = layout.gates.get((room, door), {})
        need = gate.get("ability")
        if need == "wall" and not wall:
            return False
        if need == "dash" and not dash:
            return False
        if need == "both" and not (wall and dash):
            return False
        if gate.get("coins", 0) > coins:
            return False
        return True

    def explore(use_coin_gates: bool = True):
        """Fixpoint exploration granting pickups/coins as rooms are reached."""
        wall = dash = False
        coins = 0
        visited: set[tuple[str, str]] = set()
        rooms_seen: set[str] = set()
        backtracks = 0
        seen_locked: set[tuple[str, str]] = set()
        while True:
            frontier = deque()
            # spawn entry: treat every door of the spawn room as reachable start
            slug = layout.rooms[layout.spawn]
            for door in table.get(slug, {}).get("doors", []):
                frontier.append((layout.spawn, door))
            local_visited: set[tuple[str, str]] = set()
            while frontier:
                room, entry = frontier.popleft()
                if (room, entry) in local_visited:
                    continue
                local_visited.add((room, entry))
                rooms_seen.add(room)
                slug = layout.rooms[room]
                loadout = loadout_name(wall, dash)
                for exit_door in table.get(slug, {}).get("doors", []):
                    if not crossable(slug, entry, exit_door, loadout):
                        # A crossing that opens at a bigger loadout is a
                        # geometric lock: it counts as a seen-then-unlocked
                        # backtrack opportunity once abilities arrive.
                        if crossable(slug, entry, exit_door, "both"):
                            seen_locked.add((room, exit_door))
                        continue
                    if not gate_open(room, exit_door, wall, dash, coins if use_coin_gates else 10**9):
                        seen_locked.add((room, exit_door))
                        continue
                    if (room, exit_door) in seen_locked:
                        backtracks += 1
                        seen_locked.discard((room, exit_door))
                    nxt = step.get((room, exit_door))
                    if nxt:
                        frontier.append(nxt)
            new_wall = wall or layout.pickups.get("glove") in rooms_seen
            new_dash = dash or layout.pickups.get("boots") in rooms_seen
            new_coins = sum(
                table.get(layout.rooms[r], {}).get("coins", 0) for r in rooms_seen
            )
            if local_visited == visited and new_wall == wall and new_dash == dash and new_coins == coins:
                return rooms_seen, visited, backtracks, seen_locked
            visited = local_visited
            wall, dash, coins = new_wall, new_dash, new_coins

    def fixed_loadout_states(wall: bool, dash: bool):
        """(room, entry) states reachable with a FIXED loadout, optimistic coins,
        no ability pickups granted."""
        coins = 10**9  # coin-optimistic: traps/goal checks are about abilities
        visited: set[tuple[str, str]] = set()
        frontier = deque()
        slug = layout.rooms[layout.spawn]
        for door in table.get(slug, {}).get("doors", []):
            frontier.append((layout.spawn, door))
        while frontier:
            room, entry = frontier.popleft()
            if (room, entry) in visited:
                continue
            visited.add((room, entry))
            slug = layout.rooms[room]
            loadout = loadout_name(wall, dash)
            for exit_door in table.get(slug, {}).get("doors", []):
                if not crossable(slug, entry, exit_door, loadout):
                    continue
                if not gate_open(room, exit_door, wall, dash, coins):
                    continue
                nxt = step.get((room, exit_door))
                if nxt and nxt not in visited:
                    frontier.append(nxt)
        return visited

    def can_retreat(start: tuple[str, str], wall: bool, dash: bool) -> bool:
        """Can this state reach the spawn room at a fixed loadout?"""
        visited = set()
        frontier = deque([start])
        while frontier:
            room, entry = frontier.popleft()
            if room == layout.spawn:
                return True
            if (room, entry) in visited:
                continue
            visited.add((room, entry))
            slug = layout.rooms[room]
            loadout = loadout_name(wall, dash)
            for exit_door in table.get(slug, {}).get("doors", []):
                if not crossable(slug, entry, exit_door, loadout):
                    continue
                if not gate_open(room, exit_door, wall, dash, 10**9):
                    continue
                nxt = step.get((room, exit_door))
                if nxt:
                    frontier.append(nxt)
        return False

    rooms_seen, _, backtracks, still_locked = explore()
    metrics = {}
    unreached = set(layout.rooms) - rooms_seen
    if unreached:
        problems.append(f"problem unreachable rooms: {sorted(unreached)}")
    if layout.goal not in rooms_seen:
        problems.append("problem goal is not reachable")
    metrics["rooms"] = len(layout.rooms)
    for wall, dash in [(False, False), (True, False), (False, True), (True, True)]:
        name = loadout_name(wall, dash)
        states = fixed_loadout_states(wall, dash)
        loadout_rooms = {room for room, _ in states}
        metrics[f"rooms-at-{name}"] = len(loadout_rooms)
        metrics[f"goal-reachable-at-{name}"] = layout.goal in loadout_rooms
        trapped = sorted(
            {room for (room, entry) in states if not can_retreat((room, entry), wall, dash)}
        )
        if trapped:
            problems.append(
                f"problem absorbing trap at loadout {name}: states in {trapped} cannot retreat to spawn"
            )
    metrics["edges"] = len(layout.edges)
    metrics["backtrack-unlock-events"] = backtracks
    metrics["still-locked-doors-at-end"] = len(still_locked)

    # cycle rank of the room graph (undirected)
    room_adj: dict[str, set[str]] = {r: set() for r in layout.rooms}
    for a, _, b, _ in layout.edges:
        room_adj[a].add(b)
        room_adj[b].add(a)
    metrics["cycle-rank"] = len(layout.edges) - len(layout.rooms) + 1
    metrics["mean-degree"] = round(2 * len(layout.edges) / max(1, len(layout.rooms)), 2)
    metrics["dead-end-rooms"] = sum(1 for r, n in room_adj.items() if len(n) <= 1)

    # all-unlocked diameter over rooms (edge = any crossable pair at 'both')
    def bfs_far(start: str) -> dict[str, int]:
        dist = {start: 0}
        queue = deque([start])
        while queue:
            room = queue.popleft()
            slug = layout.rooms[room]
            for entry in table.get(slug, {}).get("doors", []):
                for exit_door in table.get(slug, {}).get("doors", []):
                    if not crossable(slug, entry, exit_door, "both"):
                        continue
                    nxt = step.get((room, exit_door))
                    if nxt and nxt[0] not in dist:
                        dist[nxt[0]] = dist[room] + 1
                        queue.append(nxt[0])
        return dist

    eccentricities = []
    for room in layout.rooms:
        dist = bfs_far(room)
        if len(dist) == len(layout.rooms):
            eccentricities.append(max(dist.values()))
    metrics["all-unlocked-diameter"] = max(eccentricities) if eccentricities else None

    # class adjacency repetition: same class prefix (slug minus trailing -a/-b/-c)
    def class_of(slug: str) -> str:
        return slug.rsplit("-", 1)[0]

    repeats = sum(
        1
        for a, _, b, _ in layout.edges
        if class_of(layout.rooms[a]) == class_of(layout.rooms[b])
    )
    metrics["same-class-adjacent-edges"] = repeats

    # route multiplicity spawn->goal: greedy edge-disjoint shortest paths
    def shortest_path(banned: set[frozenset]) -> list[str] | None:
        prev = {layout.spawn: None}
        queue = deque([layout.spawn])
        while queue:
            room = queue.popleft()
            if room == layout.goal:
                path = [room]
                while prev[path[-1]] is not None:
                    path.append(prev[path[-1]])
                return list(reversed(path))
            for nxt in room_adj[room]:
                if frozenset((room, nxt)) in banned or nxt in prev:
                    continue
                prev[nxt] = room
                queue.append(nxt)
        return None

    banned: set[frozenset] = set()
    disjoint = 0
    while True:
        path = shortest_path(banned)
        if path is None:
            break
        disjoint += 1
        banned |= {frozenset(pair) for pair in zip(path, path[1:])}
        if disjoint > 8:
            break
    metrics["edge-disjoint-spawn-goal-routes"] = disjoint

    print_report(problems, metrics)
    return 1 if problems else 0


def print_report(problems, metrics):
    for key, value in metrics.items():
        print(f"metric {key} {value}")
    for problem in problems:
        print(problem)
    print("verdict", "ok" if not problems else f"FAIL {len(problems)} problems")


if __name__ == "__main__":
    raise SystemExit(main())
