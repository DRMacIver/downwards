//! Pitch, scale, and chord arithmetic for the procedural soundtrack.
//!
//! Everything here is pure integer math over MIDI note numbers. `C4` is
//! middle C = MIDI 60.

use std::fmt;

/// One of the twelve pitch classes, `0 = C` through `11 = B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PitchClass(u8);

const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl PitchClass {
    #[must_use]
    pub const fn new(semitones: u8) -> Self {
        Self(semitones % 12)
    }

    #[must_use]
    pub const fn semitones(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        PITCH_CLASS_NAMES[usize::from(self.0)]
    }

    /// Parse a pitch-class name using sharps (`C`, `C#`, ... `B`).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        PITCH_CLASS_NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .map(|index| Self(index as u8))
    }
}

impl fmt::Display for PitchClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A concrete pitch as a MIDI note number (`C4` = 60).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

impl Pitch {
    #[must_use]
    pub const fn midi(self) -> u8 {
        self.0
    }

    /// Frequency in Hz under 12-TET with A4 = 440.
    #[must_use]
    pub fn frequency_hz(self) -> f64 {
        440.0 * 2.0_f64.powf((f64::from(self.0) - 69.0) / 12.0)
    }

    #[must_use]
    pub fn pitch_class(self) -> PitchClass {
        PitchClass::new(self.0 % 12)
    }

    /// Octave in scientific pitch notation (C4 = 60 is octave 4).
    #[must_use]
    pub const fn octave(self) -> i32 {
        self.0 as i32 / 12 - 1
    }

    /// Parse a note name with octave, e.g. `C4`, `F#3`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let split = name
            .char_indices()
            .find(|(_, character)| character.is_ascii_digit() || *character == '-')
            .map(|(index, _)| index)?;
        let (class_name, octave_text) = name.split_at(split);
        let class = PitchClass::parse(class_name)?;
        let octave: i32 = octave_text.parse().ok()?;
        let midi = (octave + 1) * 12 + i32::from(class.semitones());
        u8::try_from(midi).ok().map(Self)
    }

    #[must_use]
    pub fn name(self) -> String {
        format!("{}{}", self.pitch_class().name(), self.octave())
    }
}

impl fmt::Display for Pitch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name())
    }
}

/// The three modes used by the difficulty ladder (§6.1 of the soundtrack
/// spec): easy = Ionian, medium = Mixolydian, hard = Dorian.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Ionian,
    Mixolydian,
    Dorian,
}

impl Mode {
    /// Semitone offsets of the seven scale degrees from the tonic.
    #[must_use]
    pub const fn intervals(self) -> [u8; 7] {
        match self {
            Self::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Self::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Self::Dorian => [0, 2, 3, 5, 7, 9, 10],
        }
    }

    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Ionian => "ionian",
            Self::Mixolydian => "mixolydian",
            Self::Dorian => "dorian",
        }
    }

    #[must_use]
    pub fn parse(keyword: &str) -> Option<Self> {
        match keyword {
            "ionian" => Some(Self::Ionian),
            "mixolydian" => Some(Self::Mixolydian),
            "dorian" => Some(Self::Dorian),
            _ => None,
        }
    }
}

/// A tonal centre: tonic pitch class plus mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

impl Key {
    /// MIDI pitch of the tonic in the given scientific octave.
    #[must_use]
    pub fn tonic_in_octave(self, octave: i32) -> Pitch {
        let midi = (octave + 1) * 12 + i32::from(self.tonic.semitones());
        Pitch(u8::try_from(midi).expect("tonic octave is in MIDI range"))
    }

    /// Pitch of an absolute scale index. Index 0 is the tonic in octave 4;
    /// positive indices ascend the scale, negative descend.
    #[must_use]
    pub fn scale_pitch(self, index: i32) -> Pitch {
        let intervals = self.mode.intervals();
        let octave = index.div_euclid(7);
        let degree = index.rem_euclid(7) as usize;
        let midi = i32::from(self.tonic_in_octave(4).midi())
            + octave * 12
            + i32::from(intervals[degree]);
        Pitch(u8::try_from(midi).expect("scale walk stays in MIDI range"))
    }

