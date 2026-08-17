# Decision Log

Decision records mined from development transcripts, grouped by theme and
dated where the transcripts date them (most decisions cluster around
2026-08-14 through 2026-08-16). Reversals record both the original decision
and the reversal. Formal ADRs live in `docs/decisions/`; this log is broader
and includes decisions that never got an ADR.

Quotes are the designer's words unless noted.

---

## Technology and architecture

### Engine: Macroquad
**Decision.** Macroquad 0.4.15. **Rationale.** Lean, macOS-native,
pixel-perfect rendering. Bevy rejected as unnecessary size/complexity; ggez
rejected for macOS support issues. (See ADR 0001.)

### Lua adopted, then removed
**Original (early).** mlua 0.12 with vendored Lua as an *internal* content
language (room/trap definitions, generator rules), not player-facing modding:
"Primarily internal content I think... anything performance critical should be
in rust." Lua returned structured data only; Rust owned validation and
runtime types.

**Reversal (2026-08-14).** "BTW is the lua actually useful? Maybe we should
just rip it out given that we're going to be doing everything procedurally
generated in rust." → "Let's just remove it then yeah." The `downwards-lua`
crate, mlua dependency, and vendored interpreter were fully removed.
**Rationale.** Lua carried exactly one static room literal and no runtime
callbacks, and folding a VM into simulation cloning/hashing/replay work was
incompatible with the cheap-cloneable-simulation requirement the AI solver
depends on. The one room became a typed Rust constant. Documented as ADR 0003.

### Deterministic simulation core
**Decision.** Authoritative game state is fully separated from graphics,
devices, and wall-clock time. Fixed-tick simulation (60Hz) with integer
subpixels (1/256px): positions, velocities, timers, and game time never touch
floating point. Fixed tick order (input edges → intents → movement → hazards/
triggers/exits → events). Layered dependency direction core ← ai ←
client/tools. **Rationale.** Deterministic replays, AI rollouts, and
frame-perfect witness certification are the foundation of the entire
validation methodology.

### Multi-door tiling constraint (foundational, never reversed)
**Decision.** "Rather than having a single 'level exit', doors should be
placed on walls (or usually ceilings or floors), and the constraint should be
reachability of any door from any other door (possibly only with traversal
abilities). This means that we can tile the dungeon with these levels so that
they join up with each other." Replaced the original single-exit model with
2–4 boundary doors per room with matching sockets. **Rationale.** Seamless
metroidvania tiling without hand-authored junctions.

---

## Generation strategy

