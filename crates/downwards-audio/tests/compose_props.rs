//! §10.2: property tests over arbitrary valid hazard sets, using a seeded
//! deterministic generator (the workspace has no property-testing dependency,
//! and the composer itself must stay dependency-free, so the cases are
//! enumerated from a fixed xorshift stream — fully reproducible).

use downwards_audio::{
    AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs, compose,
    derive_grid, fnv1a64,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

fn arbitrary_hazards(rng: &mut Rng) -> Vec<HazardTiming> {
    let count = rng.below(6) as usize;
    (0..count)
        .map(|_| {
            let period = 1 + rng.below(400) as u32;
            let active = 1 + rng.below(u64::from(period)) as u32;
            let phase = rng.below(u64::from(period)) as u32;
            HazardTiming {
                period,
                active,
                phase,
            }
        })
        .collect()
}

fn arbitrary_inputs(rng: &mut Rng, case: usize) -> RoomMusicInputs {
    let difficulty = match rng.below(3) {
        0 => Difficulty::Easy,
        1 => Difficulty::Medium,
        _ => Difficulty::Hard,
    };
    let ability = match rng.below(4) {
        0 => AbilityReq::None,
        1 => AbilityReq::Wall,
        2 => AbilityReq::Dash,
        _ => AbilityReq::Both,
    };
    RoomMusicInputs {
        slug: format!("prop-room-{case}"),
        difficulty,
        ability_requirement: ability,
        coin_count: rng.below(6) as u32,
        doors: DoorSet {
            west: rng.below(2) == 1,
            east: rng.below(2) == 1,
            ceiling: rng.below(2) == 1,
            floor: rng.below(2) == 1,
        },
        hazards: arbitrary_hazards(rng),
        layer_gates: LayerGates {
            gloves: rng.below(2) == 1,
            boots: rng.below(2) == 1,
        },
    }
}

#[test]
fn grid_derivation_is_total_deterministic_and_exact() {
    let mut rng = Rng(0x00d1_ce00_5eed_0001);
    for case in 0..400 {
        let inputs = arbitrary_inputs(&mut rng, case);
        let first = derive_grid(&inputs.hazards, inputs.difficulty);
        let second = derive_grid(&inputs.hazards, inputs.difficulty);
        assert_eq!(first, second, "derivation must be deterministic");

        let step = u64::from(first.grid.step_ticks);
        let offset = u64::from(first.grid.grid_offset);
        assert!(offset < step);
        for &index in &first.locked {
            let hazard = inputs.hazards[index];
            assert!(
                u64::from(hazard.period).is_multiple_of(step),
                "locked hazard period must be a grid multiple"
            );
            assert_eq!(
                u64::from(hazard.fire_tick()) % step,
                offset,
                "locked hazard fire residue must equal the grid offset"
            );
        }
    }
}

#[test]
fn unlocked_hazards_never_get_pitched_material() {
    let mut rng = Rng(0x00d1_ce00_5eed_0002);
    for case in 0..300 {
        let inputs = arbitrary_inputs(&mut rng, case);
        let track = compose(&inputs);
        let choice = derive_grid(&inputs.hazards, inputs.difficulty);
        assert_eq!(track.hazard_voices.len(), inputs.hazards.len());
        for voice in &track.hazard_voices {
            if choice.locked.contains(&voice.index) {
                assert!(voice.locked);
                assert!(voice.fire_stab.is_some());
                assert_eq!(voice.windup_degrees.len(), voice.windup_steps as usize);
                assert!((1..=4).contains(&voice.windup_steps));
            } else {
                assert!(!voice.locked, "off-grid hazards must be noise-only");
                assert!(voice.fire_stab.is_none());
                assert!(voice.windup_degrees.is_empty());
            }
        }
    }
}

#[test]
fn compose_twice_is_byte_identical() {
    let mut rng = Rng(0x00d1_ce00_5eed_0003);
    for case in 0..150 {
        let inputs = arbitrary_inputs(&mut rng, case);
        let first = compose(&inputs).serialize();
        let second = compose(&inputs).serialize();
        assert_eq!(first, second);
        // The digest is a pure function of the body.
        assert_eq!(fnv1a64(first.as_bytes()), fnv1a64(second.as_bytes()));
    }
}

#[test]
fn composed_tracks_reference_only_declared_voices_and_layers() {
    let mut rng = Rng(0x00d1_ce00_5eed_0004);
    for case in 0..150 {
        let inputs = arbitrary_inputs(&mut rng, case);
        let track = compose(&inputs);
        for note in &track.notes {
            let voice = track
                .voices
                .iter()
                .find(|voice| voice.name == note.voice)
                .expect("note voice is declared");
            assert!(track.layers.contains(&voice.layer));
            assert!(note.start_step < track.grid.loop_steps());
            assert!(note.vel <= 15);
            assert!(note.len_steps >= 1);
        }
        // Layer voices exist exactly when gated.
        assert_eq!(
            track.layers.contains(&"gloves".to_owned()),
            inputs.layer_gates.gloves
        );
        assert_eq!(
            track.layers.contains(&"boots".to_owned()),
            inputs.layer_gates.boots
        );
    }
}
