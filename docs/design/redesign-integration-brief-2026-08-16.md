# Downwards Dungeon Redesign — Integration Brief

## 1. Summary

**38 rooms** were dispatched across treatments (tutorial spine, junction-decision, cache-guard, add-stakes, add-route-choice, soften, build-out, ceremony). **32 approved, 6 rejected/contested** at final critique.

### Still contested (final critique rejected)
| Room | Why rejected |
|---|---|
| **needle-room** | Grid on disk is byte-identical to HEAD; the described spike-wall chute and wall-jump climb are absent — the old trivial 19t/1j bypass is what the game loads. Fix instructions: re-apply the 18x32 grid (preserved in report + scratchpad), verify with non-empty `git diff` against the real tracked path, delete stray `--` file. |
| **current-fork** | Reported row-17 edit (spike-flanked open drop, one-way lid removed) never landed on disk; file matches HEAD. Re-apply row 17 = `..........###^#....#^###..........`, then re-run all 18 pairs + `--route` (bar ≥48/64). |
| **coin-duct** | Grid on disk doesn't match report — no sealed coin pocket, no capping shelf, wrong rung layout. Re-apply described geometry before the coin move to Rect(290,160,10,10) can be trusted. |
| **vent-spire** | Committed grid contains zero spikes; even the "preserved backup" is the unmodified baseline. Loadout-00 cross pairs are inconclusive (mandatory-pair failure). Full re-apply + re-validate required per fix instructions. |
| **broken-aqueduct** | Raised-lip takeoff (row-17 cols 11-13 solid, ledge extended to col 21) never persisted; measured numbers match the pre-fix baseline. Re-apply, verify headroom, re-run pairs + `--route`. |
| **glass-threshold** | High-line rungs/ceiling traverse absent from disk (file byte-identical to HEAD). Stashed scratchpad draft doesn't match the narrative and may violate one-way headroom — re-derive layout, write via Bash with same-invocation md5/diff verification. |

All six rejections share the same root cause: a concurrent process (rogue `retune_demo_dungeon -- --route` artifact runs / `git checkout -- rooms/`) silently reverting grid files in the shared worktree. The designs themselves were generally judged plausible; the deliverables didn't survive to disk. **Verify every room grid against its report before regenerating artifacts**, and confirm no rogue retune processes are running (`ps aux`).

## 2. Coin-move table

Coin rects live in `room_coin_specs` in `demo_dungeon.rs`. "(new)" = room currently has no registered coin. Re-run `--route` after each move lands — many robustness numbers were measured against stale coin positions.

