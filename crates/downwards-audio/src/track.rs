//! The `Track` model and the human-editable `.track` artifact (§4–5 of the
//! soundtrack spec): line-based text, `#` comments, space-separated tokens.
//!
//! `parse(serialize(track)) == track` exactly. The artifact carries no hazard
//! event *times* — those are always recomputed live from `HazardTiming` — only
//! the sounds a hazard makes.

use std::{error::Error, fmt};

use crate::{
    tempo::HazardTiming,
    theory::{Key, Mode, Pitch, PitchClass, fnv1a64},
};

/// The musical grid: step length in 60 Hz ticks, steps per beat, grid offset
/// φ, and loop length in bars (meter is always 4/4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TempoGrid {
    pub step_ticks: u32,
    pub beat_steps: u32,
    pub grid_offset: u32,
    pub loop_bars: u32,
}

impl TempoGrid {
    /// Beats per minute, derived: `3600 / (step_ticks * beat_steps)`.
    #[must_use]
    pub fn bpm(self) -> f64 {
        3600.0 / f64::from(self.step_ticks * self.beat_steps)
    }

    #[must_use]
    pub const fn beat_ticks(self) -> u32 {
        self.step_ticks * self.beat_steps
    }

    #[must_use]
    pub const fn bar_steps(self) -> u32 {
        4 * self.beat_steps
    }

    #[must_use]
    pub const fn loop_steps(self) -> u32 {
        self.loop_bars * 4 * self.beat_steps
    }

    #[must_use]
    pub const fn loop_ticks(self) -> u32 {
        self.loop_steps() * self.step_ticks
    }
}

/// Pulse duty cycles of the NES-style pulse voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Duty {
    Eighth,
    Quarter,
    Half,
}

impl Duty {
    #[must_use]
    pub const fn fraction(self) -> f64 {
        match self {
            Self::Eighth => 0.125,
            Self::Quarter => 0.25,
            Self::Half => 0.5,
        }
    }

    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Eighth => "0.125",
            Self::Quarter => "0.25",
            Self::Half => "0.5",
        }
    }

    #[must_use]
    pub fn parse(keyword: &str) -> Option<Self> {
        match keyword {
            "0.125" => Some(Self::Eighth),
            "0.25" => Some(Self::Quarter),
            "0.5" => Some(Self::Half),
            _ => None,
        }
    }
}

/// Oscillator family of a pattern voice.
///
/// Format v2 additions: `Pad` (two detuned saws through a low-pass — the
/// hybrid chip-plus-pad palette of the research survey §5) and `NoiseSoft`
/// (a darker, longer shaker timbre for groove percussion, where `Noise`
/// stays the crisp tick).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoiceKind {
    Pulse { duty: Duty },
    Triangle,
    Noise,
    NoiseSoft,
    Pad,
}

/// Delayed-onset pitch vibrato of a melodic voice (format v2): depth in
/// cents (0..=50) and rate in deci-Hz (e.g. 55 = 5.5 Hz, 1..=120).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Vibrato {
    pub cents: u8,
    pub rate_dhz: u8,
}

/// A named pattern voice bound to a layer, with a 0..=15 mixer gain, a
/// 0..=15 reverb send (format v2, default 0), and optional vibrato.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceDef {
    pub name: String,
    pub kind: VoiceKind,
    pub layer: String,
    pub gain: u8,
    pub send: u8,
    pub vibrato: Option<Vibrato>,
}

/// One pattern event. `pitch: Some` is a `note` line; `pitch: None` is a
/// `hat` line (a one-step noise tick).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub voice: String,
    pub start_step: u32,
    pub len_steps: u32,
    pub pitch: Option<Pitch>,
    pub vel: u8,
}

/// Noise timbre of a hazard fire hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoiseTimbre {
    Snare,
    Crash,
}

/// The editable *sound* of one timed hazard (never its times).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HazardVoice {
    pub index: usize,
    pub locked: bool,
    pub windup_steps: u32,
    pub windup_degrees: Vec<i8>,
    pub fire_hit: NoiseTimbre,
    pub fire_stab: Option<Pitch>,
    pub active_bed_level: u8,
}

