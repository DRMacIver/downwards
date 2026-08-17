# Engineering Notes

Pipelines, regeneration procedures, testing philosophy, agent-workflow
lessons, and gotchas. The authoritative room-authoring reference is
`docs/design/room-iteration-playbook.md`; this document collects the
operational knowledge around it.

## Room validation / iteration protocol

The core loop for any new or redesigned room:

1. **Solvability.** Re-run every ordered door pair (including self-pairs) at
   minimum loadout with `--pair`; every mandatory pair must print "solved" —
   "inconclusive" is a reject. Self-pairs matter because a player bounced off
   a locked door must be able to retreat (essential for dead-end vaults and
   one-way redirects).
2. **Robustness.** Run the `--route` shaky-hand study; the worst of the four
   noise families (BoundaryTiming, CorrelatedTiming, HoldRelease,
   DropRepeatFrame) must clear **48/64**. Per-family scores diagnose the
   failing design aspect (one-way headroom, jump timing, drop buffering...).
3. **Hazard sanity.** Deaths occur only — and definitely — where the brief
   claims danger. A hazard with zero deaths under noise is decorative.
4. **Design critique.** Does the room have a job, a decision, the right
   difficulty? "Valid but dull" is a rejection.

Per-room auditor: `./target/release/examples/audit_room_grid <grid> <spec>
--shaky` — iterate until verdict `ok`, all intended pairs solve at minimum
loadout, and all coins solve from at least one door.

Dungeon-wide: `dungeon_layout_metrics.py` enforces the structural hard
targets (grid embedding, reachability, cycle rank, diameter, absorbing-trap
detection — verdict `ok` requires zero absorbing traps at every loadout).
`audit_dungeon_traversal` solves every (room, entry→exit, loadout)
combination and checks all rooms reachable and return-to-entrance always
possible without coins.

## The `retune_demo_dungeon` `--` gotcha (repeated, cross-session)

The playbook originally documented `retune_demo_dungeon -- --route <room>`.
**This is wrong.** The bare `--` is consumed as the artifact output path,
which (a) writes a junk file literally named `--` into the repo root and (b)
runs full-dungeon mode, regenerating witnesses for all 101 routes and
potentially rewriting unrelated room grids. This silently corrupted
measurements in many independent agent sessions.

Correct invocation:

```
./target/release/examples/retune_demo_dungeon --route demo-dungeon.<slug>
```

Hygiene: run `git status --short` immediately after any invocation; delete
any stray `--` file. Also: unquoted shell loops over room lists word-split —
use explicit separate calls.

## Regeneration cycle after a movement-policy bump

Witness/difficulty caches are keyed per `PLAYER_MOVEMENT_POLICY_VERSION`;
bumping it invalidates everything. Regenerate in order:

