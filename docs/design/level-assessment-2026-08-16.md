# Downwards Demo Dungeon — Level Design Review Synthesis

*Synthesized from independent per-room assessments (witness traces + 1-tick input-noise robustness trials). 90 of 101 rooms were assessed; floor ordering below is approximate, reconstructed from coin numbering and door connections.*

---

## 1. Executive Summary

**The owner's complaint is confirmed, but it's concentrated, not uniform.** 27 of 90 assessed rooms (~30%) are filler — rooms whose mandatory witness route completes with 0–2 inputs, zero deaths under all noise models, and geometry (one-way platforms, spike clusters, interior walls) that the actual path never touches. The other ~70% is genuinely good: the dungeon has a strong core of hazard-backed execution rooms whose noise trials show real deaths, not just timing wobble.

**Where the filler lives:**
- **The opening (rooms 1–10) is almost entirely filler.** Hollow Landing, Moss Walk, Split Root, Root Cellar, Threshold, Old Lift, Sluice, Coin Loft — the player can hold right for the first several screens without jumping. Nothing is taught before the wall-jump vault.
- **Junction rooms are systematically dead.** Nearly every three-way fork (Split Root, Cullet Fork, Pressure Fork, Split Furnace, Split Kiln, Orbit Fork, Tidal Fork, Bell Switchback, Current Fork) follows the same template: flat floor, drop through a one-way slot, decorative spikes and platforms in the unused upper two-thirds. Same 82-tick, 2-jump witness in almost all of them.
- **Dead-end coin caches are unguarded.** Bell Niche, Coin Duct, Root Cellar, and Ember Vault hand over their coin in 1–29 ticks with essentially zero inputs — the coin sits at the door mouth while the room's challenge geometry goes untouched.

**Difficulty curve shape:** flat-zero through ~room 10, a sharp spike at Boots Vault / Needle Turn (room ~18, possibly too early for its harshness), then a healthy sawtooth through the middle furnace/glass regions (alternating demanding gauntlets with junctions — except the junctions are *too* empty to count as breathers with intent), and a genuinely excellent sustained climax from Observatory (~87) through Crown Sanctum: Shadow Duct, Nova Niche, Aurora Spire, Void Pass, Starwell Climb, Skybridge, Astral Seal all show 30–60% death rates under noise. The last 15 rooms need almost no work. A recurring secondary pattern: many mid-game rooms are **timing-sensitive but consequence-free** (low shaky success, zero deaths, no spikes) — precision without stakes, which reads as retry-friction rather than challenge.

---

## 2. Room Table (approximate floor order)

