//! §10.6: the offline renderer and a block-driven sequencer (as the live
//! callback drives it) must produce byte-identical output — "sequencer shared
//! by both paths" is a tested fact, not an aspiration.

use downwards_audio::{
    AbilityMask, AbilityReq, Difficulty, DoorSet, HazardTiming, LayerGates, RoomMusicInputs,
    Sequencer, compose, offline::render, sample_index_for_tick,
};

#[test]
fn offline_render_equals_blockwise_render_at_odd_block_sizes() {
    let inputs = RoomMusicInputs {
        slug: "paths-agree".to_owned(),
        difficulty: Difficulty::Hard,
        ability_requirement: AbilityReq::Both,
        coin_count: 4,
        doors: DoorSet {
            ceiling: true,
            floor: true,
            ..DoorSet::default()
        },
        hazards: vec![
            HazardTiming {
                period: 70,
                active: 20,
                phase: 10,
            },
            HazardTiming {
                period: 157,
                active: 40,
                phase: 3,
            },
        ],
        layer_gates: LayerGates {
            gloves: true,
            boots: true,
        },
    };
    let track = compose(&inputs);
    let abilities = AbilityMask {
        gloves: true,
        boots: false,
    };
    let rate = 44_100;
    let reference = render(&track, &inputs.hazards, abilities, 1, rate);

    for block_size in [48_usize, 1024] {
        let mut sequencer = Sequencer::new(&track, &inputs.hazards, rate);
        sequencer.set_layer_gains_immediate(abilities);
        let mut produced = vec![0.0_f32; reference.len()];
        let mut cursor = 0;
        while cursor < produced.len() {
            let end = (cursor + block_size).min(produced.len());
            sequencer.render(&mut produced[cursor..end]);
            cursor = end;
        }
        assert!(
            reference
                .iter()
                .zip(&produced)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "block size {block_size} must render byte-identically to the offline path"
        );
    }
}

#[test]
fn seek_restarts_in_lockstep_with_a_room_tick() {
    // A hard snap to tick T must continue exactly like a fresh session that
    // rendered up to T (modulo the killed voices and the 5 ms fade-in): the
    // tick-event schedule after the seek point is identical.
    let inputs = RoomMusicInputs {
        slug: "seek-check".to_owned(),
        difficulty: Difficulty::Easy,
        ability_requirement: AbilityReq::None,
        coin_count: 0,
        doors: DoorSet::default(),
        hazards: vec![HazardTiming {
            period: 90,
            active: 30,
            phase: 0,
        }],
        layer_gates: LayerGates::default(),
    };
    let track = compose(&inputs);
    let rate = 44_100;
    let seek_tick = 90_u64; // exactly one hazard period in
    let seek_sample = sample_index_for_tick(seek_tick, rate);

    let mut fresh = Sequencer::new(&track, &inputs.hazards, rate);
    fresh.set_layer_gains_immediate(AbilityMask::default());
    let mut sought = fresh.clone();
    sought.seek(seek_sample);

    // Because the fixture's period equals a whole number of grid steps and
    // φ = 0, the state at tick 90 equals the state at tick 0 for scheduling
    // purposes; after the fade-in settles both must emit the hazard fire at
    // the same offset. Render one period from each and compare onsets.
    // Render ticks 90..270 so the tick-180 fire sits inside the window.
    let span = (sample_index_for_tick(270, rate) - seek_sample) as usize;
    let mut a = vec![0.0_f32; span];
    let mut b = vec![0.0_f32; span];
    // Advance the fresh sequencer to the seek point first.
    let mut skip = vec![0.0_f32; seek_sample as usize];
    fresh.render(&mut skip);
    fresh.render(&mut a);
    sought.render(&mut b);

    // The next hazard fire is at tick 180; both renders start at tick 90, so
    // the fire must produce an energy onset at the same offset in each.
    let expected = (sample_index_for_tick(180, rate) - seek_sample) as usize;
    let rms = |samples: &[f32]| -> f64 {
        let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        (sum / samples.len().max(1) as f64).sqrt()
    };
    // The strongest local energy jump in a ±512-sample neighbourhood must
    // sit at the expected fire sample, and be a clear rise. (The 2026-08
    // gentler mix no longer saturates the clipper, so ordinary melodic
    // rests further out produce comparable short-RMS jumps — the
    // neighbourhood is kept tight around the fire, whose *position* is the
    // claim under test.)
    let jump_at = |samples: &[f32], position: usize| {
        rms(&samples[position..position + 256]) / rms(&samples[position - 256..position]).max(1e-9)
    };
    let best_position = |samples: &[f32]| {
        (0..5)
            .map(|index| expected - 512 + index * 256)
            .max_by(|&x, &y| {
                jump_at(samples, x)
                    .partial_cmp(&jump_at(samples, y))
                    .unwrap()
            })
            .unwrap()
    };
    assert_eq!(best_position(&a), expected, "fresh path must fire at tick 180");
    assert_eq!(
        best_position(&b),
        expected,
        "sought path must fire at tick 180 in lockstep"
    );
    assert!(jump_at(&a, expected) > 1.25, "fresh fire must be a clear rise");
    assert!(jump_at(&b, expected) > 1.25, "sought fire must be a clear rise");
}
