# Diverse room corpus research plan

Status: approved direction, 2026-08-14

This document is the durable plan for the next procedural-generation phase. It records the
product decisions made after playtesting the first v6 catalogue. It supersedes band-quota-driven
curation as the project’s active research direction. The existing v6 manifests remain useful as
runtime fixtures, but their difficulty labels are not accepted evidence that their routes are
meaningfully difficult.

## Objective

Build a deterministic corpus of **500–1,000 distinct room geometries**. Each room is a reusable
dungeon tile with two to four boundary doors, not one indivisible start-to-exit level. The corpus
must support later multi-room dungeon assembly and serious comparison of:

- directed door-to-door difficulty;
- route and play-style diversity;
- ability requirements and directional asymmetry;
- terrain and obstacle utility;
- robustness to imperfect human timing;
- morphology, topology, and visual diversity.

The unit of difficulty analysis is a **directed source-door to target-door challenge under a
specific ability loadout**. A room deliberately may be easy in one direction and hard or
unreachable with the current loadout in the reverse direction. This is desirable for branches
that are easy to descend before collecting an ability and easier to climb back out of afterward.

Every room therefore retains a route matrix rather than one room-level difficulty label. Summary
statistics such as minimum, median, maximum, spread, and directional asymmetry may describe that
matrix, but may not replace it.

## Priority order

1. Correctness of simulation, evidence, and claims.
2. Positive reachability certification and honest uncertainty.
3. Useful continuous difficulty measurements.
4. Route and behavioral diversity.
5. Terrain and obstacle utility/readability.
6. Corpus-scale morphology and topology diversity.
7. New obstacle vocabulary.

When a correctness problem is found, corpus expansion and feature work stop until it is fixed and
covered by a regression test.

## Evidence rules

- A replay verified by the authoritative deterministic simulation is positive reachability
  evidence.
- Failure of a bounded solver is only `inconclusive`; it is never proof of impossibility.
- An ability is called required only when the structural graph establishes that **all** relevant
  paths require it. Merely selecting one path containing an ability edge is insufficient.
- Any exact positive success under a smaller loadout is ability-bypass evidence, even if replaying
  the same buttons with more abilities changes the trajectory.
- The easiest known successful route governs difficulty comparisons. A harder alternate route
  cannot make a challenge difficult when a simpler successful controller is known.
- Search effort is operational evidence about the solver, not player difficulty.
- Absence of an AI traversal near a terrain feature is not proof that the feature is unreachable
  or useless. Terrain-utility metrics use positive corroboration and report uncertainty.

## Immediate correctness gate

No new production corpus is promoted until all of these are true:

- lower-loadout successes are retained and veto false wall-jump/dash requirement claims;
- required abilities are computed as unavoidable across alternate structural paths;
- the easiest-known audit retains later simple-controller successes instead of stopping at the
  first witness;
- every advertised route completes every expected direct-controller/loadout audit without a
  budget-limited result;
- walk-only and fixed-direction auto-jump bypasses are reported rather than hidden by harder
  witnesses;
- terrain cannot improve selection merely by adding tiles inside a permissive heuristic envelope;
- the strict runtime catalogue loader understands and validates the final evidence schema;
- representative route and pickup traces still replay exactly after regeneration.
- when one exact physical room has several constructive derivations, construction-loadout and
  structural claims remain variant-specific; vector insertion order may not silently choose the
  room's authoritative route plan or ability contract.

The all-success direct-controller audit, terrain-ranking correction, lower-loadout positive-bypass
veto, and directed all-path ability-unavoidability checks are implemented and covered by focused
regressions. Structural unavoidability is explicitly scoped to the authored route graph; it is not
misrepresented as proof that no unmodelled physical shortcut exists. Temporary selection
manifests in `/tmp` remain experiments, not production assets.

### Human-calibration stop (2026-08-15)

Human playtesting found `STILL FLAME HALL`, the strongest predicted difficulty outlier in the
current 68-room playtest export, to be essentially trivial. This is a correctness stop for
difficulty-guided generation and selection. The retained 384-tick witness used 25 jump presses
and 40 horizontal reversals in a hazard-free room made from broad one-way shelves; the finite
direct-controller vocabulary missed the simple human route, and open-loop shaky-hand failures
were timeouts rather than deaths. “Easiest known” was therefore only easiest within an
incomplete candidate set, while the controller-demand coordinates largely measured solver
thrashing.

No further claim that route duration, transitions, reversals, or blind-continuation robustness
measure human difficulty is accepted until successful replays are simplified, fewest-decision
and explicit low-complexity bypass searches are added, and the revised outputs are calibrated
against human feedback. A separate WallJump-only/no-Dash authored challenge now provides a
controlled calibration target with lethal contact windows and exact tractability/no-known-bypass
evidence. It remains outside the corpus. See the
[`hard no-Dash human-calibration report`](../validation/hard-no-dash-human-calibration-2026-08-15.md).
The first human playtest accepted that fixture as a useful high-end anchor while finding it
slightly too hard for the player's current skill. A second fixture halved the accepted wall-jump
sequence (3 versus 6), widened contact/recovery surfaces, and removed the ceiling check that
requires releasing Jump early.

Human playtesting found the combined change far more than 50% easier and suitable as a good
tutorial, so the midpoint hypothesis is rejected and the fixture is relabelled accordingly. Both
fixtures remain outside the corpus. The gap between them is currently uncalibrated; future
interpolation must vary one principal burden at a time rather than assuming geometric reductions
combine linearly.

The next calibration round is now frozen as a twelve-room, hand-authored no-Dash gallery rather
than another generated batch. It retains those two anchors and separates broad-vs-narrow contacts,
short-vs-long climbs, recovery-vs-no-recovery, landing chains, and release-early low-jump timing.
Stable public IDs are deliberately not a difficulty order. Corpus difficulty tuning remains paused
until human feedback on this first round is recorded and compared with the structural facts and
existing metrics. See the
[`human calibration gallery plan and feedback sheet`](human-calibration-gallery-plan.md).

### Calibration-first generation resumed (2026-08-15)

Human feedback and subsequent movement tuning made the revised gallery a useful starting range:
not high-end difficulty, but broadly suitable for a reasonably challenging game. Generation has
therefore resumed only as a separate twelve-key playtest pilot. It does **not** reactivate the old
duration/controller-demand/shaky-hand difficulty ranking and does not alter corpus source policy.

The new `calibrated-wall-jump-v2` grammar is WallJump-only/no-Dash and varies five isolated shapes
derived from the played gallery: short staged turns, regular alternating rhythm, rhythm with one
recovery shelf, a climb plus rising landing chain, and the same traverse with a visible jump-cut
bank. Every playtest key must have a mechanically simplified clean replay, every jump press must be
accepted, retained wall contacts must stay in the calibrated 3–7 range without repeated-wall
thrashing, a complete finite baseline controller audit must find no positive, and the ordinary
bounded baseline solver must remain inconclusive. These are typed acceptance/refusal boundaries,
not a scalar human-difficulty model or proof of physical impossibility.

The initial run found and rejected two real design failures before freezing the batch: reflecting
the low-bridge finish retained a visibly thrashy route, and reflecting the recovery shelf admitted
a replay-certified baseline bypass which the finite direct-controller vocabulary missed. V1 keeps
those shapes directional and varies their shaft placement instead. Human inspection then found
that v1's causeway had accidentally placed downward spikes above upward spikes, pointing both
lethal faces into an inaccessible internal seam. V2 reverses the paired bank so its tips face the
traversable spaces, restores the five-contact climb which that dead hazard had masked, and reruns
all solver/simplifier and ability-removal gates. The twelve accepted rooms are
available through `downwards --calibrated [seed]` for another human-feedback round; no room becomes
corpus input merely by passing this pilot. See the
[`calibrated WallJump generator v2 report`](../validation/calibrated-wall-jump-generator-v2-2026-08-15.md).