| Slug | Purpose | Challenge | Fun | Filler? |
|---|---|---|---|---|
| hollow-landing | Spawn room, walk right | trivial | dull | **YES** |
| moss-walk | First corridor, unused platforms | trivial | dull | **YES** |
| split-root | 3-way junction, drop-through | trivial | dull | **YES** |
| root-cellar | Coin cache (coins 2–3), unguarded | trivial | dull | **YES** |
| watch-post | Base-kit zigzag climb for coin 5 | moderate | interesting | no |
| lantern-gallery | Junction + one-way ladder climb | light | passable | no |
| sluice | Flat corridor, one spike hop | trivial | dull | **YES** |
| broken-aqueduct | Corridor with one spike hop | trivial | dull | **YES** |
| old-lift | Empty walk-through connector | trivial | dull | **YES** |
| gravity-lift | Rote zigzag switchback, 525 ticks | light | passable | **YES** |
| three-way-hall area: coin-loft | Coin cache 8–9, controlled fall | trivial | dull | **YES** |
| crossroads | 4-way hub, advertises 21-coin gate | trivial | passable | no |
| needle-room | Spike shaft ignored by coin route | trivial | dull | **YES** |
| underpass | Junction + free coin 13 | light | passable | **YES** |
| treasury | Spike-floor platform climb, coins 14–15 | moderate | interesting | no |
| climber-vault | WallJump unlock vault | light | passable | no |
| wall-antechamber | "Stone Lessons" that teaches nothing | light | passable | **YES** |
| broad-chimney | Dash-less wall-jump chimney, coin 17 | moderate | passable | no |
| bell-switchback | 3-way junction, unused geometry | light | passable | **YES** |
| bell-niche | Coin 18 at the door mouth, 1 tick | trivial | dull | **YES** |
| tempo-hall | Spiked chimney wall-jump, coin 19 | moderate | interesting | no |
| wall-gate | Wall-jump proficiency check | moderate | interesting | no |
| threshold | Hold-right corridor | trivial | dull | **YES** |
| rafter-shrine | Hazard-free wall-jump climb, coin 20 | light | passable | no |
| split-spire | No-dash pillar climb junction | moderate | interesting | no |
| landing-chain | Flat walk, untouched platforms | trivial | dull | **YES** |
| needle-turn | Spike-lined needle chute, coin 21 | demanding | interesting | no |
| boots-vault | Dash unlock behind spiked chute | demanding | interesting | no |
| wall-gallery | 4-way hub + ceiling climb | moderate | passable | no |
| gale-landing | Spike-pit coin 22 detour | moderate | interesting | no |
| low-passage | Flat 8-dash corridor, coin 23 free | light | dull | **YES** |
| current-fork | 3-way junction, fiddly drop | light | passable | no* |
| coin-duct | Coin 24 at entry mouth, 1 tick | trivial | dull | **YES** |
| pulse-gallery | Spike hops guarding coin 25 | light | passable | no |
| dash-chasm | Chained-dash spike chasm | demanding | interesting | no |
| storm-split | Junction + spiked platform climb | moderate | interesting | no |
| storm-cache | Hazard-free dash climb, coin 26 | light | passable | no |
| relay-chasm | Platform relay over spike pits | moderate | interesting | no |
| brake-tower | 6-dash ladder, no stakes, coin 27 | moderate | passable | no |
| dash-seal | 8-dash flat walk, 64/64 success | trivial | dull | **YES** |
| alloy-threshold | Spike-floor dash corridor, coin 28 | moderate | passable | no |
| windshaft | Dash-ladder coin 29, trivial floor route | moderate | interesting | no |
| split-furnace | 3-way junction, decorative spikes | light | passable | **YES** |
| ember-vault | Unguarded coin 30, 29 ticks | trivial | dull | **YES** |
| gear-gallery | Hazard-free coin tower, coin 31 | light | passable | no |
| crosswind-chimney | Mandatory up-and-over wall climb | moderate | interesting | no |
| cooling-duct | Short hazard-free climb, coin 32 | light | passable | no |
| lift-shaft | Junction + spiked platform ascent | moderate | interesting | no |
| spark-niche | Spike-floored coin 34 chamber | moderate | interesting | no |
| hammer-hall | Spike-bed crossing, coin 33 | moderate | interesting | no |
| rivet-run | Dash gauntlet over 3 spike beds | moderate | interesting | no |
| blast-gallery | 6-dash chain gauntlet, coin 35 | demanding | interesting | no |
| pressure-fork | 3-way junction, dead upper 2/3 | light | passable | **YES** |
| ash-cache | Coin 36 behind 2 dashes, no threat | light | passable | no |
| vent-spire | Trivial floor + hazard-free coin 37 climb | light | passable | no |
| piston-pass | Dash/hop relay over spikes | demanding | interesting | no |
| crucible-climb | Spike-lined wall-jump channel | moderate | interesting | no |
| cinder-bridge | Ascending dash relay, coin 38 | demanding | interesting | no |
| foundry-seal | Wall-jump climb, coin 39, no teeth | moderate | passable | no |
| foundry-fork | Junction + zig-zag dash ascent | moderate | interesting | no |
| glass-threshold | One-way hops over spikes, coin 40 | moderate | interesting | no |
| prism-run | Dash gauntlet, coin 41 | demanding | interesting | no |
| split-kiln | 3-way junction, decoration only | light | dull | **YES** |
| shard-vault | Spike-guarded vault, coin 42 | demanding | interesting | no |
| glass-gallery | Flat corridor + optional coin 43 climb | moderate | passable | no |
| mirror-fork | Junction with real spiked climb | moderate | interesting | no |
| mirror-duct | Spike-gap wall-jump pocket, coin 44 | moderate | interesting | no |
| temper-hall | Dash-over-spikes, coin 45 | moderate | interesting | no |
| furnace-lift | Spiked ladder ascent junction | moderate | interesting | no |
| lens-niche | Compact dash/wall-jump pocket, coin 46 | moderate | interesting | no |
| sliver-run | Three identical spike hops | moderate | passable | no |
| hot-glass | Dash chain gauntlet, coin 47 | demanding | interesting | no |
| cullet-fork | 3-way junction, unused upper 2/3 | trivial | dull | **YES** |
| cullet-cache | Spike-guarded coin 48 | moderate | interesting | no |
| annealing-spire | Hazard-free spire climb, coin 49 | moderate | passable | no |
| razor-pass | Dash gauntlet, 3 spike beds | demanding | interesting | no |
| lattice-climb | Spike-lined mandatory shaft | demanding | interesting | no |
| crystal-bridge | Spike-pit traverse, coin 50 | moderate | interesting | no |
| glass-seal | Wall-jump shafts, coin 51 | moderate | interesting | no |
| star-threshold | Spiked chute + ladder, coin 52 | demanding | interesting | no |
| comet-run | Spike-run floor, coin 53 | demanding | interesting | no |
| orbit-fork | Empty crossroads | light | passable | **YES** |
| moon-vault | Spike-floor vault, coin 54 | moderate | interesting | no |
| constellation-hall | 6-dash spike-floor gauntlet, coin 55 | demanding | interesting | no |
| zenith-shaft | Offset wall-jump ascent over spikes | demanding | interesting | no |
| meteor-run | Completely empty box | trivial | dull | **YES** |
| vacuum-gallery | Spiked chute + zigzag climb | demanding | interesting | no |
| tidal-fork | 3-way junction, decorative spikes | light | passable | **YES** |
| lunar-cache | Spike-lined descent, coin 60 | demanding | interesting | no |
| eclipse-fork | Junction with real ceiling climb | moderate | interesting | no |
| shadow-duct | Alternating spiked shafts, coin 56 | demanding | interesting | no |
| observatory | Spiked wall-jump shaft, coin 57 | demanding | interesting | no |
| refraction-shaft | Alternating-face spike climb | demanding | interesting | no |
| nova-niche | Alternating spike-face shaft, coin 58 | demanding | interesting | no |
| aurora-spire | Late spike gauntlet, coin 61 | demanding | interesting | no |
| void-pass | Read-the-wall spiked chute | demanding | interesting | no |
| starwell-climb | Mixed relay over spike floor | demanding | interesting | no |
| skybridge | Spike-floor gauntlet, coin 62 | demanding | interesting | no |
| astral-seal | Final-coin capstone, coin 63 | demanding | interesting | no |
| gatehouse | 64-coin gate antechamber | moderate | passable | no |
| crown-sanctum | Finale gauntlet | demanding | interesting | no |

