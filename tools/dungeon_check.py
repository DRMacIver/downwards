#!/usr/bin/env python3
"""Rejection checker for candidate dungeon layouts — the convergence
instrument for the generator loop.

Accepts a layout iff ALL of:
  1. tools/dungeon_layout_metrics.py verdict ok (grid-exact embedding,
     exit on top row, geometric gating only, no absorbing traps, ...)
  2. tools/dungeon_difficulty.py finds a mandatory route and its score
     falls inside the requested difficulty band, on difficulty scale
     v2 (see that tool for why the scale was rebuilt):
         low  [14, 22)
         mid  [22, 30)   dungeon-v2, the shipped baseline, scores 25.4
         high [30, 120)
  3. structural sanity:
     * no two adjacent rooms share the same grid slug, and no
       same-class adjacency at all
     * pickups are a peak-so-far challenge on their route: each
       pickup's approach leg contains a crossing at >= 60% of the
       hardest crossing seen earlier on the route, and the leg is
       never free (>= 100 ticks)
     * every state reachable from the spawn can get back to it, at
       every loadout the dungeon can hand out: the reachable state
       graph is one strongly connected component
     * loadout consistency (see below)
     * act coherence, read against the dungeon's loadout

LOADOUT (designer requirement, 2026-08-17). A dungeon declares which
traversal items it contains: both / wall (glove only) / dash (boots
only) / none. The checker enforces that the layout contains exactly
those pickups, that its crown guard actually demands them, and that
its acts make sense for them:

  both  goal unreachable bare and reachable with both; each ability
        strictly expands the reachable room set; >= 1 backtrack unlock
  wall  goal unreachable bare, reachable with the glove alone; the
        glove strictly expands the map; >= 1 backtrack unlock
  dash  the same with the boots
  none  no pickups at all, so the coin gate is the only key: it must
        bind (a real requirement) and the coin tour must leave the
        pre-gate route, i.e. paying it is a journey; >= 1 backtrack
        unlock (the gate opening counts)

Usage:
  python3 tools/dungeon_check.py <layout.txt> [--band low|mid|high]
      [--loadout both|wall|dash|none] [--json]

Band and loadout default to the `# seed N size S band B acts A
loadout L` header comment the generator writes; without a header the
band falls back to `low` and the loadout is inferred from the pickups
the layout actually contains. Exit code 0 = accept, 1 = reject.
"""

import json
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from dungeon_difficulty import (  # noqa: E402
    Assessor, assess, one_way_states,
)
from dungeon_layout_metrics import Layout  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
PASSABILITY = ROOT / "docs" / "design" / "rooms-v2-passability.json"
# Scale v2 (see tools/dungeon_difficulty.py). dungeon-v2 scores 25.68.
SCORE_BANDS = {"low": (14.0, 22.0), "mid": (22.0, 30.0), "high": (30.0, 120.0)}
PICKUPS_FOR = {
    "both": {"glove", "boots"},
    "wall": {"glove"},
    "dash": {"boots"},
    "none": set(),
}


def loadout_of(layout):
    """The loadout a layout's pickups say it is."""
    have = set(layout.pickups)
    for name, wanted in PICKUPS_FOR.items():
        if have == wanted:
            return name
    return None


def run_metrics(path: pathlib.Path):
    proc = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "dungeon_layout_metrics.py"), str(path)],
        capture_output=True, text=True,
    )
    metrics = {}
    problems = []
    for line in proc.stdout.splitlines():
        if line.startswith("metric "):
            _, key, value = line.split(" ", 2)
            metrics[key] = value
        elif line.startswith("problem"):
            problems.append(line)
    return proc.returncode, metrics, problems