## First room-centric pilots (2026-08-15)

The historical first terrain-only pipeline slice enumerated all four construction kits, three
strategies, and three intents, exact-deduplicated static geometry, and evaluated every ordered door
and pickup target under all four play loadouts. It emitted canonical JSON/JSONL with exact replay
witnesses and bounded-inconclusive rows. These runs measured reachability plumbing and scale only;
they predate easiest-controller fronts, shaky-hand curves, QD selection, and held-out audits. The
current source policy is the build-config-schema-4, Baseline-plus-certified-Dash policy documented
below; it does not enumerate WallJump or combined construction keys.

- P0, seed 0: 36/36 profiles constructed and collapsed to 21 exact static rooms. All 21 passed
  their canonical construction loadout and the complete kit. The full matrix contained 440 door
  rows (421 positive, 19 inconclusive) and 224 pickup rows (216 positive, 8 inconclusive).
- P1 calibration, seeds 0–3: 144/144 profiles constructed and collapsed to 84 exact static rooms,
  exactly 21 per seed with no cross-seed static duplicate in this sample. 81/84 passed both their
  canonical construction loadout and the complete kit. The matrix contained 2,296 door rows
  (2,197 positive) and 1,024 pickup rows (984 positive). Complete-kit and dash evaluations were
  positive in every cell; baseline was 498/574 door and 222/256 pickup positives; wall-jump was
  551/574 and 250/256. Non-successes remain inconclusive.
- The 60/144 profile aliases are explained by generator capability equivalence, not cross-seed
  repetition: cyclic geometry currently ignores the construction kit, and growth distinguishes
  dash from non-dash but not wall-jump. These aliases are retained as constructive provenance but
  count once toward the 500–1,000 distinct-geometry target. Future duplicate gates must report
  profile aliases separately from accidental cross-seed duplicates.
- The P1 evidence bundle occupied 5.3 MiB and stored 3,181 exact positive witnesses. Evaluation
  completed in roughly 80 seconds in the release build on the development machine. This is still
  only a lower-bound runtime measurement because easiest-controller and noise studies are absent.

Three canonical baseline growth rooms in P1 had construction-loadout inconclusive cells. Those are
priority audit cases: they may reveal solver vocabulary gaps or realized-geometry defects, and may
not be silently discarded or labelled unreachable.

### P2 scale baseline, seeds 0–35 (2026-08-15)

The first target-scale run is retained as a diagnostic baseline, not promoted as the finished
corpus. It established the following exact facts before deeper challenge-quality selection:

- 1,296/1,296 construction profiles produced 756 distinct simulation/static geometries. There
  were 540 explained profile aliases, no same-static/different-simulation variants, and no
  cross-seed duplicate in this block.
- All 756 rooms were stored in immutable per-seed evidence shards. Of these, 712 passed every
  door and pickup objective under both their construction loadout and the complete kit. The full
  matrix contains 19,794/20,720 positive door rows and 8,719/9,172 positive pickup rows, backed by
  28,513 exact replay witnesses. Every non-positive row remains explicitly bounded-inconclusive.
- The 712 eligible reusable room definitions contain 2,144 socket occurrences, and every socket
  has a compatible mate on a different eligible room. A stronger no-reuse multiplicity-balanced
  subset search was computationally inconclusive and is not claimed as a passed gate; it is also
  stronger than the reusable-definition mate-coverage requirement.
- Raw morphology is substantially more expressive than v5: 756 unique rooms split into 247
  two-door, 237 three-door, and 272 four-door layouts. Static pairwise distance has median
  0.36514, but nearest-neighbour median 0.08333 and p10 0.01042 show a meaningful near-duplicate
  tail that selection still must address. Versioned SVG contact sheets and suspicious-pair sheets
  are generated for human review.

The first easiest-controller audit over all 21 seed-0 eligible rooms found 89 complete-kit
directed routes for which the finite controller vocabulary had a positive: 32 were run-only, 38
were monotone-simple, and only 19 required another controller class. This validates the user's
playtest criticism. The main cause is now identified: the initial terrain-only stage replaced
hazard tiles with `Empty` while the generators retained a continuous solid boundary floor. That
often leaves elaborate upper route geometry above a trivial ground corridor. P2 is therefore a
useful negative/control corpus and scale benchmark, not the final 500–1,000-room deliverable.
Generator experiments must remove this bypass using terrain and route constraints before adding
new obstacles. A room may still deliberately contain an easy direction, but the route matrix must
record it honestly and selection may not advertise a harder alternate as the route's difficulty.

Preliminary P2 shards used an accidental Rust-`Debug` feature-stage spelling in room IDs. Stable
identity is now explicitly versioned as room-ID v2 with canonical kebab-case stage slugs; final
artifacts will be regenerated rather than treating the preliminary identity bytes as durable.
They also predate solver-policy v3/direct-probe-audit v2 and are therefore diagnostic historical
evidence only; strict verification must reject them as current deep-analysis inputs.

## Difficulty is a vector, not a band

Gentle/Standard/Technical may remain as UI shorthand or a later human-calibrated projection, but
fixed band quotas are not the corpus objective. Store the underlying measurements and preserve
incomparability when two routes are difficult in different ways.

For every directed door pair and tested loadout, record at least:

### Feasibility and ability demand

- positive witness or bounded-inconclusive result;
- minimum successful tested loadout;
- abilities used by the easiest known replay;
- abilities structurally unavoidable across all abstract paths;
- ability-event counts and gate order;
- wrong-door outcomes and alternate reachable doors.

### Controller and trajectory demand

- completion ticks and distance travelled;
- semantic action spans and meaningful transitions;
- ordinary jumps, wall jumps, dashes, drops, and direction changes;
- debounced horizontal reversals;
- meaningful vertical-input decisions;
- accepted dash-direction changes;
- control vocabulary and distinct successful verbs;
- horizontal/vertical span and travel;
- decision/junction count on the structural route;
- simplest successful controller persona.

### Precision, pressure, and recovery

- minimum static and active-hazard clearance along the replay;
- landing/support margin and narrowest observed input window;
- number and duration of exposed hazard crossings;
- death versus recovery after a perturbed action;
- time and distance to regain a stable support;
- checkpoint/retry cost once those systems exist;
- timed-obstacle phase sensitivity.

### Pairwise difficulty comparisons

Prefer defensible partial-order statements over a universal score. For example, route A is clearly
harder than route B when the easiest known A route requires a strict superset of abilities or
decisions, has no simpler-controller bypass, and is no more robust under every tested noise level.
If one route demands more precision while another demands more planning or traversal vocabulary,
keep them incomparable and retain both in the corpus.

Solver persona, policy version, budgets, loadout, noise model, and replay fingerprints are part of
the evidence identity.

## “Shaky hand” testing

Exact play establishes feasibility and exposes the maximum challenge the solver can execute. It
does not estimate how reliably a person can reproduce the route. Every promising route therefore
gets a deterministic noisy-replay study in addition to its exact witness.

Use seeded, recorded perturbation schedules so every result is reproducible. Add noise families
incrementally:

1. shift individual press/release boundaries by ±1, ±2, and ±4 ticks;
2. apply correlated early/late reaction lag across several consecutive actions;
3. occasionally hold an input one frame too long or release it one frame too early;
4. drop or repeat a single input frame;
5. perturb jump duration and dash-direction timing independently;
6. after controller/persona support exists, replan from the perturbed state instead of requiring
   blind continuation of the original replay.