/// A complete per-room track.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub slug: String,
    pub hand_tuned: bool,
    pub grid: TempoGrid,
    pub key: Key,
    pub layers: Vec<String>,
    pub voices: Vec<VoiceDef>,
    pub notes: Vec<NoteEvent>,
    pub hazard_voices: Vec<HazardVoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for TrackParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "track line {}: {}", self.line, self.message)
    }
}

impl Error for TrackParseError {}

impl Track {
    /// Serialize to the canonical `.track` text, including a freshly computed
    /// `generated-digest` header over the canonical body.
    #[must_use]
    pub fn serialize(&self) -> String {
        let body = self.serialize_body();
        let digest = fnv1a64(canonical_body(&body).as_bytes());
        format!(
            "downwards-track v2\nslug {}\ngenerated-digest {digest:016x}\nhand-tuned {}\n\n{body}",
            self.slug, self.hand_tuned
        )
    }

    /// The body: every section after the `hand-tuned` line.
    #[must_use]
    pub fn serialize_body(&self) -> String {
        let mut out = String::new();
        let grid = self.grid;
        out.push_str(&format!(
            "grid step-ticks {} beat-steps {} offset {} loop-bars {}\n",
            grid.step_ticks, grid.beat_steps, grid.grid_offset, grid.loop_bars
        ));
        out.push_str(&format!(
            "key {} {}\n\n",
            self.key.tonic.name(),
            self.key.mode.keyword()
        ));
        for layer in &self.layers {
            out.push_str(&format!("layer {layer}\n"));
        }
        out.push('\n');
        for voice in &self.voices {
            let kind = match voice.kind {
                VoiceKind::Pulse { duty } => format!("pulse {}", duty.keyword()),
                VoiceKind::Triangle => "triangle".to_owned(),
                VoiceKind::Noise => "noise".to_owned(),
                VoiceKind::NoiseSoft => "noise-soft".to_owned(),
                VoiceKind::Pad => "pad".to_owned(),
            };
            out.push_str(&format!(
                "voice {} {kind} layer {} gain {}",
                voice.name, voice.layer, voice.gain
            ));
            if voice.send > 0 {
                out.push_str(&format!(" send {}", voice.send));
            }
            if let Some(vibrato) = voice.vibrato {
                out.push_str(&format!(" vib {} {}", vibrato.cents, vibrato.rate_dhz));
            }
            out.push('\n');
        }
        out.push('\n');
        for note in &self.notes {
            match note.pitch {
                Some(pitch) => out.push_str(&format!(
                    "note {} {} {} {} {}\n",
                    note.voice,
                    note.start_step,
                    note.len_steps,
                    pitch.name(),
                    note.vel
                )),
                None => out.push_str(&format!(
                    "hat {} {} {}\n",
                    note.voice, note.start_step, note.vel
                )),
            }
        }
        if !self.hazard_voices.is_empty() {
            out.push('\n');
        }
        for hazard in &self.hazard_voices {
            if hazard.locked {
                let degrees: Vec<String> =
                    hazard.windup_degrees.iter().map(i8::to_string).collect();
                let stab = hazard
                    .fire_stab
                    .expect("locked hazard voices carry a fire stab")
                    .name();
                out.push_str(&format!(
                    "hazard {} locked windup {} degrees {} fire {} stab {} bed {}\n",
                    hazard.index,
                    hazard.windup_steps,
                    degrees.join(","),
                    timbre_keyword(hazard.fire_hit),
                    stab,
                    hazard.active_bed_level
                ));
            } else {
                out.push_str(&format!(
                    "hazard {} unlocked fire {} bed {}\n",
                    hazard.index,
                    timbre_keyword(hazard.fire_hit),
                    hazard.active_bed_level
                ));
            }
        }
        out
    }

    /// The digest of the current body, as stored in `generated-digest`.
    #[must_use]
    pub fn body_digest(&self) -> u64 {
        fnv1a64(canonical_body(&self.serialize_body()).as_bytes())
    }

