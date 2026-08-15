# Generator v5 validation report — 2026-08-14

> Historical report: v5 used a small set of whole-room, single-exit families. Its exit and pickup
> certificates remain reproducible evidence for that generator, but they do not satisfy the v6
> requirement to certify every ordered boundary-door pair and every pickup from every door. See
> the [generator v6 report](generator-v6-2026-08-14.md) for the current architecture and evidence.

This report records the all-route single-room generation gate for *Downwards*. It covers
generator version 5, seeds 0 through 999 inclusive, and all four exact traversal loadouts.
Generator v5 supersedes the historical exit-only v4 gate by giving optional routes comfortable
structural margins and requiring independent solver certificates for every pickup.

The release-mode command for each row is:

```sh
target/release/downwards-tools validate-seeds 0 1000 <tier>
```

For every accepted seed, the tool constructs the room through core validation, issues an exit
`AcceptanceCertificate`, and independently issues a `PickupReachabilityCertificate` for every
declared pickup from a fresh room state. Every replay frame is checked against both its expected
state digest and its ordered event-stream digest. An inconclusive exit search, inconclusive pickup
search, replay divergence, or certificate error fails the scenario and the command.

## Results

| Loadout | Family acceptance | Exit certificates | Pickup certificates | Exit witness fingerprint | Pickup witness fingerprint |
| --- | --- | ---: | ---: | --- | --- |
| Baseline | HazardRun 504/504; TerracedAscent 496/496 | 1000/1000 | 1000/1000 | `50bb8aeb3cd191b5` | `be05fd2af994acc0` |
| Wall jump | TerracedAscent 504/504; Chimney 496/496 | 1000/1000 | 1000/1000 | `277de2ac7f4d844a` | `84457bf4c1ef8121` |
| Dash | HazardRun 504/504; DashGallery 496/496 | 1000/1000 | 1000/1000 | `2f026fe5a409c37f` | `ae0043761c169a08` |
| Wall jump + dash | Chimney 504/504; DashGallery 496/496 | 1000/1000 | 1000/1000 | `1da55fd6072d186f` | `c222784986fb4203` |

The diagnostics below describe the accepted exit witnesses. Pickup certificates establish
positive reachability but do not yet run the exit difficulty analysis.

| Loadout | Mean completion | Mean transitions | Mean timing robustness | Clearance (min / p50 / p95 / max) | Provisional bands | Accepted traversal events |
| --- | ---: | ---: | ---: | --- | --- | --- |
| Baseline | 192.891 ticks | 8.984 | 0.611 | 0 / 1 / 20 / 20 px | Standard 1000 | 3,992 jumps |
| Wall jump | 140.663 ticks | 13.480 | 0.554 | 9 / 20 / 70 / 70 px | Standard 504; Technical 496 | 5,496 jumps, including 992 wall jumps |
| Dash | 171.104 ticks | 11.960 | 0.594 | 0 / 1 / 17 / 17 px | Standard 504; Technical 496 | 3,000 jumps; 1,488 dashes |
| Wall jump + dash | 118.184 ticks | 16.496 | 0.536 | 2 / 9 / 70 / 70 px | Technical 1000 | 4,512 jumps, including 1,008 wall jumps; 1,488 dashes |

Completion ranges (min / p50 / p95 / max) were 180 / 184 / 209 / 209 ticks (baseline),
79 / 180 / 209 / 209 (wall jump), 158 / 184 / 184 / 184 (dash), and
79 / 79 / 158 / 158 (wall jump + dash). Input-transition ranges were 7 / 7 / 11 / 11,
11 / 11 / 16 / 16, 7 / 7 / 17 / 17, and 16 / 16 / 17 / 17 respectively. Timing-robustness
ranges were 0.595 / 0.615 / 0.619 / 0.619, 0.500 / 0.595 / 0.619 / 0.619,
0.561 / 0.615 / 0.615 / 0.615, and 0.500 / 0.500 / 0.576 / 0.576. Every exit witness had an
applicable hazard-clearance measurement. Clearance is integer Chebyshev edge distance between
authoritative player and hazard AABBs; zero can mean legal exact edge contact and does not by
itself imply a death.

All 4,000 generated scenarios therefore passed both their required-exit objective and every
declared pickup objective. The generator currently declares one pickup per room, so this run
issued 4,000 exit certificates and 4,000 pickup certificates. The complete four-loadout batch
was run twice; both runs reported identical metrics and both aggregate fingerprints for every
loadout.

The two fingerprints intentionally remain separate. The exit aggregate covers room-completion
witnesses and their event streams; the pickup aggregate folds the seed, exact pickup ID, and its
event-aware witness fingerprint. A change to either route is therefore visible without conflating
the two acceptance claims.

As a focused regression for the playtest report that prompted this gate, baseline seed 1's
`optional-cache` has a verified 103-tick pickup witness (81 expanded nodes and 13,324 simulated
ticks), fingerprint `downwards-witness-v2-afe0292f8cb72b55`. Its separate exit witness completes
in 180 ticks.

## Generator v5 route margin

The generated pickup remains an optional gameplay challenge, but it is no longer optional for
acceptance. Baseline branches use staged supports with at most 20 pixels of vertical rise,
comfortably below the roughly 31-pixel full-jump apex. Dash branches leave at least 20 pixels
beneath the conservative combined range of a jump into an upward dash, while wall-jump branches
retain continuous opposing walls. These structural budgets provide legibility and tuning margin;
the separate pickup witnesses provide the authoritative positive reachability evidence.

## Visual diversity

The catalogue's three-word names uniquely identify seed/loadout scenarios; they do not imply
unique room geometry. Grouping exact visible geometry within each 1,000-seed loadout gives:

| Loadout | Exact visual shapes | Scenarios |
| --- | ---: | ---: |
| T1 baseline | 251 | 1,000 |
| T2 wall jump | 134 | 1,000 |
| T3 dash | 141 | 1,000 |
| T4 wall jump + dash | 24 | 1,000 |

Most seeds therefore share visual geometry with other seeds, especially in T4. Challenge timing
usually differs even when the visible geometry matches, because periodic-hazard parameters remain
part of the generated scenario. The browser exposes the exact-shape group through a six-digit
display tag, match count, and first matching seed. These counts are an honest content-diversity
baseline, not evidence of 4,000 visually distinct levels.

## Interpretation and limits

The exit complexity bands and timing ratios are deterministic diagnostics, not calibrated claims
about human difficulty. The formal tool labels them “diagnostic heuristics only; calibrate against
human playtests.” Pickup certificates currently prove a reproducible route exists but do not
attach the full exit difficulty analysis to that route. The sampled gate cannot prove seeds
outside 0–999, does not persist every individual witness to disk, and does not validate multi-room
dungeon topology. Human playtesting remains necessary for movement feel and perceived difficulty.