*\*current-fork was rated non-filler by its reviewer but matches the dead-junction template exactly; treat it as part of that group.*

---

## 3. Standout Rooms Worth Protecting

Do not touch these — their witness traces show dense mixed-input routes and their noise trials show real spike deaths, i.e. challenge with genuine stakes:

- **Crown Sanctum, Astral Seal, Gatehouse sequence** — the finale lands. Astral Seal (7 jumps / 4 wall-jumps / 6 dashes, 11–30 deaths per 64) is exactly what a last-coin room should be.
- **The late-game corridor 86–98**: Shadow Duct, Observatory, Nova Niche, Refraction Shaft, Void Pass, Aurora Spire, Starwell Climb, Skybridge. The alternating-spike-face wall shafts (Nova Niche, Void Pass, Refraction Shaft) are the dungeon's best mechanic — reading which wall face is safe before each wall-jump.
- **Boots Vault** — the Dash unlock behind a spiked wall-jump chute is the model for how ability rewards should be earned.
- **Needle Turn** — excellent, though possibly too harsh for floor ~18 (40/85 noise deaths); consider its position, not its design.
- **Mid-game gauntlets**: Dash Chasm, Blast Gallery, Piston Pass, Cinder Bridge, Hot Glass, Prism Run, Razor Pass, Lattice Climb, Star Threshold, Constellation Hall, Zenith Shaft.
- **Well-built pockets**: Mirror Duct, Lens Niche, Shard Vault, Lunar Cache, Moon Vault, Cullet Cache, Treasury, Watch Post — proof that dead-end coin rooms *can* work.
- **Mirror Fork and Eclipse Fork** — the only junctions that work, because the third exit costs a real climb. These are the template for fixing the other nine forks.

