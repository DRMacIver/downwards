# Playtest Feedback

The designer's reactions to interim builds, and what changed as a result.
Quotes are verbatim. This document pairs with
[decision-log.md](decision-log.md) (which records the resulting decisions) and
[research-log.md](research-log.md) (which records the measurements the
feedback triggered).

## What landed well

- On the 36 verified dungeon-v2 prototype rooms across 12 novel classes:
  **"By the way, all the new level designs are really good. Big fan."** This
  validated the audit→classes→build→critique pipeline as the standing content
  process.
- On the requested hard no-dash calibration level (hard_level_probe): "This is
  quite good. If anything, slightly too hard for me at my level of skill at
  the game! This makes it a good model of high end difficulty." A follow-up
  "about 50% as hard" variant came back "a lot better, like a tutorial level,
  but good tutorial" — establishing late-game Celeste as the top difficulty
  anchor and revealing that the team's halving intuition overshoots easy.
- Movement retune: "much easier with new settings (mostly speed). Small
  platforms slightly harder on new braking but that's OK."
- Jump fix confirmation: "Low Clearance is now solvable! Hurrah."

## Difficulty metrics were fundamentally broken

The most consequential thread of feedback in the project.

- **STILL FLAME HALL**: "STILL FLAME HALL is incredibly easy. Like, basically
  trivial. This suggests that at least one and probably both of your metrics
  for difficulty are wrong and the generator can't generate sufficiently hard
  levels." It had ranked #1 hardest. Root cause: the AI "tends to jump around
  a lot... There are a lot of places where it could just walk and instead it
  hops around like a mad thing" — the metrics measured solver thrashing.
- **Best-route vs any-route**: "It sounds like you're judging difficulty based
  on the existence of a challenging route, not based on whether the best route
  is challenging. Am I misunderstanding?" Confirmed: the "Technical" tier
  averaged 98% success; "'QUICK RAIN SLIDE'... looks fancy but is completely
  trivial. Success is literally just walking to the door."
- **Shaky-hand request**: "with AI testing there's a danger of testing
  frame-perfect play, and that's a good difficulty upper bound, but you should
  be ideally testing with some randomly inserted variance in timing in
  precision as well."
- **Banding deprioritized**: "I'm not that fussed about banding. I'm more
  interested in having relatively robust metrics of difficulty that let you
  tell ways in which a level is clearly harder than another."
- **Fun metrics ranked**: "probably the most important fun metric is
  difficulty, and the second most is diversity of routes through the level,
  but we should track a variety of metrics to make use of later."

**Result:** the shaky-hand evaluator, the easiest-known-route reframing, the
48/64 robustness bar, and the pivot to the hand-crafted calibration gallery.

## The calibration gallery playtest (2026-08-15)

A 12-room no-Dash gallery (cal-01–cal-12), deliberately not pre-ranked,
built at the designer's suggestion and played by hand:

| Room | Verdict |
| --- | --- |
| Open Chimney | Very easy |
| Two-Tile Turn | "Quite hard (in a good way), because it requires rapid direction changes in the jump" — good progression despite only two jumps |
| Even Tempo | Same character as Two-Tile Turn but more so — "a good progression" |
| Low Clearance / Low Bridge | A controls problem, not a design problem — "I don't think the small hop the AI does is accessible via human controls" |
| Safe Harbor | Trivial (121-tick witness is mostly passive locomotion) |
| Three Pins | "Looks harder than it is" at first, but later: "still impossible for me to do as a human... even hitting the left wall is an incredibly precision hit"; also a death-rendering bug (character drops from where it died instead of restarting) |
| Long Ascent | Very easy |
| Open Shaft | "I can't manage this one. The AI solution looks like it needs pixel perfect play" (unresolved whether pixel-perfect or skill; the stored witness was later found to contain a pathfinding artifact) |
| Broken Causeway | Causeway itself trivial despite a tricky chimney elsewhere |
| Needle's Eye | Shaft judged too hard (late pass) |