Report curves and failure modes rather than one number:

- success probability at each noise strength;
- largest noise strength with at least 90%, 75%, and 50% success;
- deaths, wrong doors, recoveries, and timeouts;
- first divergent action/region;
- post-error recovery time;
- whether successful noisy traces converge back to the original route or form a distinct route.

Perfect-play measurements come first while the corpus remains too easy. Noise robustness becomes
a stronger selection constraint as exact-play difficulty rises.

Implementation status (2026-08-15): `downwards-ai` now records a deterministic shaky-hand study
with an exact control plus individual ±1/±2/±4-tick boundaries, correlated timing shifts,
one-frame hold/release edits, and dropped/repeated frames. Reports keep deaths orthogonal to
terminal outcome and distinguish the requested target, another door, another exit, and timeout.
Adaptive replanning is explicitly `Unsupported`; the current study measures blind continuation
and exact same-tick convergence only. `downwards-lab` combines this evidence with exact traversal,
controller, accepted ability events, hazard clearance, and operational solver cost in a versioned
route-difficulty vector. Its tolerance-aware comparison can report one route clearly harder,
equivalent within evidence, incomparable, or insufficiently measured; solver effort is excluded
from player-difficulty comparisons.

Exact canonical route measurements now also retain every authoritative landing through target
contact. For each landing they record footprint overlap, the contiguous supporting surface,
one-way versus solid material, and signed left/right edge margins (negative means overhang). The
deep-analysis artifact persists and independently validates these coordinates. This is geometric
precision evidence only: a small observed margin is not by itself a narrow input window, and a
route without a measured landing is not assigned zero precision demand.

The corpus layer now applies those studies to the easiest retained exact controller for every
directed door pair and exact successful loadout. It keeps missing, complete-finite-vocabulary
non-success, and budget-limited non-success as distinct evidence states. Route-derived seeds bind
every curve to room/source/target/loadout/replay identity. The default final policy uses 64 trials
per curve point (with exhaustive boundary cases sometimes producing more); a 16-trial pilot over
all eight cells of a two-door room took 53 ms in release mode. These curves still measure blind
continuation, not human-calibrated or replanning success.

## Route diversity and fun proxies

The most important current fun proxy is useful challenge; the second is diversity of routes
through a room. No AI metric is labelled proof that a room is fun.

Track route diversity at several levels:

- distinct structural paths and fork/rejoin choices;
- directed door-pair and loadout coverage;
- materially distinct spatial trajectories;
- semantic-action and accepted-event edit distance;
- different ability use or gate order;
- safe versus risky paths and their time/precision trade-off;
- recovery routes after mistakes;
- alternate successful controller personas;
- pickup detours that branch and rejoin rather than duplicate the main route.

Two witnesses are not considered diverse merely because their timing differs. Deduplicate by
coarse trajectory, action semantics, ability/event sequence, and structural path before counting
alternatives.

Implementation status (2026-08-15): the corpus route-choice pass now exactly replays every
retained direct-controller witness under its own loadout and safely joins a matching canonical
matrix positive without calling the latter easiest. It collapses action durations, traversal
sample counts, and event ticks before counting alternatives, then reports separate spatial,
semantic-action, accepted-event, and coarse gate/path-style classes and distance distributions.
Complete finite-vocabulary non-success, bounded-inconclusive non-success, and missing audits remain
different states. Operational replay/search work is stored outside all diversity coordinates.

The pickup-detour pass now evaluates every exact source-door × play-loadout × pickup cell. It
keeps target-directed pickup witnesses, opportunistic pickup collection on finite canonical door
witnesses, same-source spatial/action differences, authored shortest-spine branch placement, and
cross-loadout positive facts separate. A pickup found only by the target-directed solver is not
called mandatory, and an authored off-spine pickup is not called a gameplay detour without replay
evidence. Solver work is again recorded only as operational cost.

Additional proxies worth retaining for later human calibration include rhythm variation,
readability, route-choice legibility, retry speed, surprise without ambiguity, and the amount of
meaningful space traversed.

The first P1 witnesses reinforce the playtest criticism, but are not yet easiest-controller
audits: 638/3,181 first-found positive witnesses were a single constant-input span and 796/3,181
used at most five spans. Under the complete kit, no sampled room had every direction reduce to a
run-only controller, but several had half of their directed door routes do so. This is useful
directional variety only if the other directions are materially engaging; it may not be hidden by
choosing a harder alternate witness for display.

## Terrain and obstacle policy

The next experiments deliberately **reduce** obstacle complexity. The generator must demonstrate
that it can place and assess the existing vocabulary before gaining more pieces.

Use staged feature sets:

1. boundary doors, solids, one-way platforms, and pickups;
2. static hazards only after terrain-only route quality is convincing;
3. one carefully placed timed hazard after static-hazard placement is convincing;
4. moving platforms, switches, doors, or new traps only after the preceding stage has metrics and
   regressions showing useful placement.

Each feature-set version is a separate experiment stratum. A more feature-rich candidate does not
automatically outrank a simpler one.

For terrain and obstacles, record:

- components supporting structural routes, recovery nodes, pickups, and ability gates;
- components approached by certified trajectories;
- features intersecting a successful or failed traversal envelope;
- obstacle-triggered timing decisions, detours, deaths, and recoveries;
- isolated or uncorroborated components and tiles;
- geometry that changes no route, action, or robustness measurement when removed.

Prefer fewer uncorroborated components and tiles. Do not reward total terrain volume or treat a
permissive proximity/gate envelope as proof of usefulness. A later ablation pass should remove one
feature/component at a time and replay the solver corpus; geometry whose removal changes no
observed route, reachability result, difficulty metric, or diversity descriptor is a candidate for
simplification, not automatically proven useless.

Component-only ablation has a known blind spot: a decorative ledge or pillar joined to the solid
floor or wall belongs to the boundary component and is therefore skipped. A versioned support-face
audit inventories maximal exposed horizontal supports and vertical solid faces backed by interior
tiles, including boundary-connected extensions while never emitting immutable shell or
door-aperture cells. Intersecting faces are coalesced into disjoint ablation units. For each unit it
reconstructs the room while preserving the shell, spawn, doors, exits, pickups, hazards, and every
non-unit tile, then replays the exact canonical and retained direct controllers under their
recorded loadouts. Results distinguish unchanged success, changed success, death, wrong target,
exhaustion, and typed nonconstructible stages; operational cost is separate. An unchanged known
controller is positive redundancy evidence for that controller only, while a changed or failed
controller is not called proof of necessity or unreachability.

## Corpus construction

The target of 500–1,000 refers to distinct room geometries, not route rows. Each room contributes
its complete directed route/loadout matrix.

1. Generate deterministic candidates in bounded seed blocks across strategies, ability profiles,
   and staged feature sets.
2. Reject invalid rooms and exact static/collision duplicates cheaply.
3. Group exact physical aliases without erasing their native derivation keys, then compute
   variant-specific structural/loadout facts and veto false ability claims. A deterministic
   canonical variant may be chosen only by an explicit recorded policy after feasibility, not by
   generation insertion order.
4. Positively certify door and pickup objectives with exact replays.
5. Run easiest-known controller/persona audits, then noisy-replay evaluation on viable candidates.
6. Compute morphology, topology, terrain utility, route behavior, and obstacle interaction
   descriptors.
7. Archive candidates across multi-dimensional quality-diversity cells.
8. Select deterministic farthest-separated representatives while satisfying socket-mate coverage
   and retaining a broad range of difficulty vectors and directional asymmetries.
