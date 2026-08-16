# Room Archetype Clusters — 2026-08-16

Clustering of all 101 rooms in `docs/design/room-descriptions-2026-08-16.json`, grouped by how the rooms actually play (structure + movement verbs + hazard usage), not by name. Motivating critique: *"many of the levels are more than a bit samey... it felt like I was repeating the same levels over and over again."* The data supports the critique: the descriptions flag 44/101 rooms as high sameyness-risk and 46 as medium, with only 11 low.

## Clusters

### 1. Spiked wall-jump chimneys (alternating-face ascents) — 18 rooms
The dominant "challenge room" template: a horizontal traverse is interrupted by a narrow vertical chimney whose facing walls carry anti-phased spike bands, forcing a left-right-left wall-jump rhythm; you top out, often grab a coin, then descend an open chamber on one-way platforms. The room-to-room variation is almost entirely in which rows the spike bands sit on — the verb sequence (run in, wall-jump chain with side swaps, drop out through platforms) is identical. `refraction-shaft`'s own description admits it is "shared with nova-niche, zenith-shaft, lattice-climb."

Members: annealing-spire, aurora-spire, boots-vault, crucible-climb, foundry-seal, glass-gallery, glass-seal, lattice-climb, lunar-cache, nova-niche, observatory, refraction-shaft, shadow-duct, star-threshold, tempo-hall, vacuum-gallery, void-pass, zenith-shaft.

Verdict: **over-represented.** The best five or six (aurora-spire, zenith-shaft, void-pass, observatory, boots-vault, annealing-spire — all quality 4) each add a real twist (dash transfer, roof traverse, ability reward, read-ahead puzzle). The remaining dozen are the same climb with reshuffled spike rows; this cluster alone is a fifth of the dungeon and is the single biggest driver of "I've done this room before."

### 2. Flat spike-floor hop/dash corridors — 19 rooms
A horizontal ground-level transit: solid floor, one to three up-facing spike beds (occasionally a full spike floor with platform islands), doors at floor level on both side walls, and a vast empty upper two-thirds. Verbs: run, jump, sometimes one dash. Most exist purely to connect two other rooms; the descriptions repeatedly note the hazard is "clearable with an ordinary running jump" and the vertical space is "decorative."

Members: alloy-threshold, broken-aqueduct, comet-run, dash-chasm, dash-seal, gale-landing, glass-threshold, hammer-hall, landing-chain, low-passage, piston-pass, prism-run, pulse-gallery, razor-pass, relay-chasm, rivet-run, sliver-run, sluice, temper-hall.

Verdict: **badly over-represented — the core filler cluster.** 15 of the 19 are quality 1–2 and high sameyness-risk. comet-run, dash-seal, and prism-run at least commit to their gauntlet; the rest are interchangeable. Halving this cluster (or giving each survivor a distinct hook) is the highest-leverage cut.

### 3. Dead-end coin vaults (drop-in / climb-back-out) — 18 rooms
A single-door pocket, usually entered through the ceiling or floor, holding one or two coins: drop/fall in, hop a scattered staircase of one-way platforms (occasionally a spiked shaft), grab the coin, retrace your steps out the same door. Failure typically costs nothing. Many contain large sealed or unused chambers.

Members: ash-cache, bell-niche, coin-duct, coin-loft, cooling-duct, cullet-cache, ember-vault, lens-niche, mirror-duct, moon-vault, needle-room, rafter-shrine, root-cellar, shard-vault, spark-niche, storm-cache, treasury, watch-post.

Verdict: **over-represented.** 18 optional coin errands is far more than the reward economy needs, and 12 of them are quality 1–2. shard-vault and moon-vault (quality 4, real lethality on the way in/out) show what the archetype can be; coin-loft, root-cellar, watch-post et al. are unguarded closets. Keep ~6, make the coin worth the trip in each.

### 4. Flat drop-slot junctions — 10 rooms
A walkable bottom band linking west and east doors, plus a one-way slot in the floor as a third exit — the whole "room" is a routing decision taken in about three seconds. Decorative spikes or a token hop at most; the upper story is scenery. Several descriptions use the identical phrase "the upper two-thirds is dead space."

Members: bell-switchback, cullet-fork, current-fork, eclipse-fork, orbit-fork, pressure-fork, split-furnace, split-kiln, split-root, tidal-fork.

Verdict: **over-represented and near-clones of each other.** All ten are quality 1–2 and high sameyness-risk. The dungeon needs junctions, but not ten of the same one; two or three memorable hubs would do the same topological work.

### 5. Platform-ascent junction halls — 12 rooms
Open arenas that are simultaneously junctions and vertical connectors: a flat side-to-side band plus a zigzag staircase of one-way platforms rising to a ceiling door, sometimes over a spike bed that only punishes falls. Verbs: run, jump, drop-through, precise-landing; hazards rarely gate anything.

Members: foundry-fork, furnace-lift, gravity-lift, lantern-gallery, lift-shaft, mirror-fork, needle-turn, split-spire, storm-split, underpass, vent-spire, wall-gallery.

Verdict: **moderately over-represented.** More playable than cluster 4, but the one-way-platform zigzag is the same move set every time. furnace-lift's committed lateral offsets and split-spire's branching descent are the strongest; several others (mirror-fork, vent-spire, lantern-gallery) are interchangeable.