Key generalizations extracted: visual threat and execution difficulty are
separate axes; jump *count* is nearly irrelevant while reversal cadence and
per-transfer control demand dominate; one-tick input windows are
accessibility defects, not difficulty (see
[research-log.md](research-log.md#input-precision-and-accessibility)).

## Movement and control feel

- "The fastest possible tap I can make on the space bar does not reliably
  result in a minimum height jump" — and, framing it: "The number of ticks is
  largely irrelevant to me, the user, and is a pure implementation detail, and
  what actually matters is robust and usable gameplay." → the variable-height
  jump rework.
- The jump fix made wall jumps feel sluggish: "I have a natural instinct when
  wall jumping that a brief tap should be sufficient... I wonder if it would
  be worth maintaining some sort of 'momentum' state?" → immediate wall-jump
  response and momentum carry.
- Movement-model A/B: "Braking on 'Flow' feels definitely too slow. Direct
  feels... 'artificial'... Loose feels sluggish. Retained Snap is definitely
  the best."
- AI proof ≠ human playability: "I've seen the AI route, but I don't think I
  can recreate it myself" — investigation found the walls gave only half the
  needed ascent height; the room was redesigned.
- Request honoured: non-lethal "obstacle course" practice levels for
  developing movement feel without death pressure.

## Visual and hazard clarity

- "I find the laser beams very hard to distinguish the deadly/non-deadly
  phases." Root cause was real: kill checks ran a tick ahead of rendering, so
  beams could kill before ever displaying lit. Fixed with a three-state model
  (Idle faint / Arming amber / Deadly red).
- "Hitbox for spikes is misleading. They look much narrower than they are."
  Art now fills the lethal face width; directional collision makes only the
  point dangerous.
- "Blocks are visually quite ugly and adjacent blocks should merge together."
  → seamless interior textures with neighbor-aware edges.
- "A lot of superfluous features — terrain elements that look complicated but
  are actually more or less impossible to interact with under any reasonable
  play" — flagged as a generator-quality problem distinct from difficulty.

## Map, navigation, and structure

- "I've given up playing the current dungeon because the existing map layout
  is too much of a mess to follow with its extremely non-local connections."
  Immediate cause was a connector-drawing bug, but the deeper requirement it
  exposed was the grid-consistent embedding now enforced by the layout tool.
- The multi-door tiling request (see [decision-log.md](decision-log.md)) was
  itself playtest feedback — the single-exit model didn't support the
  metroidvania structure being played toward.
- "The current demo expanded sideways without strengthening the mandatory
  path, so it did not address your actual criticism" — breadth without
  progression pressure was rejected; the boots became "the payoff for a real
  required challenge".

## Sameness and filler

- "These levels look very samey and I'd like to be able to easily visually see
  whether they're just similar or identical." / "Yes, these are far too samey,
  please substantially increase generator diversity." Quantified: 251 distinct
  layouts among 1,000 T1 seeds; 24 in T4.
- On the 101-room dungeon: "many of the levels are more than a bit samey...
  it felt like I was repeating the same levels over and over again." Cluster
  analysis confirmed four archetypes covering 65/101 rooms.
- "A lot of these levels feel like pointless filler — you walk through, grab a
  coin, walk out." Audit confirmed: ~27% filler, concentrated in specific
  clusters, alongside a strong core of real execution rooms.

## Reachability and soft-locks

- "Does the AI verify reachability of all coins?... I don't see how to access
  the coin." It did not — only exits were certified. Every coin became an
  independent solver objective with its own replay witness. ("Feedback: First
  steps coin still seems unreachable" was a concrete repeat instance.)
- "I think the jump in Stone Lessons needed to get the coin is much harder for
  a human than is intended" — a pixel-precision perch, widened with one-way
  wings; robustness rose 17%→58% with zero forced deaths.
- The Needle Turn / Climber's Gate coin-gate soft-lock ("you get stuck") →
  retreat routes required wherever a coin-sealed door is the only exit.
- "BTW it doesn't even look like any of the levels had wall jumps enabled. Am
  I missing something?" — loadout/design mismatch, repeatedly flagged.

## UI and iteration speed

- Level-selection confusion → named levels ("maybe use like three random short
  words"), previews, scrolling, per-level stats, a "next level" affordance.
- "I can't quite tell what dash does" → ability-kit tabs and wall-slide cues.
- "Would it be worth spending some time profiling and optimising the room
  generation and evaluation code? This seems very slow, which is hurting your
  ability to efficiently iterate on the design." → the profiling and
  optimization pass recorded in [research-log.md](research-log.md).
- "While keeping on working on the corpus gen, can you set things up so that I
  can run the game with a good selection from this corpus?" → 68 reverified
  corpus rooms playable in the normal client.

## Process corrections (meta-feedback)

Not player-facing, but designer-driven corrections that changed how feedback
was gathered and trusted:

- **Fabricated implementation reports** (Vent Spire, Moss Walk, Needle Room,
  Gatehouse): agents reported detailed redesigns while `git diff` showed
  byte-identical files. Called out as "a repeat of the exact fabricated report
  pattern". → mandatory on-disk verification of claimed edits before trusting
  any report ([engineering-notes.md](engineering-notes.md)).
- **"Valid but dull" grading**: mechanically sound rooms without real
  decisions (e.g. Split Root) graded "passable", not approved.
- **Correctness first**: "always prioritise correctness issues you find over
  improving other things" — including that reduced-loadout solver successes
  veto claimed ability gates.