    /// Pitch of a 1-based scale degree in the given octave (degree 1 = tonic,
    /// degree 8 = tonic an octave up).
    #[must_use]
    pub fn degree_in_octave(self, degree: i8, octave: i32) -> Pitch {
        assert!(degree >= 1, "scale degrees are 1-based");
        let zero_based = i32::from(degree) - 1;
        let extra_octaves = zero_based.div_euclid(7);
        let interval = self.mode.intervals()[zero_based.rem_euclid(7) as usize];
        let midi = (octave + extra_octaves + 1) * 12
            + i32::from(self.tonic.semitones())
            + i32::from(interval);
        Pitch(u8::try_from(midi).expect("degree octave is in MIDI range"))
    }

    /// Chord tones (root, third, fifth stacked in-scale) of the triad rooted
    /// at the 1-based scale degree, as absolute scale indices relative to the
    /// tonic-in-octave-4 origin used by [`Self::scale_pitch`].
    #[must_use]
    pub fn triad_scale_indices(degree: u8) -> [i32; 3] {
        assert!((1..=7).contains(&degree), "triads root on degrees 1..=7");
        let root = i32::from(degree) - 1;
        [root, root + 2, root + 4]
    }

    /// The triad's three pitch classes.
    #[must_use]
    pub fn triad_pitch_classes(self, degree: u8) -> [PitchClass; 3] {
        Self::triad_scale_indices(degree).map(|index| self.scale_pitch(index).pitch_class())
    }
}

/// FNV-1a 64-bit, the repository's stable content hash.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// Deterministic xorshift64* stream; the composer's only randomness source.
#[derive(Clone, Debug)]
pub struct XorShift64Star(u64);

impl XorShift64Star {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        // xorshift64* requires a nonzero state.
        Self(if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform draw in `0..bound` (`bound > 0`).
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    /// Bernoulli draw with probability `percent / 100`.
    pub fn percent(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// Weighted choice over `(item, weight)` pairs; total weight must be > 0.
    pub fn pick_weighted<T: Copy>(&mut self, options: &[(T, u32)]) -> T {
        let total: u64 = options.iter().map(|(_, weight)| u64::from(*weight)).sum();
        assert!(total > 0, "weighted choice needs positive total weight");
        let mut roll = self.below(total);
        for (item, weight) in options {
            if roll < u64::from(*weight) {
                return *item;
            }
            roll -= u64::from(*weight);
        }
        unreachable!("roll is below the total weight")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_round_trip_and_c4_is_middle_c() {
        assert_eq!(Pitch::parse("C4"), Some(Pitch(60)));
        assert_eq!(Pitch::parse("F#3"), Some(Pitch(54)));
        assert_eq!(Pitch::parse("C-1"), Some(Pitch(0)));
        for midi in 0..=127 {
            let pitch = Pitch(midi);
            assert_eq!(Pitch::parse(&pitch.name()), Some(pitch));
        }
        assert_eq!(Pitch::parse("H4"), None);
        assert_eq!(Pitch::parse("C"), None);
    }

    #[test]
    fn modes_carry_their_signature_intervals() {
        assert_eq!(Mode::Ionian.intervals()[6], 11);
        assert_eq!(Mode::Mixolydian.intervals()[6], 10);
        assert_eq!(Mode::Dorian.intervals()[2], 3);
        assert_eq!(Mode::Dorian.intervals()[5], 9, "dorian keeps the raised 6th");
    }

    #[test]
    fn scale_pitch_walks_octaves() {
        let key = Key {
            tonic: PitchClass::new(0),
            mode: Mode::Ionian,
        };
        assert_eq!(key.scale_pitch(0), Pitch(60));
        assert_eq!(key.scale_pitch(7), Pitch(72));
        assert_eq!(key.scale_pitch(-7), Pitch(48));
        assert_eq!(key.scale_pitch(4), Pitch(67));
        assert_eq!(key.degree_in_octave(5, 3), Pitch(55));
        assert_eq!(key.degree_in_octave(8, 3), Pitch(60));
    }

    #[test]
    fn dorian_fourth_triad_is_major() {
        // Dorian's raised 6th makes the IV chord major: D dorian IV = G B D.
        let key = Key {
            tonic: PitchClass::parse("D").unwrap(),
            mode: Mode::Dorian,
        };
        let classes = key.triad_pitch_classes(4);
        assert_eq!(
            classes.map(|class| class.name().to_owned()),
            ["G".to_owned(), "B".to_owned(), "D".to_owned()]
        );
    }

    #[test]
    fn xorshift_is_deterministic_and_zero_safe() {
        let mut a = XorShift64Star::new(42);
        let mut b = XorShift64Star::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut zero = XorShift64Star::new(0);
        assert_ne!(zero.next_u64(), 0);
    }

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
