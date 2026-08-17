//! Thin soundtrack integration: watches the live simulation and drives the
//! `downwards-audio` engine. Music starts/switches on room entry, inventory
//! layers follow the run abilities, hazard voices are synthesized in
//! lockstep with the room clock, and F3 mutes.

use downwards_audio::{AbilityMask, AudioEngine, HazardTiming, Track};
use downwards_core::Simulation;

pub struct AudioDirector {
    engine: AudioEngine,
    muted: bool,
    current_room: Option<String>,
}

impl AudioDirector {
    pub fn new(no_audio: bool) -> Self {
        // The audible wind-up gesture is derived from the same telegraph
        // length the renderer draws; keep the two constants in lockstep.
        debug_assert_eq!(
            u64::from(downwards_audio::WARNING_TICKS),
            crate::TIMED_HAZARD_WARNING_TICKS
        );
        Self {
            engine: AudioEngine::new(no_audio),
            muted: false,
            current_room: None,
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.engine.set_muted(self.muted);
    }

    /// Call once per frame with whether the fixed-step simulation advanced
    /// since the last call (menus and pause screens duck the music).
    pub fn observe(&mut self, simulation: &Simulation, stepped: bool) {
        if !self.engine.is_active() {
            return;
        }
        let room = simulation.room();
        let abilities = simulation.abilities();
        let mask = AbilityMask {
            gloves: abilities.wall_jump,
            boots: abilities.dash,
        };

        let room_key = room.id().to_owned();
        if self.current_room.as_deref() != Some(&room_key) {
            self.current_room = Some(room_key);
            let hazards: Vec<HazardTiming> = room
                .timed_hazards()
                .iter()
                .map(|hazard| HazardTiming {
                    period: hazard.period_ticks(),
                    active: hazard.active_ticks(),
                    phase: hazard.phase_ticks(),
                })
                .collect();
            let track = room_track(room, &hazards);
            self.engine.enter_room(&track, &hazards, mask);
        }

        self.engine.set_abilities(mask);
        self.engine.set_paused(!stepped);
        self.engine.publish_tick(simulation.room_tick());
    }
}

/// Resolve the room's track: the checked-in artifact when one exists for the
/// room's slug (dungeon v2 rooms carry their rooms-v2 slug as the room
/// name), otherwise compose in-memory from the live room with the metadata
/// defaults (medium difficulty, no ability requirement).
fn room_track(room: &downwards_core::Room, hazards: &[HazardTiming]) -> Track {
    if let Some(source) = downwards_content::generated_tracks::track_source(room.name()) {
        match Track::parse(source) {
            Ok((track, _)) => match track.check_hazard_sync(hazards) {
                Ok(()) => return track,
                Err(error) => {
                    eprintln!("track for {} lost hazard sync: {error}", room.name());
                }
            },
            Err(error) => eprintln!("track for {} failed to parse: {error}", room.name()),
        }
    }
    let doors = {
        let mut doors = downwards_audio::DoorSet::default();
        for door in room.doors() {
            match door.side {
                downwards_core::BoundarySide::Left => doors.west = true,
                downwards_core::BoundarySide::Right => doors.east = true,
                downwards_core::BoundarySide::Ceiling => doors.ceiling = true,
                downwards_core::BoundarySide::Floor => doors.floor = true,
            }
        }
        doors
    };
    downwards_audio::compose(&downwards_audio::RoomMusicInputs {
        slug: room.name().to_owned(),
        difficulty: downwards_audio::Difficulty::Medium,
        ability_requirement: downwards_audio::AbilityReq::None,
        coin_count: room.pickups().len() as u32,
        doors,
        hazards: hazards.to_vec(),
        layer_gates: downwards_audio::LayerGates::default(),
    })
}