9. Re-run held-out audits not used by selection to detect metric gaming.
10. Emit versioned manifests/datasets and reproduce them byte-for-byte.

Evaluation is computationally staged without changing evidence semantics. Construction,
descriptor deduplication, socket inventory, and construction/complete-kit target matrices run on
the full overgenerated pool. Direct-controller and cheap structural measurements then form a broad
archive shortlist. Expensive support-face ablations, complete route-choice fronts, landing and
pickup analyses, and shaky-hand curves run on every eventual corpus member plus a deterministic
held-out set before final selection is accepted. Candidates remain identifiable across stages, and
a missing expensive measurement is an explicit evidence state rather than a favorable score.

Do not force equal difficulty-band quotas. Do require broad coverage of observed difficulty
coordinates, ability/loadout cases, boundary-door directions, graph topologies, route asymmetry,
and behavior niches. If a region of the desired corpus cannot be filled, report the deficit and
improve generation or evaluation; never weaken a gate silently.

The final multi-generator artifact uses a new tagged candidate-key schema rather than extending
the historical staged-v6 record. A physical room ID is derived only from the exact static-layout
and simulation-geometry descriptors, so it is independent of which derivation was encountered
first. Every alias retains its complete native regeneration key, route graph, construction
loadout, and provenance. Feasibility is evaluated per alias where its claimed structure or
construction loadout matters; the shared physical rollout matrix may be computed once. Any
canonical presentation alias is a separate, versioned post-feasibility decision and is never the
source of the physical identity.

Implementation status: the generator-neutral key/candidate layer and first in-memory v2 batch are
now present alongside the untouched historical pipeline. Exact attempt-zero records retain typed
construction failures, group aliases by full descriptor equality, evaluate all four loadouts once
per physical room, and project construction-loadout feasibility back onto each native derivation.
The optional canonical regeneration key is selected by a recorded post-feasibility policy and is
never reused as structural truth. The evaluator now materializes one exact, versioned solver and
difficulty configuration per loadout before evaluating any room; the stable configuration ID
binds every numeric solver field, macro name/action, solver-policy version, witness-fingerprint
version, and difficulty field. This prevents a stateful configuration callback from silently
changing search bounds between rooms.

The additive generator-neutral artifact-v3/checkpoint path is implemented without modifying the
historical staged-v6 formats. Each immutable single-seed shard stores the exact build and four
evaluation configs, every construction success/failure, full room-v3 descriptor pair, every
tagged native key, route graph, boundary-port association and generator-specific derivation/
embedding provenance, all route and pickup matrix cells, gate/canonical decisions, and normalized
nonzero-RLE positive replays. Schema v3 also retains the exact per-source and aggregate shared
search effort for every matrix. Its run manifest binds the full content-addressed ability-promotion
audit configuration, and every room must repeat that exact record rather than choosing its own
budget or controller vocabulary. Every native alias has an ordered promotion record: ordinary
sources are explicitly not applicable, while ability sources retain structural versions, exact
advertised doors, intended-proof provenance, reverse/missing-ability matrix states, and the complete
four-loadout finite direct-controller assessment with normalized positive replays, semantic traces,
probe provenance, statistics, completeness, fronts, and bypasses. Artifact summaries count ordinary,
promoted, and unpromoted ability aliases separately; an ordinary not-applicable gate is never called
a passed ability promotion.

The strict verifier denies unknown fields and any unreachability variant, exact-regenerates successes
and failures, reconstructs descriptor alias groups, room IDs, matrices, gates and canonical policy,
then authoritatively replays every matrix positive from the exact source door under the recorded
loadout. Recorded search observations are checked against their exact loadout configuration for
node/tick/path bounds, cumulative snapshot consistency, and compatible bounded terminal reasons.
Those operational values are checksum-bound observations; they are not claimed to be independently
recomputed without rerunning ordinary solver search. Promotion is deliberately stronger: after
replay-only matrix rehydration, verification reconstructs each stored assessment, reruns the small
finite advertised-pair audit under all four loadouts on that exact regenerated native alias, and
requires complete gate equality before recomputing canonical selection and summaries through the
shared full-room validator. `CompleteFiniteVocabularyNoPositive` therefore remains a scoped
no-known-bypass observation and is never trusted from serialized data alone. Stream and summary
hashes are recomputed. Verified shards can be replay-only rehydrated into the exact rich
`EvaluatedCorpusBatchV2` values used by deep metrics and selection; the multi-shard loader rejects
duplicate seeds/room IDs or build, route-evaluation, or promotion-audit policy mismatches and returns
deterministic room order. The build-config content ID now uses the validated build-config schema in
its domain and prefix, so schema-3 records cannot retain the obsolete schema-2 identity label. The
resumable `corpus build-v3-shards` runner skips only a fully verified config-matching checkpoint, fails
closed on partial/stale/corrupt directories, and publishes the create-new checkpoint only after
byte-exact disk read-back and independent verification; `corpus verify-v3-shards` performs the
same audit without generation. Route-plan waypoint-rescue evidence is explicitly omitted from
artifact schema v3 while its producer API is unstable, leaving it available as a later additive
stream rather than conflating it with the canonical matrices. Focused artifact tests cover
deterministic deep round trips, exact batch rehydration, corruption, authoritative positive replay,
fail-closed incomplete shards and non-overwriting checkpoint publication. Serialization itself never
promotes a room: the artifact preserves promoted, refused, and bounded gate outcomes exactly, and
production trust comes only from the regenerated replay and finite-audit rerun.

`corpus_v2_pilot` now runs that path with deterministic JSON output and an explicitly bounded,
opt-in deep-analysis limit. The historical schema-3 seed-0 release smoke attempted 18 native keys,
constructed 16, and grouped them into 14 exact physical rooms; 11 received a canonical eligible
alias. Baseline recorded 112/126 positive door rows and 37/48 pickup rows, while the complete kit
recorded 126/126 and 48/48. Two Dash aliases promoted; both WallJump aliases were refused for a
Dash-only matrix bypass. Only 32/48 socket occurrences had a mate on another physical room inside
this deliberately tiny one-seed pool, so socket closure remains a corpus-scale selection gate
rather than a fact inferred from inventory declarations. The in-memory release smoke took roughly
25 seconds. Rendering, writing, strict verification, native promotion-audit reruns, and exact
checkpoint publication took roughly 43 seconds; a second invocation skipped the already-verified
shard in under two seconds. This is calibration evidence for the superseded baseline+WallJump+Dash
source policy, not a final corpus run or a yield forecast for build-config schema 4.

The first artifact-v3 breadth calibration after single-ability source integration covered seeds
0--3. It attempted 72 native keys, constructed 59, grouped them into 47 exact physical rooms, and
selected canonical aliases for 33. Across all four loadouts, 1,559/1,632 directed-door rows and
590/616 pickup rows were replay-positive; 46/47 physical rooms passed the complete-kit all-target
gate. Eleven constructed aliases made an ability claim: four promoted and seven were refused or
bounded. All four promotions were Dash. Four WallJump aliases had a replay-positive Dash-only
advertised-pair bypass and two had bounded intended routes, so no WallJump alias promoted; one Dash
alias was independently refused for a WallJump-only bypass. This is retained as a generator
deficit. The 33 eligible rooms exposed 112 socket occurrences, 102 of which already
had a mate on another eligible room; the remaining sparse signatures are a multi-seed coverage
question, not silently discarded inventory. The four-seed yield projects to roughly 528 eligible
rooms at 64 seeds before deep-metric filtering, so the provisional large-run ceiling is 80--96
seeds under that historical policy. Because WallJump has now been removed from source enumeration,
these counts are retained for diagnosis but must be recalibrated before a large run.

