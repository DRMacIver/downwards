#!/usr/bin/env python3
"""Procedural room generator for the rooms-v2 vocabulary.

Generates candidate 32x18 room grids for a *family contract* (door signature +
per-loadout traversability profile derived from an exemplar room), prefilters
them with a cheap approximate movement model, then verifies survivors with the
real auditor (audit_room_grid) and keeps only exact profile matches, ranked by
dissimilarity from the family's existing members.

The generator does NOT try to be a good designer. It aims for *adequate,
audited-valid baselines* that an agent can then critique and polish. See
docs/design/room-generator.md and the room-variant skill.

Usage:
  python3 tools/room_generator.py --family <exemplar-slug> \
      [--seeds 0:200] [--keep 5] [--jobs 8] [--out DIR] [--prefix slug-]

Requires target/release/examples/audit_room_grid (cargo build --release
-p downwards-content --example audit_room_grid).
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import pathlib
import random
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ROOMS = ROOT / "crates" / "downwards-gen" / "rooms-v2"
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"
AUDITOR = ROOT / "target" / "release" / "examples" / "audit_room_grid"

WIDTH, HEIGHT = 32, 18
SOLID, EMPTY, ONEWAY = "#", ".", "-"
SPIKES = {"^": (0, -1), "v": (0, 1), "<": (-1, 0), ">": (1, 0)}

# Door mouths (must match audit_room_grid::aperture_tiles).
MOUTHS = {
    "west": [(0, r) for r in range(13, 17)],
    "east": [(WIDTH - 1, r) for r in range(13, 17)],
    "ceiling": [(c, 0) for c in range(14, 18)],
    "floor": [(c, 17) for c in range(14, 18)],
}
# Tile the solver spawns near when entering by each door (from door_geometry,
# pixels / 10).
ARRIVALS = {"west": (1, 14), "east": (30, 14), "ceiling": (15, 1), "floor": (15, 14)}

LOADOUTS = ("none", "wall", "dash", "both")


# --------------------------------------------------------------------------
# Contract


def gating_class(solved: set[str] | list[str]) -> str:
    """Collapse a set of solving loadouts to its gating meaning.

    The bounded search sometimes fails to re-find a bare route under a larger
    loadout (an artifact, not physics), so raw solved-set equality is too
    strict: compare the minimal requirement instead.
    """
    solved = set(solved)
    if "none" in solved:
        return "open"
    if "wall" in solved and "dash" in solved:
        return "either"
    if "wall" in solved:
        return "wall"
    if "dash" in solved:
        return "dash"
    if "both" in solved:
        return "both"
    return "closed"


def load_contract(exemplar: str) -> dict:
    data = json.loads(PASSABILITY.read_text())
    if exemplar not in data:
        raise SystemExit(f"{exemplar} not present in {PASSABILITY}")
    entry = data[exemplar]
    return {
        "exemplar": exemplar,
        "doors": sorted(entry["doors"]),
        "coins": entry["coins"],
        # pair -> set of loadouts that must solve; loadouts absent must stay
        # unsolved (the auditor's bounded search must stay inconclusive).
        "pairs": {
            pair: gating_class(l for l, v in loads.items() if v is not None)
            for pair, loads in entry["pairs"].items()
        },
        "coin_routes": {
            index: gating_class(l for l, v in loads.items() if v is not None)
            for index, loads in entry["coin_routes"].items()
        },
    }


def family_members(contract: dict) -> list[str]:
    """Existing vocabulary grids with the same doors+profile as the contract."""
    data = json.loads(PASSABILITY.read_text())
    members = []
    for slug, entry in data.items():
        profile = {
            pair: gating_class(l for l, v in loads.items() if v is not None)
            for pair, loads in entry["pairs"].items()
        }
        if sorted(entry["doors"]) == contract["doors"] and profile == contract["pairs"]:
            members.append(slug)
    return sorted(members)


# --------------------------------------------------------------------------
# Grid model


class Grid:
    def __init__(self) -> None:
        self.cells = [[SOLID] * WIDTH for _ in range(HEIGHT)]

    def get(self, c: int, r: int) -> str:
        return self.cells[r][c]

    def set(self, c: int, r: int, glyph: str) -> None:
        self.cells[r][c] = glyph

    def in_interior(self, c: int, r: int) -> bool:
        return 1 <= c < WIDTH - 1 and 1 <= r < HEIGHT - 1

    def render(self) -> str:
        return "\n".join("".join(row) for row in self.cells) + "\n"


def carve_rect(grid: Grid, c0: int, r0: int, c1: int, r1: int, glyph: str = EMPTY) -> None:
    for r in range(max(0, r0), min(HEIGHT, r1 + 1)):
        for c in range(max(0, c0), min(WIDTH, c1 + 1)):
            grid.set(c, r, glyph)


# --------------------------------------------------------------------------
# Approximate movement model (prefilter only; the auditor is ground truth).
#
# States are empty tiles. The player occupies ~1x2 tiles, so a tile is
# standable if it and the tile above are open and the tile below is support.


def open_at(grid: Grid, c: int, r: int) -> bool:
    if not (0 <= c < WIDTH and 0 <= r < HEIGHT):
        return False
    return grid.get(c, r) in (EMPTY, ONEWAY) or grid.get(c, r) in SPIKES


def support_below(grid: Grid, c: int, r: int) -> bool:
    if r + 1 >= HEIGHT:
        return True
    return grid.get(c, r + 1) in (SOLID, ONEWAY)


def reachable(grid: Grid, start: tuple[int, int], loadout: str) -> set[tuple[int, int]]:
    """Tiles reachable from `start` under a crude movement model."""
    wall = loadout in ("wall", "both")
    dash = loadout in ("dash", "both")
    # Probe-measured: bare gains height only by jumping THROUGH a one-way
    # tile <= 3 rows up; wall stretches that to 5; solid ledges need dash.
    jump_rise = 5 if wall else 3
    gap = 5 if dash else 2  # columns a jump can cross
    seen: set[tuple[int, int]] = set()
    frontier = [start]
    while frontier:
        c, r = frontier.pop()
        if (c, r) in seen or not open_at(grid, c, r):
            continue
        seen.add((c, r))
        moves: list[tuple[int, int]] = []
        # walk / fall
        moves += [(c - 1, r), (c + 1, r), (c, r + 1)]
        grounded = support_below(grid, c, r)
        if grounded:
            # jump up: bare/wall only through one-way tiles, dash onto ledges
            for dr in range(1, jump_rise + 1):
                target = (c, r - dr)
                passes_oneway = any(
                    0 <= r - k < HEIGHT and grid.get(c, r - k) == ONEWAY
                    for k in range(1, dr + 1)
                )
                if passes_oneway or dash:
                    moves.append(target)
                    moves.append((c - 1, r - dr))
                    moves.append((c + 1, r - dr))
            # jump across a gap at same height or slightly up
            for dc in range(2, gap + 1):
                moves.append((c - dc, r))
                moves.append((c + dc, r))
                moves.append((c - dc, r - 1))
                moves.append((c + dc, r - 1))
        if wall and (
            not open_at(grid, c - 1, r) or not open_at(grid, c + 1, r)
        ):
            # climb a wall face
            moves.append((c, r - 1))
        for move in moves:
            if move not in seen and open_at(grid, *move):
                frontier.append(move)
    return seen


def landing_tile(grid: Grid, door: str) -> tuple[int, int]:
    """Where an arriving player ends up (arrival tile, then falls)."""
    c, r = ARRIVALS[door]
    while r + 1 < HEIGHT and not support_below(grid, c, r) and open_at(grid, c, r + 1):
        r += 1
    return (c, r)


def profile_matches(grid: Grid, contract: dict) -> bool:
    """Cheap check that the approximate model agrees with the contract."""
    doors = contract["doors"]
    reach: dict[tuple[str, str], set[tuple[int, int]]] = {}
    for door in doors:
        start = landing_tile(grid, door)
        for loadout in LOADOUTS:
            reach[(door, loadout)] = reachable(grid, start, loadout)
    for pair, klass in contract["pairs"].items():
        src, dst = pair.split("->")
        if src == dst:
            continue  # self-pairs (retreats) are nearly always fine
        near_exit = set()
        for mc, mr in MOUTHS[dst]:
            for dc, dr in ((0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)):
                near_exit.add((mc + dc, mr + dr))
        reached = {l: bool(reach[(src, l)] & near_exit) for l in LOADOUTS}
        if klass == "open" and not reached["none"]:
            return False
        if klass != "open" and reached["none"]:
            return False
        if klass in ("wall", "either") and not reached["wall"]:
            return False
        if klass in ("dash", "either") and not reached["dash"]:
            return False
        if klass != "closed" and not reached["both"]:
            return False
    return True


# --------------------------------------------------------------------------
# Generation


def place_doors(grid: Grid, doors: list[str]) -> None:
    for door in doors:
        for c, r in MOUTHS[door]:
            grid.set(c, r, EMPTY)
        # clearance pocket just inside the mouth
        if door == "west":
            carve_rect(grid, 1, 12, 4, 16)
        elif door == "east":
            carve_rect(grid, WIDTH - 5, 12, WIDTH - 2, 16)
        elif door == "ceiling":
            carve_rect(grid, 13, 1, 18, 4)
            # one-way perch just under the mouth: jump-through from below,
            # then a short bare hop back up into the door
            for c in range(14, 18):
                grid.set(c, 4, ONEWAY)
        elif door == "floor":
            carve_rect(grid, 13, 13, 18, 16)


def generate_candidate(seed: int, contract: dict) -> tuple[str, str]:
    """Return (grid_text, spec_text) for one seeded candidate."""
    rng = random.Random(seed)
    grid = Grid()

    # 1. carve an open interior, then re-add structure
    carve_rect(grid, 1, 1, WIDTH - 2, HEIGHT - 2)
    place_doors(grid, contract["doors"])

    # 2. structure in the vocabulary's house style: full-width strata with
    # offset holes. Chunky floors give the auditor's bounded search short,
    # findable routes (fiddly rung ladders read as "inconclusive").
    gutter_row = rng.choice((15, 16))
    for c in range(1, WIDTH - 1):
        if grid.get(c, gutter_row + 1) == EMPTY:
            grid.set(c, gutter_row + 1, SOLID)

    # Measured physics (probe rooms vs the auditor): bare ascent works ONLY
    # by jumping up through one-way floors spaced <= 3 rows; solid ledges are
    # effectively unclimbable bare; wall handles ~4-5 row spacing; dash is
    # the strongest traversal. So strata are mostly one-way at spacing 2-3.
    strata: list[int] = []
    r = gutter_row - rng.randint(2, 3)
    while r >= 4:
        strata.append(r)
        r -= rng.randint(2, 3)
    for r in strata:
        glyph = ONEWAY if rng.random() < 0.85 else SOLID
        for c in range(1, WIDTH - 1):
            grid.set(c, r, glyph)
        for _ in range(rng.randint(1, 2)):
            hole = rng.randint(1, WIDTH - 6)
            carve_rect(grid, hole, r, hole + rng.randint(2, 4), r)

    # partial walls between strata to shape rooms out of the open floors
    for _ in range(rng.randint(1, 4)):
        c0 = rng.randint(4, WIDTH - 5)
        r0 = rng.randint(2, gutter_row - 3)
        for r in range(r0, min(r0 + rng.randint(2, 5), gutter_row)):
            grid.set(c0, r, SOLID)

    # a few free shelves for texture
    for _ in range(rng.randint(2, 5)):
        length = rng.randint(2, 6)
        c0 = rng.randint(1, WIDTH - 2 - length)
        r0 = rng.randint(3, gutter_row - 1)
        glyph = ONEWAY if rng.random() < 0.5 else SOLID
        for c in range(c0, c0 + length):
            grid.set(c, r0, glyph)

    # re-open door pockets that structure may have buried
    place_doors(grid, contract["doors"])

    # 3. repair connectivity for bare-required pairs by dropping helper rungs
    for _ in range(8):
        if profile_matches(grid, contract):
            break
        repair_once(grid, contract, rng)
    if not profile_matches(grid, contract):
        raise ValueError("did not converge")

    # 4. spikes (never pointing into walls)
    spikes = []
    for _ in range(rng.randint(0, 3)):
        for _attempt in range(30):
            c = rng.randint(2, WIDTH - 3)
            r = rng.randint(2, HEIGHT - 3)
            if grid.get(c, r) != EMPTY:
                continue
            if grid.get(c, r + 1) == SOLID and grid.get(c, r - 1) == EMPTY:
                grid.set(c, r, "^")
                spikes.append((c, r))
                break

    # 5. coins on standable tiles, far from every door landing
    landings = [landing_tile(grid, d) for d in contract["doors"]]
    stand = [
        (c, r)
        for r in range(2, HEIGHT - 1)
        for c in range(1, WIDTH - 1)
        if grid.get(c, r) == EMPTY
        and grid.get(c, r - 1) == EMPTY
        and support_below(grid, c, r)
    ]
    rng.shuffle(stand)
    stand.sort(
        key=lambda t: -min(abs(t[0] - l[0]) + abs(t[1] - l[1]) for l in landings)
    )
    coins = []
    for c, r in stand:
        if len(coins) >= contract["coins"]:
            break
        if all(abs(c - pc) + abs(r - pr) > 4 for pc, pr in coins):
            coins.append((c, r))
    if len(coins) < contract["coins"]:
        raise ValueError("not enough coin spots")

    # 6. optionally one timed hazard over a mid-room chokepoint
    hazards = []
    if rng.random() < 0.6:
        c = rng.randint(8, 22)
        r = rng.randint(3, gutter_row - 3)
        if grid.get(c, r) == EMPTY and grid.get(c + 1, r) == EMPTY:
            period = rng.choice((150, 160, 180, 200))
            active = rng.choice((30, 45, 60))
            hazards.append((c * 10, r * 10, 20, 20, period, active, 0))

    spec_lines = [
        f"# generated candidate seed {seed} for family {contract['exemplar']}",
        "# (procedural baseline -- needs agent critique before entering the vocabulary)",
    ]
    spec_lines += [f"door {d}" for d in contract["doors"]]
    spec_lines += [f"coin {c * 10 + 2} {r * 10}" for c, r in coins]
    spec_lines += [f"hazard {' '.join(map(str, h))}" for h in hazards]
    return grid.render(), "\n".join(spec_lines) + "\n"


def repair_once(grid: Grid, contract: dict, rng: random.Random) -> None:
    """Nudge the grid toward the contract: open bare routes, gate open ones."""
    for pair, klass in contract["pairs"].items():
        src, dst = pair.split("->")
        if src == dst:
            continue
        start = landing_tile(grid, src)
        bare = reachable(grid, start, "none")
        near = set()
        for mc, mr in MOUTHS[dst]:
            for dc, dr in ((0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)):
                near.add((mc + dc, mr + dr))
        reached = bool(bare & near)
        if klass == "open" and not reached:
            # build a rung staircase from the closest bare-reachable tile all
            # the way to the target mouth, alternating a 2-column offset
            tc, tr = ARRIVALS[dst]
            best = min(bare, key=lambda t: abs(t[0] - tc) + abs(t[1] - tr), default=None)
            if best is None:
                continue
            c, r = best
            step = 0
            while abs(r - tr) > 2 or abs(c - tc) > 2:
                if r > tr + 1:
                    r -= 3
                elif r < tr - 1:
                    r += 3
                if abs(c - tc) > 1:
                    c += 2 if tc > c else -2
                c = min(max(c, 2), WIDTH - 4)
                if grid.in_interior(c, r):
                    # 4-wide one-way rung with cleared headroom above it
                    for cc in range(c - 1, c + 3):
                        if grid.in_interior(cc, r):
                            grid.set(cc, r, ONEWAY)
                        for rr in (r - 1, r - 2):
                            if grid.in_interior(cc, rr) and grid.get(cc, rr) == SOLID:
                                grid.set(cc, rr, EMPTY)
                step += 1
                if step > 12:
                    break
        elif klass != "open" and reached:
            # raise a barrier: pick a tile on the mouth approach and wall a
            # column above it so a bare jump can't make the rise
            mc, mr = rng.choice(MOUTHS[dst])
            bc = min(max(mc + rng.choice((-2, -1, 1, 2)), 1), WIDTH - 2)
            for r in range(max(1, mr - 6), mr):
                if grid.in_interior(bc, r):
                    grid.set(bc, r, SOLID)


# --------------------------------------------------------------------------
# Audit + match


def parse_audit(stdout: str) -> dict:
    pairs: dict[str, dict[str, int | None]] = {}
    coin_routes: dict[str, dict[str, int | None]] = {}
    verdict = False
    for line in stdout.splitlines():
        parts = line.split()
        if not parts:
            continue
        if parts[0] == "pair":
            _, src, dst, loadout, status = parts[:5]
            slot = pairs.setdefault(f"{src}->{dst}", {})
            slot[loadout] = int(parts[5]) if status == "solved" else None
        elif parts[0] == "coin":
            index, loadout, status = parts[1], parts[4], parts[5]
            slot = coin_routes.setdefault(index, {})
            ticks = int(parts[6]) if status == "solved" else None
            if ticks is not None and (slot.get(loadout) is None or ticks < slot[loadout]):
                slot[loadout] = ticks
            else:
                slot.setdefault(loadout, ticks)
        elif parts[0] == "verdict":
            verdict = parts[1] == "ok"
    return {"pairs": pairs, "coin_routes": coin_routes, "verdict_ok": verdict}


def audit_matches(result: dict, contract: dict) -> bool:
    if not result["verdict_ok"]:
        return False
    profile = {
        pair: gating_class(l for l, v in loads.items() if v is not None)
        for pair, loads in result["pairs"].items()
    }
    if profile != contract["pairs"]:
        return False
    coin_profile = {
        index: gating_class(l for l, v in loads.items() if v is not None)
        for index, loads in result["coin_routes"].items()
    }
    return coin_profile == contract["coin_routes"]


def audit_candidate(paths: tuple[pathlib.Path, pathlib.Path]) -> dict:
    grid_path, spec_path = paths
    proc = subprocess.run(
        [str(AUDITOR), str(grid_path), str(spec_path)],
        capture_output=True,
        text=True,
    )
    return parse_audit(proc.stdout)


# --------------------------------------------------------------------------
# Ranking


def tile_distance(a: str, b: str) -> float:
    same = sum(1 for x, y in zip(a, b) if x == y)
    return 1.0 - same / len(a)


def pick_diverse(
    survivors: list[tuple[int, str, str]], exemplars: list[str], keep: int
) -> list[tuple[int, str, str]]:
    """Greedy max-min-distance selection against exemplars and each other."""
    chosen: list[tuple[int, str, str]] = []
    pool = list(survivors)
    references = list(exemplars)
    while pool and len(chosen) < keep:
        best = max(
            pool,
            key=lambda cand: min(
                (tile_distance(cand[1], ref) for ref in references), default=1.0
            ),
        )
        pool.remove(best)
        chosen.append(best)
        references.append(best[1])
    return chosen


# --------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--family", required=True, help="exemplar slug to match")
    parser.add_argument("--seeds", default="0:200", help="seed range A:B")
    parser.add_argument("--keep", type=int, default=5)
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--out", default=None, help="output directory")
    args = parser.parse_args()

    if not AUDITOR.exists():
        raise SystemExit(
            "auditor missing: cargo build --release -p downwards-content --example audit_room_grid"
        )

    contract = load_contract(args.family)
    members = family_members(contract)
    exemplar_grids = [
        (ROOMS / f"{slug}.txt").read_text() for slug in members if (ROOMS / f"{slug}.txt").exists()
    ]
    out = pathlib.Path(args.out) if args.out else ROOT / "target" / "room-gen" / args.family
    out.mkdir(parents=True, exist_ok=True)

    lo, hi = (int(x) for x in args.seeds.split(":"))
    generated: list[tuple[int, str, str]] = []
    for seed in range(lo, hi):
        try:
            grid_text, spec_text = generate_candidate(seed, contract)
        except ValueError:
            continue
        generated.append((seed, grid_text, spec_text))
    print(f"generated {len(generated)}/{hi - lo} candidates past the prefilter")

    cand_paths = []
    for seed, grid_text, spec_text in generated:
        grid_path = out / f"cand-{seed}.txt"
        spec_path = out / f"cand-{seed}.spec.txt"
        grid_path.write_text(grid_text)
        spec_path.write_text(spec_text)
        cand_paths.append((seed, grid_path, spec_path))

    survivors: list[tuple[int, str, str]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {
            pool.submit(audit_candidate, (grid_path, spec_path)): seed
            for seed, grid_path, spec_path in cand_paths
        }
        for future in concurrent.futures.as_completed(futures):
            seed = futures[future]
            result = future.result()
            if audit_matches(result, contract):
                grid_path = out / f"cand-{seed}.txt"
                spec_path = out / f"cand-{seed}.spec.txt"
                survivors.append((seed, grid_path.read_text(), spec_path.read_text()))
    print(f"{len(survivors)} candidates match the family profile exactly")

    chosen = pick_diverse(survivors, exemplar_grids, args.keep)
    report = {
        "family": args.family,
        "members": members,
        "contract": contract["pairs"],
        "generated": len(generated),
        "matched": len(survivors),
        "kept": [seed for seed, _, _ in chosen],
    }
    for seed, grid_text, spec_text in chosen:
        (out / f"keep-{seed}.txt").write_text(grid_text)
        (out / f"keep-{seed}.spec.txt").write_text(spec_text)
    (out / "report.json").write_text(json.dumps(report, indent=1))
    print(f"kept {len(chosen)} diverse baselines in {out}")
    for seed, _, _ in chosen:
        print(f"  keep-{seed}.txt")
    return 0


if __name__ == "__main__":
    sys.exit(main())