    /// Parse the `.track` text. Rejects unknown directives, undeclared voice
    /// or layer references, and out-of-range values. Returns the track plus
    /// the `generated-digest` recorded in the file (which may differ from the
    /// body's actual digest if a human edited the body).
    pub fn parse(source: &str) -> Result<(Self, u64), TrackParseError> {
        Parser::new(source).parse()
    }

    /// Validate that every locked hazard voice's rising edges land on the
    /// (possibly hand-edited) grid: `fire_tick ≡ grid_offset (mod step_ticks)`
    /// and `step_ticks` divides the period. Hard-errors otherwise so an edit
    /// can never silently break sync.
    pub fn check_hazard_sync(&self, hazards: &[HazardTiming]) -> Result<(), TrackParseError> {
        for voice in &self.hazard_voices {
            if !voice.locked {
                continue;
            }
            let Some(timing) = hazards.get(voice.index) else {
                return Err(TrackParseError {
                    line: 0,
                    message: format!(
                        "hazard voice {} has no matching room hazard",
                        voice.index
                    ),
                });
            };
            let step = self.grid.step_ticks;
            if !timing.period.is_multiple_of(step)
                || timing.fire_tick() % step != self.grid.grid_offset % step
            {
                return Err(TrackParseError {
                    line: 0,
                    message: format!(
                        "locked hazard {} (period {}, fire tick {}) misses the grid (step {}, offset {})",
                        voice.index,
                        timing.period,
                        timing.fire_tick(),
                        step,
                        self.grid.grid_offset
                    ),
                });
            }
        }
        Ok(())
    }
}

const fn timbre_keyword(timbre: NoiseTimbre) -> &'static str {
    match timbre {
        NoiseTimbre::Snare => "snare",
        NoiseTimbre::Crash => "crash",
    }
}

/// Strip a `#` comment. Because sharp note names contain `#` (`F#4`), a
/// comment starts only at a `#` that begins the line or follows whitespace.
#[must_use]
pub fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            return &line[..index];
        }
    }
    line
}

