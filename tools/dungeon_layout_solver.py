#!/usr/bin/env python3
"""Grid-embedded dungeon layout solver: the seed of the layout generator.

Layouts embed on a 2D grid (every edge a unit step in its door direction,
one room per cell) so the in-game map is exact. This tool works in the
generator's direction: first find a SKELETON (a set of grid cells and
edges scored on diameter, cycle rank, dead-ends and door-shape supply),
then INSTANTIATE each cell by selecting any vocabulary room whose door
signature fits, subject to same-class-adjacency and pinned placements,
and finally emit a layout file for tools/dungeon_layout_metrics.py.

Dungeon v2 rev 3 ("the descent", docs/design/dungeon-v2-layout.txt) was
produced with this pipeline; ability gating (which room guards which
seam) was chosen by hand from the per-pair loadout data in
docs/design/rooms-v2-passability.json and validated with the metrics
tool, which remains the verdict of record.

Usage:
  python3 tools/dungeon_layout_solver.py skeleton [seeds]
      Search for a skeleton and print it (shape map + edge list).
  python3 tools/dungeon_layout_solver.py fill <skeleton.json> <pins.json>
      Fill a skeleton with vocabulary rooms and emit a layout to stdout.
      pins.json maps "x,y" -> instance and instance -> slug overrides.
"""

import itertools
import json
import pathlib
import random
import sys
from collections import Counter, deque

ROOT = pathlib.Path(__file__).resolve().parent.parent
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"

DELTA = {"e": (1, 0), "w": (-1, 0), "f": (0, 1), "c": (0, -1)}
OPP = {"e": "w", "w": "e", "f": "c", "c": "f"}
DIRNAME = {"e": "east", "w": "west", "f": "floor", "c": "ceiling"}
SHORT = {"east": "e", "west": "w", "floor": "f", "ceiling": "c"}


def vocabulary():
    table = json.loads(PASSABILITY.read_text())
    vocab = {}
    for slug, entry in table.items():
        doors = frozenset(SHORT[d] for d in entry["doors"])
        vocab[slug] = {"doors": doors, "class": slug.rsplit("-", 1)[0]}
    return vocab


# --- skeleton search ---------------------------------------------------

# Default footprint used for rev 3: a 6x6 block with a 3-cell roof row.
CELLS = (
    [(x, 0) for x in range(2, 5)]
    + [(x, y) for y in (1, 2, 3, 4) for x in range(1, 7)]
    + [(x, 5) for x in (3, 4, 5)]
)
CROWN = (5, 5)


def analyse(cells, edges):
    adj = {c: [] for c in cells}
    for a, b in edges:
        adj[a].append(b)
        adj[b].append(a)
    worst = 0
    for s in cells:
        dist = {s: 0}
        queue = deque([s])
        while queue:
            u = queue.popleft()
            for v in adj[u]:
                if v not in dist:
                    dist[v] = dist[u] + 1
                    queue.append(v)
        if len(dist) < len(cells):
            return None
        worst = max(worst, max(dist.values()))
    return worst, adj


def shape_of(cell, adj):
    return frozenset(
        {(1, 0): "e", (-1, 0): "w", (0, 1): "f", (0, -1): "c"}[
            (n[0] - cell[0], n[1] - cell[1])
        ]
        for n in adj[cell]
    )


def shape_supply(vocab):
    """How many cells of each door signature the vocabulary can seat.

    Duplicate instances of a grid are allowed, but same-class rooms may
    not be adjacent, so the practical cap is a small multiple of the
    number of distinct classes with that signature.
    """
    classes = {}
    for slug, info in vocab.items():
        classes.setdefault(info["doors"], set()).add(info["class"])
    return {sig: min(2 * len(cls) + 1, 8) for sig, cls in classes.items()}


def penalty(cells, edges, supply, max_diameter=8, rank_range=(5, 9)):
    result = analyse(cells, edges)
    if result is None:
        return 1000
    diameter, adj = result
    score = 0
    if diameter > max_diameter:
        score += (diameter - max_diameter) * 20
    rank = len(edges) - len(cells) + 1
    if rank < rank_range[0]:
        score += (rank_range[0] - rank) * 8
    if rank > rank_range[1]:
        score += (rank - rank_range[1]) * 3
    shapes = {c: shape_of(c, adj) for c in cells}
    dead = [c for c in cells if len(adj[c]) == 1]
    score += 8 * abs(len(dead) - 1)
    if CROWN not in dead or shapes.get(CROWN) != frozenset("w"):
        score += 15
    counts = Counter(shapes.values())
    for sig, n in counts.items():
        cap = supply.get(sig, 0)
        if n > cap:
            score += 6 * (n - cap)
    # single-class signatures must not sit adjacent to themselves
    for sig in (frozenset("fw"), frozenset("cfw"), frozenset("efw"), frozenset("cefw")):
        placed = [c for c, s in shapes.items() if s == sig]
        for a, b in itertools.combinations(placed, 2):
            if abs(a[0] - b[0]) + abs(a[1] - b[1]) == 1:
                score += 5
    return score


