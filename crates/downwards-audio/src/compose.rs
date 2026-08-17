//! The room → `Track` composition mapping (§6 of the soundtrack spec).
//!
//! `compose` is pure, total, and deterministic: the same `RoomMusicInputs`
//! always yields a byte-identical serialized track. The only randomness is
//! the melody walk, seeded from the slug.

use crate::{
    melody::{DoorSet, generate_melody},
    tempo::{Difficulty, HazardTiming, derive_grid},
    theory::{Key, Mode, PitchClass, XorShift64Star, fnv1a64},
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
    /// Mixer gains 0..=15 (§6.3). The bass gain dropped 10 → 8 in the
    /// round-3 expressiveness pass: the part is far busier now, and the brief
    /// is "a bass at a low level underlying the music".
    pub const LEAD_GAIN: u8 = 7;
    pub const ARP_GAIN: u8 = 5;
    pub const BASS_GAIN: u8 = 8;
    pub const PERC_GAIN: u8 = 3;
    pub const GLOVES_GAIN: u8 = 6;
    pub const BOOTS_GAIN: u8 = 5;

    /// Note velocities 0..=15. Since the round-3 pass these are *bases* that
    /// the phrase-arc/intensity shaping moves around, never flat values.
    pub const ARP_BEAT_VELS: [u8; 4] = [7, 5, 6, 5];
    pub const HAT_OFFBEAT_VEL: u8 = 4;
    pub const HAT_BEAT3_VEL: u8 = 6;
    pub const HAT_DOWNBEAT_VEL: u8 = 5;
    pub const GLOVES_VELS: [u8; 2] = [7, 6];
    pub const BOOTS_VELS: [u8; 4] = [5, 6, 6, 7];

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

/// Seed salt for the accompaniment RNG stream (bass pattern choice); kept
/// apart from the melody stream so bass tuning never reshuffles the lead.
const ACCOMPANIMENT_SALT: u64 = 0x00ba_5511_e50f_f00d_u64;

/// The 16-bar tension/release arc (§ round-3 critique: "energising without
/// exhausting"): bars 9–10 breathe (a layer drops, percussion thins), bars
/// 13–15 peak, bar 16 is the turnaround that rebuilds into the loop start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BarRole {
    Normal,
    Breathe,
    Peak,
    Turnaround,
}

const fn bar_role(bar: u32) -> BarRole {
    match bar {
        8 | 9 => BarRole::Breathe,
        12..=14 => BarRole::Peak,
        15 => BarRole::Turnaround,
        _ => BarRole::Normal,
    }
}

/// One bass pattern event: (offset in eighth-note units from the bar start,
/// length in units, which tone, base velocity).
type BassEvent = (u32, u32, BassTone, u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BassTone {
    Root,
    RootLow,
    Fifth,
}

/// Seeded rhythmic bass vocabulary, keyed to difficulty: calmer patterns on
/// easy rooms, driving syncopated grooves on hard. Bars are 8 eighth-units.
const fn bass_vocabulary(difficulty: Difficulty) -> &'static [&'static [BassEvent]] {
    use BassTone::{Fifth, Root, RootLow};
    match difficulty {
        Difficulty::Easy => &[
            &[(0, 6, Root, 10), (6, 2, Fifth, 8)],
            &[(0, 4, Root, 10), (4, 4, Fifth, 8)],
            &[(0, 8, Root, 9)],
        ],
        Difficulty::Medium => &[
            &[(0, 3, Root, 11), (3, 2, Fifth, 8), (5, 1, Root, 7), (6, 2, Root, 9)],
            &[(0, 2, Root, 11), (2, 1, Root, 7), (3, 2, Fifth, 9), (6, 2, Root, 8)],
            &[(0, 3, Root, 10), (3, 3, Fifth, 9), (6, 2, Root, 8)],
        ],
        Difficulty::Hard => &[
            &[
                (0, 1, Root, 11),
                (1, 1, Root, 7),
                (2, 1, Fifth, 8),
                (3, 1, Root, 7),
                (4, 1, Root, 10),
                (5, 1, Fifth, 8),
                (6, 1, Root, 7),
                (7, 1, Fifth, 8),
            ],
            &[
                (0, 2, Root, 11),
                (2, 1, RootLow, 8),
                (3, 1, Root, 7),
                (4, 1, Fifth, 9),
                (5, 1, Root, 7),
                (6, 2, Root, 9),
            ],
            &[
                (0, 1, Root, 11),
                (1, 1, RootLow, 7),
                (2, 2, Root, 9),
                (4, 1, Fifth, 9),
                (5, 1, Root, 7),
                (6, 1, Fifth, 8),
                (7, 1, Root, 7),
            ],
        ],
    }
}

