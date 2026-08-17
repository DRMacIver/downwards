# Open Questions and Todo

Deferred work and unresolved questions as of the 2026-08-17 transcript mining.
Cross-reference [decision-log.md](decision-log.md) for the decisions that
created these items and [research-log.md](research-log.md) for supporting
evidence. Items may have moved since; verify against the code before acting.

## Layout and progression

- **Keyed cul-de-sacs** (Unexplored-style; see
  [vision-and-inspirations.md](vision-and-inspirations.md)). The layout
  metrics tool must upgrade from "can retreat now" to "can retreat given what
  is obtainable from here": a no-return pocket passes iff the traversal item
  at its bottom (plus routes it opens) gets you out. Deliberately breaks the
  strict retreat invariant; mechanically self-teaching.
- **Get-out-alive victory.** Crown pickup starts an escape; winning means
  climbing back to the top. Needs a client win-condition change, a
  return-route requirement in layout tooling, and a pressure mechanism —
  prototype in order of complexity: countdown timer (nearly free), escalating
  hazards (scale hazard windows by a global clock), rising lava (most
  evocative, most complex).
- **Macro-design metrics to formalize:** locality/compactness (objective
  "messy map" measure), region identity (themed columns), in-game signposting
  (region names on the map). In the descent layout, depth is the progress bar
  and strata are regions.
- **Column identity work:** give west and east columns exclusive archetype
  signatures (tide-shaft west-only; promote east to timing rooms); move `gb`
  out of the column-head slot and `t2` out of the east column.
- **Unique landmark grids:** `sa2` (spawn companion) and `sb2` (crown
  antechamber) should get unique grids, preserving door topology, so
  navigation landmarks feel distinct.
- **Layout rev 3 ("THE DESCENT") follow-through:** the descent-consistent
  embedding is stable but was a significant topology redesign from rev 2 —
  validate saved moves when v2 ships.

## Ability gating and endgame

- Gate the ability arc on the critical path (cheapest known option: gate `oc`
  floor on wall; or gate the crown door); remove the absorbing trap by
  re-pointing `edge t1 floor kv ceiling`. (Some of this has since been
  addressed by the progression rebuild — verify current state.)
- Redesign Act 4 to require both dash and wall: insert a new hard room
  between `sb2` and the crown, moving the coin gate to the front of the new
  gauntlet for a rising curve into the crown.
- Standing correctness blockers before feature work: reduced-loadout
  successes veto claimed ability gates; ability requirements must be
  unavoidable across *all* structural routes.

## Room content

- **Cuts/reordering (deferred to integrator):** cut Old Lift and Threshold
  (pure hold-right corridors), merging their connections; optionally reorder
  Needle Turn after Boots Vault.
- **Below-40 light-touch polish (pass 2):** broad-chimney, rafter-shrine,
  relay-chasm, sliver-run, lantern-gallery, crosswind-chimney, foundry-fork,
  glass-gallery, glass-seal — small spike/coin/geometry tweaks, none
  gameplay-blocking.
- **Filler redesign strategies by type:** cut/merge (no purpose),
  teach-a-mechanic (skill never exercised by mandatory route),
  add-a-decision (riskier coin placement), keep-as-breather (only where the
  dungeon locally needs rest).
- **Novel classes to build** (sketched with TimedHazard parameters, see
  `docs/design/novel-level-classes-2026-08-16.json`): Tide Shaft, Metronome
  Gallery, Two-Clock Fork, The Flue, Metronome Bank, Duct Junction.
- **Early-game rooms need the evidence-driven rebuild** given to late
  capstones — several opening rooms still have visibly bad AI traces.
- Minor: verify shutter-chute-a floor→ceiling at wall loadout (currently
  `inconclusive PathHorizon` at the 600-tick cap; none/dash/both solve — no
  softlock, but re-run with a raised horizon or accept the seal as
  redundant).

## Difficulty calibration and playtesting

- **Next calibration round:** six matched-pair levels isolating single
  factors — contact-pad height 2 vs 4 tiles; transfer cadence 9 vs 13 ticks;
  sequence length 2 vs 5 transfers; recovery catcher absent/present; support
  width 1 vs 3; no ceiling vs multi-tick jump-cut window. Record hold
  durations, inter-jump intervals, failures, retry cost. Prioritize input
  margin, cadence, and recovery cost; downweight ticks/counts/density.