| Slug | Current | Proposed Rect | Rationale |
|---|---|---|---|
| hollow-landing | Rect(158, 100, 8, 10) | Rect(190, 80, 8, 10) | Top of new one-way staircase; teaches jump-up-through + press-down-to-drop. Old spot is now empty air. |
| moss-walk | Rect(222, 90, 8, 10) | Rect(170, 90, 8, 10) | Over the mandatory row-11 crossing rung; old position unreachable in new grid. **Must move or coin is uncollectable.** |
| sluice | (none) | Rect(123, 110, 6, 6) **and** Rect(153, 110, 6, 6) (new, two coins) | On the central one-way span over the spike bed; slow-down over spikes, not a detour. |
| wall-antechamber (coin-16) | Rect(264, 120, 8, 10) | Rect(161, 100, 8, 10) | Pillar-top at the chimney top-out; wall-jump lesson pays. Re-run `--route` post-move (pre-move showed 29/64, expected to clear). |
| dash-seal | (none) | Rect(222, 102, 6, 6) (new) | East face of divider atop the optional one-way ladder, past the mandatory dash. |
| needle-room (coin-11) — **contested** | Rect(188, 110, 8, 10) | Rect(156, 20, 8, 10) | Belfry landing at top of chute; only apply after the grid is re-landed. |
| climber-vault | (none) | Rect(268, 22, 8, 8) (new, untested) | Single wall-jump off buttress after glove pickup; unreachable at 00. Solver-unverified. |
| split-root | (none) | Rect(131, 100, 8, 10) (new) | Far west end of the optional loft platform; forces full traverse after the two-jump climb. |
| bell-switchback | (none) | Rect(206, 30, 8, 10) (new) | New row-4 shelf reachable only via the wall-jump chute. |
| cullet-fork | (approved earlier proposal) | Rect(234, 68, 10, 10) | Middle floating one-way under the `vvvvv` row; independent of slot rework. |
| pressure-fork | (none) | Rect(211, 70, 8, 8) (new) | Row-8 one-way under the vvvv cluster; overshoot lethal, undershoot free retry. |
| split-furnace | (none) | Rect(142, 90, 8, 8) (new) | Third ladder rung under the decorative vvvv. Critic notes this shortcuts the full ladder — optional tweak in their fix notes. |
| orbit-fork | (none) | Rect(90, 70, 8, 10) (new) | New row-8 shelf under down-spike cluster at top of the zigzag ladder. |
| tidal-fork | (approved earlier proposal) | Rect(272, 92, 8, 8) | Far-right lip one-way at row 10, payoff of the ascents. |
| bell-niche (coin-18) | Rect(144, 20, 8, 10) | Rect(122, 150, 8, 10) | Shaft floor at the bottom of the left-drift commitment. **Critic already applied this edit to demo_dungeon.rs — confirm and commit.** |
| root-cellar (coin-02) | current | Rect(20, 126, 10, 10) | Upper cache coin on the row-14 landing platform. |
| root-cellar (coin-03) | ~(237, 89) | Rect(20, 158, 10, 10) | Floor pocket beneath the platform; makes `--route` exercise the whole descent. |
| ember-vault (coin-30) | Rect(224, 30, 8, 10) | Rect(261, 160, 8, 10) | Dash-only pocket at shaft bottom. After moving, optionally make the row-3 shelf solid `#`; re-run `--route` (deaths > 0 expected). |
| coin-loft (coin-9) | Rect(244, 90, 8, 10) | Rect(110, 50, 8, 10) | New wall-jump alcove shelf. If coin-route pass fails, drop shelf one row (row 7). coin-8 stays at Rect(144, 110, 8, 10). |
| ash-cache (coin-36) | current | Rect(240, 130, 8, 10) | Row-14 spike perch; overshoot lethal, short jump safe. |
| gear-gallery (coin-31) | Rect(214, 20, 8, 10) | Rect(150, 20, 8, 10) | Top tower rung above new spike bed; old spot floats off-geometry. |
| temper-hall (coin-45) | Rect(144, 90, 8, 10) | Rect(200, 148, 8, 10) | Old spot is now walked past by the safe high line; new spot is mid-flight over the spike bed, restoring the risky-low-line intent. Re-study post-move. |
| gravity-lift | (none) | Rect(60, 116, 8, 8) (new) | Dead-end pocket west of the new mid gap; makes the leftover loop a route choice. |
| landing-chain | (none) | Rect(260, 38, 10, 12) (new) | Top of the untouched original chain — payoff for the optional ascent (geometry-only, unverified). |
| vent-spire (coin-37) — **contested** | Rect(214, 20, 8, 10) | Rect(140, 20, 8, 10) | Above the top ladder rung; only after the grid is re-landed. |
| coin-duct (coin-24) — **contested** | current | Rect(290, 160, 10, 10) | Sealed pocket behind the spike hop; only after the grid is re-landed. |
| gatehouse (optional) | (none) | Rect(40, 20, 10, 10) | Only if a coin is ever wanted; top of ceremonial vault ladder. Not required. |

Keeps (no change): cinder-bridge (coin-38), brake-tower (coin-27), low-passage (coin-23), annealing-spire (coin-49), cooling-duct (coin-32), foundry-seal, needle-turn (coin-21), void-pass proposes a **new** coin at Rect(130, 160, 10, 10) (chute floor, second full run of the mechanic), skybridge (coin-62), meteor-run (no coin per brief).

## 3. Structure changes for the integrator

From the plan:
- **Cut old-lift and threshold** (recommended): merge their door connections into neighbors. Fallbacks if kept: old-lift = 2-rung one-way ladder with small `^^` patch (loadout 00); threshold = one wall-jump chimney obstacle (loadout 10).
- **Needle Turn reorder (optional)**: softening was done in place and approved, so the connection-graph move (to after Boots Vault) is no longer required — but remains the "better fix" if rewiring is cheap.
- **Meteor-run kept and built** — no door rewire needed.
- **Verify skybridge / cinder-bridge mandatory paths** post-integration (both softened and approved).
- **Below-40 light-touch list deliberately deferred** (broad-chimney, rafter-shrine, relay-chasm, sliver-run, lantern-gallery, crosswind-chimney, foundry-fork, glass-gallery, glass-seal) — not in this batch.

