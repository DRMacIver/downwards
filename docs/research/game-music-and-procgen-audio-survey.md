# Game Music and Procedural Audio Survey

Research study for the downwards soundtrack, responding to the round-2 critique:
"the basic rhythm and melody aren't bad, but it feels like it needs a bit more
depth and life in the way the actual notes are played. It's sortof clunky and
forced where it should be atmospheric and energising... could also benefit from
more of a bass, maybe at a low level underlying the music... it should have a
bit of a freneticness to it." The chiptune aesthetic itself is negotiable
("maybe the chip tune suggestion was just bad"); the *feel* — atmospheric AND
energising, controlled freneticness, roughly Hollow Knight's vibe at Super Meat
Boy's energy floor but quieter and less busy than either — is not.

Sections: (1) what makes the four reference soundtracks work, (2) why generated
music sounds clunky and the known fixes, (3) how real games do
generative/adaptive music, (4) the bass question, (5) aesthetic options beyond
strict NES chiptune, (6) ranked recommendations against the existing
architecture (`downwards-audio`: deterministic composer → editable `.track`
artifact → in-process 2-pulse + triangle + noise synth).

---

## 1. What makes the references work

### Celeste (Lena Raine)

- **Motif-per-meaning, transformed rather than multiplied.** Madeline is piano,
  Theo guitar, Oshiro a theremin-ish synth; Badeline's theme is a minor
  inversion of Madeline's, and at the Summit the two merge into a 50/50
  major/minor split. Identity comes from *transforming* a small motif, not
  writing more material ([OSV interview](https://www.originalsoundversion.com/interview-composer-lena-raine-talks-celeste-soundtrack-working-in-game-audio/)).
- **Piano = self, synths = environment.** Raine: "the piano is the core sound
  for Madeline as a person whereas the synths can represent aspects of her &
  the mountain" — an instrumentation split between a human/acoustic anchor and
  environmental texture (same source).
- **Density is a late subtraction.** Her first instinct was upbeat chiptune;
  she pulled the tempo back to give the music "space to breathe"
  ([KEXP](https://www.kexp.org/read/2019/3/29/confronting-myself-lena-raine-soundtracking-celeste-and-her-new-album-oneknowing/)).
  And explicitly: "I also made the music laidback in a lot of places, because I
  didn't want to add to the stress" ([Bandcamp Daily](https://daily.bandcamp.com/high-scores/lena-raine-celeste-feature)) —
  the closest direct precedent for "intense platformer, calm-ish score".
- **Mask-and-reveal layering** (the panic-attack sequence): a meditative piano
  ostinato never stops; synth swells *engulf* it as tension rises and recede to
  reveal it again. Energy is added by burying the anchor, not by adding notes
  (KEXP, above).
- **One arrangement, authored as a stack of stems** that can be "taken away or
  built up" over a chapter — the B-side intensity is the same motif vocabulary
  with denser, faster treatment (OSV interview). She also composes the full
  linear cue first, then decomposes into interactive layers
  ([Gaming Trend podcast](https://gamingtrend.com/podcasts/gaming-trend-podcast-behind-the-music-of-celeste-lena-raine-interview/)).

### Hollow Knight (Christopher Larkin)

- Team Cherry's brief — "dark elegance", "minimal instrumentation", "classical
  and melancholic" — is essentially the atmospheric half of our target already
  ([Bandcamp Daily](https://daily.bandcamp.com/features/christopher-larkin-review),
  [Wikipedia](https://en.wikipedia.org/wiki/Music_of_Hollow_Knight)).
- **One or two very simple themes per score**, reused as leitmotif; recurrence
  does the emotional work, not harmonic complexity
  ([Indie Game Fans](https://indiegamefans.com/soundtrack-spotlight-hollow-knight/)).
- **Instrument-as-area-identity, chosen for connotation:** harp + hang drum for
  Greenpath (nature), harpsichord for the Mantis (aristocracy), organ for Soul
  Sanctum (sacredness), rain-patter piano ostinato for City of Tears. Each zone
  is 1–2 signature timbres over a shared base — cheap, systematic per-area
  identity (Bandcamp Daily, above).
- **Reverb as narrative distance:** for Queen's Station he recorded crowd
  ambience, "put a lot of echo and put it right down in the mix" so it reads as
  the *memory* of a living place. Mix depth signals temporal/spatial distance —
  directly transferable to a descent (deeper = more washed, more distant).
- **Two-tier stems:** full arrangement for a zone's hub, an ambient-only subset
  for its peripheral rooms — same material, different stem count (Indie Game
  Fans, above).
- **Energy = swell, not speed.** The score "swells to an operatic scale" as
  stakes rise: more instruments and louder dynamics on the *same slow harmonic
  material*, not faster rhythms (Wikipedia, above). Danger without dread was a
  deliberate choice — the world should feel dangerous, not frightening.

### Super Meat Boy (Danny Baranowsky)

- **"Melody is king"** even at maximum aggression; and "harmonic progressions
  that are complex without being overwrought" — sophistication lives in chord
  choice and voice-leading while the surface stays simple and driving
  ([Bandcamp Daily](https://daily.bandcamp.com/high-scores/high-scores-danny-baranowsky)).
- **Designed for repetition:** ~90-second loops heard hundreds of times, so
  variation comes from dynamics and layering, not through-composed length.
- **Intensity/rebuild oscillation:** even inside a short loop he alternates
  peaks with rebuild sections so relentlessness doesn't flatten — matching the
  attempt/fail/retry rhythm of the game. This is "controlled" freneticness in
  one sentence.
- Palette is 8/16-bit *nostalgic* (Mega Man, Contra, Sonic) but not chip-pure:
  real DI'd guitar through amp-sim plus software synths — the aggression reads
  as rock, not as square waves.

### Unexplored (Ludomotion / Matthijs Dierckx)

- The documented adaptive system is Unexplored 2's: "an orchestral adaptive
  soundtrack ... arranged reactively, making subtle changes to the score at
  appropriate times, whether a moment of high emotion, or to provide
  foreshadowing" ([Games Press](https://www.gamespress.com/Unexplored-2-Watch-gameplay-footage-from-the-upcoming-PC-and-Xbox-Seri)).
  No evidence it is *generative* in the algorithmic-composition sense — it's
  authored material with a reactive arrangement layer, i.e. the same
  stems-and-state family as Larkin/Raine with finer state tracking. The
  designer's "way too enthusiastic" verdict is therefore about its *composed
  character* (Hisaishi/Uematsu/Soule-style melody-forward orchestral), not its
  technology. Deeper technical detail exists only in audio/video form
  ([Hunchback Talks ep. 8](https://creators.spotify.com/pod/profile/hunchback-music/episodes/Hunchback-Talks---Episode-8-ft--Matthijs-Dierckx-e22aefh)).

### Cross-cutting: the recipe for "atmospheric and energising"

1. **Small, simple thematic material; elaboration by orchestration and
   dynamics, never by note-density.** All three well-documented composers
   converge here.
2. **Density is a layer property, decoupled from composition.** Author full
   material; choose how much is audible at once (Larkin's two-tier stems,
   Raine's build/strip, Baranowsky's loop dynamics).
3. **Reverb/mix depth is an independent atmosphere control** — a rhythmically
   active part pushed back in the mix still reads as quiet and distant.
4. **Energy comes from swell and tension/release cycles, not tempo** —
   peaks that recede are the "controlled" in controlled freneticness.
5. **Timbres carry meaning** (piano=self, organ=sanctity) — a systematic way to
   give procedurally assembled rooms identity, which downwards already gestures
   at (tonic-per-ability, mode-per-difficulty) but only in pitch space, not
   timbre space.

---

## 2. Why generated music sounds "clunky and forced" — and the fixes

The diagnosis in the expressive-performance literature is precise: a perfect
timing grid with identical velocity and duration for structurally different
notes is exactly what the ear flags as non-human. The fix is **structured,
purposeful deviation tied to musical structure** — phrase position, meter,
harmony, contour — not random jitter, which removes obvious mechanicalness but
adds no intention ([overview](https://medium.com/be-open/the-illusion-of-authenticity-how-to-humanize-ai-generated-music-a99acf5fd267)).

The **GERM(S) model** (Juslin, Friberg, Bresin 2001) decomposes expressive
performance into Generative rules (clarify structure), Emotional expression,
Random motor noise, Movement principles, and Stylistic unexpectedness — with
randomness deliberately the smallest layer
([paper](https://journals.sagepub.com/doi/10.1177/10298649020050S104)).

### The KTH rule system / Director Musices

The canonical rule set for rendering a dead score expressively (Friberg,
Bresin, Sundberg, ["Overview of the KTH rule system"](https://www.researchgate.net/publication/26450064_Overview_of_the_KTH_rule_system_for_musical_performance);
[Director Musices](https://www.diva-portal.org/smash/get/diva2:1246181/FULLTEXT01.pdf)).
The directly implementable rules:

- **Phrase Arch** — tempo + loudness arc over each phrase (cresc/accel in,
  dim/rit out), nested per phrase-tree level, calibrated against human
  performances ([Friberg 1995](https://www.speech.kth.se/qpsr/1995/1995_36_2-3_063-070.pdf)).
- **Punctuation** — micro-pauses (tens of ms) at boundaries between small
  melodic gestures, so a note stream parses as phrases that *breathe*
  ([paper](https://www.researchgate.net/publication/261628023_Musical_punctuation_on_the_microlevel_Automatic_identification_and_performance_of_small_melodic_units)).
- **Duration Contrast** — exaggerate existing duration differences (short
  shorter, long longer) for rhythmic definition.
- **Melodic / Harmonic Charge** — emphasize (louder, slightly later) notes and
  chord roots that are chromatically distant from the key — tension made
  audible ([harmonic charge](https://www.speech.kth.se/music/performance/Texts/harmonic_charge.htm)).
- **High Loud / Faster Uphill** — higher pitch slightly louder; ascending runs
  slightly faster — the pitch/effort coupling instrumentalists give for free.
- **Final Ritard** — cadential slowdown follows tempo ∝ √(time remaining), the
  velocity curve of a runner braking, not a linear ramp
  ([Friberg & Sundberg 1999](https://pubs.aip.org/asa/jasa/article-abstract/105/3/1469/558501/)).
- **Articulation rules** (Bresin) — gate length (sounding duration vs. slot) by
  interval size and metrical position: legato on leaps and strong beats,
  detached on stepwise/repeated notes. This is the single most relevant rule to
  the current downwards output, where every note fills its slot at a fixed
  linear attack/release.

Real-time weighting of these rules shifts emotional character — more Duration
Contrast and tempo variability reads as energetic/angry; smoothing reads as
tender ([pDM](https://dl.acm.org/doi/abs/10.1162/014892606776021308)) — which
maps naturally onto downwards' difficulty tiers.

### Groove and micro-timing

- Charles Keil's **participatory discrepancies**: groove lives in small
  *systematic* timing relationships between voices (snare consistently a hair
  late, bass a hair early), not per-note randomness
  ([study](https://www.researchgate.net/publication/249978407_Participatory_Discrepancies_and_the_Perception_of_Beats_in_Jazz)).
- Microtiming deviations sit within roughly ±50 ms; evidence they enhance
  groove is genuinely mixed, and exaggerated deviations hurt
  ([Sci. Reports](https://www.nature.com/articles/s41598-019-55981-3),
  [Frontiers](https://www.frontiersin.org/journals/psychology/articles/10.3389/fpsyg.2016.01487/full)).
- Practitioner convergence: apply a **groove template** (classic swing settings
  are 54–65% at 16th-note resolution) to the whole part
  ([guide](https://samplefocus.com/blog/swing-shuffle-and-humanization-how-to-program-grooves/)),
  then at most ±5–20 ms timing and ±5–12% velocity noise *on top of* the
  structural shaping ([humanize guide](https://unison.audio/how-to-humanize-midi/)).
  Note: at 60 Hz ticks, one tick is 16.7 ms — the game's own clock is already
  at humanization granularity, so tick-level offsets are viable without
  breaking the deterministic tick model.

### Chip/tracker-specific life

Tracker musicians solve the same problem within one monophonic channel:
**vibrato** (delayed-onset pitch LFO), **mid-note duty-cycle sweeps** (timbral
movement substituting for acoustic dynamics), **portamento/glide**, fast
**arpeggio** chords, and **echo channels** — copy a phrase into a second voice,
offset a few rows, drop the volume — faking reverb and creating built-in
call-and-response ([BeatScribe tutorials](https://beatscribe.wordpress.com/tag/chiptune-tutorial/)).
These are all composer/artifact-level devices needing little or no DSP.

### Surveys, for orientation

[Fernández & Vico 2013](https://arxiv.org/pdf/1402.0585) (techniques:
grammars, Markov, evolutionary, neural) and
[Herremans et al. 2017](https://dl.acm.org/doi/abs/10.1145/3108242)
(classification by *function*) — the latter's framing matters here: a
background-music generator for a game succeeds on different criteria (non-
fatiguing, state-appropriate, coherent under repetition) than a composition
tool, which legitimizes the current rule-based approach over anything learned.

---

## 3. How real games do generative/adaptive music well

- **Spore** — all audio ran in embedded Pure Data; Eno/Chilvers plus EA's Kent
  Jolly and Aaron McLeran built "the Shuffler", which procedurally reassembles
  short *authored* fragments driven by game state
  ([Development of Spore](https://en.wikipedia.org/wiki/Development_of_Spore),
  [GDC 2008](https://www.gdcvault.com/play/323/Procedural-Music-in)). Lesson:
  author small musically-valid fragments; let rules recombine them.
- **Mini Metro** — Disasterpeace maps live simulation state (trains, lines,
  passengers) through serialist series onto pitch/duration/velocity/timbre; no
  looping music at all, no RNG needed — game state is the seed
  ([talk writeup](https://disasterpeace.com/blog/mini-metro.serialism-talk.html),
  [interview](https://designingsound.org/2016/02/18/the-programmed-music-of-mini-metro-interview-with-rich-vreeland-disasterpeace/)).
- **Ape Out** — a bank of thousands of individually recorded percussion hits;
  gameplay geometry maps to kit geometry, and each chapter reskins the
  *instrumentation* over the same trigger logic
  ([Variety](https://variety.com/2019/gaming/columns/how-ape-out-creates-a-soundscape-worthy-of-smashing-1203150773/)).
- **Rez / Tetris Effect** — player actions emit notes **quantized to the beat
  grid** in the current key, so arbitrary input always reads as in time
  ([Tetris Effect analysis](https://www.nicholassinger.com/blog/tetriseffect),
  [Hydelic](https://splice.com/blog/hydelic-q-and-a/)). downwards' hazard
  windups/fires already live on this principle; it generalizes to coin pickups
  and deaths.
- **No Man's Sky** — Paul Weir's "Pulse" engine tracks gameplay "levels of
  interest" and recombines a library of 65daysofstatic stems and fragments
  ([Vice interview](https://www.vice.com/en/article/paul-weir-no-mans-sky-audio-generative-music-interview/));
  the later *Journeys* album is curated Pulse output — proof the approach makes
  actual music, not wash.
- **Peggle 2 / Peggle Blast** — peg hits ascend a diatonic scale keyed to the
  current music phrase (7 phrases, each with its own key); milestone stingers
  layer on top ([GANG writeup](https://www.audiogang.org/peggle-2-storytelling-through-adaptive-music/),
  [Audiokinetic](https://blog.audiokinetic.com/real-time-synthesis-for-sound-creation-in-peggle-blast/)).
  The macro key/phrase table keeps long-term coherence while individual hits
  stay event-driven.
- **Proteus** — every world object carries a motif/texture phased in by
  proximity and world state ([case study](https://www.gamedeveloper.com/audio/the-sound-and-music-of-proteus---an-academic-case-study));
  **Everyday Shooter** hard-wires enemy types and combos to guitar riffs
  ([Wikipedia](https://en.wikipedia.org/wiki/Everyday_Shooter)).
- **Middleware vocabulary** ([FMOD/Wwise overview](https://www.thegameaudioco.com/making-your-game-s-music-more-dynamic-vertical-layering-vs-horizontal-resequencing)):
  **vertical layering** = synchronized stems muted/faded by intensity (downwards
  already does this with inventory layers); **horizontal re-sequencing** =
  bar-aligned jumps between authored segments; **stingers** = short event-tied
  bursts; **transition regions** = the authored points where jumps are legal.
  The standard AAA pattern is both combined.

**Structural devices that give life while staying deterministic:** fragment
pools + a seeded shuffler; live-state→parameter mapping; beat-grid
quantization of event sounds; macro phrase/key tables driven by progress
rather than micro-RNG; vertical layers gated at transition points. Every one
of these is reproducible given (seed, state) — compatible with downwards'
byte-identical-artifact requirement. Notably, the games most praised for
generative audio (Spore, NMS, Ape Out) generate *arrangements of authored
fragments*, not notes from scratch; the ones that generate notes (Mini Metro,
Peggle) constrain them to a scale/phrase table. downwards currently generates
notes from scratch with light constraints — the survey suggests moving toward
richer authored pattern vocabulary (bass riffs, drum grooves, fill shapes)
selected and varied by seed, rather than more sophisticated note-walks.

---

## 4. The bass question

The designer asked for "more of a bass, maybe at a low level underlying the
music". The current bass is a triangle playing root/fifth half notes in
octave 2 at gain 10 (`compose.rs`) — harmonically correct, rhythmically inert.

- **Sub vs. bassline roles:** sub-bass (~20–60 Hz) is felt, near-sine, mono,
  static; the bassline (~60–250 Hz) carries harmonic/rhythmic identity and can
  afford timbral movement ([ADSR](https://www.adsrsounds.com/mixing-tutorials/sub-separation/),
  [Sub-bass](https://en.wikipedia.org/wiki/Sub-bass)). Keep other voices out of
  60–250 Hz so the bass owns it ([mixing bass](https://www.soundonsound.com/techniques/mixing-bass)).
- **Chip idioms:** the NES triangle is *the* bass channel precisely because its
  clean (if steppy) low end reads as bass where pulse turns buzzy — but it has
  no volume envelope, so its only dynamic tools are note length and retrigger;
  Famitracker practice uses short-note cuts for percussive triangle "kicks"
  ([FamiTracker ch.8](https://btothethree.tumblr.com/post/112222495122/how-to-use-famitracker-chapter-8-percussive),
  [NESDev](https://forums.nesdev.org/viewtopic.php?t=4215)). Octave-shifting
  the same channel yields tom-like fills. Pulse-bass with duty modulation is
  the buzzier alternative when triangle is busy
  ([duty guide](https://www.mattcurrent.org/chiptune-complete-guide/)); the
  Sunsoft trick layers a sample transient under triangle for attack
  ([BeatScribe](https://beatscribe.wordpress.com/2013/11/27/the-sunsoft-dpcm-bass-trick-in-famitracker-tutorial/)).
- **References' grounding** (weakly sourced — production breakdowns are
  scarce): Hollow Knight grounds with sustained low piano/string pads rather
  than percussive sub; Super Meat Boy leans on drum-and-bass-style low end in
  its most aggressive tracks. Both suggest for downwards a *sustained* low
  layer under a *rhythmic* mid-bass, rather than one voice doing both.
- **Sidechain/duck feel without a compressor:** trigger an attack-decay duck
  envelope from the same clock that fires the kick/noise hit and multiply the
  bass by `1 - duck` in the mixer; tie the envelope to the sequencer step
  counter, never a free-running LFO, so it stays locked and deterministic
  ([Noise Engineering](https://noiseengineering.us/blogs/loquelic-literitas-the-blog/tips-and-tricks-ducking/),
  [MOD WIGGLER](https://www.modwiggler.com/forum/viewtopic.php?t=225026)).
- **Felt-but-unobtrusive:** low relative level, mono, rhythmically locked to
  the percussion transients so ear parses one event, register-separated from
  everything else ([frequency ranges](https://www.masteringthemix.com/blogs/learn/understanding-the-different-frequency-ranges)).
  Concretely for downwards: the complaint is probably less "no bass exists"
  than that root–fifth half notes in octave 2 neither *move* (no rhythmic
  interplay with the hats) nor *underpin* (octave 2 triangle ≈ 65–130 Hz
  fundamentals, thin sub weight). A second, lower sustained tone (octave 1
  root, drones per chord) plus a more rhythmic mid-bass line with occasional
  passing tones would address both readings cheaply.

---

## 5. Aesthetic options beyond strict NES chiptune

DSP feasibility note first: every primitive below (saw/sine oscillators,
state-variable filters, LFOs, chorus, delay, Schroeder/Freeverb and FDN
reverbs, resampling) exists in maintained pure-Rust crates —
[fundsp](https://github.com/SamiPerttu/fundsp) has filters, chorus, and an FDN
reverb built in; [dasp](https://github.com/RustAudio/dasp) covers buffers,
interpolation, envelopes. Freeverb itself is famously ~one screen of code
([CCRMA](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html),
[Valhalla](https://valhalladsp.com/2009/05/30/schroeder-reverbs-the-forgotten-algorithm/)).
The current synth's sample-at-a-time, bit-identical design is compatible with
all of these — filters/delays/reverbs are per-sample state machines too.

| Palette | Needs | Effort | Atmospheric | Frenetic | Notes |
|---|---|---|---|---|---|
| Chillsynth / synthwave-lite | detuned saw stacks, chorus, (gated) reverb | low–moderate | strong | moderate | lush but soft; needs a percussive layer for edge ([techniques](https://blog.imseankim.com/synthwave-retro-production-techniques-modern-tools/)) |
| FM (Genesis YM2612) | sine ops + envelope-driven mod index; 4-op needs algorithm routing | 2-op cheap, 4-op heavy | weaker without careful patches | strong | era-authentic action energy; no sample data needed ([basics](https://nesdoug.com/2022/02/04/sega-genesis-fm-instrument-basics/)) |
| SNES-style 16-bit | sample playback, gaussian-ish interpolation, FIR echo | highest | strongest | harder | requires authoring actual samples; echo gives space for free ([S-SMP](https://snes.nesdev.org/wiki/S-SMP)) |
| Lo-fi beats | low-pass ~3 kHz, vinyl noise, swing, soft saturation | lowest | strong | weakest | identity is laid-back; fights the frenetic half ([guide](https://blog.native-instruments.com/lo-fi-hip-hop-beats/)) |
| **Hybrid chip + pad** | one filtered saw/triangle pad voice (SVF + slow LFO) + light reverb send; chip voices unchanged | low | strong | strong | atmosphere and freneticness decoupled into separate voices |

The hybrid is the standout for this brief: the existing pulse/triangle/noise
voices keep doing the sharp, frenetic work (which the designer says is "not
bad"), while a single new sustained pad voice plus a reverb send carries all
the atmosphere. It is also the smallest step from the current engine, keeps
the artifact format's voice model intact (one new `VoiceKind`), and — echoing
Baranowsky — moves the palette from "literal NES" to "8-bit-nostalgic", which
his SMB score shows reads as energy rather than limitation. Note Celeste
itself is essentially chip-adjacent-synth + piano + pads: the reference the
designer loves is already this hybrid.

---

## 6. Recommendations (ranked, trade-offs flagged, nothing decided)

Legend for where each change lives: **[C]** composer-only (compose/melody),
**[A]** needs `.track` artifact-format additions, **[S]** synth/sequencer DSP.
Current relevant facts: `NoteEvent` already has `vel: 0..=15` and `len_steps`;
voices have gains; layers exist and fade via `CosRamp`; there is no timing
offset, no gate-fraction, no vibrato, no filter, no delay/reverb; melodic
envelopes are a plain linear `GateEnv`.

1. **Articulation and gate variety [C, possibly A].** Highest ratio of impact
   to effort for "clunky → alive". Today every note fills its slot with the
   same attack/release. Apply the KTH articulation rules in the composer:
   detached (gate ~50–70%) for stepwise/repeated notes and offbeats, legato
   for leaps and downbeats; add micro-rests (Punctuation) at fill boundaries.
   Much of this is expressible now via `len_steps` < slot; a fractional-gate
   or articulation field in the artifact would make it finer-grained.
   Trade-off: none serious; this is the literature's core fix.

2. **Phrase-arc dynamics [C].** The velocity field already exists; the
   composer barely uses it (three constants). Shape velocities over each
   4-bar phrase (arch up then down), accent metrical strong beats, add
   Melodic-Charge accents on out-of-chord tension notes, High-Loud coupling
   to pitch. Zero format or synth changes. Trade-off: 16 velocity steps is
   coarse; consider widening to 0..=127 in the artifact (small [A]) if arcs
   sound steppy.

3. **A real bass part [C], plus optional sub layer [C or A].** Replace
   root–fifth half notes with a difficulty-dependent bass *pattern* vocabulary
   (sustained drone for easy, walking/syncopated riffs locking with the hats
   for medium/hard, occasional octave drops), and add a low-level octave-1
   sustained root under it. Pure composer work with existing voices; a second
   dedicated bass voice (e.g. sine-ish sub) would be a small [A]+[S] addition.
   Trade-off: two low voices need register discipline to avoid mud (§4).

4. **Reverb/delay send in the synth [S, small A for send levels].** The
   single biggest "atmospheric" lever per §1 — mix depth as distance. A
   Schroeder/Freeverb or fundsp FDN reverb on a per-voice send (pad and lead
   wet, bass and perc dry) is modest, deterministic DSP. Cheaper interim: an
   echo *voice* à la trackers — composer emits a delayed, quieter copy of the
   lead ([C] only, works today). Trade-off: reverb is the one item that
   genuinely changes the synth's character and CPU profile; the echo-voice
   trick gets ~60% of the effect for free.

5. **A pad voice — the hybrid palette [S, A].** One new `VoiceKind` (detuned
   saw or triangle stack through a low-pass with slow LFO), used for sustained
   chord tones at low gain. Combined with 4 this is what buys "atmospheric"
   without touching the parts the designer already half-likes. Trade-off:
   first departure from the NES palette — but the designer has explicitly
   opened that door, and Celeste validates chip-plus-pad.

6. **Groove template + micro-offsets [A, S].** Add a per-note tick offset (or
   per-voice groove table) to the artifact; sequencer applies it. At 60 Hz
   ticks one tick = 16.7 ms, squarely in the ±5–20 ms humanization range, so
   even integer-tick offsets (hats slightly late, bass on the grid — a
   participatory discrepancy) may suffice. Trade-off: evidence for groove
   micro-timing is the most mixed in the literature (§2); rank below
   articulation/dynamics, and never do random jitter alone.

7. **Expression events: vibrato, duty sweeps, glide [A, S].** Tracker-style
   per-note effects — delayed vibrato on held lead notes, duty change at
   phrase peaks, portamento into downbeats. Medium effort across artifact and
   sequencer; strong per-note life, very idiomatic if any chip flavor stays.

8. **Tension/release macro-structure [C].** Baranowsky's intensity/rebuild
   inside the loop: designate 4 of the 16 bars as a "rebuild" (drop hats and
   arp, thin the lead, let bass + pad carry), then re-enter. Pure composer
   change using the existing voice set; directly answers "energising without
   exhausting". Trade-off: interacts with hazard-driven percussion, needs care
   that the rebuild bars don't collide with locked hazard windups.

Suggested sequencing if all are wanted: 1+2+3+8 (composer-only, artifact
mostly unchanged, quick designer feedback loop) → 4's echo-voice trick → then
the [S]/[A] wave (pad voice, reverb send, groove offsets, expression events)
as one artifact-format revision rather than several.

---

*Research compiled 2026-08-16/17 from web sources cited inline; codebase facts
from `crates/downwards-audio/src/{compose,melody,track,sequencer}.rs` and
`synth/voice.rs` at commit 5d6001f.*
