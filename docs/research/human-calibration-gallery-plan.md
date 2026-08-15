# Human calibration gallery

Status: first-round gallery implemented and frozen for human feedback,
2026-08-15.

## Purpose

The generated corpus and its controller-derived metrics have not predicted
human difficulty reliably. In particular, **Still Flame Hall** was judged
trivial despite being ranked as a difficult corpus room. The hand-authored
**Needle's Eye** challenge was judged a useful high-end reference (slightly
too hard for the current tester), while **Stepping Stones**, intended as a
rough midpoint, was judged a good tutorial and much more than 50% easier.

The next calibration unit is therefore a gallery of twelve hand-authored,
Dash-disabled rooms. The gallery is an experiment for learning which concrete
geometric and control demands predict human experience. It is not a generated
corpus and its labels are not evidence that the current metrics are correct.

## Fixed anchors

1. **Stepping Stones** — accepted tutorial anchor: three broad wall contacts,
   one forgiving landing transfer, no low-ceiling early-release check.
2. **Needle's Eye** — above-target high-end reference: six narrow wall
   contacts, sparse tiny landings, and a ceiling check where the player must
   release Jump early to stay low. Its shaft is already too demanding for the
   current tester.

## First-round inventory

The remaining rooms vary burdens more independently than the failed midpoint
attempt. Public IDs are stable presentation identifiers, not a difficulty
order. Witness figures below are tractability/route-sanity diagnostics only.

| ID | Title | Primary contrast | Stored route facts |
| --- | --- | --- | --- |
| cal-01 | Stepping Stones | Human-accepted tutorial anchor | 136 ticks; 3 wall jumps; broad contacts/recovery |
| cal-02 | Open Chimney | Long continuous broad-wall climb | 80 ticks; 7 alternating wall jumps |
| cal-03 | Two-Tile Turn | Very short narrow-contact climb | 24 ticks; 2 alternating wall jumps |
| cal-04 | Even Tempo | Regular three-tile alternating bands | 69 ticks; 5 alternating wall jumps |
| cal-05 | Low Clearance | Generous climb plus one release-early low-jump check | 100 ticks; 3 wall jumps; full hold hits the ceiling hazard |
| cal-06 | Safe Harbor | Short climb plus recovery traversal | 121 ticks; 2 wall jumps; witnessed supports 5/3/4 tiles |
| cal-07 | Three Pins | Short climb with narrow contact windows | 34 ticks; 3 alternating wall jumps |
| cal-08 | Long Ascent | Longer sequence, broad contacts, recovery shelf | 68 ticks; 7 alternating wall jumps |
| cal-09 | Open Shaft | Moderate climb without intermediate recovery | 59 ticks; 6 alternating wall jumps |
| cal-10 | Broken Causeway | Alternating climb plus tiny landing chain | 155 ticks; supports 2/1/1 tiles |
| cal-11 | Low Bridge | Same climb/landings plus release-early low-jump gate | 155 ticks; full hold hits the ceiling hazard |
| cal-12 | Needle's Eye | Above-target high-end reference | 182 ticks; 6 wall jumps; narrow contacts/tiny landings |

Every room must have a stable native ID, a WallJump-enabled/Dash-disabled
loadout, one exact completion target, and a deterministic replay-certified
witness. Witness length, solver search effort, action transitions, and blind
open-loop perturbation failures are diagnostics only; they are not treated as
human difficulty scores.

## Certification boundary

For admission to the gallery, each room must:

- exact-replay a clean witness to its named exit under authoritative physics;
- contain no Dash or restart actions and no death in the retained witness;
- actually exercise its advertised wall-jump or timing mechanic;
- retain its authored collision rows and structural contrast in unit tests;
- reject the stored witness when WallJump is removed;
- run a bounded portfolio of simple controllers where practical, reporting
  positives honestly without interpreting a bounded miss as impossibility.