/// Canonical body: comments stripped, lines trimmed, blank lines removed,
/// `\n`-joined (§5.3).
#[must_use]
pub fn canonical_body(body: &str) -> String {
    body.lines()
        .map(|line| strip_comment(line).trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Canonical body of a full `.track` file: everything after the `hand-tuned`
/// header line.
#[must_use]
pub fn canonical_body_of_file(source: &str) -> Option<String> {
    let mut lines = source.lines();
    let position =
        lines.position(|line| strip_comment(line).trim().starts_with("hand-tuned "))?;
    let body: Vec<&str> = source.lines().skip(position + 1).collect();
    Some(canonical_body(&body.join("\n")))
}

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>,
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        let lines = source
            .lines()
            .enumerate()
            .map(|(index, raw)| (index + 1, strip_comment(raw).trim()))
            .filter(|(_, line)| !line.is_empty())
            .collect();
        Self { lines, cursor: 0 }
    }

    fn error(&self, line: usize, message: impl Into<String>) -> TrackParseError {
        TrackParseError {
            line,
            message: message.into(),
        }
    }

    fn next_line(&mut self) -> Option<(usize, &'a str)> {
        let item = self.lines.get(self.cursor).copied();
        if item.is_some() {
            self.cursor += 1;
        }
        item
    }

    fn expect_line(&mut self, what: &str) -> Result<(usize, &'a str), TrackParseError> {
        self.next_line()
            .ok_or_else(|| self.error(0, format!("missing {what}")))
    }

    fn parse(mut self) -> Result<(Track, u64), TrackParseError> {
        let (line, header) = self.expect_line("header")?;
        // v2 added pad voices, per-voice reverb sends, vibrato, the soft
        // noise timbre, and the aeolian mode; v1 files remain parseable.
        if header != "downwards-track v2" && header != "downwards-track v1" {
            return Err(self.error(line, "expected `downwards-track v1|v2` header"));
        }
        let slug = self.keyword_value("slug")?;
        let digest_text = self.keyword_value("generated-digest")?;
        let (digest_line, _) = self.lines[self.cursor - 1];
        let recorded_digest = u64::from_str_radix(&digest_text, 16)
            .map_err(|_| self.error(digest_line, "generated-digest must be 16 hex chars"))?;
        let hand_tuned = match self.keyword_value("hand-tuned")?.as_str() {
            "true" => true,
            "false" => false,
            other => {
                let (line, _) = self.lines[self.cursor - 1];
                return Err(self.error(line, format!("hand-tuned must be true or false, got {other}")));
            }
        };

        let grid = self.parse_grid()?;
        let key = self.parse_key()?;

        let mut layers = Vec::new();
        let mut voices: Vec<VoiceDef> = Vec::new();
        let mut notes = Vec::new();
        let mut hazard_voices: Vec<HazardVoice> = Vec::new();
        // Section order is fixed: layers, voices, notes/hats, hazards.
        let mut section = 0;
        while let Some((line, text)) = self.next_line() {
            let mut parts = text.split_whitespace();
            let directive = parts.next().expect("non-empty line has a first token");
            let rank = match directive {
                "layer" => 0,
                "voice" => 1,
                "note" | "hat" => 2,
                "hazard" => 3,
                other => {
                    return Err(self.error(line, format!("unknown directive {other:?}")));
                }
            };
            if rank < section {
                return Err(self.error(
                    line,
                    format!("{directive} line out of section order"),
                ));
            }
            section = rank;
            let rest: Vec<&str> = parts.collect();
            match directive {
                "layer" => {
                    let [name] = rest.as_slice() else {
                        return Err(self.error(line, "layer takes exactly one name"));
                    };
                    if layers.iter().any(|existing: &String| existing == name) {
                        return Err(self.error(line, format!("duplicate layer {name:?}")));
                    }
                    layers.push((*name).to_owned());
                }
                "voice" => {
                    voices.push(self.parse_voice(line, &rest, &layers)?);
                }
                "note" => {
                    let [voice, start, len, pitch, vel] = rest.as_slice() else {
                        return Err(self
                            .error(line, "note takes voice, start-step, len-steps, pitch, vel"));
                    };
                    let event = NoteEvent {
                        voice: self.known_voice(line, voice, &voices)?,
                        start_step: self.parse_step(line, start, grid)?,
                        len_steps: self.parse_number(line, len, 1, grid.loop_steps(), "len-steps")?,
                        pitch: Some(Pitch::parse(pitch).ok_or_else(|| {
                            self.error(line, format!("invalid pitch {pitch:?}"))
                        })?),
                        vel: self.parse_number(line, vel, 0, 15, "velocity")? as u8,
                    };
                    notes.push(event);
                }
                "hat" => {
                    let [voice, start, vel] = rest.as_slice() else {
                        return Err(self.error(line, "hat takes voice, start-step, vel"));
                    };
                    notes.push(NoteEvent {
                        voice: self.known_voice(line, voice, &voices)?,
                        start_step: self.parse_step(line, start, grid)?,
                        len_steps: 1,
                        pitch: None,
                        vel: self.parse_number(line, vel, 0, 15, "velocity")? as u8,
                    });
                }
                "hazard" => {
                    hazard_voices.push(self.parse_hazard(line, &rest, &hazard_voices)?);
                }
                _ => unreachable!("directive was matched above"),
            }
        }

        if !layers.iter().any(|layer| layer == "base") {
            return Err(self.error(0, "track must declare the base layer"));
        }

        let track = Track {
            slug,
            hand_tuned,
            grid,
            key,
            layers,
            voices,
            notes,
            hazard_voices,
        };
        Ok((track, recorded_digest))
    }

    fn keyword_value(&mut self, keyword: &str) -> Result<String, TrackParseError> {
        let (line, text) = self.expect_line(keyword)?;
        let mut parts = text.split_whitespace();
        if parts.next() != Some(keyword) {
            return Err(self.error(line, format!("expected `{keyword} <value>`")));
        }
        let value = parts
            .next()
            .ok_or_else(|| self.error(line, format!("{keyword} needs a value")))?;
        if parts.next().is_some() {
            return Err(self.error(line, format!("{keyword} takes one value")));
        }
        Ok(value.to_owned())
    }

    fn parse_grid(&mut self) -> Result<TempoGrid, TrackParseError> {
        let (line, text) = self.expect_line("grid line")?;
        let parts: Vec<&str> = text.split_whitespace().collect();
        let ["grid", "step-ticks", step, "beat-steps", beats, "offset", offset, "loop-bars", bars] =
            parts.as_slice()
        else {
            return Err(self.error(
                line,
                "expected `grid step-ticks <u32> beat-steps <2|4|8> offset <u32> loop-bars <u32>`",
            ));
        };
        let step_ticks = self.parse_number(line, step, 1, 100_000, "step-ticks")?;
        let beat_steps = self.parse_number(line, beats, 2, 8, "beat-steps")?;
        if ![2, 4, 8].contains(&beat_steps) {
            return Err(self.error(line, "beat-steps must be 2, 4, or 8"));
        }
        let grid_offset = self.parse_number(line, offset, 0, step_ticks - 1, "offset")?;
        let loop_bars = self.parse_number(line, bars, 1, 1024, "loop-bars")?;
        Ok(TempoGrid {
            step_ticks,
            beat_steps,
            grid_offset,
            loop_bars,
        })
    }

    fn parse_key(&mut self) -> Result<Key, TrackParseError> {
        let (line, text) = self.expect_line("key line")?;
        let parts: Vec<&str> = text.split_whitespace().collect();
        let ["key", tonic, mode] = parts.as_slice() else {
            return Err(self.error(line, "expected `key <tonic> <mode>`"));
        };
        Ok(Key {
            tonic: PitchClass::parse(tonic)
                .ok_or_else(|| self.error(line, format!("unknown tonic {tonic:?}")))?,
            mode: Mode::parse(mode)
                .ok_or_else(|| self.error(line, format!("unknown mode {mode:?}")))?,
        })
    }

    fn parse_voice(
        &self,
        line: usize,
        rest: &[&str],
        layers: &[String],
    ) -> Result<VoiceDef, TrackParseError> {
        let (name, kind, tail) = match rest {
            [name, "pulse", duty, tail @ ..] => {
                let duty = Duty::parse(duty)
                    .ok_or_else(|| self.error(line, format!("invalid pulse duty {duty:?}")))?;
                (*name, VoiceKind::Pulse { duty }, tail)
            }
            [name, "triangle", tail @ ..] => (*name, VoiceKind::Triangle, tail),
            [name, "noise", tail @ ..] => (*name, VoiceKind::Noise, tail),
            [name, "noise-soft", tail @ ..] => (*name, VoiceKind::NoiseSoft, tail),
            [name, "pad", tail @ ..] => (*name, VoiceKind::Pad, tail),
            _ => {
                return Err(self.error(
                    line,
                    "expected `voice <name> <pulse|triangle|noise|noise-soft|pad> ...`",
                ));
            }
        };
        let ["layer", layer, "gain", gain, options @ ..] = tail else {
            return Err(self.error(line, "voice must carry `layer <layer> gain <0..15>`"));
        };
        if !layers.iter().any(|known| known == layer) {
            return Err(self.error(line, format!("voice references undeclared layer {layer:?}")));
        }
        // v2 optional suffixes, in fixed order: `send <0..15>`, `vib <cents> <dhz>`.
        let mut send = 0_u8;
        let mut vibrato = None;
        let mut options = options;
        if let ["send", value, rest @ ..] = options {
            send = self.parse_number(line, value, 0, 15, "send")? as u8;
            options = rest;
        }
        if let ["vib", cents, rate, rest @ ..] = options {
            vibrato = Some(Vibrato {
                cents: self.parse_number(line, cents, 1, 50, "vibrato cents")? as u8,
                rate_dhz: self.parse_number(line, rate, 1, 120, "vibrato rate")? as u8,
            });
            options = rest;
        }
        if !options.is_empty() {
            return Err(self.error(line, format!("trailing voice tokens {options:?}")));
        }
        Ok(VoiceDef {
            name: name.to_owned(),
            kind,
            layer: (*layer).to_owned(),
            gain: self.parse_number(line, gain, 0, 15, "gain")? as u8,
            send,
            vibrato,
        })
    }

    fn parse_hazard(
        &self,
        line: usize,
        rest: &[&str],
        existing: &[HazardVoice],
    ) -> Result<HazardVoice, TrackParseError> {
        let voice = match rest {
            [index, "locked", "windup", steps, "degrees", degrees, "fire", timbre, "stab", stab, "bed", bed] =>
            {
                let windup_steps = self.parse_number(line, steps, 1, 4, "windup steps")?;
                let windup_degrees: Vec<i8> = degrees
                    .split(',')
                    .map(|degree| {
                        let value: i8 = degree.parse().map_err(|_| {
                            self.error(line, format!("invalid wind-up degree {degree:?}"))
                        })?;
                        if value < 1 {
                            return Err(
                                self.error(line, "wind-up degrees are 1-based scale degrees")
                            );
                        }
                        Ok(value)
                    })
                    .collect::<Result<_, _>>()?;
                if windup_degrees.len() != windup_steps as usize {
                    return Err(self.error(line, "degrees list length must equal windup steps"));
                }
                HazardVoice {
                    index: self.parse_number(line, index, 0, 255, "hazard index")? as usize,
                    locked: true,
                    windup_steps,
                    windup_degrees,
                    fire_hit: self.parse_timbre(line, timbre)?,
                    fire_stab: Some(
                        Pitch::parse(stab)
                            .ok_or_else(|| self.error(line, format!("invalid stab pitch {stab:?}")))?,
                    ),
                    active_bed_level: self.parse_number(line, bed, 0, 15, "bed level")? as u8,
                }
            }
            [index, "unlocked", "fire", timbre, "bed", bed] => HazardVoice {
                index: self.parse_number(line, index, 0, 255, "hazard index")? as usize,
                locked: false,
                windup_steps: 0,
                windup_degrees: Vec::new(),
                fire_hit: self.parse_timbre(line, timbre)?,
                fire_stab: None,
                active_bed_level: self.parse_number(line, bed, 0, 15, "bed level")? as u8,
            },
            _ => return Err(self.error(line, "malformed hazard line")),
        };
        if existing.iter().any(|other| other.index == voice.index) {
            return Err(self.error(line, format!("duplicate hazard index {}", voice.index)));
        }
        Ok(voice)
    }

    fn known_voice(
        &self,
        line: usize,
        name: &str,
        voices: &[VoiceDef],
    ) -> Result<String, TrackParseError> {
        if voices.iter().any(|voice| voice.name == name) {
            Ok(name.to_owned())
        } else {
            Err(self.error(line, format!("note references undeclared voice {name:?}")))
        }
    }

    fn parse_timbre(&self, line: usize, keyword: &str) -> Result<NoiseTimbre, TrackParseError> {
        match keyword {
            "snare" => Ok(NoiseTimbre::Snare),
            "crash" => Ok(NoiseTimbre::Crash),
            other => Err(self.error(line, format!("unknown fire timbre {other:?}"))),
        }
    }

    fn parse_step(&self, line: usize, text: &str, grid: TempoGrid) -> Result<u32, TrackParseError> {
        self.parse_number(line, text, 0, grid.loop_steps() - 1, "start-step")
    }

    fn parse_number(
        &self,
        line: usize,
        text: &str,
        minimum: u32,
        maximum: u32,
        what: &str,
    ) -> Result<u32, TrackParseError> {
        let value: u32 = text
            .parse()
            .map_err(|_| self.error(line, format!("invalid {what} {text:?}")))?;
        if value < minimum || value > maximum {
            return Err(self.error(
                line,
                format!("{what} {value} outside {minimum}..={maximum}"),
            ));
        }
        Ok(value)
    }
}