/// The breathing-bar bass: long, calm, low in every difficulty.
const BREATHE_BASS: &[BassEvent] = &[
    (0, 4, BassTone::Root, 8),
    (4, 4, BassTone::Fifth, 7),
];

/// Fold a scale-lattice index by octaves until its pitch sits in the bass
/// register (MIDI 26..=50), where the triangle underpins without muddying.
fn fold_bass_index(key: Key, mut index: i32) -> i32 {
    while key.scale_pitch(index).midi() > 50 {
        index -= 7;
    }
    while key.scale_pitch(index).midi() < 26 {
        index += 7;
    }
    index
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
    let sixteenth = grid.beat_steps / 4;
    let mut accompaniment =
        XorShift64Star::new(fnv1a64(inputs.slug.as_bytes()) ^ ACCOMPANIMENT_SALT);
    let vocabulary = bass_vocabulary(inputs.difficulty);
    // One seeded pattern per 4-bar chord block, chosen up front in a fixed
    // order so later rules never reshuffle earlier choices.
    let block_patterns: [usize; 4] = std::array::from_fn(|_| {
        accompaniment.below(vocabulary.len() as u64) as usize
    });
    for bar in 0..16_u32 {
        let role = bar_role(bar);
        let chord = progression[(bar / 4) as usize];
        let next_chord = progression[(((bar + 1) % 16) / 4) as usize];
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
        // Round-3: metric velocity shape instead of a flat level, +1 in the
        // peak bars, and the whole layer drops out in the breathing bars.
        if role != BarRole::Breathe {
            let arp_tones = stacked(3);
            for beat in 0..4_u32 {
                let mut vel = defaults::ARP_BEAT_VELS[beat as usize];
                if role == BarRole::Peak {
                    vel += 1;
                }
                notes.push(NoteEvent {
                    voice: "arp".to_owned(),
                    start_step: base + beat * grid.beat_steps,
                    len_steps: grid.beat_steps,
                    pitch: Some(arp_tones[(beat % 3) as usize]),
                    vel,
                });
            }
        }

        // Bass (round-3 rewrite): a seeded rhythmic pattern per chord block
        // instead of root–fifth half notes, with walking approaches into
        // chord changes, a calm override in the breathing bars, and a
        // turnaround walk home in bar 16. Kept low: gain 8, velocities <= 12.
        let root_index = fold_bass_index(key, i32::from(chord) - 1 - 14);
        let fifth_index = fold_bass_index(key, i32::from(chord) - 1 + 4 - 14);
        let next_root_index = fold_bass_index(key, i32::from(next_chord) - 1 - 14);
        let tone_index = |tone: BassTone| match tone {
            BassTone::Root => root_index,
            BassTone::RootLow => {
                let low = root_index - 7;
                if key.scale_pitch(low).midi() < 26 {
                    root_index
                } else {
                    low
                }
            }
            BassTone::Fifth => fifth_index,
        };
        // Stepwise approach tones into a target: one and two scale steps
        // below it (or above, when below would leave the register).
        let approach_indices = |target: i32| -> [i32; 2] {
            if key.scale_pitch(target - 2).midi() >= 26 {
                [target - 2, target - 1]
            } else {
                [target + 2, target + 1]
            }
        };
        let chord_changes = next_chord != chord;
        let arc_shift: i32 = match role {
            BarRole::Peak | BarRole::Turnaround => 1,
            BarRole::Breathe => -1,
            BarRole::Normal => 0,
        };
        let mut push_bass = |unit: u32, len_units: u32, index: i32, vel: u8| {
            notes.push(NoteEvent {
                voice: "bass".to_owned(),
                start_step: base + unit * eighth,
                len_steps: (len_units * eighth).max(1),
                pitch: Some(key.scale_pitch(index)),
                vel: (i32::from(vel) + arc_shift).clamp(1, 12) as u8,
            });
        };
        if role == BarRole::Turnaround {
            // Turnaround: walk home so the loop rebuilds into its start.
            let [far, near] = approach_indices(next_root_index);
            push_bass(0, 2, root_index, 10);
            push_bass(2, 2, fifth_index, 9);
            push_bass(4, 2, far, 9);
            push_bass(6, 2, near, 10);
        } else {
            let pattern = if role == BarRole::Breathe {
                BREATHE_BASS
            } else {
                vocabulary[block_patterns[(bar / 4) as usize]]
            };
            // Walking approach into the chord change: the last beat of a
            // block's final bar steps into the next chord's root.
            let walk_last_beat = chord_changes && bar % 4 == 3 && role == BarRole::Normal;
            for &(unit, len_units, tone, vel) in pattern {
                if walk_last_beat && unit >= 6 {
                    continue;
                }
                push_bass(unit, len_units, tone_index(tone), vel);
            }
            if walk_last_beat {
                let [far, near] = approach_indices(next_root_index);
                push_bass(6, 1, far, 8);
                push_bass(7, 1, near, 9);
            }
        }

        // Perc hats, density by difficulty (designer feedback 2026-08:
        // sparse on easy rooms). Easy: soft ticks on beats 2 and 4 only.
        // Medium/hard: offbeat 8ths plus a beat-3 accent. Round-3 intensity
        // arc: breathing bars thin the kit, peak bars add a downbeat tick,
        // and the turnaround bar carries a small rising fill. Fire-hit
        // suppression happens live in the sequencer, which knows the hazard
        // clocks.
        let mut push_hat = |step: u32, vel: u8| {
            notes.push(NoteEvent {
                voice: "perc".to_owned(),
                start_step: step,
                len_steps: 1,
                pitch: None,
                vel,
            });
        };
        for eighth_index in 0..8_u32 {
            let step = base + eighth_index * eighth;
            let vel = match inputs.difficulty {
                Difficulty::Easy => match (role, eighth_index) {
                    (BarRole::Breathe, _) => None,
                    (_, 2 | 6) => Some(defaults::HAT_OFFBEAT_VEL),
                    _ => None,
                },
                Difficulty::Medium | Difficulty::Hard => match role {
                    BarRole::Breathe => match eighth_index {
                        2 | 6 => Some(defaults::HAT_OFFBEAT_VEL - 1),
                        _ => None,
                    },
                    _ => {
                        if eighth_index % 2 == 1 {
                            Some(defaults::HAT_OFFBEAT_VEL)
                        } else if eighth_index == 4 {
                            Some(defaults::HAT_BEAT3_VEL)
                        } else if eighth_index == 0 && role == BarRole::Peak {
                            Some(defaults::HAT_DOWNBEAT_VEL)
                        } else {
                            None
                        }
                    }
                },
            };
            if let Some(vel) = vel {
                push_hat(step, vel);
            }
        }
        if role == BarRole::Turnaround {
            // Rising fill on the last beat, rebuilding into the loop start.
            let beat4 = base + 3 * grid.beat_steps;
            if inputs.difficulty == Difficulty::Easy {
                push_hat(beat4, defaults::HAT_DOWNBEAT_VEL);
            } else {
                push_hat(beat4, defaults::HAT_DOWNBEAT_VEL);
                if sixteenth > 0 {
                    push_hat(beat4 + sixteenth, defaults::HAT_DOWNBEAT_VEL);
                    push_hat(beat4 + 3 * sixteenth, defaults::HAT_BEAT3_VEL);
                }
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
                    vel: defaults::GLOVES_VELS[position],
                });
            }
        }

        // Boots riff: run up the chord in the bar's last beat at 16th rate
        // (or step rate if the grid is coarser). Rests in the breathing bars
        // and ramps its velocities so the run pushes into the next downbeat.
        if inputs.layer_gates.boots && role != BarRole::Breathe {
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
                    vel: defaults::BOOTS_VELS[position as usize],
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