Surfaced by room agents:
- **low-passage**: room name now overstates the geometry (crawl ceiling removed entirely); consider rename or doc note.
- **gravity-lift**: retune prints "segmented Gravity Lift leg 2 lost its recovery traverse" — this is the intentionally cut traverse, but check whether the segmentation heuristic is load-bearing elsewhere.
- **climber-vault**: 00 grid-only bypass of the wall-jump gate exists; moot because the east connection requires WallJump — do not remove that connection requirement.
- **landing-chain**: keep row 14 col 30 empty (east-door arrival point; a platform there panics ArrivalBlocked).
- **Playbook doc fix**: `docs/design/room-iteration-playbook.md` line ~69 — `retune_demo_dungeon -- --route <room>` is wrong; the bare `--` becomes an output path and triggers a full artifact run (writing a stray `--` file and clobbering room grids). Correct form: `retune_demo_dungeon --route demo-dungeon.<slug>`. Also document the scratchpad-copy `DOWNWARDS_ROOM_GRID_DIR` workflow, and consider separate worktrees for parallel agents.
- **Housekeeping**: `rm -f -- --` at repo root; check for leftover dirty files from concurrent agents (`demo_dungeon.rs`, `tidal-fork.txt`); confirm no rogue retune processes before committing.

## 4. Risk list (needs human review)

**File-integrity risks (verify grid contents before regenerating witnesses):**
- All six rejected rooms (§1) — re-apply and re-validate.
- moss-walk (confirm md5 370da00868f651e65561dcdd6a3f94f… per report: `370da00868f651e65561dcdd6acdcf94f` — check `370da00868f651e65561dcdd6ac3f94f`), bell-niche (grid never committed; write from scratchpad `bell-niche-final.txt` immediately before commit and re-verify), ash-cache (row 16 must read `#.........####.....####^^......#` — reverted 5+ times during review), skybridge (restore from scratchpad `skybridge-final.txt` if reverted, md5 06b68c0232a3d5e6a22233e549f0f232), current-fork/vent-spire/needle-room/coin-duct/broken-aqueduct/glass-threshold as above.

**Stale robustness studies (re-run `--route` after coin moves):**
- wall-antechamber (29/64 pre-move; expected to clear post-move; also watch the 1-tile pillar-cap landing), bell-switchback (whole-dungeon pass panicked on needle-room before per-family numbers printed; witness got *more* complex — 4wj/134t), ember-vault, temper-hall, root-cellar, coin-loft, ash-cache, gear-gallery.

**Below-bar or zero-margin robustness (approved but shaky):**
- dash-seal 37/64 (diagnosed as locomotion noise, in-family with other dash rooms), split-root 21/64 (all timeouts, 0 deaths, baseline was 19), split-furnace 20/64 with new 12-death channel on a "light mandatory" route (critic left optional tuning notes), orbit-fork 27/64, gear-gallery 32/64, annealing-spire 33/64, cooling-duct 34/64, brake-tower 25/64, gravity-lift ~10-17/64 (critic reproduced worse than reported), landing-chain 44/64, needle-turn 3 families below bar (accepted as hard-but-fair), void-pass 46/64 (2 short of bar, deaths slightly under target band).
- Exactly-on-the-bar, no margin: pressure-fork (48/64), tidal-fork (48/64), low-passage (48/64 CorrelatedTiming), temper-hall (48/64), broken-aqueduct HoldRelease pinned at 48 (once re-landed). Any physics/tuning drift re-checks these first.

**Unverified-by-solver geometry (human eyeball or coin-route pass):**
- hollow-landing optional staircase, bell-niche climb-out ladder, root-cellar cache line, coin-loft alcove wall-jump, split-root loft, orbit-fork coin shelf, landing-chain ascent, temper-hall east wall-jump shaft (only west proven; east->west solving is consistent evidence), gatehouse vault ladder.

**Behavioral regressions to sign off:**
- pressure-fork: floor->west/east now inconclusive at 00 (acceptable only because floor key 52 guarantees 11 — confirm no pre-ability entry exists).
- cinder-bridge: west->east now solvable at 00/10 (dash check softened); east->west retreat still dash-only with no resync (optional second shelf spec in critique).
- dash-seal: solver showed nondeterminism on 10/00 crossings mid-iteration — one more confirmation run recommended.
- meteor-run: `--route` results varied with dirty sibling grids (46 vs 50/64) — confirm the 50/64 on a clean tree. One agent also admits possibly clobbering concurrent edits via early `git checkout -- rooms/`.
- Pre-existing test failures noted during runs: gatehouse (should now pass post-redesign — confirm), gravity-lift, moss-walk headroom test — fix as part of the test pass.