### Away from templates (2026-08-14)
**Context.** The v5 template generator produced critically low diversity
(e.g. 251 distinct layouts in 1,000 tier-1 seeds; 24 in tier 4), and the
designer found the output "far too samey".
**Decision.** "The generator really shouldn't be template based." Kick off a
generation research program: multiple prototypes (cyclic-graph,
reachability-growth, rhythm-weave; later graph-first "Compositional Route
Cut"), run through the AI, with data gathering on diversity and difficulty.
Unexplored cited as the methodological reference. This explicitly
countermanded any plan to just add parameter variation to templates.
**Follow-up.** The sameness was diagnosed as architectural, not cosmetic —
fixed builders with one-tile nudges — hence the pivot to compositional
grammars, constraint-based route embedding, offline overgeneration with AI
certification, and quality-diversity curation.

### Scale back obstacles until placement is proven
**Decision.** "Feel free to add new obstacles, but right now I'm not convinced
you're using the current ones well enough... scale back the obstacles until
it's clear that you can place them well, and gradually build up." The initial
corpus was built mainly from terrain and one-way platforms.

### Reversal: hand-crafted calibration gallery before more generation (2026-08-14/15)
**Context.** Automated difficulty metrics were shown to be unreliable (see
[Difficulty methodology](#difficulty-and-evaluation-methodology)).
**Decision.** "Build a gallery of about a dozen hand crafted levels that you
make yourself rather than trying to generate, and I give you feedback that you
can use to tune the next round of generation and evaluation." Procedural
generation was *paused, not abandoned* — to resume once metrics were
calibrated against human feedback. Generation remains "a tool, not the
definitive content source"; manual authoring is preferred for real dungeon
content for now.

### The validated content pipeline (2026-08-15/16)
**Decision.** Dungeon redesign runs as a human+AI pipeline: audit existing
levels → cluster into archetypes → invent novel classes → build rooms per
class against a solver-based auditor → assemble → adversarial critique →
refine. This produced the dungeon-v2 rooms the designer praised ("all the new
level designs are really good. Big fan") and is the standing process (also
recorded in project memory as the level-design pipeline; primary docs:
`docs/design/room-clusters-2026-08-16.md`,
`docs/design/redesign-integration-brief-2026-08-16.md`).

### Wall-jump corpus content excluded
**Decision.** Zero WallJump-tagged rooms shipped in the 596-room corpus.
**Rationale.** The physical-v2/v3 wall gates admitted replay-certified
Dash-only bypasses (rooms solvable without the claimed-required ability); the
v4 replacement fixed the bypass shape but constructed only 5/15 fixed cases
against a predeclared 12/15 admission threshold, so it never reached
promotion. Both v3 and v4 rejections are frozen as documented negative
evidence (`docs/validation/compositional-wall-gate-v3-rejected-2026-08-15.md`,
`...wall-chimney-v4-rejected-2026-08-15.md`); v2 remains the production
baseline. Enforced principle: an ability requirement must be **structurally
unavoidable across all reduced-loadout searches** — any reduced-loadout solver
success vetoes a claimed gate.

---

## Dungeon layout and progression

### Ability gates must be geometric
**Decision.** "I don't think portals should be locked by acquiring traversal
items... If you want to block off areas of the map by traversal, it should be
done by routes that are inaccessible (or just very very hard to access)
without them." Ability-keyed door locks are forbidden; the layout metrics
tool hard-fails on any `gate <room> <door> ability` directive.
**Softening.** "If there's a super challenging route without the traversal
item, that's a fun easter egg for extremely skilled players, not a bug" —
extreme solver-found bypasses are documented sequence breaks; easy/moderate
ones are design failures.

### Coin gates, tolerance, then late strictness
**Decision.** Coins unlock doors at escalating thresholds through the dungeon
(originally exact-match 6/12/18/19/21/28/40/52/64).
**Reversal 1 (gate tolerance).** Exact-match gating replaced with graduated
slack (1 skippable coin early, 2–4 mid-game, more later). **Rationale.**
Makes strawberry-difficulty coins genuinely optional instead of accidentally
mandatory; prevents soft-gating.
**Reversal 2 (Crown strictness).** Late in the project the Crown gate was
pushed back to full strictness: all 64 coins plus both Wall Jump and Dash,
verified by a mechanical regression that removes each floor individually and
proves progression becomes impossible — not by declarative tests.

### Progression rebuilt after structural breaks
**Context.** Playtesting/auditing found the ability arc was entirely optional
(a bare-handed spawn→crown route existed) and the Winged Boots were reachable
without the branches meant to gate them.
**Decision.** Both treated as confirmed blockers. Boots now require all 21
pre-Dash coins plus a four-wall-jump alternating climb; ability gates were
added on the critical path. "Make the boots the payoff for a real required
challenge, and turn the side regions into actual crown prerequisites rather
than optional scenery."

### Grid embedding and layout hard targets
**Decision.** The dungeon must embed consistently on a 2D grid (every edge
joins grid-adjacent cells via door direction, no shared cells) so the in-game
map can be exact; room instance IDs are preserved through rewiring for save
compatibility. Layout hard targets: 26–34 rooms, cycle rank ≥5, diameter ≤8,
≥2 backtrack-unlock events, zero same-class adjacencies, dead ends only at
vaults/goal, easy→hard difficulty trend, no class >1/3 of the dungeon,
multiple edge-disjoint spawn→goal routes — enforced by
`dungeon_layout_metrics.py`. Hard constraint: zero absorbing traps at every
loadout (see [open-questions-and-todo.md](open-questions-and-todo.md) for the
planned keyed-cul-de-sac relaxation).

---

## Difficulty and evaluation methodology

This is where the largest reversals happened; the full experimental detail is
in [research-log.md](research-log.md).

### Reject pixel-perfect AI play as ground truth
**Decision.** "The AI difficulty evaluation has pixel perfect play, which it
should not do, it should have some sort of 'shaky hand' measure where timing
and precision are both lightly randomised." Implemented as per-family
jitter-survival curves (BoundaryTiming, CorrelatedTiming, HoldRelease,
DropRepeatFrame). Note the sequencing: shaky-hand metrics were *deferred* on
2026-08-14 while levels were still too easy ("right now... you should assess
difficulty under perfect play"), then adopted as a hard gate from 2026-08-15.

### Robust comparative metrics over difficulty bands
**Decision.** "I'm not that fussed about banding. I'm more interested in
having relatively robust metrics of difficulty that let you tell ways in which
a level is clearly harder than another." Evaluation moved from a single
ComplexityBand scalar to preserved difficulty vectors and three separated
categories: challenge descriptors, fairness/quality, and operational
confidence — operational solver metrics are never conflated with player
difficulty.

### Judge the easiest route, not the existence of a hard one
**Context.** "It sounds like you're judging difficulty based on the existence
of a challenging route, not based on whether the best route is challenging.
Am I misunderstanding?" — confirmed as a real bug (a "Technical" tier
averaging 98% success).
**Decision.** Difficulty is assessed on the *simplest known successful*
replay: greedy replay simplification, simple-bypass probes (walk-only,
walk+few-jumps), and failure-to-find-simple treated as bounded evidence, not
proof.

### Separation of certificate, robustness, and human difficulty
**Decision** (after the gallery pivot): the AI witness is an exact
tractability certificate; shaky-hand curves measure controller robustness;
human difficulty comes from playtest feedback. The witness's tick/button
counts are explicitly *not* fed back into "hard".

### The 75% robustness bar
**Decision.** Worst shaky-hand family must clear 48/64 (75%) on mandatory
routes — the standing floor for all later room work (raised from a policy
that only flagged zero-success families). Below-bar rooms print FRAGILE and
get reworked. A hazard with 0 deaths under noise is decorative and must be
repositioned or removed. **Later refinement (recommendation, not fully
enforced):** rooms sitting exactly at 48/64 are fragile to any tuning change;
target 56/64+ on core content.

### Corpus scale
**Decision.** "Let's aim for more like a corpus of 500-1000 diverse levels"
(up from ~120). Final delivered corpus: 596 rooms. Catalogue standardized at
9 rooms per ability kit (3 per difficulty band; 6 was too tight to cover all
generator strategies).

---

## Movement feel

### Movement model: "Retained Snap"
**Decision.** From named prototypes: "Braking on 'Flow' feels definitely too
slow. Direct feels... 'artificial'... Loose feels sluggish. Retained Snap is
definitely the best." Locked movement policy v2 defaults: 110 px/s move
speed, 72ms acceleration, 203ms braking, 50% wall-ascent carry, 50% wall-jump
carry, 250ms wall memory. **Conflict note:** an earlier tuning round recorded
"120 is definitely the best speed", but 110 px/s is what shipped as the v2
default — per the prefer-later rule, 110 is authoritative.
(`docs/validation/player-movement-policy-v2-2026-08-15.md`.)

### Variable-height jump redesigned around robustness
**Context.** "The fastest possible tap I can make on the space bar does not
reliably result in a minimum height jump... The number of ticks is largely
irrelevant to me, the user, and is a pure implementation detail, and what
actually matters is robust and usable gameplay."
**Decision.** v1 (two-tick minimum hold) rejected; v2 shipped: immediate taps
give a stable one-tick (~6.8px) jump, buffered taps retain release intent,
holds up to 10 ticks add height, release clamps to jump-cut speed. HUD text
changed to "TAP/RELEASE JUMP EARLY FOR LOW • HOLD FOR HIGH". A
buffered-release consumption bug (fixed 16.9px jumps) was found and fixed.

### Wall jumps respond immediately
**Decision.** Wall-jump presses skip the ground jump's 100ms tap/hold
classification delay; a brief tap gives useful minimum ascent with a short
outward-commitment window. **Rationale.** Direct response to the jump-fix
regression making wall jumps feel sluggish. Related: wall-ascent momentum
conversion — a rising airborne player striking a wall converts 50% of
horizontal speed to upward speed, once per contact.

### Spike semantics tightened twice
1. Spike direction became authored data (`^v<>` variants) rather than
   inferred: "spike direction... should just be a feature of their placement."
   Point lethal, back/sides solid-safe.
2. Movement policy v5→v6: up/down spikes safe only from the back face;
   left/right spikes lethal front *and* back. Removed AI-only exploits
   (perching on spike sides). All witnesses regenerated.

### Coin banking
**Context.** "I don't think it's possible to get the Ember Vault coin safely."
**Decision.** Two-stage pickup: `PickupTouched` on overlap, `PickupCollected`
(banked) only on a grounded stationary landing or on exiting through a door;
death before banking restores the coin. AI success criteria require the
banked state, which guarantees the AI never returns control while unsafe.
**Hardening.** Coins never bank inside a timed-hazard rectangle even while
dormant: "A square inside a shifting hazard like the laser beam should never
be considered safe for coin pickup."

### Dash-squeeze / slide
**Decision.** Horizontal dash compresses the collider to fit one-tile gaps
("Dashing through it might be a nice feature"), auto-expanding when clear.
**Generalized (movement policy 8).** "When doing a long slide (dash into an
area you shouldn't fit in), the slide should continue until you escape" — the
compressed slide self-propels at ground speed until it exits the passage
instead of parking mid-passage. All witnesses regenerated.

### Other movement decisions
- Stamina-based climbing: "Skip stamina for now."
- Legacy physics paths deleted: `Option<MovementTuning>` removed; the sim
  always uses `MovementTuning::GAMEPLAY_DEFAULT`, with all downstream
  artifacts regenerated.
- Post-death input lockout (~0.4s) so a held direction can't carry the
  respawned player back through the entry door.

---

## UI/UX

- **HUD notice box.** First given dodge logic (slide away from the player);
  then fully removed after "That box really has to go. It makes the game much
  worse. You do actually need to be able to see the entire map to play this
  game." Transient notices moved to a one-line auto-expiring bottom rail
  outside the 320×180 view; the detailed panel appears only on pause.
- **Dungeon map** is an on-demand Tab overlay (BFS layout, explored rooms,
  gold dots for uncollected coins), never a persistent HUD element.
- **Floor lab flow** continues into the destination room through the matching
  door rather than returning to a flat gallery.
- **AI-assist mode** targets the coin first, then the nearest non-entry
  openable door, and returns control the instant a goal completes (and, per
  coin banking, never while unsafe).
- **Dungeon v2 is the default experience**: title screen
  (Continue/New Game/Controls/Quit), pause menu, controls documentation, and a
  victory screen — "a complete piece of game software", not a dev harness.
- **Level menus** got three-word procedural names, miniature previews,
  scrolling, and prominent ability-kit tabs after the designer reported the
  selection UI confusing and dash undiscoverable.

---

## Room design playbook decisions

Standing rules reaffirmed across dozens of room redesigns (authoritative doc:
`docs/design/room-iteration-playbook.md`):

- **ASCII grid authoring.** Geometry moved from Rust literals to
  one-character-per-tile grids in `rooms/*.txt` (`#` solid, `-` one-way,
  `^v<>` hazards, `.` empty); edits became one-character changes plus retune.
- **Hazards punish commitment/overshoot**, never single-frame jitter;
  undershoot lands safe, full commitment is what spikes punish.
- **One-way headroom**: exactly two empty tiles above every `-` tile —
  non-negotiable, eventually enforced programmatically.
- **Mandatory routes light, optional routes demanding**: through-routes pass
  the 75% bar; coin lines and alternate high routes may demand precision.
  Tutorial rooms must genuinely force the mechanic they teach ("Stone Lessons
  must actually force one wall-jump chain before Broad Chimney demands five").
- **"Valid but dull" is a rejection category**: passing the solver and the
  robustness bar is not enough — a room with no decision, no test, and no job
  is rejected.
- **Drop-through platforms cannot gate a mandatory descent** (down+jump always
  passes them); only ascents can be one-way-gated. This invalidated early
  "sealed chimney" designs.
- **Coin placement**: on mandatory crossing geometry or at the far end of
  built/tested structure; cache coins require a genuine commit whose failure
  costs something. Coin rect changes must land in **both** the grid file and
  `demo_dungeon.rs` (a repeated silent-staleness bug).
- **Room treatment taxonomy** to prevent scope creep: tutorial, ceremony,
  cache-guard, junction-decision, add-stakes, add-route-choice, soften,
  route-choice — one job per room. Example: the Gatehouse was deliberately
  made "ceremony, not test" (167 ticks, 64/64 robust, 3 jumps, no hazards) —
  its job is the 64-coin gate and a stately procession, not a challenge.
- **Validation protocol**: `--pair` on all ordered door pairs including
  self-pairs at minimum loadout (all "solved"), `--route` robustness at the
  48/64 bar, deaths nonzero wherever a hazard is claimed real.

---

## Workflow and process

- **Model allocation.** Large per-item agent fanouts run on Opus (or Sonnet);
  Fable is reserved for planning/synthesis — designer-requested for token
  budget reasons, applied to the 40-room redesign pipeline (40 Opus builders,
  Sonnet critics, Fable integration).
- **Pipeline shape.** Plan (Fable) → parallel room builds (Opus, tool-checked)
  → independent critic re-verification (Sonnet) → one fix round → integration
  brief (Fable). Supported by the `DOWNWARDS_ROOM_GRID_DIR` runtime override
  so grid edits don't force rebuilds.
- **No git during implementation.** Implementer agents may not run git
  commands (a stash collision once silently destroyed unrelated work); writes
  must be verified on disk (`cat`/md5) before a report is trusted — adopted
  after multiple fabricated-report incidents (see
  [engineering-notes.md](engineering-notes.md)).
- **Persistent plans.** "Can you make persistent notes of this plan somewhere
  so it doesn't get lost in context compaction?" → `docs/research/corpus-plan.md`
  and the other dated planning docs.
