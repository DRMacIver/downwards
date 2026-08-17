//! The room → `Track` composition mapping (§6 of the soundtrack spec).
//!
//! `compose` is pure, total, and deterministic: the same `RoomMusicInputs`
//! always yields a byte-identical serialized track. The only randomness is
//! the melody walk, seeded from the slug.

use crate::{
    melody::{DoorSet, generate_melody},
    tempo::{Difficulty, HazardTiming, derive_grid},
    theory::{Key, Mode, PitchClass},
    track::{Duty, HazardVoice, NoiseTimbre, NoteEvent, Track, VoiceDef, VoiceKind},
};

/// Ability requirement recorded in the room metadata; carried by the tonic
/// (§6.1): none→C, wall→D, dash→E, both→G.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityReq {
    None,
    Wall,
    Dash,
    Both,
}

/// Which inventory layers exist in this room (§6.7): computed offline from
/// the passability data; a layer exists only when the ability materially
/// matters in the room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LayerGates {
    pub gloves: bool,
    pub boots: bool,
}

/// Every input the composer reads. Prose metadata fields are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomMusicInputs {
    pub slug: String,
    pub difficulty: Difficulty,
    pub ability_requirement: AbilityReq,
    pub coin_count: u32,
    pub doors: DoorSet,
    pub hazards: Vec<HazardTiming>,
    pub layer_gates: LayerGates,
}

/// Tuning constants for the whole mapping, kept in one place (§11 step 4).
/// Retuned 2026-08 after designer feedback: gentler mix, more headroom
/// (peaks ~0.7–0.8 instead of riding the soft clipper), quieter percussion,
/// softer hazard presence.
pub mod defaults {
    /// Mixer gains 0..=15 (§6.3).
    pub const LEAD_GAIN: u8 = 7;
    pub const ARP_GAIN: u8 = 5;
    pub const BASS_GAIN: u8 = 10;
    pub const PERC_GAIN: u8 = 3;
    pub const GLOVES_GAIN: u8 = 6;
    pub const BOOTS_GAIN: u8 = 5;

    /// Note velocities 0..=15.
    pub const ARP_VEL: u8 = 6;
    pub const BASS_VEL: u8 = 10;
    pub const HAT_OFFBEAT_VEL: u8 = 4;
    pub const HAT_BEAT3_VEL: u8 = 6;
    pub const GLOVES_VEL: u8 = 7;
    pub const BOOTS_VEL: u8 = 6;

    /// Default hazard bed level (0 disables the bed).
    pub const BED_LEVEL: u8 = 2;
}

/// The chord progression as 1-based scale-degree roots (§6.1).
#[must_use]
pub const fn progression(difficulty: Difficulty) -> [u8; 4] {
    match difficulty {
        Difficulty::Easy => [1, 5, 6, 4],       // I – V – vi – IV
        Difficulty::Medium => [1, 7, 4, 1],     // I – ♭VII – IV – I
        Difficulty::Hard => [1, 4, 7, 1],       // i – IV – ♭VII – i
    }
}

const fn mode_for(difficulty: Difficulty) -> Mode {
    match difficulty {
        Difficulty::Easy => Mode::Ionian,
        Difficulty::Medium => Mode::Mixolydian,
        Difficulty::Hard => Mode::Dorian,
    }
}

const fn tonic_for(ability: AbilityReq) -> u8 {
    match ability {
        AbilityReq::None => 0, // C
        AbilityReq::Wall => 2, // D
        AbilityReq::Dash => 4, // E
        AbilityReq::Both => 7, // G
    }
}

/// Lead pulse duty: wide duties everywhere for a rounder, less nasal tone
/// (designer feedback 2026-08); hard rooms keep a slightly reedier quarter
/// duty as their edge.
const fn lead_duty(difficulty: Difficulty) -> Duty {
    match difficulty {
        Difficulty::Easy | Difficulty::Medium => Duty::Half,
        Difficulty::Hard => Duty::Quarter,
    }
}