def check(path: pathlib.Path, band: str, loadout: str | None = None):
    reasons = []

    # 1. layout metrics
    code, metrics, problems = run_metrics(path)
    if code != 0:
        for problem in problems or [f"metrics failed (exit {code})"]:
            reasons.append(f"metrics: {problem}")

    # 2. difficulty band
    breakdown = assess(path)
    score = None
    if breakdown is None:
        reasons.append("difficulty: no mandatory route (spawn->pickups->coins->goal->exit)")
    else:
        score = breakdown["score"]
        lo, hi = SCORE_BANDS[band]
        if not (lo <= score < hi):
            reasons.append(
                f"difficulty: score {score} outside band {band} [{lo}, {hi})"
            )

    # 3a. adjacency repetition
    layout = Layout(path)

    def class_of(slug):
        return slug.rsplit("-", 1)[0]

    for a, _, b, _ in layout.edges:
        sa, sb = layout.rooms.get(a), layout.rooms.get(b)
        if sa is None or sb is None:
            continue
        if sa == sb:
            reasons.append(f"structure: adjacent rooms {a}/{b} share grid {sa}")
        elif class_of(sa) == class_of(sb):
            reasons.append(f"structure: adjacent rooms {a}/{b} share class {class_of(sa)}")

    # 3b. pickups are peak-so-far, never free
    if breakdown is not None:
        seen_peak = 0.0
        for leg in breakdown["legs"]:
            ticks = [c["ticks"] for c in leg["crossings"]]
            leg_peak = max(ticks, default=0)
            if leg["leg"] in ("to-glove", "to-boots"):
                pickup = leg["leg"].split("-", 1)[1]
                if leg["ticks"] < 100:
                    reasons.append(
                        f"structure: {pickup} approach is free ({round(leg['ticks'])} ticks)"
                    )
                if seen_peak and leg_peak < 0.6 * seen_peak:
                    reasons.append(
                        f"structure: {pickup} approach peak {leg_peak} below 60% of "
                        f"route peak-so-far {seen_peak}"
                    )
            seen_peak = max(seen_peak, leg_peak)

    # 3c. loadout consistency: the layout contains exactly the traversal
    # items it claims, and nothing it does not.
    declared = loadout or loadout_of(layout) or "both"
    actual = set(layout.pickups)
    if actual != PICKUPS_FOR[declared]:
        reasons.append(
            f"loadout: declared {declared} wants pickups "
            f"{sorted(PICKUPS_FOR[declared]) or 'none'} but the layout has "
            f"{sorted(actual) or 'none'}"
        )

    # 3d. nobody can be stranded: at every loadout this dungeon can hand
    # out, every state reachable from the spawn can get back to it.
    table = json.loads(PASSABILITY.read_text())
    assessor = Assessor(layout, table)
    feasible = ["none"] if declared == "none" else (
        ["none", "wall", "dash", "both"] if declared == "both"
        else ["none", declared]
    )
    for at in feasible:
        stuck = one_way_states(assessor, at)
        if stuck:
            rooms = sorted({layout.rooms.get(room, room) for room, _ in stuck})
            reasons.append(
                f"structure: at loadout {at}, {len(stuck)} state(s) in "
                f"{rooms[:4]} cannot return to the spawn state"
            )

    # 3e. act coherence, read against the declared loadout
    if metrics:
        try:
            at_none = int(metrics.get("rooms-at-none", 0))
            at_wall = int(metrics.get("rooms-at-wall", 0))
            at_dash = int(metrics.get("rooms-at-dash", 0))
            at_both = int(metrics.get("rooms-at-both", 0))
            backtracks = int(metrics.get("backtrack-unlock-events", 0))
        except ValueError:
            reasons.append("acts: could not parse loadout metrics")
            at_none = at_wall = at_dash = at_both = backtracks = 0
        opened = {"wall": at_wall, "dash": at_dash, "both": at_both}
        if declared == "both":
            if metrics.get("goal-reachable-at-none") != "False":
                reasons.append("acts: goal reachable with no abilities")
            if metrics.get("goal-reachable-at-both") != "True":
                reasons.append("acts: goal not reachable even with both abilities")
            if not (at_wall > at_none or at_dash > at_none):
                reasons.append("acts: neither ability expands the bare-reachable set")
            if not (at_both > max(at_wall, at_dash)):
                reasons.append("acts: the second ability opens no further territory")
        elif declared in ("wall", "dash"):
            if metrics.get("goal-reachable-at-none") != "False":
                reasons.append(
                    f"acts: goal reachable bare, so the {declared} item gates nothing"
                )
            if metrics.get(f"goal-reachable-at-{declared}") != "True":
                reasons.append(
                    f"acts: goal not reachable with the dungeon's only item ({declared})"
                )
            if opened[declared] <= at_none:
                reasons.append(
                    f"acts: the {declared} item opens no new territory "
                    f"({opened[declared]} rooms vs {at_none} bare)"
                )
        else:  # none: the coin gate is the only key, so it must be a real one
            if metrics.get("goal-reachable-at-none") != "True":
                reasons.append(
                    "acts: a dungeon with no traversal items must be finishable bare"
                )
            requirement = max(
                [g.get("coins", 0) for g in layout.gates.values()] + [0])
            if requirement < 1:
                reasons.append(
                    "acts: no traversal items and no coin gate — nothing keys the crown"
                )
            if breakdown is not None:
                tour = next((leg for leg in breakdown["legs"]
                             if leg["leg"] == "coin-tour"), None)
                if tour is None or not tour["crossings"]:
                    reasons.append(
                        "acts: the coin gate is paid off the pre-gate route, so the "
                        "dungeon's only key costs no journey"
                    )
        if backtracks < 1:
            reasons.append("acts: no backtrack-unlock events")

    return reasons, score, metrics, breakdown


def main() -> int:
    arguments = [a for a in sys.argv[1:] if not a.startswith("--")]
    as_json = "--json" in sys.argv
    band = loadout = None
    for i, argument in enumerate(sys.argv[1:-1], 1):
        if argument == "--band":
            band = sys.argv[i + 1]
        if argument == "--loadout":
            loadout = sys.argv[i + 1]
    if not arguments:
        print(__doc__)
        return 1
    path = pathlib.Path(arguments[0])
    header = path.read_text()
    if band is None:
        match = re.search(r"#.*\bband (\w+)", header)
        band = match.group(1) if match and match.group(1) in SCORE_BANDS else "low"
    if loadout is None:
        match = re.search(r"#.*\bloadout (\w+)", header)
        loadout = match.group(1) if match and match.group(1) in PICKUPS_FOR else None
    reasons, score, _, _ = check(path, band, loadout)
    if as_json:
        print(json.dumps({
            "accept": not reasons, "band": band, "loadout": loadout,
            "score": score, "reasons": reasons,
        }, indent=1))
    else:
        print(f"band {band} loadout {loadout or 'inferred'} score {score}")
        for reason in reasons:
            print(f"reject: {reason}")
        print("accept" if not reasons else f"REJECT ({len(reasons)} reasons)")
    return 0 if not reasons else 1


if __name__ == "__main__":
    raise SystemExit(main())
