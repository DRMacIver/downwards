# Generator v4 validation report — 2026-08-14

> Historical report: v4 certified required exits but did not certify its optional pickups.
> Generator v5 replaced this gate with separate exit and per-pickup certificates. Both versions
> predate v6 multi-door compositional generation; see the
> [generator v6 report](generator-v6-2026-08-14.md) for the current contract.

This report records the first formal single-room generation gate for *Downwards*. It covers
generator version 4, seeds 0 through 999 inclusive, and all four exact traversal loadouts.

The release-mode command for each row was:

```sh
target/release/downwards-tools validate-seeds 0 1000 <tier>
```

For every accepted seed, the tool constructed the room through core validation and issued an
owned acceptance certificate binding its provenance, loadout, required exit, solver witness, and
difficulty report. Every replay frame was checked against both its expected state digest and its
ordered event-stream digest. An inconclusive search or certificate error would have failed the
command.

## Results

| Loadout | Family acceptance | Accepted | Mean completion | Mean input transitions | Mean ±1/±2 timing robustness | Minimum hazard clearance (min / p50 / max) | Provisional bands | Accepted ability events | Witness fingerprint |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- | --- | --- |
| Baseline | HazardRun 504/504; TerracedAscent 496/496 | 1000/1000 | 192.891 ticks | 8.984 | 0.611 | 0 / 1 / 20 px | Standard 1000 | 3,992 jumps | `fc9fec8a4b4b0fa0` |
| Wall jump | TerracedAscent 504/504; Chimney 496/496 | 1000/1000 | 140.663 ticks | 13.480 | 0.554 | 9 / 20 / 70 px | Standard 504; Technical 496 | 5,496 jumps, including 992 wall jumps | `163b459960b2cd39` |
| Dash | HazardRun 504/504; DashGallery 496/496 | 1000/1000 | 171.104 ticks | 11.960 | 0.594 | 0 / 1 / 17 px | Standard 504; Technical 496 | 3,000 jumps; 1,488 dashes | `d74e15bd710eedd4` |
| Wall jump + dash | Chimney 504/504; DashGallery 496/496 | 1000/1000 | 118.184 ticks | 16.496 | 0.536 | 2 / 9 / 70 px | Technical 1000 | 4,512 jumps, including 1,008 wall jumps; 1,488 dashes | `1df7063d29b079aa` |

Completion ranges were 180–209 ticks (baseline), 79–209 (wall jump), 158–184 (dash), and
79–158 (wall jump + dash). Robustness ranges were 0.595–0.619, 0.500–0.619, 0.561–0.615, and
0.500–0.576 respectively. Clearance is integer Chebyshev edge distance between authoritative
player and hazard AABBs; zero can mean legal exact edge contact and does not by itself imply a
death.

The complete certificate-producing four-tier batch was run twice. Both runs produced the same
family totals, metric distributions, and per-tier aggregate witness fingerprints shown above.
The aggregate fingerprints include each frame's semantic action, state digest, and event-stream
digest. This is evidence that seed mapping, search ordering, simulation events, and witness
generation are stable for this build.

## What the batch changed

Earlier batches correctly rejected two generator defects instead of silently accepting them:

- three-tile floor spike runs exceeded the measured baseline jump envelope; and
- a low decorative shelf formed an impassable side wall at the end of some otherwise valid
  two-tile spike runs.

Those seed-to-room changes produced generator versions 3 and 4. Version 4 caps the relevant
clusters at two tiles and preserves a clear lower jump arc. The dash solver also gained a staged
jump/up-dash probe after direct simulation demonstrated a valid first-platform route.

## Interpretation and limits

The complexity bands and timing ratios are deterministic diagnostics, not calibrated statements
about human difficulty. The current generated catalogue advertises Standard and Technical rooms;
Gentle exists as an analysis band but is not yet a generation target. Search effort is only one
capped component of the score.

This v4 gate validated each room's required exit and exact loadout. Optional collectibles were not
part of that historical command; generator v5 adds their independent targeted certificates. The
v4 evidence also did not prove that no seed outside the sampled range can fail, persist all 1,000
individual witnesses to disk, or validate multi-room dungeon topology. Those broader limitations
still apply, and human playtesting is still required to tune feel and calibrate the metrics.