- Integrate the recorded human playtest data (jsonl button timings and
  replay frames) from the frozen Vacuum Gallery and Lunar Cache floor labs
  before the next design phase.
- More non-lethal obstacle-course practice levels for movement feel.

## UI/UX

- Disabled-ability feedback is not discoverable in normal play (only the F1
  debug overlay shows it) — needs an obvious in-game cue.
- Residual menu items from the earlier feedback round: verify the "next
  level" flow, per-level stats, scrolling menu, and preview polish all
  shipped.

## Engine / input

- Queue/timestamp *release* transitions in the render→simulation input path
  (the buffered-jump release latch is missing), fixing an over-height
  artifact when a buffered jump is accepted late.
- Multi-door generator integration needs a `RoomDraft` boundary-carving
  helper and a `NodeRole::Port` enum; the single spawn/exit API was
  deliberately left untouched.

## Tooling and pipeline hygiene

- Concurrent-agent file reversion (see
  [engineering-notes.md](engineering-notes.md)) still needs a structural fix:
  isolate/kill concurrent tuner runs, a write-lock protocol, or per-agent
  worktrees — current mitigations are workarounds.
- The one-way headroom unit test (2 empty tiles above every `-`) could not
  run in edit-only sessions; confirm the automated pass at integration for
  hand-verified rooms (coin-duct, glass-threshold, needle-room).
- Vent-spire coin 37 move to `Rect(140,20,8,10)` was proposed but not applied
  to `demo_dungeon.rs`; its FRAGILE 23/64 route numbers are stale — re-run
  `--route` after the move lands.
- Witness regeneration for redesigned routes
  (`demo-dungeon-witnesses-v1.txt`) must happen at integration time via the
  artifact-writing tool, not in session agents.
- **After v2 ships, delete the legacy demo dungeon**: `demo_dungeon.rs`
  (~5.7k lines), palette courses, witness artifacts, retune/audit machinery,
  the old `--dungeon` mode, floor lab, old map/save handling. Keep the
  solver, `audit_room_grid`, the rooms-v2 pipeline, and the level-lab
  workbench.
- Shared batch certification (one `solve_targets` search per source door,
  measured 57.8% tick reduction) is designed but unimplemented; two-worker
  room-level parallelism deferred pending finalization design (QD/socket
  selection stage is the residual ~2.5 min bottleneck).
- Whether the profiling/optimization work fully resolved the iteration-speed
  complaint is unclear — 17-minute corpus builds are fine for finals, too
  slow for design iteration.

## Corpus and manifests

- Corpus finalization is reported complete (596 rooms exact-matched, exact
  route fusion, deterministic Pareto selection, ambiguous fronts marked
  `NotApplicable`), **but** the earlier-planned held-out anti-gaming audits
  and human-facing assessment were never explicitly confirmed done — treat
  those as still open despite the "complete" framing. This is a real
  source conflict; the completion note is later, the audit gap survives it.
- `docs/research/corpus-plan.md` has stale claims needing correction:
  Baseline+Dash-only enumeration (not four kits), schema/artifact version
  conflation, stale rooms/seed projections, historical wall-audit language,
  V3/V4 rejection misclassifications, missing evidence links, stale
  "final fresh seed-0 cache" claim.
- **Manifest version compatibility blocks release:** regenerated manifests
  (selection v4, route-band policy v2) are unloadable by production (expects
  v3 / policy v1). Update the strict loader, run full catalogue and client
  tests, require byte-identical regenerations before promotion.
- Deferring shaky analysis to selected rooms, or dropping final selected-row
  recomputation, would change evidence coverage and requires an explicit
  versioned policy change.

## Deliberately deferred features

- Health/run economy (health bar exists in design; infinite-lives replay was
  accepted for the prototype).
- Optional-route certification for multi-exit rooms.
- Multi-room dungeon *generation* (as opposed to authored assembly) — pending
  core level-feel and difficulty feedback; generation remains "a tool, not
  the definitive content source" for now.
- Stamina-based climbing ("skip stamina for now") and the metaprogression
  details of per-run ability unlocks.