The retained witness is greedily simplified to remove obvious controller
thrashing. This prevents the earlier mistake of treating an unnecessarily
jumpy AI route as evidence that a room is hard.

During review, two first-pass witnesses did exhibit exactly that failure: one
used 11 wall jumps for a five-band climb, and another repeatedly hopped up the
same wall. Both were rejected and replaced with clean, vertically progressive,
alternating routes before the gallery was frozen. Regression tests retain the
actual wall-side/height traces and tiny landing supports.

## Playtest protocol

The in-game gallery presents mechanics and stable IDs, but not a predicted
difficulty order. Jump height uses the ordinary human control: tap or release
Jump early for a low jump and hold it for a high jump. The stored AI witness
has no secret low-jump action. For each room, collect:

- completed or abandoned;
- approximate attempts/deaths and completion time;
- perceived difficulty on a 1–7 scale;
- enjoyment on a 1–7 scale;
- readability: obvious route, discoverable route, or confusing route;
- dominant burden: timing, contact precision, landing precision, endurance,
  route reading, or something else;
- any trivial shortcut, frustrating section, or especially satisfying move.

Feedback is appended before changing a room. The first twelve-room round is a
calibration dataset; it should not be silently tuned while being evaluated.
After feedback, compare human ratings against structural facts and the existing
metrics, revise the evaluator, and only then tune the next generation round.

### First-round feedback sheet

Use the public `cal-01` through `cal-12` IDs shown by the client. Approximate
values and free-form reactions are more useful than false precision.

| ID | Completed? | Attempts/deaths | Difficulty 1–7 | Enjoyment 1–7 | Readability | Dominant burden / notes |
| --- | --- | ---: | ---: | ---: | --- | --- |
| cal-01 | Yes |  |  |  | Obvious | Good tutorial anchor (earlier session) |
| cal-02 | Yes |  |  |  |  | Very easy; plausibly appropriate for its broad-wall role |
| cal-03 | Yes |  |  |  |  | Quite hard in a good way; rapid direction changes dominate |
| cal-04 | Yes |  |  |  |  | Same demand as cal-03, intensified; good progression |
| cal-05 | Blocked |  |  |  |  | Suspected controls defect: AI's small hop may not be human-expressible |
| cal-06 | Yes |  |  |  |  | Trivial; intended role unclear to tester |
| cal-07 | Yes |  |  |  |  | Looks harder than it is; perhaps easier than cal-04, with order confound |
| cal-08 | Yes |  |  |  |  | Very easy |
| cal-09 | No |  |  |  |  | Could not complete; AI route appears pixel-perfect; skill confound possible |
| cal-10 | Yes |  |  |  |  | Chimney slightly tricky; causeway itself essentially trivial |
| cal-11 | Blocked |  |  |  |  | Same control problem as cal-05: required hop feels lower than human controls permit |
| cal-12 | No |  |  |  |  | Shaft is too difficult for the current tester; struggle begins before the later precision finish |

### First feedback tranche: 2026-08-15

The initial report covered `cal-02` through `cal-10`; a follow-up completed
`cal-11` and `cal-12`. It reinforces several distinctions that must be kept
separate during analysis:

- Rapid direction reversal between opposite walls produced useful difficulty
  in `cal-03` and a stronger version in `cal-04`.
- Route length alone did not: both broad-wall climbs (`cal-02`, `cal-08`) were
  judged very easy.
- Generous recovery/landing geometry (`cal-06`) was trivial, and the nominally
  narrow causeway after `cal-10`'s chimney was also trivial in practice.
- Visual threat was not reliable: `cal-07` looked harder than it played.
- `cal-09` is a possible upper-bound execution case, but “could not complete”
  does not yet distinguish intended precision, an accidental near-pixel-perfect
  requirement, or player-skill mismatch.
- `cal-05` is quarantined from difficulty calibration pending a control-path
  audit. An exact one-tick replay pulse is not valid evidence of human
  tractability, even though it uses the same input available to a human.