def anneal(seed, cells=CELLS, iterations=12000):
    supply = shape_supply(vocabulary())
    cs = set(cells)
    full = [
        ((x, y), (x + dx, y + dy))
        for (x, y) in cells
        for dx, dy in ((1, 0), (0, 1))
        if (x + dx, y + dy) in cs
    ]
    rng = random.Random(seed)
    current = [e for e in full if rng.random() < 0.75]
    best = penalty(cells, current, supply)
    temperature = 8.0
    for _ in range(iterations):
        edge = rng.choice(full)
        trial = [e for e in current if e != edge] if edge in current else current + [edge]
        trial_score = penalty(cells, trial, supply)
        if trial_score <= best or rng.random() < pow(2.718, -(trial_score - best) / temperature):
            current, best = trial, trial_score
        temperature *= 0.999
        if best == 0:
            break
    return best, current


# --- room selection ----------------------------------------------------

def fill(cells, edges, pins, vocab, rng):
    """Assign a room slug to every cell: exact door-signature match,
    no same-class adjacency, honouring pinned (cell -> slug) choices."""
    result = analyse(cells, edges)
    if result is None:
        return None
    _, adj = result
    shapes = {c: shape_of(c, adj) for c in cells}
    by_shape = {}
    for slug, info in vocab.items():
        by_shape.setdefault(info["doors"], []).append(slug)
    order = sorted(cells, key=lambda c: len(by_shape.get(shapes[c], [])))
    assignment = {}

    def rec(i):
        if i == len(order):
            return True
        cell = order[i]
        if cell in pins:
            options = [pins[cell]]
        else:
            options = list(by_shape.get(shapes[cell], []))
            rng.shuffle(options)
        for slug in options:
            if vocab[slug]["doors"] != shapes[cell]:
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


def emit(cells, edges, assignment, names, out=sys.stdout):
    result = analyse(cells, edges)
    _, adj = result
    for cell in sorted(cells, key=lambda c: (c[1], c[0])):
        out.write(f"room {names[cell]} {assignment[cell]}\n")
    seen = set()
    for a, b in edges:
        d = {(1, 0): "e", (-1, 0): "w", (0, 1): "f", (0, -1): "c"}[
            (b[0] - a[0], b[1] - a[1])
        ]
        key = frozenset((a, b))
        if key in seen:
            continue
        seen.add(key)
        out.write(
            f"edge {names[a]} {DIRNAME[d]} {names[b]} {DIRNAME[OPP[d]]}\n"
        )


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("skeleton", "fill"):
        print(__doc__)
        return 1
    if sys.argv[1] == "skeleton":
        seeds = int(sys.argv[2]) if len(sys.argv) > 2 else 8
        best = None
        for seed in range(seeds):
            score, edges = anneal(seed)
            if best is None or score < best[0]:
                best = (score, edges)
                print(f"seed {seed} penalty {score} edges {len(edges)}")
            if score == 0:
                break
        score, edges = best
        result = analyse(CELLS, edges)
        diameter, adj = result if result else (None, None)
        print(f"final penalty {score} diameter {diameter}")
        print(json.dumps({"cells": [list(c) for c in CELLS],
                          "edges": [[list(a), list(b)] for a, b in edges]}))
        return 0
    spec = json.loads(pathlib.Path(sys.argv[2]).read_text())
    cells = [tuple(c) for c in spec["cells"]]
    edges = [(tuple(a), tuple(b)) for a, b in spec["edges"]]
    pins_raw = json.loads(pathlib.Path(sys.argv[3]).read_text()) if len(sys.argv) > 3 else {}
    pins = {tuple(int(v) for v in k.split(",")): slug for k, slug in pins_raw.items()}
    vocab = vocabulary()
    assignment = fill(cells, edges, pins, vocab, random.Random(0))
    if assignment is None:
        print("no assignment found", file=sys.stderr)
        return 1
    names = {c: f"r{c[0]}{c[1]}" for c in cells}
    emit(cells, edges, assignment, names)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
