#!/usr/bin/env python3
"""Run audit_room_grid over every rooms-v2 prototype and collect a passability table.

Usage: python3 tools/room_passability.py [--shaky] [slug ...]

Writes docs/design/rooms-v2-passability.json:
{
  "<slug>": {
    "doors": ["west", ...],
    "coins": N,
    "pairs": {"west->east": {"none": ticks|null, "wall": ..., "dash": ..., "both": ...}},
    "coin_routes": {"0": {"none": ticks|null, ...}},   # best over entry doors
    "verdict_ok": true
  }, ...
}
A null entry means the bounded search was inconclusive (not proof of impossibility).
"""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ROOMS = ROOT / "crates" / "downwards-gen" / "rooms-v2"
OUT = ROOT / "docs" / "design" / "rooms-v2-passability.json"
AUDITOR = ROOT / "target" / "release" / "examples" / "audit_room_grid"


def audit(slug: str, shaky: bool) -> dict:
    grid = ROOMS / f"{slug}.txt"
    spec = ROOMS / f"{slug}.spec.txt"
    cmd = [str(AUDITOR), str(grid), str(spec)] + (["--shaky"] if shaky else [])
    proc = subprocess.run(cmd, capture_output=True, text=True)
    doors = [
        line.split()[1]
        for line in spec.read_text().splitlines()
        if line.split("#")[0].strip().startswith("door")
    ]
    entry: dict = {
        "doors": doors,
        "coins": 0,
        "pairs": {},
        "coin_routes": {},
        "shaky": {},
        "verdict_ok": False,
    }
    for line in proc.stdout.splitlines():
        parts = line.split()
        if not parts:
            continue
        if parts[0] == "pair":
            _, src, dst, loadout, status = parts[:5]
            key = f"{src}->{dst}"
            slot = entry["pairs"].setdefault(key, {})
            slot[loadout] = int(parts[5]) if status == "solved" else None
        elif parts[0] == "coin":
            index, loadout, status = parts[1], parts[4], parts[5]
            entry["coins"] = max(entry["coins"], int(index) + 1)
            slot = entry["coin_routes"].setdefault(index, {})
            ticks = int(parts[6]) if status == "solved" else None
            if ticks is not None and (slot.get(loadout) is None or ticks < slot[loadout]):
                slot[loadout] = ticks
            else:
                slot.setdefault(loadout, ticks)
        elif parts[0] == "shaky":
            # shaky <coin> from <entry> <loadout> worst <family> <s>/<t>
            index, family, ratio = parts[1], parts[6], parts[7]
            successes, trials = ratio.split("/")
            slot = entry["shaky"].setdefault(index, {})
            fraction = int(successes) / int(trials)
            if slot.get("worst", 1.1) > fraction:
                slot["worst"] = fraction
                slot["family"] = family
        elif parts[0] == "verdict":
            entry["verdict_ok"] = parts[1] == "ok"
    return entry


def main() -> int:
    arguments = sys.argv[1:]
    shaky = "--shaky" in arguments
    slugs = [argument for argument in arguments if not argument.startswith("--")]
    if not slugs:
        slugs = sorted(p.stem for p in ROOMS.glob("*.txt") if not p.name.endswith(".spec.txt"))
    table = json.loads(OUT.read_text()) if OUT.exists() else {}
    for slug in slugs:
        print(f"auditing {slug}...", flush=True)
        table[slug] = audit(slug, shaky)
    OUT.write_text(json.dumps(table, indent=1, sort_keys=True))
    bad = [slug for slug in slugs if not table[slug]["verdict_ok"]]
    print(f"wrote {OUT} ({len(table)} rooms; {len(bad)} without verdict ok: {bad})")
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