/// Compose the complete track for one room.
#[must_use]
pub fn compose(inputs: &RoomMusicInputs) -> Track {
    let choice = derive_grid(&inputs.hazards, inputs.difficulty);
    let grid = choice.grid;
    let key = Key {
        tonic: PitchClass::new(tonic_for(inputs.ability_requirement)),
        mode: mode_for(inputs.difficulty),
    };
    let progression = progression(inputs.difficulty);

    let mut layers = vec!["base".to_owned()];
    if inputs.layer_gates.gloves {
        layers.push("gloves".to_owned());
    }
    if inputs.layer_gates.boots {
        layers.push("boots".to_owned());
    }

    let mut voices = vec![
        VoiceDef {
            name: "lead".to_owned(),
            kind: VoiceKind::Pulse {
                duty: lead_duty(inputs.difficulty),
            },
            layer: "base".to_owned(),
            gain: defaults::LEAD_GAIN,
        },
        VoiceDef {
            name: "arp".to_owned(),
            kind: VoiceKind::Pulse { duty: Duty::Half },
            layer: "base".to_owned(),
            gain: defaults::ARP_GAIN,
        },
        VoiceDef {
            name: "bass".to_owned(),
            kind: VoiceKind::Triangle,
            layer: "base".to_owned(),
            gain: defaults::BASS_GAIN,
        },
        VoiceDef {
            name: "perc".to_owned(),
            kind: VoiceKind::Noise,
            layer: "base".to_owned(),
            gain: defaults::PERC_GAIN,
        },
    ];
    if inputs.layer_gates.gloves {
        voices.push(VoiceDef {
            name: "gloves-arp".to_owned(),
            kind: VoiceKind::Triangle,
            layer: "gloves".to_owned(),
            gain: defaults::GLOVES_GAIN,
        });
    }
    if inputs.layer_gates.boots {
        voices.push(VoiceDef {
            name: "boots-arp".to_owned(),
            kind: VoiceKind::Pulse { duty: Duty::Eighth },
            layer: "boots".to_owned(),
            gain: defaults::BOOTS_GAIN,
        });
    }

    let mut notes = generate_melody(
        &inputs.slug,
        key,
        &grid,
        progression,
        inputs.coin_count,
        inputs.doors,
        "lead",
    );

    let bar_steps = grid.bar_steps();
    let eighth = grid.beat_steps / 2;
    for bar in 0..16_u32 {
        let chord = progression[(bar / 4) as usize];
        // The triad's three tones stacked ascending from the root in a given
        // octave: root, third above it, fifth above that.
        let stacked = |octave: i32| -> [crate::theory::Pitch; 3] {
            let classes = key.triad_pitch_classes(chord);
            let mut midi = (octave + 1) * 12 + i32::from(classes[0].semitones());
            let mut tones = [crate::theory::Pitch(0); 3];
            tones[0] = crate::theory::Pitch(midi as u8);
            for position in 1..3 {
                let mut next =
                    (octave + 1) * 12 + i32::from(classes[position].semitones());
                while next <= midi {
                    next += 12;
                }
                tones[position] = crate::theory::Pitch(next as u8);
                midi = next;
            }
            tones
        };
        let base = bar * bar_steps;

        // Arp: chord tones at quarter-note rate (slowed from 8ths, designer
        // feedback 2026-08), lowest-to-highest, restarting each bar, octave 3.
        let arp_tones = stacked(3);
        for beat in 0..4_u32 {
            notes.push(NoteEvent {
                voice: "arp".to_owned(),
                start_step: base + beat * grid.beat_steps,
                len_steps: grid.beat_steps,
                pitch: Some(arp_tones[(beat % 3) as usize]),
                vel: defaults::ARP_VEL,
            });
        }

        // Bass: root on beat 1 and fifth on beat 3, octave 2, half notes.
        let bass_tones = stacked(2);
        let half_note = grid.beat_steps * 2;
        notes.push(NoteEvent {
            voice: "bass".to_owned(),
            start_step: base,
            len_steps: half_note,
            pitch: Some(bass_tones[0]),
            vel: defaults::BASS_VEL,
        });
        notes.push(NoteEvent {
            voice: "bass".to_owned(),
            start_step: base + half_note,
            len_steps: half_note,
            pitch: Some(bass_tones[2]),
            vel: defaults::BASS_VEL,
        });

        // Perc hats, density by difficulty (designer feedback 2026-08:
        // sparse on easy rooms). Easy: soft ticks on beats 2 and 4 only.
        // Medium/hard: offbeat 8ths plus a beat-3 accent. Fire-hit
        // suppression happens live in the sequencer, which knows the hazard
        // clocks.
        for eighth_index in 0..8_u32 {
            let step = base + eighth_index * eighth;
            let vel = match inputs.difficulty {
                Difficulty::Easy => match eighth_index {
                    2 | 6 => Some(defaults::HAT_OFFBEAT_VEL),
                    _ => None,
                },
                Difficulty::Medium | Difficulty::Hard => {
                    if eighth_index % 2 == 1 {
                        Some(defaults::HAT_OFFBEAT_VEL)
                    } else if eighth_index == 4 {
                        Some(defaults::HAT_BEAT3_VEL)
                    } else {
                        None
                    }
                }
            };
            if let Some(vel) = vel {
                notes.push(NoteEvent {
                    voice: "perc".to_owned(),
                    start_step: step,
                    len_steps: 1,
                    pitch: None,
                    vel,
                });
            }
        }

        // Gloves counter-melody: broken chord tones on beats 2 and 4,
        // octave 4, one 8th long.
        if inputs.layer_gates.gloves {
            let gloves_tones = stacked(4);
            for (position, beat) in [1_u32, 3].iter().enumerate() {
                let cycle = (bar as usize * 2 + position) % 3;
                notes.push(NoteEvent {
                    voice: "gloves-arp".to_owned(),
                    start_step: base + beat * grid.beat_steps,
                    len_steps: eighth.max(1),
                    pitch: Some(gloves_tones[cycle]),
                    vel: defaults::GLOVES_VEL,
                });
            }
        }

        // Boots riff: run up the chord in the bar's last beat at 16th rate
        // (or step rate if the grid is coarser).
        if inputs.layer_gates.boots {
            let boots_tones = stacked(4);
            let spacing = (grid.beat_steps / 4).max(1);
            let count = (grid.beat_steps / spacing).min(4);
            for position in 0..count {
                let pitch = if position < 3 {
                    boots_tones[position as usize]
                } else {
                    crate::theory::Pitch(boots_tones[0].midi() + 12)
                };
                notes.push(NoteEvent {
                    voice: "boots-arp".to_owned(),
                    start_step: base + 3 * grid.beat_steps + position * spacing,
                    len_steps: spacing,
                    pitch: Some(pitch),
                    vel: defaults::BOOTS_VEL,
                });
            }
        }
    }

    // Hazard voices (§6.6): sounds only, never times.
    let hazard_voices = inputs
        .hazards
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if choice.locked.contains(&index) {
                let windup_steps =
                    div_round(crate::tempo::WARNING_TICKS, grid.step_ticks).clamp(1, 4);
                let windup_degrees: Vec<i8> = match windup_steps {
                    1 => vec![5],
                    2 => vec![5, 7],
                    3 => vec![5, 6, 7],
                    _ => vec![3, 5, 6, 7],
                };
                HazardVoice {
                    index,
                    locked: true,
                    windup_steps,
                    windup_degrees,
                    fire_hit: NoiseTimbre::Snare,
                    fire_stab: Some(key.tonic_in_octave(2)),
                    active_bed_level: defaults::BED_LEVEL,
                }
            } else {
                HazardVoice {
                    index,
                    locked: false,
                    windup_steps: 0,
                    windup_degrees: Vec::new(),
                    fire_hit: NoiseTimbre::Crash,
                    fire_stab: None,
                    active_bed_level: defaults::BED_LEVEL,
                }
            }
        })
        .collect();

    Track {
        slug: inputs.slug.clone(),
        hand_tuned: false,
        grid,
        key,
        layers,
        voices,
        notes,
        hazard_voices,
    }
}

const fn div_round(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}
