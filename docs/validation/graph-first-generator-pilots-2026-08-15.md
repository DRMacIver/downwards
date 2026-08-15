# Graph-first generator pilots — 2026-08-15

Status: **experimental; not a production corpus**.

This report records the first bounded tests of generators that derive route graphs before room
coordinates. It deliberately separates structural construction, exact replay evidence, finite
controller observations, and heuristic terrain attribution. A bounded solver miss is
inconclusive, never unreachable. No difficulty band or scalar fun score is assigned.

The corresponding research contract is
[`../research/corpus-plan.md`](../research/corpus-plan.md).

## Sources

### Recursive partition route v1

The generator recursively splits a traversal region into two to four chambers, realizes collision
cuts and optional fork/rejoin branches, and then attaches two to four boundary ports plus a pickup.
The v1 grammar authors only baseline-capable route edges. Ability-bearing construction keys may
select a different deterministic random stream, but v1 makes no wall-jump or dash requirement
claim.

Construction-only expressivity over 128 Standard/Mixed-BSP/baseline keys:

| Measure | Result |
| --- | ---: |
| Constructed | 128 / 128 |
| Distinct static tile maps | 127 / 128 |
| Distinct pure graph topologies | 42 / 128 |
| Distinct coordinate-free route/rhythm derivations | 128 / 128 |
| Distinct full normalized derivations | 128 / 128 |

A separate invariant sweep constructed 576/576 keys across three profiles, three intents, and 64
seeds. Every route edge claimed by v1 has a baseline-conservative rise, and no route graph claims a
wall-jump or dash edge. These are construction facts only.

The first authoritative gameplay sample used seeds 0–3, all three intents, Mixed-BSP, and baseline
construction:

| Measure | Baseline construction | Complete kit |
| --- | ---: | ---: |
| Constructed rooms | 12 / 12 | 12 / 12 |
| Rooms passing every door and pickup target | 12 / 12 | 12 / 12 |
| Positive directed door rows | 80 / 80 | 80 / 80 |
| Positive pickup rows | 31 / 31 | 31 / 31 |
| Bounded-inconclusive target rows | 0 | 0 |
| Direct finite-controller positives | 57 / 80 | 57 / 80 |
| Direct classes: run / monotone-only / other | 13 / 35 / 9 | 13 / 34 / 10 |
| Complete finite vocabulary with no positive | 23 / 80 | 23 / 80 |

The authoritative positives and finite-controller misses are compatible: the latter describe a
fixed diagnostic controller vocabulary, not all possible play. Four sampled floor arrivals were
trigger-disjoint, did not immediately self-trigger, retained source-search rows, and suppressed
only their own source target as intended.

Of 40 unordered door pairs, 37 had a measured directional difference. Total canonical duration
difference was 2,279 ticks (maximum 233 for one pair); total horizontal-reversal difference was 42
(maximum 7). The 12 rooms were distinct at graph, route, normalized-derivation, and embedded-route
signature layers.

Terrain accounting found 850 interior tiles: 566 statically attributed to the route plan and 773
positively corroborated by known traversal. One component containing 77 tiles remained
uncorroborated. That is an ablation target, not proof of uselessness.

The eight-seed, all-profile v1 construction inventory found a composition correctness failure before
promotion. All 72 keys constructed, with 24 rooms each at two, three, and four ports, but only 4 of
20 distinct socket signatures had an opposite mate. Left/right sockets were closed; ceiling doors
used 15 offsets while floor doors used only three, leaving 16 distinct signatures and 59 port
occurrences without a mate.

Partition-route v2 replaced that mapping with a frozen three-slot ceiling/floor grid. It closed the
72-key socket inventory, but its fixed ceiling connector crossed authored secondary separators.
The authoritative rerun exposed the mismatch: construction produced only 67/80 positive baseline
door rows and 70/80 complete-kit door rows, while all 31 pickup rows remained positive under each
loadout. Every bounded miss targeted the ceiling port. Inspection found solid partition columns
punching through declared connector landings. V2 is therefore rejected despite its socket closure;
increasing solver budgets or clearing the columns after rasterization would have hidden a generator
correctness error.

Partition-route v3 makes route/headroom/arrival cells pre-raster constraint reservations. A key is
rejected when a reservation conflicts with any authored root or secondary cut; no post-raster tile
is cleared or redrawn. It then checks every authored cut byte-for-byte, exact landing material and
standing clearance, and the shared conservative directed and reverse baseline transition contract.
Exact attempt zero intentionally may fail. In construction-only tests, 125/128 Standard
Mixed-BSP/baseline keys constructed (124 distinct static maps, 40 graph signatures, 125 route
signatures, and 125 full derivations); the broader profile/intent/loadout invariant sweep produced
566/576 rooms with ten deterministic typed failures. The eight-seed socket pool produced 71/72
rooms and 212 socket occurrences; every occurrence had an opposite compatible mate on a different
generated room. No v1 replay result is carried forward as evidence for the changed geometry.

The v3 authoritative rerun used the same seeds 0--3, all three intents, Mixed-BSP, and baseline
construction. All 12 rooms passed both all-target gates: 80/80 directed door routes and 31/31
pickup routes were positive under baseline, and the complete kit repeated 80/80 plus 31/31 with no
bounded miss. Four floor entries remained trigger-disjoint, did not self-trigger, and retained
their source rows. The finite controller audit found 62/80 baseline positives (15 run-only, 35
monotone-simple-only, 12 other) and 59/80 complete-kit positives (15/34/10); the remaining 18 and
21 rows are complete finite-vocabulary no-positive observations, not reachability failures. Every
one of the 40 unordered door pairs had a measured directional demand difference. All 12 rooms were
distinct at every recorded signature layer. Terrain accounting found 823 interior tiles in 102
components: 521 tiles were statically attributed and 721 positively corroborated, leaving zero
wholly uncorroborated components but 102 uncorroborated tiles for later support-face ablation. This
passes the bounded v3 pilot; larger-profile breadth, easiest-controller, and ablation evidence are
still required before corpus promotion.