- `cal-11` independently reproduces `cal-05`'s human-control problem. Its
  low-ceiling contrast is likewise quarantined rather than counted as hard.
- `cal-12` remains useful as an upper-bound reference, but not as the desired
  high-end target: its shaft alone is already too demanding for the current
  tester, before its later landing sequence can calibrate anything.

The tester initially missed `cal-11` and `cal-12` because it was not apparent
that the gallery browser could scroll. This is a presentation failure, not
missing feedback. Input policy v1 introduced a current/total level position and
an explicit `MORE BELOW`/`MORE ABOVE` affordance; the current v2 client retains
both. Future galleries must keep them rather than relying on the player
discovering off-screen rows.

No room is changed in response to this tranche. The observations are retained
against the frozen first-round bytes before diagnosing controls or designing a
second round.

## First-tranche diagnosis

### Invalid or confounded entries

`cal-05` is not valid human-difficulty evidence. Its final jump succeeds when
held for only 1–3 physics ticks (16.7–50 ms at 60 Hz); holding for 4–10 ticks
hits ceiling hazard `(22,4)`. The client can technically emit a one-tick pulse,
including a press/release latched between render frames, but its duration is
phase-, frame-rate-, and hardware-dependent. Exact replay tractability was
therefore too weak an admission rule. A second-round variant should move that
hazard bank from row 4 to row 3: the measured window then becomes 1–6 ticks
(up to 100 ms), while 7–10 ticks still fail and preserve the contrast between
low and full-height jumps.

`cal-11` receives the same quarantine on direct human evidence. It was intended
as a second release-early low-jump calibration, but the tester again could not
access the small hop shown by the stored witness. Two independently authored
rooms now show that replay-certified short pulses are not an acceptable
human-control contract. The next round must admit a timing gate only after a
multi-tick input envelope is tested through the live client path, not merely
after one exact action sequence reaches the exit. A follow-up sweep measured
`cal-11`'s exact safe envelope at the shown onset as only 2–5 ticks
(33–83 ms): one tick underjumps into a floor hazard, while six or more ticks
hit the ceiling. The stored witness uses the minimum successful two-tick pulse.
This is technically expressible, but input/frame batching makes it an
unreliable human contract.

`cal-09` is also confounded. The displayed stored witness contains six wall
jumps, including a nearly level reversal from player y=119 to y=115. A separate
bounded search found a clean 69-tick route with only three progressive
alternating wall jumps at y=118, 88, and 54. The apparent pixel-perfect move is
an avoidable witness artifact, not a geometry requirement. The human failure
still says the presented room/route was unsuitable, but this room must not be
promoted to an upper-bound anchor without a route-discoverability retest.

`cal-12` is a valid tractability witness but an overly severe human target. The
tester reports that the shaft itself is already too hard, consistent with the
earlier judgment that the room sits above their desired high-end difficulty.
Future upper-mid and high rooms should reduce shaft contact/reversal pressure
before adding the tiny-landing finish; otherwise the compound room cannot tell
which burden caused the failure.

### Human jump-control audit

The intended variable-height mechanism is to release Jump early for a low hop
and hold it for a high jump. Space, Z, and Up all feed the same held boolean;
ground, coyote, and wall jumps share the same vertical curve. Authoritative
physics gives approximately 6.8 pixels of rise for a one-tick hold, 19.2 for
five ticks, and 30.4 for ten ticks, so the mechanic is real and substantial
when a jump begins immediately. The stored AI has no secret low-jump action: it
can merely schedule that same boolean at exact 60 Hz ticks.

The frozen first-round human-facing implementation had three defects:

- its persistent HUD did not say “release early for low, hold for high,” and
  its gallery copy used “jump cut” without defining it;
- live state is sampled once per rendered frame and repeated across fixed-step
  catch-up ticks, while only a press is queued, so short holds depend on frame
  phase and jitter; and
- a released-before-contact buffered jump consumes its release before the
  buffered jump fires. Ground and wall probes then rise about 16.9 pixels
  instead of producing the intended minimum hop.

