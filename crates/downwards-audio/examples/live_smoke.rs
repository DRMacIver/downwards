//! Manual smoke: open the default output device, play two seconds of a
//! composed room with a mid-run ability grant. Not run in CI.
fn main() {
    let inputs = downwards_audio::RoomMusicInputs {
        slug: "two-clock-fork-c".into(),
        difficulty: downwards_audio::Difficulty::Hard,
        ability_requirement: downwards_audio::AbilityReq::None,
        coin_count: 3,
        doors: downwards_audio::DoorSet { west: true, floor: true, ..Default::default() },
        hazards: vec![
            downwards_audio::HazardTiming { period: 70, active: 20, phase: 10 },
            downwards_audio::HazardTiming { period: 157, active: 40, phase: 0 },
        ],
        layer_gates: downwards_audio::LayerGates { gloves: false, boots: true },
    };
    let track = downwards_audio::compose(&inputs);
    let engine = downwards_audio::AudioEngine::new(false);
    println!("engine active: {}", engine.is_active());
    engine.enter_room(&track, &inputs.hazards, downwards_audio::AbilityMask::default());
    let start = std::time::Instant::now();
    let mut tick = 0u64;
    while start.elapsed().as_secs_f64() < 2.0 {
        engine.publish_tick(tick);
        tick += 1;
        std::thread::sleep(std::time::Duration::from_micros(16_667));
        if tick == 60 {
            engine.set_abilities(downwards_audio::AbilityMask { gloves: false, boots: true });
        }
    }
    println!("smoke ok, published {tick} ticks");
}