---

## 4. The Filler Problem

Four distinct failure modes, four distinct fixes:

### A. Cut or merge candidates (rooms that add only distance)
- **meteor-run** — literally an empty 4-tile-high box. Build it or delete it.
- **old-lift**, **landing-chain**, **threshold** — pure hold-right corridors with untouched decoration. Each could be merged into a neighbor or cut outright unless given a job (see group C).
- **split-root** — reviewer explicitly suggests collapsing it and connecting Moss Walk directly to the Root Cellar drop.
- **gravity-lift** — 525 ticks of rote zigzag for zero reward; either shorten to one switchback or add per-shelf coins.

### B. Teach-a-mechanic candidates (the broken tutorial spine, rooms 1–17)
The dungeon currently teaches *nothing* before Broad Chimney demands 5 wall-jumps. Rebuild these as the tutorial:
- **hollow-landing** → first jump + one-way drop-through (raise east door mouth 1–2 tiles).
- **moss-walk** → coin + one-way platform tutorial (block the flat floor, put the coin on the platform staircase).
- **sluice** → coin-gated-door tutorial (its coins on the one-way platforms feed the coins=6 door it leads to).
- **wall-antechamber** ("Stone Lessons") → must actually force a wall-jump; currently 0 wall-jumps on the witness despite being the lesson before Broad Chimney. Raise coin-16 atop the central pillar.
- **needle-room** → the spike shaft is fully built and fully bypassed; move coin-11 to the shaft top and it becomes the first spike-wall lesson.
- **dash-seal** → currently 8 dashes across a flat floor with 64/64 noise success; should be the room that *proves* the dash (see recommendations).

### C. Add-a-decision candidates (dead junctions and unguarded caches)
All nine dead forks share one fix: give the third exit or the unused upper geometry a stake, per the Mirror Fork model.
- **Junctions**: bell-switchback, cullet-fork, pressure-fork, split-furnace, split-kiln, orbit-fork, tidal-fork, current-fork, split-root. Standard patch: coin on the upper one-way platforms under the existing (currently decorative) spike clusters, and/or spike the lips of the floor-drop slot so committing to the descent is a deliberate act.
- **Unguarded caches**: bell-niche, coin-duct, root-cellar, ember-vault, coin-loft. Standard patch: move the coin to the *far* side of the room's existing geometry (bottom of the inner shaft, past the spike strip) so the built decoration becomes the actual test and the exit climb is earned.
- **low-passage** and **broken-aqueduct**: break the flat floor with spike gaps so the existing platforms/dash-chain become mandatory.

### D. Keep-as-breather (legitimate rest, needs only light touches)
- **crossroads** — a real 4-way hub advertising the 21-coin gate; fine as a breather, would benefit from making the gate visually aspirational.
- **underpass** — a free coin between two coin-gated doors is acceptable pacing; optionally guard the ceiling route.
- **gatehouse** — pre-finale antechamber; keep calm, but decide whether it's ceremony (simplify the fiddly dash reversals) or a last test (spike the platform stagger). Currently it's neither.

---

## 5. Top 10 Prioritized Redesign Recommendations