### 6. Optional coin-climb arenas — 6 rooms
A trivial floor-level crossing with the actual content hanging above it: an ascending relay of small platforms (often dash-gapped, sometimes capped by ceiling spikes) leading to a corner coin. The floor route ignores everything.

Members: blast-gallery, brake-tower, cinder-bridge, crystal-bridge, hot-glass, windshaft.

Verdict: **about right in count, uneven in quality.** cinder-bridge and hot-glass (quality 4) make the climb a genuine dash puzzle; brake-tower is filler. The archetype is fine — it just overlaps heavily with clusters 2 and 5 when the climb is toothless.

### 7. Breathers, hubs and ability/ceremonial rooms — 8 rooms
Deliberately hazard-free spaces: the spawn antechamber, the four-way Crossroads hub, ability vaults (climbing gloves), the teaching antechamber, the ziggurat gatehouse, and a couple of pure connectors.

Members: climber-vault, crossroads, gatehouse, hollow-landing, moss-walk, old-lift, threshold, wall-antechamber.

Verdict: **roughly right.** Pacing rooms are supposed to be calm; gatehouse and crossroads have real identity. hollow-landing and old-lift (quality 1) are empty even by breather standards and could merge into neighbours.

### 8. Signature set-pieces (mixed mandatory climbs, gates and the finale) — 6 rooms
Rooms where multiple mechanics genuinely interlock: a spike-floor crossing that turns vertical mid-room, an over-the-roof dash commitment, a wall-to-wall dash relay, the only timed room in the game, and the terminal crown circuit.

Members: astral-seal, constellation-hall, crown-sanctum, meteor-run, skybridge, starwell-climb.

Verdict: **under-represented.** These six (all quality 4, mostly low sameyness-risk) are what the whole dungeon should feel like more often. meteor-run in particular is a category of one — the only room with any timed element.

### 9. Plain up-and-over chimney transits — 4 rooms
Hazard-light vertical detours on a horizontal route: a full-height divider forces a wall-jump climb and a staged descent, with no or minimal spikes. Effectively the training-wheels version of cluster 1.

Members: broad-chimney, crosswind-chimney, gear-gallery, wall-gate.

Verdict: **acceptable count**, but three of the four blur into cluster 1 in memory. wall-gate earns its place as the explicit ability-check barrier.

*(Cluster membership totals: 18+19+18+10+12+6+8+6+4 = 101.)*

## Summary

### Bloated clusters (interchangeable filler)
1. **Flat spike-floor corridors (19)** — 15 of 19 are Q1–2/high-risk. The dungeon's largest block of interchangeable rooms.
2. **Spiked wall-jump chimneys (18)** — a strong archetype repeated ~3x more than it can bear; the middle dozen differ only in spike-row placement.
3. **Dead-end coin vaults (18)** — two-thirds are unguarded errands with sealed dead volumes.
4. **Flat drop-slot junctions (10)** — ten near-identical three-second rooms; all Q1–2.

Together clusters 2, 3 (weak members), 4 and the weak halves of 1 and 5 account for roughly **45–50 rooms that a player experiences as repeats**. That is the sameyness critique, quantified.

### Standouts worth keeping as-is
All 11 low-sameyness-risk rooms, every one quality 4: **astral-seal, aurora-spire, boots-vault, crown-sanctum, meteor-run, observatory, shard-vault, skybridge, wall-gate, void-pass, zenith-shaft.**
Strong seconds (Q4, medium risk — keep, maybe sharpen): annealing-spire, cinder-bridge, constellation-hall, hot-glass, lattice-climb, lunar-cache, moon-vault, nova-niche, starwell-climb.

Quality distribution overall: Q1 ×9, Q2 ×37, Q3 ×36, Q4 ×19, Q5 ×0 — nearly half the dungeon sits at Q1–2, and nothing reaches Q5.

### Barely-explored mechanical dimensions (quantified)
- **Timed/dynamic hazards: 1/101 rooms** (meteor-run's three phased shutters). Every other hazard in the game is a static spike. The descriptions say "no timed elements" as a refrain in ~70 rooms.
- **Hazard variety: effectively one hazard type.** 100/101 rooms use only static spikes (up/down/side); no moving platforms, crumbling blocks, wind, projectiles, or enemies appear anywhere in the data.
- **Crawl/duck/squeeze geometry: 8/101 rooms** (astral-seal, crown-sanctum, crucible-climb, low-passage, shard-vault, skybridge, wall-gate, zenith-shaft), and always as a single one-beat moment, never a sustained low-clearance passage.
- **Loop/circuit layouts: 4/101 rooms** (crown-sanctum, moon-vault, observatory, tempo-hall). Almost every room is a line (A→B) or an out-and-back; rooms you traverse differently on a second visit essentially don't exist.
- **Genuine multi-route rooms: ~0.** 21 rooms branch topologically (junction forks), but no room offers two meaningfully different *solutions* to the same crossing (e.g. a risky fast line vs a safe slow line with different rewards).
- **Designed descent: rare.** One-way drop-through platforms appear in ~70 rooms, but descent is almost always a free-fall cooldown after a climb; only crosswind-chimney, shard-vault and moon-vault treat going *down* as the challenge.
- **Dead space: ~75 rooms** explicitly describe empty/unused/decorative volume — often "the upper two-thirds" — which both wastes the 32×18 canvas and makes distinct rooms read alike.
- **Hazard-free rooms: 24/101**, far more than pacing requires once the 10 flat junctions and 18 coin closets are counted alongside the 8 intentional breathers.