### Recursive compositional route cuts v1

The coordinate-free grammar starts from one source-to-sink edge and composes repeated subdivision,
zero to three ordered collision-cut obligations, zero to two fork/rejoin rewrites, pickup marking,
and two to four boundary-port attachments. Mission derivation is independent of embedding retry
identity.

In the first 128-seed Standard/complete-kit derivation test, at least 116 keys had distinct pure
topology signatures and at least 116 had distinct ordered derivation signatures. The bounded
exact-attempt embedding then constructed 64/64 representative attempt-0 keys. Static geometry,
embedded route-plan signatures, and coordinate-free topology each had at least 58/64 distinct
values. Focused checks cover single-opening cut separators, collision-distinct fork supports,
complete mission-to-route ownership, an actual rise/fall rhythm, baseline-conservative links, safe
door arrivals, and a finite socket inventory closed under opposite mates. The source explicitly
makes no wall-jump or dash requirement claim.

The first authoritative 12-key run constructed every room and positively certified all 42 pickup
rows under both baseline and the complete kit. Door results were 94/112 baseline and 109/112
complete kit; every non-positive row targeted the same logical `port-1`. Baseline misses were
bounded by the path horizon, while the three complete-kit misses for Gentle seed 3 exhausted
simulated ticks. Only 4/12 baseline and 11/12 complete-kit rooms therefore passed the complete
all-target gate. All 12 rooms were distinct at topology, derivation, embedded-route, static, and
simulation layers, and all 909 interior terrain tiles were statically attributed and positively
corroborated. The systematic target failure is under generous-search and segmented-route
diagnosis; no geometry has been shortened and no miss has been relabelled unreachable.

The simplest failing key localized a generator contract error rather than a search-budget issue.
Its 12-edge authored baseline route contained two early transfers with support-edge gaps of nine
tiles, because the shared `edge_verb` classification considered vertical delta but not horizontal
distance. A 2,400-tick-horizon, 500k-node/20m-tick search still found no baseline witness; the
finite 302-controller audit was complete with no positive. The complete kit reached the ceiling in
91 ticks using five dashes. This is positive evidence of an unintended dash bypass, not proof that
baseline traversal is impossible. That embedding was superseded by the version-2 bounded
horizontal/vertical movement constraints below; exact keys that cannot embed now fail explicitly
rather than receive hidden bridge terrain.

### Recursive compositional route cuts v2

Version 2 keeps the coordinate-free grammar and replaces only the embedding contract. A memoized
bounded constraint search assigns spine and fork supports, retains the required horizontal
reversal, checks the shared conservative transition in both directions for every mission edge,
and rejects exact keys whose cut, standing-clearance, floor-arrival, or raster contract cannot be
satisfied. Raster validation requires the exact declared support material, not merely any
collision tile. No retry, bridge terrain, or geometry fallback is hidden inside one exact key.

For Standard/complete-kit attempt-zero seeds 0--255, 253/256 keys constructed. The three failures
are deterministic typed spine-constraint exhaustions (seed 9 after 117,890 explored assignments;
seeds 93 and 211 at the 500,000-assignment bound). All 253 constructed rooms have distinct static
and embedded-route signatures, and 252 have distinct coordinate-free topology signatures. Every
emitted socket occurrence has a compatible opposite on a different candidate in that exact pool.
One west-side horizontal socket offset is deliberately excluded from the emitted domain because
the fixed pool produced it only as a ceiling occurrence; the larger shared inventory remains
closed and the restriction is explicit rather than a fallback.

The authoritative seeds 0--3, all-intent baseline run constructed all 12 rooms. The complete kit
certified 112/112 directed door rows and 42/42 pickups. Baseline certified all 42 pickups but only
101/112 door rows; every bounded miss targeted `port-1` and ended at the default path horizon.
This is not evidence that the rooms are impossible. Two representative misses were positively
rescued without changing geometry: a zero-cut room has a 365-tick exact baseline witness (316
ticks when composed through its pickup), and a room with two cuts plus one fork has a 369-tick
witness (285 ticks through its pickup). The finite 302-controller vocabulary was complete with no
positive in each case, which cleanly separates controller-vocabulary evidence from reachability.
The source remains unpromoted until a generic route-plan waypoint certifier records equivalent
positive evidence for every default miss; solver budgets will not be mistaken for level geometry.

## Promotion gates

Neither source is promoted merely for constructing many unique rooms. Before contributing to the
500–1,000-room corpus, a source must provide:

- exact all-door and all-pickup replay evidence at its construction loadout and the complete kit;
- honest bounded-inconclusive accounting;
- easiest-known controller evidence rather than a hand-picked hard route;
- variant-specific structural and reduced-loadout bypass evidence;
- route/static/collision diversity that is not only coordinate jitter;
- socket-mate coverage;
- terrain attribution and replay ablation evidence;
- deterministic regeneration and artifact verification.

Ability gates are a later grammar feature. A room is not labelled wall-jump- or dash-required until
the directed all-path structural cut, accepted ability event, and positive reduced-loadout bypass
audit support that exact claim.

Accordingly, the first final-path corpus enumeration uses only baseline construction keys for both
sources. Ability-bearing key bits remain available to isolated experiments, but are not admitted as
catalogue capability profiles merely because they perturb a random stream.