The build-config-schema-4/source-policy-3 recalibration covers the same seeds 0--3 without
WallJump construction keys. It attempted 60 keys, constructed 53, and grouped them into 41 exact physical rooms; 33
received canonical eligible aliases. Across the four exact loadouts, 1,386/1,432 directed-door
rows and 519/536 pickup rows were replay-positive, and all 41 rooms passed the complete-kit
all-target gate. Five Dash aliases constructed: four promoted and one was correctly refused for a
WallJump-only matrix bypass. Exact physical/static/simulation/collision diversity was 41/41.
Across the physical pool, 130/139 socket occurrences already had a different-room mate. That
four-seed sample yielded 8.25 rooms/seed and projected roughly 528 rooms at 64 seeds before
deep-evidence and archive filtering. The artifact-v3 seed-0 immutable shard built under
build-config schema 4/source policy 3 contains 12 physical rooms, 11 eligible aliases, 396/408
positive directed-door rows, and 149/160 positive pickup rows; independent verification completed
successfully. These are calibration measurements, not final corpus counts.

### Build-config schema 4 / source-policy 3 persistent tranche, seeds 0--15

The first persistent tranche is checkpointed under `content/corpora/v1/shards` with config identity
`downwards-corpus-build-config-v4-ad2497a5ad4ef050`. Its 16 independently verified artifact-v3
shards attempted 240 exact keys, constructed 215, rejected 25 with typed finite causes, and grouped
the successes into 175 physical rooms plus 40 same-geometry aliases. All 175 static descriptors and
all 175 simulation descriptors are distinct across the full tranche. The four-loadout evidence has
4,813/5,048 replay-positive directed-door rows and 1,836/1,908 replay-positive pickup rows; the
remaining 235 door rows and 72 pickup rows remain explicitly inconclusive. The complete kit passes
all targets in 169/175 physical rooms. Of 25 constructed Dash aliases, 22 promote and three remain
refused or unresolved; canonical regeneration selects 145 rooms, split across 97 PartitionRoute,
28 CompositionalRouteCut, and 20 CompositionalAbility keys. The verified shards occupy about 17 MiB
and contain 6,649 positive witnesses.

This measured canonical yield is 9.06 rooms/seed: 56 seeds would cross 500 only before deep-evidence,
socket, visual-duplicate, and archive filtering. The separate source-bound cache at
`content/corpora/v1/calibration/cache-16` recomputed all 145 canonical rooms and accepted 141 under
the complete production evidence contract. All 141 were exact-visual unique. With the production
`elites_per_cell = 8` setting and a deliberately nonbinding minimum of one, the archive retained
140 candidates; the reusable cross-room socket fixed point excluded 12 inputs, and selection kept
128 rooms as one socket-valid package covering 222 archive cells. The 42 MiB cache, provisional
selection, and 8.6 MiB final selection are all checkpointed. `finalize` fully recomputed the 128
selected rooms and `verify-final` independently repeated that computation with exact equality.

The measured final-path yield is therefore 8.00 selected rooms/seed. Although 64 seeds projects to
512 rooms, that margin is too close to the hard 500-room floor. The calibration therefore fixed the
production source range at seeds 0--79, projecting about 640 rooms before the maximum-1,000 cap,
while seeds 80--95 remain untouched for held-out generation and visual/client audits. Existing
seeds 0--15 were designated immutable and subsequently verified and skipped, as recorded below.

### Production overgeneration tranche, seeds 0--79

The predetermined production range is now fully materialized in 80 immutable artifact-v3 shards
under `content/corpora/v1/shards`. The resumable build independently verified and skipped the first
16 shards, wrote the remaining 64, and a separate whole-range verifier then accepted every shard.
The final build-config-schema-4/source-policy-3 run attempted 1,200 exact keys, constructed 1,091,
and retained 892 physical rooms plus 199 same-geometry aliases. All 892 static descriptors and all
892 simulation descriptors are globally distinct. The shard set occupies about 83 MiB.

The exact four-loadout evidence contains 24,127/25,584 replay-positive directed-door rows and
8,880/9,228 replay-positive pickup rows, with 33,007 positive witnesses; all other cells remain
bounded/inconclusive. The complete kit passes all targets in 853/892 physical rooms. Of 145
constructed Dash aliases, 123 promote, 13 are vetoed by a replay-positive missing-ability matrix,
and nine remain refused because the baseline reverse route is bounded. Canonical regeneration
selects 719 rooms: 470 PartitionRoute, 132 CompositionalRouteCut, and 117 CompositionalAbility.

The 109 typed construction refusals are retained rather than retried under weaker contracts: 85
ability constraint-search exhaustions, four ability gate-contract failures, three ability rewrite
exhaustions, three ability door-invariant failures, 13 partition embedding exhaustions, and one
route-cut support exhaustion. The source-bound deep cache at `content/corpora/v1/cache` is now
complete: all 719 canonical rooms were recomputed, 703 satisfied the full production evidence
contract, and the cache occupies about 207 MiB. All 703 eligible static visuals remain distinct.

The production selector used the predetermined `500..=1000` range and `elites_per_cell = 8`.
Its archive retained 596 rooms covering all 466 retained archive cells. Those rooms form one
reusable socket package; no room was removed by the initial socket core or subsequent socket
pruning. The provisional artifact is stored at `content/corpora/v1/selection-provisional`.
Mandatory finalization then independently rebound every source shard, reran selection, fully
recomputed all 596 selected rooms (including shaky-hand evidence), required exact equality with
the cache rows, and published `content/corpora/v1/selection-final` checkpoint-last. Independent
`verify-final` then independently rebound the immutable shards and cache, recomputed all 596
selected rows again, and accepted exact equality in 782.86 seconds wall / 1421.60 seconds CPU.
The production selection is therefore fully reverified. Seeds 80--95 remain outside every
production artifact for held-out audits.

The game-facing export at `content/corpora/v1/playtest.manifest` was then built only after another
full final-selection verification. Its requested first 64 quality-diversity rooms require four
additional different-room socket mates, producing 68 replay-verified entries: 32 PartitionRoute,
15 CompositionalRouteCut, and 21 certified Dash-ability candidates; 47 use the Baseline
construction loadout and 21 use Dash. Every entry retained its authored source-to-sink positive,
so no deterministic fallback route was needed. The 67,909-byte canonical manifest has SHA-256
`a32163af520fc77d512f875bc6d5463ddbe340b84d9cb8cd42f7af44038bace5`; strict export and readback
took 929.32 seconds including the repeated corpus trust gate.

### Performance profiling and semantics-preserving optimization checkpoint

The production run exposed iteration cost as a research constraint in its own right. In the first
80-seed deep-cache run, source verification and replay rehydration took roughly fourteen minutes
before the first room checkpoint was published; after that startup, the first several hundred
room rows averaged about 2.6 seconds each. A source audit explains the fixed cost: the normal
build, cache, select, finalize, and final-verification chain can invoke the full semantic shard
verifier 29 times per shard. Because one invocation contains two all-positive replay sweeps and
the rehydration stages add four more sweeps, this is about 62 positive-witness replay sweeps per
shard before an explicit `verify-v3-shards` command. These repetitions are trust-boundary
implementation costs, not evidence that should influence level difficulty or selection.

Deep room analysis has two additional, distinct hot paths. The default shaky-hand analysis runs
one exact replay plus eight curves of 64 noisy trials for every applicable fused route cell: at
least 513 full simulations per cell before any boundary extension. Route fusion currently repeats
certification, observation, landing, and difficulty simulations before discovering exact
direct/canonical replay aliases, and route-choice then observes many of those retained witnesses
again. Geometry construction is therefore not the main performance problem; solver expansion,
semantic verification, replay measurement, and robustness trials dominate.