1. `retune_demo_dungeon` (all 101 routes, or `--route` for one room).
2. `audit_dungeon_traversal` — confirm 101/101 reachable and retreatable.
3. Delete/regenerate `demo-dungeon-traversal-v1.txt` (version-keyed only, so
   it must also be regenerated after grid edits that *don't* bump the policy).
4. `retune_gallery` and `retune_calibrated_generator`.
5. Rebuild the research `curate` tool; re-run all four tier manifests
   (`curate` writes to STDOUT).
6. `room_passability.py` and `audit_dungeon_v2`.
7. Validation suite at high budget.

**Never hand-edit route expectations.** When geometry changes, regenerate the
full 101-route artifact — deliberately more expensive than editing one row,
because it prevents new geometry silently pairing with stale AI evidence.

## Shared-checkout / concurrent-agent hazards

Recurring failure mode when multiple agents edit
`crates/downwards-gen/rooms/` concurrently: grid files silently revert to
HEAD within seconds of a write (concurrent checkouts, stash collisions, and a
background process regenerating `rooms/*.txt` from the compiled Rust table);
Write/Edit tools can silently fail; `--route` against a reverted file
produces a false "flat" witness.

Converged workaround pattern:

- Copy the rooms directory to a scratch location; iterate there with
  `DOWNWARDS_ROOM_GRID_DIR=<scratch/grids>`.
- Verify **every** write landed (md5 / `git diff` / `git status --short`) —
  never trust tool-reported success.
- Atomically `cp` final grids back and commit to pin the result.
- **No destructive git commands during iteration** — a stash collision once
  silently destroyed unrelated dungeon-map/AI-mode work.
- Preferably: separate worktrees for parallel agents.

### Fabricated-report defence

After incidents where implementer agents reported detailed redesigns while
the tracked files were byte-identical to HEAD, the standing rules are:
cat-verify every write before claiming success; provide md5 hashes and file
lengths as proof; verify claimed edits via `git diff`/`git show` before
trusting any report; treat pre-edit witness numbers as unreliable until the
post-edit grid is confirmed on disk. Under these rules all six contested
rooms of the redesign pass re-landed successfully.

## Testing philosophy

- **Structural contracts over exact coordinates.** Hand-maintained
  exact-coordinate tests go stale; write contracts ("two hazard runs,
  recovery between them") and delegate exact route expectations to the
  mechanically generated artifact.
- **Walk the intended progression exactly.** Don't test completion via a coin
  threshold while skipping branches; prove an under-coined player is
  physically rejected at gates. Comprehensive check: mechanically remove each
  floor in turn and prove progression becomes impossible.
- **Ability-removal bypass testing.** Remove Wall Jump or Dash and re-run the
  search at the same budget; any found route is an unintended bypass and the
  *geometry* is defective, not the audit ("I'm not weakening the audit...").
  Reduced-loadout successes veto claimed ability gates.
- **Bidirectional reachability.** Stage each room's return route through exact
  replay — budget exhaustion doesn't prove a branch is trapped, and staged
  returns catch real bugs (Shadow Duct's start platform made its branch
  enterable but not leaveable).
- **Behavioral audit over scores.** Inspect actual replay traces (reversals,
  contact timing, rejected presses) rather than trusting aggregate numbers;
  rooms with solver thrashing (>120 action spans, >16 jumps) are authoring
  defects, not "difficulty".
- **Evidence typing.** Route/robustness evidence stays explicitly typed
  (Positive/Inconclusive/Missing/Bounded); ambiguous Pareto fronts remain
  `NotApplicable::AmbiguousNondominatedFront`, never averaged or defaulted.
  Artifact formats are fail-closed: stale or unverifiable shards are rejected;
  every positive replay is independently regenerated before reuse.
- **Test cross-contamination.** Context patches can leak one room's staged
  diagnostics into another room's tests; run focused per-room tests and
  re-run both rooms after a fix.
- **Scoped commits.** One commit per room rebuild, with full artifact
  regeneration, tests, strict Clippy, and formatting green at each
  checkpoint — keeps human feedback attributable and reversible.

## Design/authoring technique notes

- **Forcing a built route to be mandatory:** block the flat alternate with a
  non-lethal wall/pit, move the coin onto the mandatory route's visible path,
  target 3–4 trivial jumps at loadout 00, keep rises ≤2 tiles, re-validate all
  door pairs.
- **Coin placement burden:** implementers leave coins alone unless the brief
  explicitly moves them.
- **Segmented routes for robustness** (Watch Post pattern): author routes in
  segments that park against side walls so jittered runs re-converge; raised
  a 0/64 room to fully robust and generalized to other frame-perfect rooms.
- **Staged search decomposition:** when the monolithic beam solver exhausts
  its budget on multi-stage rooms, decompose along visible recovery shelves
  and store the exact-replayed composed route.
- **Curated teaching routes:** replace the suffix of a solver route with a
  small authored controller search so the stored demonstration shows one clean
  intended ascent, not solver fumbling.
- **Replay simplification** exposes AI jump-spam; seed a follow-up search with
  the simplified route and lock the corrected sequence with regression tests.
- **Floor ports are exit ledges:** a solid landing beside a two-tile clear
  shaft with a 20px trigger — an ordinary run/fall reaches it, no
  solver-specific vocabulary needed.
- **Port-placement pathology:** a central floor ledge overlapping a side
  door's ledge by one tile can make the underside irreversible; reserve a
  clean central band for ceiling/floor ports.

## Client input handling

- Sample jump input once per render; retain transitions across zero-step
  frames; consume during fixed-step catch-up. When releasing one alias while
  another is held: `if pressed && released { queue_transition(!held); }
  queue_transition(held);` — preserves same-frame taps while respecting
  aggregate held state.
- Post-death input lockout ~0.4s (~25 ticks) prevents held movement carrying
  the respawn back through the entry door.
- Known gap: release transitions should be queued/timestamped like presses
  (see [open-questions-and-todo.md](open-questions-and-todo.md)).

## Corpus pipeline operations

- **Calibration cache:** build separately for seeds 0–15 in a fresh directory
  (`corpus_v3_offline_selection cache <shard-root> 0 16 <cache-path>`);
  `verify-cache` can rebind the source; `inspect-selection` is storage-only.
  Keep the calibration cache strictly separate from the full-range run.
- **Witness fusion/dedup:** identity is `(source_door_id, target_door_id,
  loadout)` plus exact replay equality; replays must end at first target
  contact; `SearchStats`/`WitnessFingerprint` excluded from identity; Pareto
  dominance via strict minimization over the seven
  `ControllerDemandCoordinates`.
- **Benchmark discipline:** timing goes to JSONL/stderr only, never into
  evidence DTOs; fixed content-addressed rooms, release mode, warmup + 3–5
  reps, report min/median wall plus CPU/RSS, assert identical output hashes.
- **Disk space:** during long corpus builds, delete only reproducible Rust
  incremental caches (multi-GiB), never sources, evidence artifacts, or
  release binaries; clean at coordinated idle points.
- The generator-neutral, checkpointed corpus path gives all generator
  strategies one consistent denominator for construction failures, loadout
  matrices, and challenge metrics without laundering provenance.

## Agent workflow lessons (summary)

- Fan out per-room work to Opus/Sonnet agents; reserve Fable for synthesis
  (designer-requested; recorded in project memory).
- The validated pipeline shape: plan → parallel tool-checked builds →
  independent critic verification → one fix round → integration brief.
- Gate acceptance on tool output, never on agent self-report.
- `DOWNWARDS_ROOM_GRID_DIR` lets sessions iterate on grids without rebuilds.
- Persistent planning docs (e.g. `docs/research/corpus-plan.md`) exist
  specifically to survive context compaction — keep them updated.