#### Human jump-input policy v1 (historical)

Human jump-input policy v1 repaired the client boundary without altering core
physics, stored AI actions, room geometry, or first-round witness bytes:

- the HUD, command help, gallery cards, and documentation now say explicitly
  that tapping/releasing early gives a low jump and holding gives a high jump;
- aggregate keyboard press/release transitions are retained in order across
  render frames and consumed at fixed-step boundaries, including a release and
  re-press that occur before any simulation tick;
- a physical tap emits a stable two-tick minimum (about 10.2 pixels for an
  immediate jump), which the action-level sweep predicted would lie inside
  both frozen low-ceiling success windows;
- an early release remains held until a buffered jump is accepted or its buffer
  expires, with one post-accept ascent tick retained before the cut; and
- F1 exposes remaining jump-boost ticks, the two-tick human tap policy, and its
  version.

The v1 retry then supplied decisive human evidence that the isolated sweep had
missed: its stable two-tick minimum, about 10.2 pixels of rise, was still too
high for the tester to clear `cal-05` **Low Clearance**. That result superseded
v1 as the current policy even though two ticks had appeared to lie inside the
room's exact action-level window. It remains part of the experiment history;
neither the frozen room nor its stored witness was changed to conceal the
failure.

#### Human jump-input policy v2 (historical)

Human jump-input policy v2 changes only the live keyboard-to-action adapter:

- a physical immediate tap emits a stable one-tick minimum, producing about
  6.8 pixels of rise;
- a buffered tap's early release remains pending until the jump is accepted;
  if acceptance occurs after movement, one following ascent tick is retained
  before applying the release;
- continuing to hold Jump still exposes the ordinary full variable-height
  window; and
- core physics, stored AI actions, room geometry, exits, and frozen witness
  bytes remain unchanged.

The two low-ceiling rooms intentionally test different parts of this policy.
`cal-05` should now expose the stable shortest immediate tap that v1 could not
produce. `cal-11` **Low Bridge** is not intended to accept that shortest tap:
one tick underjumps into its floor hazard, while a deliberately slightly longer
2–5-tick hold clears the authored window and six or more ticks hit the ceiling.
Failure of a one-tick tap in `cal-11` is therefore not evidence that v2 lost
variable jump height.

The gallery should be retried under input policy v2 and its results recorded
separately from both the frozen first-round run and the v1 retry. Even if the
low-ceiling rooms become playable, they remain evidence about the repaired
input contract rather than clean difficulty measurements. A safe nonlethal
tutorial should still demonstrate one low and one high jump before timing gates
enter a scored round.

#### Human jump-input policy v3 (historical)

The tester's persisted Low Clearance attempts showed that v2 still exposed
render/simulation scheduling as gameplay: ordinary quick physical taps were
delivered as different-length held inputs, and the shortest intended gesture
did not reliably select the shortest jump. Policy v3 moves that distinction to
the human-input boundary. Releasing within 100 ms commits one semantic low-jump
gesture; continuing to hold commits ordinary variable-height held input. A low
gesture remains a single intent while waiting in the jump buffer and releases
immediately after acceptance.

The 100 ms threshold is an input affordance, not a level-design measurement.
Level validation must ask whether the semantic low and held gestures work over
reasonable approach and release variation. It must not certify a room merely
because an exact per-update pulse succeeds. The frozen AI witnesses and core
action semantics remain unchanged so this UI repair does not rewrite historical
corpus evidence.

Safe Harbor play under v3 exposed the cost of delaying every jump until the
tap/hold classifier resolved: 35 recorded attempts contained 108 grounded and
12 buffered-ground acceptances but only five wall-jump acceptances. The player
reported that wall jumping felt sluggish, matching the retained input evidence.

#### Human jump-input policy v4 (current)