Optimization is permitted only when output identity and evidence semantics remain unchanged. The
first two implementation targets are: (1) one process-local, strictly verified parsed/rehydrated
shard value so checkpoint and byte-hash checks do not recursively rerun semantic verification,
while retaining two independent full passes for a newly written shard and an independent final
verification boundary; and (2) collision-safe exact replay normalization and provenance merging
before expensive fusion measurement, with one measurement per unique replay. Each change must
retain corruption-closed tests, deterministic ordering, exact positive replay, promotion-audit
reruns where they are actually authoritative, and byte/equality comparison of resulting evidence.

After those changes are measured, the next candidates are deterministic bounded parallelism at
seed or room boundaries, a validated immutable room context shared by adjacent deep phases, reuse
of typed fusion observations in route-choice, single-pass cache-row loading, and incremental QD
distance/socket indexes. Parallel work must collect results in canonical order, report the
seed/room-earliest error, and avoid nested oversubscription; initial breadth is two workers because
replay and search state are memory-heavy. Reduced solver budgets, fewer shaky trials, omitted
audits, reordered finite-controller vocabularies, and relaxed construction gates are explicitly
not performance optimizations. Before/after release measurements use fixed content-addressed rooms
and shard ranges, report wall time plus CPU/RSS where available, and require identical output
hashes or exact typed equality.

The first bounded optimization pass is now measured. A process-local rich shard verifier reduced
the normal new build/cache/select/finalize/final-verify chain from 29 to six full semantic passes
per shard while retaining independent publication and final-verification boundaries. On the same
four production shards, standalone verification improved from 1.81 to 1.32 seconds wall time and
from 1.78 to 0.93 seconds CPU. Exact replay normalization before repeated fusion measurement
improved the fixed heavy-room fusion pass from 1.699 to 1.626 seconds and full room analysis from
2.264 to 2.210 seconds; the modest gain confirms that temporal perturbation dominates after
deduplication. The exact one-seed end-to-end cache improved from 30.05 to 28.39 seconds, with
recursively identical files and the same aggregate SHA-256.

The larger practical win is deterministic room-level parallelism. Exactly two static-stride
workers join before the canonical earliest error is reported; room checkpoints remain disjoint,
and run/final checkpoints remain main-thread and last. On production seed 45, the one-worker cache
took 19.65 seconds and the two-worker cache took 10.95 seconds, a 44.3% wall-time reduction with
the same recursive bytes and hash. The 596-room final recomputation completed in 1008.85 seconds
wall / 1621.60 seconds CPU. Worker count is operational and absent from evidence identity.
Production selection itself took 268.76 seconds, confirming the predicted cubic farthest-distance
rescan. An exact triangular pairwise-distance cache with incrementally maintained package minima
reduces 703-room vector-distance evaluations from 57,904,704 to 246,753 using about 3.95 MiB; it
is guarded against the retained reference implementation by adversarial tie cases and 48
deterministic differential fixtures. Its comparator, coverage, ordering, audit, and schema are
unchanged. The optimized production rerun took 104.46 seconds, a 61.1% wall-time reduction, and
its complete selection directory was byte-for-byte identical to the accepted provisional
artifact (aggregate recursive SHA-256
`966702b4c24a1274dab8e5e65dfe02a43de937503056d50ee11f8a6817b830e8`).

The final-path build-config schema is now version 4 and source-capability policy is version 3. Each
seed deterministically attempts 15
native keys: nine baseline PartitionRoute keys, three baseline CompositionalRouteCut keys, and
three certified Dash edge-rewrite keys (one per intent). WallJump and combined WallJump+Dash are
absent rather than silently weakened. Enumeration
is only source admission: it does **not** make an ability alias feasible or canonically selectable.
The baseline mappings still author no wall-jump or dash edge, and their ability bits must never be
used as random salts and then presented as capability labels. A
single-ability alias is eligible only after its versioned per-alias promotion audit establishes an
honest structural gate, witnessed accepted ability use on its easiest-known intended-loadout
direct controller (falling back to the canonical positive only when the complete finite direct
vocabulary has no positive), a baseline-positive reverse route, and no known bypass under either
exact loadout missing the claimed ability. Thus WallJump checks both Baseline and Dash-only, while
Dash checks both Baseline and WallJump-only. Any positive bypass vetoes the claim; a bounded
intended or missing-ability audit is unresolved and cannot promote. The strict Both/loadout result
is retained as separate descriptive evidence, because the difficulty unit remains directed
route × exact loadout rather than a room-global label.

The capability slice is an edge rewrite, not a new whole-room family: a selected mission edge is
replaced by a reserved wall shaft or dash transfer during constrained embedding. Directed
unavoidability is evaluated separately in each direction, so a branch may be easy to descend before
acquiring an ability and require that ability to return.

Historical and diagnostic capability machinery includes the version-1 coordinate-free edge
rewrite with explicit WallJump, Dash, and combined profiles over baseline-derived compositional
missions. It selects only
finite-graph bridges, retains exhaustive edge-deletion and missing-ability reachable sets, keeps
reverse traversal baseline, gives combined gates a seed-varied serial order, and records exact
signatures and typed finite failures. The separate physical mapping v2 now reserves paired-wall
shafts and dash-rise transfers inside coupled row/support search, including a whole-band isolation
reservation which forbids undeclared support-to-gate-upper skips. Exact Standard attempt-zero
WallJump seed 0 and Dash seed 0 pass all intended directed-door and door-to-pickup replay gates,
contain the declared accepted ability event, return in reverse under baseline, and have complete
finite direct-controller audits with no positive under both missing-ability loadouts. In
particular, WallJump seed 0 is complete-no-positive under Baseline and Dash-only, while Dash seed 0
is complete-no-positive under Baseline and WallJump-only. Those `CompleteNoPositive` results are
finite-vocabulary evidence, not proofs of impossibility. The integrated full-matrix gate therefore
correctly distinguishes the two: Dash seed 0 is promoted as structural/no-known-bypass, whereas
WallJump seed 0 is refused because its Dash-only matrix contains a replay-certified advertised-pair
positive that the smaller direct vocabulary did not find. This is a generator deficit, not a reason
to weaken the positive-bypass veto. The exact combined seed-1 request
exhausts its 242 finite rhythm assignments under the isolation contract and remains unpromoted.
The integrated gameplay gate is now complete and fail-closed in evaluation, artifact verification,
rehydration, and production selection. It reruns the finite advertised-pair audit rather than
trusting a serialized pass flag. Dash remains the sole enumerated ability source. Two WallJump
replacement experiments were rejected rather than promoted: physical v3 raised the shaft to eight
rows and constructed 12/15 attempt-zero keys, but Dash-only matrix witnesses used the landable
wall-column cap to refresh Dash and cross the upper lip; cap-free ceiling-chimney v4 removed that
bypass shape but fit only 5/15 keys under the unchanged isolation, cut, port, door-arrival, and
finite-search contracts. No budgets, bridges, or solver semantics were weakened. Graph results are recorded in
[`../validation/compositional-ability-edge-rewrite-v1-2026-08-15.md`](../validation/compositional-ability-edge-rewrite-v1-2026-08-15.md),
physical construction/replay evidence in
[`../validation/compositional-ability-physical-v2-2026-08-15.md`](../validation/compositional-ability-physical-v2-2026-08-15.md),
the replay-vetoed v3 experiment in
[`../validation/compositional-wall-gate-v3-rejected-2026-08-15.md`](../validation/compositional-wall-gate-v3-rejected-2026-08-15.md),
and the construction-yield-vetoed v4 experiment in
[`../validation/compositional-wall-chimney-v4-rejected-2026-08-15.md`](../validation/compositional-wall-chimney-v4-rejected-2026-08-15.md).