1. **Rebuild the opening five screens as a tutorial chain (hollow-landing → moss-walk → sluice).** Hollow Landing: raise the east door mouth 2 tiles so exit requires one jump. Moss Walk: place a 2-tile wall or pit across the floor row at mid-room so the one-way staircase (already built) is mandatory, and move its coin onto that chain. Sluice: extend the ^^ spike pair to a 5–6 tile bed under the existing one-way platforms and put 2 coins on them, feeding the coins=6 gate. Three edits, and the first minute teaches jump, drop-through, coin, and spike.

2. **Build meteor-run or delete it.** It is an empty box with a gauntlet name. Minimum viable: spike the floor in 3 alternating beds with 2-tile safe strips so the corridor costs timed dashes, matching its neighbors Gravity Lift and Vacuum Gallery. Otherwise remove the room and connect the doors directly.

3. **Fix wall-antechamber so "Stone Lessons" teaches the wall jump.** Move coin-16 to the top of the central pillar and close the low bypass route (extend the pillar down or add a floor wall), forcing one wall-jump chain in the pillar/right-wall gap. Critical because Broad Chimney (next room) demands 5 wall-jumps cold.

4. **Make dash-seal an actual dash seal.** Extend the row-8 ceiling spikes downward or drop a wall segment from the column-21 divider so the only crossing is a mid-air dash through a 2-tile spike-lined window over a spike pit. Currently 0 jumps, 64/64 noise success — the name is a lie.

5. **Apply the standard junction patch to the nine dead forks** (bell-switchback, cullet-fork, pressure-fork, split-furnace, split-kiln, orbit-fork, tidal-fork, current-fork, split-root). Two-part edit per room: (a) move a coin onto the existing upper one-way platforms beneath the existing decorative spike cluster; (b) move spikes to flank the one-way lips of the floor-drop slot. This converts routing into decision at near-zero geometry cost — the platforms and spikes are already in the grids.

6. **Apply the standard cache patch to bell-niche, coin-duct, root-cellar, ember-vault, coin-loft.** Move each coin from the entry mouth to the far end of the room's built-but-unused structure: bell-niche's coin-18 to the bottom of the col-11/col-17 inner shaft; coin-duct's coin-24 below the spike strip past the platform ladder; root-cellar's coin-03 into a spiked floor alcove requiring platform landings; ember-vault's coin-30 behind a spiked descent; coin-loft's coin-9 into a spike-lined alcove off the left #-block column requiring a wall-jump.

7. **Give the "precision without stakes" climbs teeth: annealing-spire, cooling-duct, gear-gallery, vent-spire, foundry-seal, rafter-shrine.** All show 25–50% noise failure with *zero* deaths — retry friction, not challenge. One edit each: spike one shaft wall or the floor beneath the one-way ledges so a mistimed wall-jump costs a death, not a re-climb. Prioritize foundry-seal (guards coin 39 for the coins=40 gate) and annealing-spire.

8. **Give the pure-execution corridors a route decision: glass-threshold, temper-hall, relay-chasm, sliver-run, razor-pass.** Each has an unused upper two-thirds. Add a high alternate line (wall-jump ledges under the existing ceiling spikes) trading speed vs safety, and drop an optional coin on the riskier line in relay-chasm and rivet-run, which currently reward nothing.

9. **Retune three difficulty outliers against their floor position.** Needle Turn (~floor 18, ~50% noise deaths) — widen the chute by one tile or shift one spike pair. Cinder-bridge and skybridge — verify the *mandatory pass-through* (not just the coin detour) isn't carrying 50% noise death rates; if it is, add one safe landing pocket mid-room. Void-pass — widen one or two safe ## wall segments in the chute (33/64 correlated-timing deaths at floor 95 reads unfair even for endgame).

10. **Decide what gatehouse is and commit.** As the 64-coin threshold to the Crown it currently demands fiddly dash-reversal timing (47–52% noise success) with zero hazard — the worst of both. Either (a) breather: flatten the one-way stagger so it's a solemn walk to the gate, or (b) ceremony-with-teeth: put a spike floor under the existing right-side platform stagger so the final climb echoes Crown Sanctum. Given Astral Seal directly precedes it, (a) is probably right.