Policy v4 retains v3's robust wall-clock low gesture for ground and coyote
jumps, but wall-jump presses are immediate. Live human simulations additionally
remember recent wall contact for six simulation updates, commit four updates of
initial ascent, and preserve away-from-wall steering for nine updates. These
numbers are implementation diagnostics; the player-facing contract is simply
that a brief wall-jump tap produces a useful, prompt launch and cannot be
cancelled accidentally by the approach direction.

The assist policy is explicit and digest-domain-separated. Historical stored
AI/corpus replays continue to run under legacy exact action semantics. New
human-viability validation must opt into the live policy rather than assuming a
legacy exact witness proves that the controls are comfortable.

### Current human-difficulty hypotheses

- Broad regular climbs are easy despite length: `cal-02` and `cal-08` both
  contain seven wall jumps and were judged very easy.
- Local reversal cadence matters more than total move count. `cal-03` was hard
  with only two wall jumps; repeating that demand made `cal-04` a useful
  progression.
- Contact-pad height alone is insufficient. `cal-03` and `cal-07` both use
  two-tile contacts, but their stored reversal intervals are roughly 9 ticks
  versus 12–14 ticks and they did not feel equally hard.
- Recovery and passive traversal sharply reduce burden. `cal-06` was trivial;
  total witness duration mostly measured locomotion.
- Raw support width is not landing precision. `cal-10`'s 2/1/1-tile chain is
  traversed by walking off and using the five-tick coyote window, so its
  causeway was forgiving despite looking narrow.
- Visual threat should be measured separately from execution demand:
  `cal-07` looked harder than it played.

Completion ticks, total jumps, total transitions, visual hazard density, and
raw platform width must consequently be downweighted as human-difficulty
features. More promising local measurements are time available before a
required reversal, input hold/release margin, approach-specific landing
margin, recovery cost after failure, and route discoverability.

### Proposed second round

Use six short matched pairs, randomized rather than ordered by an expected
difficulty, and hide the stored witness until after the first human attempt:

1. Identical two-transfer room, changing only contact height: 2 versus 4 tiles.
2. Identical contacts/count, changing only reversal time: about 9 versus 13 ticks.
3. Identical motif, changing only sequence length: 2 versus 5 transfers.
4. Collision-identical successful route, changing only a recovery catcher.
5. One isolated landing, width 1 versus 3 tiles, with coyote bypass prevented.
6. A separate, non-scored control-accessibility ladder for release-early low
   jumps, with every hold from one tick through a declared 4-, 6-, or 8-tick
   ceiling succeeding. Establish a usable envelope before treating
   early-release timing as difficulty; a full 10-tick hold may still fail to
   preserve the contrast.

Collect anticipated difficulty before playing as well as post-play difficulty,
particularly for visually threatening rooms. Record actual human hold durations,
inter-jump intervals, failure section, and retry cost. This round should not
combine a climb and a second mechanic when a short single-factor room suffices.
Needle's Eye should likewise be split into shaft-only and landing-only
variants, with a graduated shaft series below its current upper-bound demand.

## Input-policy-v1 gallery retry (historical)

The first-round rooms, collision, targets, and stored witness actions remain
frozen. The v1 retry was a separate feedback tranche because a physical tap
deterministically emitted two fixed-step Jump ticks and buffered taps retained
their release intent until the jump was accepted.

The retry used `cargo run -- --gallery`; its HUD and F1 diagnostics identified
human input policy v1. The tester found that v1's two-tick, approximately
10.2-pixel minimum was still too high for `cal-05`. This result is retained
instead of rewriting the v1 tranche after its supersession.

## Input-policy-v2 gallery retry (historical)

The same frozen rooms now run under human jump-input policy v2. Launch with
`cargo run -- --gallery`. During play, tap/release Space, Z, or Up early for a
low jump and hold it for a high jump. An immediate tap emits one fixed-step Jump
tick (about 6.8 pixels of rise). A buffered tap keeps its release intent through
acceptance; when acceptance occurs after movement, it also gets one following
ascent tick. F1 identifies human input policy v2.