Deep analysis now fuses every retained direct-controller positive with the canonical matrix
positive for the same directed route and exact loadout. All candidates are authoritatively
replayed, exact aliases are deduplicated, and the same solver-neutral route-difficulty vector is
used for every candidate. Controller demand is compared as a partial order; equal-demand
candidates use a perfect-control-only Pareto comparison which deliberately excludes shaky-hand
coordinates and operational solver effort. Incomparable fronts remain explicit and their
deterministic representative is labelled ambiguous rather than reported as a numeric easiest
route. A real 24-cell room retained 334 exact candidates and eight ambiguous fronts; fusion took
1.70 seconds and full room analysis 2.26 seconds in release, projecting roughly 19 minutes for 500
rooms on that single-fixture linear estimate. This is a runtime calibration, not a breadth claim.

The generator-neutral offline selection path is resumable without treating a cache as proof.
`corpus_v3_offline_selection cache` binds each immutable row to exact artifact-v3 checkpoints,
room/key identity, analysis and shaky-hand configurations, current policy versions, the complete
production descriptor, and typed route-choice/pickup/shaky reports. Provisional selection accepts
only opaque source-bound descriptor wrappers. `finalize` and `verify-final` rehydrate the original
evidence and recompute every selected room's analysis, shaky-hand study, reports, descriptor,
identity and ability gate before requiring exact equality. Cache schema/checkpoint version 1 is
therefore operational resumability, never independent evidence. The first real cache smoke found
and failed closed on a numeric histogram-map wire mismatch before any room checkpoint or run
completion was published. A second fresh read-back exposed raw floating-point canonicalization
drift. Histograms now use sorted `{value,count}` rows and normalized floats bind canonical decimal
text plus exact IEEE-754 bits, with corruption regressions for both. The final fresh seed-0 cache
completed 11/11 eligible room rows plus its run checkpoint in 29.4 seconds and 3.5 MiB. A
provisional socket/QD selection retained two rooms from this deliberately tiny pool; `finalize`
recomputed both, and `verify-final` independently repeated the full verification successfully.
Tiny-pool selection size is not a production yield estimate. The source-bound 16-seed calibration
reported above supersedes it: 141/145 deep rows were production-eligible, 128 survived archive and
socket selection, and full finalization plus independent `verify-final` passed at breadth.

## Quality-diversity coordinates

Candidate archive dimensions should include a deliberately redundant pool of descriptors so the
research can later determine which ones matter:

- port count and side combination;
- cycle rank, branch count, and route-choice count;
- vertical span and directional asymmetry;
- minimum successful ability set and unavoidable gate order;
- easiest-controller class;
- control vocabulary, reversals, and timing decisions;
- exact-play precision/clearance;
- noisy-play success thresholds;
- recovery likelihood;
- route-trajectory and action diversity;
- terrain/obstacle corroboration;
- morphology and collision-topology clusters;
- hazard exposure and timed-mechanism sensitivity;
- pickup detour structure.

Selection must not collapse these dimensions into one weighted “fun” score. Use Pareto dominance,
MAP-Elites-style cells, and deterministic farthest-point sampling. Preserve the raw vector for
later analysis.

The selector now uses nine independent projected archives rather than one sparse Cartesian
mega-cell: morphology/topology, directed-loadout controller demand, directional asymmetry,
canonical observed route diversity, ability bypass structure, terrain/ablation utility, landing
geometry, within-cell observed route choices, and pickup challenge/detour structure. The latter
two consume the replay-certified passes described above; legacy callers receive explicit Missing
cells, while the extended adapter requires identity-matched reports. Pickup coordinates are
fractions and per-cell witness/detour distributions, never a reward for merely adding pickups.
Spatial and action detour minima are taken independently against the best-aligned retained door
witness on each axis, without a hidden scalar trade-off. Pareto axes remain separate; operational
solver cost is an optimization cost, not difficulty. Missing, not-applicable, and
bounded-inconclusive values have distinct encodings. Exact static-visual uniqueness and
cross-room reusable-socket mate coverage are hard gates, while construction-loadout and
complete-kit all-target certification are explicit caller preconditions.

## Artifacts and reproducibility

The research loop should leave durable evidence rather than only console output:

- exact generator, simulation, solver, evaluator, and noise-policy versions;
- candidate regeneration keys and rejection classes;
- positive route/pickup replay witnesses;
- per-route/loadout metric vectors and uncertainty status;
- room-level route-asymmetry and diversity summaries;
- quality-diversity cell assignments and selection rationale;
- static/collision/route/behavior fingerprints;
- deterministic corpus manifests;
- aggregate reports and held-out audit results;
- human session measurements once available.

Large raw experimental data may live as reproducible generated artifacts rather than being checked
into source control. The selected corpus manifest, configurations, summary reports, and enough
witness data to validate claims belong in the repository.

Implementation status (2026-08-15): a per-seed deep-analysis artifact now stores a canonical
manifest, room rows, and normalized controller-witness rows. It binds generator, room-ID, solver,
and analysis policies; separates operational search cost from player-demand coordinates;
regenerates every room; and exactly replays every retained controller on load. A seed-0 smoke
shard contained 21 rooms, 1,260 retained controller witnesses, and 421 canonical directed/loadout
route vectors. Render plus independent verification completed in roughly 35 seconds and occupied
3.5 MiB. `corpus deep-shard` writes new evidence without overwrite and
`corpus verify-deep-shard` performs the independent regeneration/replay pass.

Artifact schema v2 also stores every canonical route's measured landing supports and the
room/loadout landing aggregates. Independent verification recomputes those aggregates from the
route rows, checks signed edge margins and support geometry, and rejects corruption rather than
trusting the summary.

An escalated bounded-inconclusive audit also exists, but is intentionally a targeted diagnostic,
not a routine full-corpus gate. It retries only original inconclusive cells through three recorded
solver configurations and checkpoints every room. A stubborn seed-9 room retained 14/14 bounded
results after roughly 80 seconds and rescued none; this remains an uncertainty result, not an
impossibility claim. The known-bad P2 terrain baseline will not receive an hour-scale exhaustive
retry. The audit will be used selectively on gate failures from the improved terrain generator.

## First autonomous work sequence

1. Fix lower-loadout bypass retention and all-path ability-unavoidability.
2. Replace band-quota selection with route vectors, partial-order comparisons, and
   quality-diversity coverage.
3. Implement reproducible shaky-hand/noisy-replay evaluation and tests.
4. Add terrain/obstacle ablation and positive-utility measurements.
5. Establish a terrain-first generator profile and evaluate current primitives before adding any
   new obstacle.
6. Run small deterministic pilot batches, inspect metric distributions and pathological examples,
   and revise metrics when they reward obviously trivial or decorative rooms.
7. Scale overgeneration progressively toward 500–1,000 selected distinct rooms.
8. Produce a final corpus report that separates proven facts, positive evidence, bounded
   inconclusive results, heuristic comparisons, and open human-playtest questions.

### Terrain-only route-cut checkpoint (2026-08-15)

The continuous-floor bypass diagnosis led to two frozen mechanic experiments, documented in
[`terrain-route-cut-experiments-2026-08-15.md`](terrain-route-cut-experiments-2026-08-15.md).
A grounded one-tile pier was too weak and sometimes increased uncorroborated terrain. A stronger
shelf-return collision cut produced complete-kit positive evidence for all 80 ordered door routes
and all 36 pickup routes in a four-seed sample; all 441 interior terrain tiles were attributed and
positively corroborated, and every bottom-to-ceiling witness contained a real reversal. It also
showed directional demand differences for all 40 unordered door pairs.

That result validates a reusable route-cut rewrite, not a generator: only 9 static geometries and
9 route signatures appeared among 12 rooms. The fixed shelf layout is therefore frozen and
explicitly barred from production. The next generator experiment must compose a mission graph
from variable cut counts, orientations, openings, route order, and fork/rejoin structure with
bounded constraint embedding and no whole-room fallback. It must meet both all-target reachability
gates, preserve terrain attribution, and demonstrate high raw route/static expressivity before it
can feed the 500–1,000-room selection pipeline.

The same experiment exposed a solver-composition defect rather than an impossible baseline
route. A wider diagnostic search found an exact 363-tick no-ability witness; the new generic
detour/homing direct probe finds and replays a 232-tick witness with one reversal and 18 ordinary
jumps. Solver policy is therefore version 3 and direct-probe audit policy is version 2. Historical
v6 catalogue fixtures remain explicitly labelled solver-policy-v2 artifacts: the runtime accepts
that supported historical identity and still regenerates and exactly replays every stored
representative, rather than relabelling their checksum-bound offline matrix evidence as v3.

The target-aware homing probe also exposed a batching correctness edge: a controller parameterized
for door B could be executed by a shared source batch and accidentally credited as finite-vocabulary
evidence for door A, even though A's individual audit would never have generated that controller.
The audit now shares identical executions while retaining an explicit target-membership set for
every probe. A target receives evidence only from its own controller vocabulary; batched and
individual behavioral evidence are regression-tested for equality. Ordinary multi-target solving
remains free to use any successful rollout, because that API is reachability search rather than a
claim about a target-specific finite controller class.

The first compositional route-cut embedding exposed the complementary generator-side contract bug:
`edge_verb` could label a transfer as a baseline jump from its vertical delta while ignoring a
nine-tile horizontal gap. A full-kit witness then crossed the nominally baseline route with five
dashes. Every final-path baseline edge now requires an explicit conservative horizontal and
vertical movement predicate, and the rasterized landing must still match the abstract support.
Constraint exhaustion is a typed generation failure; neither solver budgets nor unowned bridge
terrain are used to hide it. The directed contract also records a short descent between
overlapping one-way supports as an explicit drop-through, not a run; the corresponding descent
from a solid support is rejected instead of being called reversible. Directional challenge
metrics therefore retain that control decision when an ordinary edge is traversed backward.

Compositional-route-cut v2 now enforces that contract by bounded spine/fork constraint search and
exact raster/material checks. Its fixed Standard attempt-zero block constructs 253/256 keys, with
253 distinct static and route signatures and 252 coordinate-free topologies; all emitted sockets
have a different-room mate in the block. Exact failures remain typed search exhaustion. In the
12-room authoritative pilot, the complete kit certified all 112 door and 42 pickup rows, while
baseline certified 101/112 doors and all 42 pickups. The eleven bounded misses all target the
ceiling port. Two representative misses have exact 365/369-tick baseline witnesses, and composing
through the authored pickup shortens them to 316/285 ticks. Thus the geometry is retained and the
source remains pending a generic route-plan waypoint certifier; default beam loss is not treated as
level impossibility or repaired by making the route easier.

The room-level challenge sanity audit now preserves every expected route-by-loadout cell and
partitions it into exact positive, complete-finite-vocabulary no-positive, bounded-without-positive,
or missing evidence. Positive cells separately report run-only, monotone-simple-only, and other
known controllers, directed asymmetry, demand distributions, and construction/complete-kit gate
state. Optional exact-rational thresholds are pilot diagnostics only: they are serialized with
their deficits and are explicitly not production rejection gates, difficulty bands, reachability
proofs, or fun scores. `corpus deep-pilot` now prints this aggregate after its per-room JSON rows,
so every generator experiment gets the same denominators instead of relying on hand-selected
examples.

A second independent terrain-first source is now under evaluation; exact pilot evidence is kept in
[`../validation/graph-first-generator-pilots-2026-08-15.md`](../validation/graph-first-generator-pilots-2026-08-15.md).
Its recursive partition grammar
derives two to four traversal chambers, collision cuts, optional fork/rejoin branches, ports, and a
pickup before rasterization. In the first construction-only audit, 128 baseline keys produced 127
distinct static tile maps, 42 pure graph topologies, 128 coordinate-free route/rhythm derivations,
and 128 full normalized derivations; a broader 576-key invariant sweep constructed every room.
Those layers are reported separately so embedding jitter cannot masquerade as topology. Version 1
intentionally uses only baseline movement links even when an ability-bearing construction profile
selects the random stream; it therefore makes no wall-jump or dash requirement claim. These are
construction and expressive-range facts only until the authoritative all-door/all-pickup and
easiest-controller audits complete.

The first authoritative partition sample covered seeds 0--3, all three intents, the mixed-BSP
profile, and baseline construction. All 12 rooms passed both the construction-loadout and complete
kit all-target gates: 80/80 directed door routes and 31/31 pickup routes were positive under each
loadout, with no bounded-inconclusive cell. Four floor-port arrivals were trigger-disjoint, did not
self-trigger after a neutral tick, and retained their source rows with exact self-target
suppression. The finite direct-controller vocabulary found 57/80 routes under each loadout; its
construction-loadout classes were 13 run-only, 35 monotone-simple-only, and 9 other, while 23
routes remained complete-no-positive within that finite vocabulary. This is strong reachability
evidence but also confirms that the source currently supplies mostly easy/monotone challenges.
Across 40 unordered door pairs, 37 had measured directional differences. Of 850 interior terrain
tiles, 773 had positive traversal corroboration; one component containing 77 tiles remained
uncorroborated and is an ablation/placement-audit target rather than assumed useless.
An expanded 72-key v1 socket inventory then found that 16 of 20 distinct socket signatures lacked
an opposite mate, affecting 59 port occurrences. Partition-route v2 replaced that mapping with a
frozen three-slot ceiling/floor grid keyed independently of intent; its exact regression has
216/216 socket occurrences mated by construction. Its authoritative rerun nevertheless rejected
the mapping: fixed ceiling connectors crossed authored secondary separators, leaving only 67/80
baseline and 70/80 complete-kit door rows positive while every miss targeted the ceiling. Version
3 reserves route, headroom, connector, and arrival cells before rasterization and returns a typed
failure on any cut conflict; it never clears or redraws authored terrain afterward. Exact
attempt-zero construction now yields 125/128 representative and 566/576 broad keys, with every
success preserving all cuts, exact support material, and the conservative forward/reverse movement
contract. Its 71-room socket sample has 212 occurrences with different-room mates for all of them.
The v3 authoritative seeds 0--3 Mixed-BSP rerun passed both gates for all 12 rooms: baseline and
complete kit each certified 80/80 directed door rows and 31/31 pickup rows. All 40 unordered door
pairs had a measured directional difference. The finite controller audit still classified most
known positives as run-only or monotone-simple (baseline 15/35/12 and complete kit 15/34/10 for
run/monotone/other), so this is a correctness and reachability pass, not yet a challenge-quality
claim. Of 823 interior terrain tiles, 721 had positive traversal corroboration; no whole component
was uncorroborated, while 102 individual tiles remain ablation targets. The source remains
experimental until breadth and expensive finalist audits pass.