For `cal-05`, test the shortest immediate tap. For `cal-11`, deliberately hold
slightly longer: its intended safe window is 2–5 held ticks, not the one-tick
minimum. Record this retry separately from both earlier tranches, including the
actual hold duration and whether the jump was immediate or buffered.

## Input-policy-v3 gallery retry

The same frozen rooms now run under human jump-input policy v3. Launch with
`cargo run -- --gallery`. A quick physical release within 100 ms selects the
low gesture; keep holding beyond that boundary for a high jump. Record whether
the gesture felt responsive and repeatable rather than reasoning about physics
ticks. Low Clearance and Low Bridge remain specifically under review: success
of a stored exact witness is not enough to call either human-viable.

The client now appends every completed human attempt to
`playtest-history/human-attempts-v1.jsonl`. The input evidence deliberately
keeps two clocks separate: per-key `sampled_duration_us` and
`sampled_frames_held` report the Space/Z/Up states observed at render-frame
boundaries, while `delivered_jump_spans[].ticks` reports the contiguous Jump
ticks received by authoritative physics. Accepted jump kinds and the complete
per-tick action/state/event-digest history are retained in the same record.
The render samples are not claimed to be operating-system event timestamps;
comparing them with delivered ticks is precisely the diagnostic for frame-phase
and catch-up quantization. The JSONL row is flushed after death, manual reset,
success, or wrong-door completion. `--history PATH` selects a different file.

## Input-policy-v4 wall-jump retry

Policy v4 keeps the successful v3 ground-jump gesture unchanged and separates
wall-jump intent from it. A wall-jump press is delivered immediately; recently
lost wall contact remains eligible briefly; and acceptance supplies a minimum
useful full wall-jump ascent with a short outward horizontal commitment. This is intended to
make a natural brief wall-jump tap responsive without turning a delayed ground
tap into an unwanted full jump. The values are diagnostic implementation
parameters, not mechanics the player should need to count.

Safe Harbor is the primary retry target. Its policy-v3 history contained 35
deaths and 165 presses, but only five accepted wall jumps, while 108 presses
became grounded jumps and 12 became buffered landing jumps. That mismatch is
the reason for changing the control contract rather than weakening the room.
New history rows identify policy v4, allowing the before/after comparison to be
made from accepted jump kinds and outcomes rather than inferred button ticks.

## Player movement policy v2 promotion

The movement-lab values selected by the tester are now the game-wide defaults: 110 px/s top speed,
72 ms acceleration, 203 ms braking/reversal, 50% rising wall-impact carry, 50% wall-jump carry, and
250 ms recent-wall memory. Live rooms and new AI solves use the same policy. The tester's first
broad playthrough found the game substantially easier, probably mainly because of speed; small
platforms were slightly harder under the chosen braking but remained acceptable. A fractionally
shorter braking response is recorded as a possible later experiment, not applied now.

This promotion ends the practice of manually pasting new solver tick counts into content and tests.
The 15 gallery witnesses live in the generated, policy-bound
`crates/downwards-content/generated/calibration-witnesses-v2.txt` artifact. The deterministic
`retune_gallery` example runs the game-playing AI, replay-validates and simplifies candidates, and
is the only normal process that rewrites that file. Tests validate the artifact, clean completion,
and authored mechanic/geometry contracts without duplicating generated route coordinates.

Historical catalogue and corpus actions are not current-policy play evidence. Until their formats
and builds bind this movement version, the client uses fresh current-policy AI solves for V/C while
preserving old artifacts for historical verification.

Re-running the finite baseline portfolio under the same movement also found an `AutoJump` positive
for `cal-06` Safe Harbor. Its displayed WallJump witness remains a valid mechanic demonstration,
but the room is not ability-gated under policy v2. That is consistent with the tester's “trivial”
assessment and further evidence that evaluation must search simple alternatives under the exact
player movement before making a difficulty or required-mechanic claim.
