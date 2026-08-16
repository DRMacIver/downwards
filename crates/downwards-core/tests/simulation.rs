use downwards_core::{
    AbilitySet, Action, BoundarySide, COYOTE_TICKS, DASH_TICKS, DashDirection, DeathReason, Door,
    DoorEntryError, JUMP_BUFFER_TICKS, JumpKind, MovementTuning, ONE_WAY_DROP_TICKS, Pickup, Point,
    Rect, Room, RoomObjectError, SUBPIXELS_PER_PIXEL, Simulation, SimulationEvent, Tile,
    TimedHazard, WallSide,
};

const WIDTH: usize = 32;
const HEIGHT: usize = 18;
const TILE_SIZE: i32 = 10;

fn room_with_floor(spawn: Point, floor_row: usize) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[floor_row * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "test",
        "Test",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        spawn,
        vec![],
    )
    .unwrap()
}

fn room_with_left_wall(spawn: Point) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for y in 0..16 {
        tiles[y * WIDTH + 10] = Tile::Solid;
    }
    for x in 0..WIDTH {
        tiles[16 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "wall",
        "Wall",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        spawn,
        vec![],
    )
    .unwrap()
}

fn immediate_jump_rise(
    mut simulation: Simulation,
    first_move_x: i8,
    hold_ticks: usize,
) -> (JumpKind, i32) {
    let start_y = simulation.player().position_subpixels().y;
    let mut minimum_y = start_y;
    let mut jump_kind = None;

    for tick in 0..120 {
        let report = simulation.step(Action {
            move_x: if tick == 0 { first_move_x } else { 0 },
            jump: tick < hold_ticks,
            ..Action::default()
        });
        for event in report.events {
            if let SimulationEvent::Jumped(kind) = event {
                jump_kind = Some(kind);
            }
        }
        minimum_y = minimum_y.min(simulation.player().position_subpixels().y);
        if jump_kind.is_some() && simulation.player().velocity_subpixels().y >= 0 {
            break;
        }
    }

    (
        jump_kind.expect("the supplied immediate jump setup must jump"),
        start_y - minimum_y,
    )
}

fn room_with_one_way(spawn: Point, platform_row: usize, floor: bool) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 3..9 {
        tiles[platform_row * WIDTH + x] = Tile::OneWay;
    }
    if floor {
        for x in 0..WIDTH {
            tiles[16 * WIDTH + x] = Tile::Solid;
        }
    }
    Room::new(
        "one-way",
        "One way",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        spawn,
        vec![],
    )
    .unwrap()
}

fn room_with_doors(spawn: Point) -> Room {
    Room::new(
        "doors",
        "Doors",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        vec![Tile::Empty; WIDTH * HEIGHT],
        spawn,
        vec![],
    )
    .unwrap()
    .with_doors(vec![
        Door {
            id: "west".into(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 40, 4, 20),
            arrival: Point::new(30, 40),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east".into(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 40, 4, 20),
            arrival: Point::new(250, 40),
            destination_room: Some("next".into()),
            destination_door: Some("west".into()),
        },
    ])
    .unwrap()
}

#[test]
fn replay_is_deterministic_tick_by_tick() {
    let room = room_with_floor(Point::new(30, 120), 16);
    let mut left = Simulation::new(room.clone());
    let mut right = Simulation::new(room);
    for tick in 0..300 {
        let action = Action {
            move_x: if tick % 80 < 40 { 1 } else { -1 },
            jump: tick % 47 < 9,
            restart: tick == 211,
            ..Action::default()
        };
        let left_report = left.step(action);
        let right_report = right.step(action);
        assert_eq!(left_report, right_report, "diverged at tick {tick}");
        assert_eq!(left, right);
    }
}

#[test]
fn door_entry_selection_is_typed_hashed_and_preserved_by_reset() {
    let room = room_with_doors(Point::new(150, 80));
    let canonical = Simulation::with_abilities(room.clone(), AbilitySet::ALL);
    let mut west = Simulation::enter_via_door(room.clone(), AbilitySet::ALL, "west").unwrap();
    let east = Simulation::enter_via_door(room.clone(), AbilitySet::ALL, "east").unwrap();

    assert_eq!(canonical.entry_door(), None);
    assert_eq!(west.entry_door(), Some("west"));
    assert_eq!(west.player().bounds(), Rect::new(30, 40, 8, 12));
    assert_eq!(east.player().bounds(), Rect::new(250, 40, 8, 12));
    assert_ne!(canonical.digest(), west.digest());
    assert_ne!(west.digest(), east.digest());

    west.step(Action::default());
    assert_eq!(
        west.step(Action {
            restart: true,
            ..Action::default()
        })
        .events,
        vec![SimulationEvent::Reset]
    );
    assert_eq!(west.player().bounds(), Rect::new(30, 40, 8, 12));
    assert_eq!(west.entry_door(), Some("west"));

    assert_eq!(
        Simulation::enter_via_door(room, AbilitySet::NONE, "missing"),
        Err(DoorEntryError::UnknownDoor {
            room_id: "doors".into(),
            door_id: "missing".into(),
        })
    );
}

#[test]
fn a_locked_boundary_door_can_bounce_without_resetting_the_room() {
    let room = room_with_doors(Point::new(312, 40));
    let mut simulation = Simulation::with_abilities(room, AbilitySet::new(false, true));
    let report = simulation.step(Action::default());
    assert!(report.events.contains(&SimulationEvent::ExitReached {
        id: "east".to_owned(),
    }));
    assert_eq!(simulation.reached_exit(), Some("east"));
    let room_tick = simulation.room_tick();

    assert!(simulation.reject_reached_door("east"));
    assert_eq!(simulation.reached_exit(), None);
    assert_eq!(simulation.player().bounds(), Rect::new(250, 40, 8, 12));
    assert_eq!(simulation.room_tick(), room_tick);
    assert!(simulation.player().dash_available());
    assert!(!simulation.reject_reached_door("east"));
}

#[test]
fn every_door_field_contributes_to_room_content_identity() {
    let base = Room::new(
        "door-digest",
        "Door digest",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        vec![Tile::Empty; WIDTH * HEIGHT],
        Point::new(150, 80),
        vec![],
    )
    .unwrap();
    let prototype = Door {
        id: "west".into(),
        side: BoundarySide::Left,
        trigger_bounds: Rect::new(0, 40, 4, 20),
        arrival: Point::new(30, 40),
        destination_room: Some("next".into()),
        destination_door: Some("east".into()),
    };
    let first = base.clone().with_doors(vec![prototype.clone()]).unwrap();
    let mut changed = prototype;
    changed.destination_door = Some("north".into());
    let second = base.clone().with_doors(vec![changed]).unwrap();

    assert_eq!(
        Simulation::new(base.clone()).digest(),
        Simulation::new(base).digest()
    );
    assert_ne!(
        Simulation::new(first).digest(),
        Simulation::new(second).digest()
    );
}

#[test]
fn every_boundary_side_uses_the_exact_exit_event_contract() {
    let cases = [
        (
            BoundarySide::Left,
            Rect::new(0, 40, 12, 20),
            Point::new(2, 44),
        ),
        (
            BoundarySide::Right,
            Rect::new(308, 40, 12, 20),
            Point::new(310, 44),
        ),
        (
            BoundarySide::Ceiling,
            Rect::new(100, 0, 20, 14),
            Point::new(106, 1),
        ),
        (
            BoundarySide::Floor,
            Rect::new(100, 166, 20, 14),
            Point::new(106, 167),
        ),
    ];
    for (index, (side, trigger_bounds, spawn)) in cases.into_iter().enumerate() {
        let id = format!("door-{index}");
        let room = Room::new(
            format!("room-{index}"),
            "Room",
            WIDTH as u16,
            HEIGHT as u16,
            TILE_SIZE,
            vec![Tile::Empty; WIDTH * HEIGHT],
            spawn,
            vec![],
        )
        .unwrap()
        .with_doors(vec![Door {
            id: id.clone(),
            side,
            trigger_bounds,
            arrival: Point::new(150, 80),
            destination_room: None,
            destination_door: None,
        }])
        .unwrap();
        let mut simulation = Simulation::new(room);
        let report = simulation.step(Action::default());
        assert_eq!(
            report.events.last(),
            Some(&SimulationEvent::ExitReached { id: id.clone() })
        );
        assert_eq!(simulation.reached_exit(), Some(id.as_str()));
    }
}

#[test]
fn door_triggers_use_half_open_intersection_boundaries() {
    let room = Room::new(
        "exact-door",
        "Exact door",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        vec![Tile::Empty; WIDTH * HEIGHT],
        Point::new(8, 40),
        vec![],
    )
    .unwrap()
    .with_doors(vec![Door {
        id: "west".into(),
        side: BoundarySide::Left,
        trigger_bounds: Rect::new(0, 40, 8, 20),
        arrival: Point::new(30, 40),
        destination_room: None,
        destination_door: None,
    }])
    .unwrap();
    let mut simulation = Simulation::new(room);

    let touching = simulation.step(Action::default());
    assert!(
        !touching
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::ExitReached { .. }))
    );
    assert_eq!(simulation.reached_exit(), None);

    let overlapping = simulation.step(Action {
        move_x: -1,
        ..Action::default()
    });
    assert!(
        overlapping
            .events
            .contains(&SimulationEvent::ExitReached { id: "west".into() })
    );
    assert_eq!(simulation.reached_exit(), Some("west"));
}

#[test]
fn falling_player_lands_exactly_without_penetration() {
    let mut simulation = Simulation::new(room_with_floor(Point::new(40, 40), 16));
    for _ in 0..120 {
        simulation.step(Action::default());
    }
    assert!(simulation.player().grounded());
    assert_eq!(simulation.player().bounds(), Rect::new(40, 148, 8, 12));
    assert_eq!(simulation.player().velocity_subpixels().y, 0);
}

#[test]
fn coyote_window_has_an_exact_boundary() {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..5 {
        tiles[10 * WIDTH + x] = Tile::Solid;
    }
    let room = Room::new(
        "ledge",
        "Ledge",
        32,
        18,
        10,
        tiles,
        Point::new(30, 88),
        vec![],
    )
    .unwrap();
    let mut simulation = Simulation::new(room);
    simulation.step(Action::default());
    while simulation.player().grounded() {
        simulation.step(Action {
            move_x: 1,
            ..Action::default()
        });
    }
    assert_eq!(simulation.player().coyote_ticks_remaining(), COYOTE_TICKS);
    let fork = simulation.clone();

    for _ in 0..COYOTE_TICKS - 1 {
        simulation.step(Action::default());
    }
    let report = simulation.step(Action {
        jump: true,
        ..Action::default()
    });
    assert!(
        report
            .events
            .contains(&SimulationEvent::Jumped(downwards_core::JumpKind::Coyote))
    );

    let mut late = fork;
    for _ in 0..COYOTE_TICKS {
        late.step(Action::default());
    }
    let report = late.step(Action {
        jump: true,
        ..Action::default()
    });
    assert!(
        !report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Jumped(_)))
    );
}

#[test]
fn jump_buffer_fires_on_landing() {
    let room = room_with_floor(Point::new(40, 40), 16);
    let mut probe = Simulation::new(room);
    let mut history = vec![probe.clone()];
    while !probe.player().grounded() {
        probe.step(Action::default());
        history.push(probe.clone());
    }
    let landing_step = history.len() - 1;
    let mut buffered = history[landing_step - usize::from(JUMP_BUFFER_TICKS)].clone();
    buffered.step(Action {
        jump: true,
        ..Action::default()
    });
    let mut last = None;
    for _ in 1..JUMP_BUFFER_TICKS {
        last = Some(buffered.step(Action::default()));
    }
    assert!(
        last.unwrap()
            .events
            .contains(&SimulationEvent::Jumped(downwards_core::JumpKind::Buffered))
    );
    assert!(buffered.player().velocity_subpixels().y < 0);
}

#[test]
fn ground_coyote_and_wall_jumps_share_variable_height_physics() {
    let mut ground = Simulation::new(room_with_floor(Point::new(40, 148), 16));
    ground.step(Action::default());
    assert!(ground.player().grounded());

    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..5 {
        tiles[10 * WIDTH + x] = Tile::Solid;
    }
    let room = Room::new(
        "jump-envelope-ledge",
        "Jump envelope ledge",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(30, 88),
        vec![],
    )
    .unwrap();
    let mut coyote = Simulation::new(room);
    coyote.step(Action::default());
    while coyote.player().grounded() {
        coyote.step(Action {
            move_x: 1,
            ..Action::default()
        });
    }
    assert_eq!(coyote.player().coyote_ticks_remaining(), COYOTE_TICKS);

    let wall = Simulation::with_abilities(
        room_with_left_wall(Point::new(110, 40)),
        AbilitySet::new(true, false),
    );
    for (label, simulation, first_move_x, expected_kind) in [
        ("ground", ground, 0, JumpKind::Grounded),
        ("coyote", coyote, 0, JumpKind::Coyote),
        (
            "wall",
            wall,
            -1,
            JumpKind::Wall {
                side: WallSide::Left,
            },
        ),
    ] {
        assert_eq!(
            immediate_jump_rise(simulation.clone(), first_move_x, 1),
            (expected_kind, 1_744),
            "{label} low jump changed"
        );
        assert_eq!(
            immediate_jump_rise(simulation, first_move_x, 10),
            (expected_kind, 7_792),
            "{label} held jump changed"
        );
    }
}

#[test]
fn authoritative_jump_hold_window_is_observable_for_search_state() {
    let mut simulation = Simulation::new(room_with_floor(Point::new(40, 148), 16));
    simulation.step(Action::default());

    simulation.step(Action {
        jump: true,
        ..Action::default()
    });
    assert_eq!(simulation.player().jump_hold_ticks_remaining(), 9);

    simulation.step(Action::default());
    assert_eq!(simulation.player().jump_hold_ticks_remaining(), 0);
}

#[test]
fn hazard_causes_an_instant_deterministic_reset() {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    tiles[8 * WIDTH + 4] = Tile::HazardUp;
    let room = Room::new(
        "hazard",
        "Hazard",
        32,
        18,
        10,
        tiles,
        Point::new(40, 40),
        vec![],
    )
    .unwrap();
    let mut simulation = Simulation::new(room);
    let report = loop {
        let report = simulation.step(Action {
            jump: true,
            ..Action::default()
        });
        if !report.events.is_empty() {
            break report;
        }
    };
    assert_eq!(
        report.events,
        vec![
            SimulationEvent::Died(DeathReason::Hazard {
                tile_x: 4,
                tile_y: 8
            }),
            SimulationEvent::Reset,
        ]
    );
    assert_eq!(simulation.player().bounds(), Rect::new(40, 40, 8, 12));
    assert_eq!(simulation.deaths(), 1);

    simulation.step(Action {
        jump: true,
        ..Action::default()
    });
    assert_eq!(
        simulation.player().jump_buffer_ticks_remaining(),
        JUMP_BUFFER_TICKS - 1,
        "the fatal tick's held input must not leak into the fresh attempt"
    );
}

#[test]
fn upward_spike_back_is_a_nonlethal_ceiling() {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    tiles[8 * WIDTH + 4] = Tile::HazardUp;
    for x in 0..WIDTH {
        tiles[12 * WIDTH + x] = Tile::Solid;
    }
    let room = Room::new(
        "spike-back",
        "Spike back",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(41, 108),
        vec![],
    )
    .unwrap();
    let mut simulation = Simulation::new(room);
    let mut minimum_y = simulation.player().position_subpixels().y;
    for tick in 0..60 {
        let report = simulation.step(Action {
            jump: tick < 10,
            ..Action::default()
        });
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(DeathReason::Hazard { .. })))
        );
        minimum_y = minimum_y.min(simulation.player().position_subpixels().y);
    }
    assert_eq!(minimum_y, 90 * SUBPIXELS_PER_PIXEL);
    assert_eq!(simulation.deaths(), 0);
}

fn downward_spike_room(spawn_y: i32) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    tiles[8 * WIDTH + 4] = Tile::HazardDown;
    for x in 0..WIDTH {
        tiles[12 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "down-spike",
        "Down spike",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(41, spawn_y),
        vec![],
    )
    .unwrap()
}

#[test]
fn downward_spike_back_is_safe_while_its_pointed_face_kills() {
    let mut back = Simulation::new(downward_spike_room(40));
    for _ in 0..90 {
        let report = back.step(Action::default());
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(DeathReason::Hazard { .. })))
        );
    }
    assert_eq!(back.player().bounds().y, 68);

    let mut front = Simulation::new(downward_spike_room(108));
    let report = loop {
        let report = front.step(Action {
            jump: true,
            ..Action::default()
        });
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(DeathReason::Hazard { .. })))
        {
            break report;
        }
    };
    assert!(report.events.iter().any(|event| matches!(
        event,
        SimulationEvent::Died(DeathReason::Hazard {
            tile_x: 4,
            tile_y: 8
        })
    )));
}

fn right_facing_wall_spike_room(spawn_x: i32) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    tiles[10 * WIDTH + 20] = Tile::HazardRight;
    tiles[11 * WIDTH + 20] = Tile::HazardRight;
    for x in 0..WIDTH {
        tiles[12 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "wall-spike",
        "Wall spike",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(spawn_x, 108),
        vec![],
    )
    .unwrap()
}

#[test]
fn wall_spike_back_blocks_while_its_pointed_face_kills() {
    let mut back = Simulation::new(right_facing_wall_spike_room(160));
    for _ in 0..90 {
        let report = back.step(Action {
            move_x: 1,
            ..Action::default()
        });
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(DeathReason::Hazard { .. })))
        );
    }
    assert_eq!(back.player().bounds().x, 192);

    let mut front = Simulation::new(right_facing_wall_spike_room(230));
    let report = loop {
        let report = front.step(Action {
            move_x: -1,
            ..Action::default()
        });
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(DeathReason::Hazard { .. })))
        {
            break report;
        }
    };
    assert!(report.events.iter().any(|event| matches!(
        event,
        SimulationEvent::Died(DeathReason::Hazard {
            tile_x: 20,
            tile_y: 10..=11
        })
    )));
}

fn single_hazard_room(tile: Tile, spawn: Point) -> Room {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    tiles[11 * WIDTH + 20] = tile;
    for x in 0..WIDTH {
        tiles[12 * WIDTH + x] = Tile::Solid;
    }
    Room::new(
        "hazard",
        "Hazard",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        spawn,
        vec![],
    )
    .unwrap()
}

fn drop_until_death_or_rest(simulation: &mut Simulation) -> Option<(u16, u16)> {
    for _ in 0..120 {
        let report = simulation.step(Action::default());
        for event in report.events {
            if let SimulationEvent::Died(DeathReason::Hazard { tile_x, tile_y }) = event {
                return Some((tile_x, tile_y));
            }
        }
        if simulation.player().grounded() && simulation.player().velocity_subpixels().y == 0 {
            return None;
        }
    }
    panic!("player neither died nor came to rest");
}

#[test]
fn side_spike_top_kills_a_player_landing_on_it() {
    // A side-facing spike is lethal from its sides, not only its point: the
    // tile top must never be a standable perch.
    for tile in [Tile::HazardLeft, Tile::HazardRight] {
        let mut simulation = Simulation::new(single_hazard_room(tile, Point::new(200, 60)));
        assert_eq!(
            drop_until_death_or_rest(&mut simulation),
            Some((20, 11)),
            "{tile:?} top should kill on landing"
        );
    }
}

#[test]
fn down_spike_top_is_its_safe_back() {
    let mut simulation = Simulation::new(single_hazard_room(Tile::HazardDown, Point::new(200, 60)));
    assert_eq!(drop_until_death_or_rest(&mut simulation), None);
    assert_eq!(simulation.player().bounds().y, 98);
}

#[test]
fn side_spike_underside_kills_from_below() {
    for tile in [Tile::HazardLeft, Tile::HazardRight] {
        let mut room_tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        room_tiles[8 * WIDTH + 20] = tile;
        for x in 0..WIDTH {
            room_tiles[12 * WIDTH + x] = Tile::Solid;
        }
        let room = Room::new(
            "hazard-underside",
            "Hazard underside",
            WIDTH as u16,
            HEIGHT as u16,
            TILE_SIZE,
            room_tiles,
            Point::new(200, 108),
            vec![],
        )
        .unwrap();
        let mut simulation = Simulation::new(room);
        let mut died = false;
        for tick in 0..120 {
            let report = simulation.step(Action {
                jump: tick < 12,
                ..Action::default()
            });
            if report.events.iter().any(|event| {
                matches!(
                    event,
                    SimulationEvent::Died(DeathReason::Hazard {
                        tile_x: 20,
                        tile_y: 8
                    })
                )
            }) {
                died = true;
                break;
            }
        }
        assert!(died, "{tile:?} underside should kill a rising player");
    }
}

#[test]
fn up_spike_underside_is_its_safe_back() {
    let mut room_tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    room_tiles[8 * WIDTH + 20] = Tile::HazardUp;
    for x in 0..WIDTH {
        room_tiles[12 * WIDTH + x] = Tile::Solid;
    }
    let room = Room::new(
        "up-underside",
        "Up underside",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        room_tiles,
        Point::new(200, 108),
        vec![],
    )
    .unwrap();
    let mut simulation = Simulation::new(room);
    for tick in 0..120 {
        let report = simulation.step(Action {
            jump: tick < 12,
            ..Action::default()
        });
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_))),
            "an up spike's underside is its back and must stay a safe bonk"
        );
    }
}

#[test]
fn manual_restart_restores_canonical_dash_edge_state() {
    let room = room_with_floor(Point::new(40, 148), 16);
    let mut simulation = Simulation::with_abilities(room, AbilitySet::new(false, true));
    simulation.step(Action::default());

    let restart = simulation.step(Action {
        dash: true,
        restart: true,
        ..Action::default()
    });
    assert_eq!(restart.events, vec![SimulationEvent::Reset]);

    let next = simulation.step(Action {
        move_x: 1,
        dash: true,
        ..Action::default()
    });
    assert!(next.events.contains(&SimulationEvent::Dashed {
        direction: DashDirection::Right
    }));
}

#[test]
fn ability_loadout_is_explicit_hashable_and_changes_the_digest() {
    use std::collections::HashSet;

    let loadouts = HashSet::from([
        AbilitySet::NONE,
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::ALL,
    ]);
    assert_eq!(loadouts.len(), 4);

    let room = room_with_floor(Point::new(40, 148), 16);
    let baseline = Simulation::new(room.clone());
    assert_eq!(baseline.abilities(), AbilitySet::NONE);
    let digests = loadouts
        .into_iter()
        .map(|abilities| Simulation::with_abilities(room.clone(), abilities).digest())
        .collect::<HashSet<_>>();
    assert_eq!(digests.len(), 4);
}

#[test]
fn disabled_wall_jump_and_dash_have_exact_no_op_boundaries() {
    let mut simulation = Simulation::new(room_with_left_wall(Point::new(110, 40)));
    let report = simulation.step(Action {
        move_x: -1,
        move_y: -1,
        jump: true,
        dash: true,
        restart: false,
    });

    assert!(!report.events.iter().any(|event| matches!(
        event,
        SimulationEvent::Jumped(_) | SimulationEvent::Dashed { .. }
    )));
    assert_eq!(simulation.player().position_subpixels().x, 110 * 256);
    assert_eq!(simulation.player().velocity_subpixels().y, 96);
    assert_eq!(simulation.player().wall_contact(), Some(WallSide::Left));
    assert!(!simulation.player().wall_sliding());
    assert!(!simulation.player().dash_available());
    assert_eq!(simulation.player().dash_ticks_remaining(), 0);
}

#[test]
fn wall_slide_has_an_exact_speed_cap_and_requires_toward_intent() {
    let room = room_with_left_wall(Point::new(110, 40));
    let wall_jump = AbilitySet::new(true, false);
    let mut sliding = Simulation::with_abilities(room.clone(), wall_jump);
    while sliding.player().velocity_subpixels().y < 384 {
        sliding.step(Action {
            move_x: -1,
            ..Action::default()
        });
    }
    sliding.step(Action {
        move_x: -1,
        ..Action::default()
    });
    assert!(sliding.player().wall_sliding());
    assert_eq!(sliding.player().velocity_subpixels().y, 384);
    assert_eq!(sliding.player().wall_contact(), Some(WallSide::Left));

    let mut no_intent = Simulation::with_abilities(room, wall_jump);
    for _ in 0..5 {
        no_intent.step(Action::default());
    }
    assert!(!no_intent.player().wall_sliding());
    assert!(no_intent.player().velocity_subpixels().y > 384);
}

#[test]
fn wall_jump_kicks_away_from_the_contact_without_stamina() {
    let mut simulation = Simulation::with_abilities(
        room_with_left_wall(Point::new(110, 40)),
        AbilitySet::new(true, false),
    );
    let report = simulation.step(Action {
        move_x: -1,
        jump: true,
        ..Action::default()
    });

    assert!(
        report
            .events
            .contains(&SimulationEvent::Jumped(JumpKind::Wall {
                side: WallSide::Left,
            }))
    );
    assert_eq!(
        simulation.player().position_subpixels().x,
        110 * SUBPIXELS_PER_PIXEL + 768
    );
    assert_eq!(
        simulation.player().velocity_subpixels(),
        Point::new(768, -912)
    );
    assert_eq!(simulation.player().wall_contact(), None);
    assert!(!simulation.player().wall_sliding());
}

#[test]
fn human_wall_assists_remember_contact_and_commit_the_launch() {
    let mut simulation = Simulation::with_abilities(
        room_with_left_wall(Point::new(110, 40)),
        AbilitySet::new(true, false),
    );
    simulation.enable_human_wall_assists();

    simulation.step(Action {
        move_x: -1,
        ..Action::default()
    });
    assert!(simulation.human_wall_jump_available());

    // Separate briefly from the wall, then tap Jump while steering back toward it. Recent wall
    // eligibility must win and the launch must continue away from the wall despite that input.
    simulation.step(Action {
        move_x: 1,
        ..Action::default()
    });
    let report = simulation.step(Action {
        move_x: -1,
        jump: true,
        ..Action::default()
    });
    assert!(
        report
            .events
            .contains(&SimulationEvent::Jumped(JumpKind::Wall {
                side: WallSide::Left,
            }))
    );

    for _ in 0..3 {
        simulation.step(Action {
            move_x: -1,
            ..Action::default()
        });
        assert!(simulation.player().velocity_subpixels().x > 0);
        assert!(simulation.player().velocity_subpixels().y < 0);
    }
}

#[test]
fn movement_tuning_has_direct_speed_braking_and_reversal_contracts() {
    let released_speed = |tuning| {
        let mut simulation = Simulation::new(room_with_floor(Point::new(40, 148), 16));
        if let Some(tuning) = tuning {
            simulation.set_movement_tuning(tuning);
        }
        simulation.step(Action::default());
        for _ in 0..4 {
            simulation.step(Action {
                move_x: 1,
                ..Action::default()
            });
        }
        simulation.step(Action::default());
        simulation.player().velocity_subpixels().x
    };

    assert_eq!(released_speed(None), 256);
    assert_eq!(released_speed(Some(MovementTuning::LAB_DEFAULT)), 429);
    let fast_105 = MovementTuning {
        top_speed_pixels_per_second: 105,
        ..MovementTuning::LAB_DEFAULT
    };
    assert_eq!(released_speed(Some(fast_105)), 410);
    let fast_105_glide = MovementTuning {
        braking_milliseconds: 167,
        ..fast_105
    };
    assert_eq!(released_speed(Some(fast_105_glide)), 403);

    let mut retained = Simulation::new(room_with_floor(Point::new(40, 148), 16));
    retained.set_movement_tuning(MovementTuning::LAB_DEFAULT);
    retained.step(Action::default());
    for _ in 0..4 {
        retained.step(Action {
            move_x: 1,
            ..Action::default()
        });
    }
    retained.step(Action {
        move_x: -1,
        ..Action::default()
    });
    assert_eq!(retained.player().velocity_subpixels().x, 389);
}

#[test]
fn wall_carry_reflects_remembered_collision_speed_into_the_wall_jump() {
    let launch_speed = |carry_percent| {
        let mut simulation = Simulation::with_abilities(
            room_with_left_wall(Point::new(110, 40)),
            AbilitySet::new(true, false),
        );
        simulation.set_movement_tuning(MovementTuning {
            wall_carry_percent: carry_percent,
            ..MovementTuning::LAB_DEFAULT
        });
        simulation.step(Action {
            move_x: -1,
            ..Action::default()
        });
        let report = simulation.step(Action {
            move_x: -1,
            jump: true,
            ..Action::default()
        });
        assert!(
            report
                .events
                .contains(&SimulationEvent::Jumped(JumpKind::Wall {
                    side: WallSide::Left,
                }))
        );
        simulation.player().velocity_subpixels().x
    };

    assert_eq!(launch_speed(0), 768);
    assert_eq!(launch_speed(50), 807);
    assert_eq!(launch_speed(100), 847);
}

#[test]
fn wall_memory_directly_controls_post_contact_wall_jump_grace() {
    let remains_available_after = |memory_ms, wait_ticks| {
        let mut simulation = Simulation::with_abilities(
            room_with_left_wall(Point::new(110, 40)),
            AbilitySet::new(true, false),
        );
        simulation.enable_human_wall_assists();
        simulation.set_movement_tuning(MovementTuning {
            wall_momentum_milliseconds: memory_ms,
            ..MovementTuning::LAB_DEFAULT
        });
        simulation.step(Action {
            move_x: -1,
            ..Action::default()
        });
        simulation.step(Action {
            move_x: 1,
            ..Action::default()
        });
        for _ in 0..wait_ticks {
            simulation.step(Action::default());
        }
        simulation.human_wall_jump_available()
    };

    assert!(!remains_available_after(50, 3));
    assert!(remains_available_after(250, 8));
}

#[test]
fn rising_wall_impact_converts_horizontal_speed_into_one_upward_carry_impulse() {
    let impact_velocity = |ascent_percent| {
        let mut simulation = Simulation::with_abilities(
            room_with_left_wall(Point::new(110, 148)),
            AbilitySet::new(true, false),
        );
        simulation.set_movement_tuning(MovementTuning {
            wall_ascent_carry_percent: ascent_percent,
            ..MovementTuning::LAB_DEFAULT
        });
        simulation.step(Action::default());
        let report = simulation.step(Action {
            move_x: -1,
            jump: true,
            ..Action::default()
        });
        assert!(
            report
                .events
                .contains(&SimulationEvent::Jumped(JumpKind::Grounded))
        );
        let first = simulation.player().velocity_subpixels().y;
        simulation.step(Action {
            move_x: -1,
            jump: true,
            ..Action::default()
        });
        (first, simulation.player().velocity_subpixels().y)
    };

    assert_eq!(impact_velocity(0), (-912, -864));
    assert_eq!(impact_velocity(50), (-971, -923));
    assert_eq!(impact_velocity(100), (-1_030, -982));
}

#[test]
fn legacy_is_default_and_tuning_identity_is_digest_bound() {
    let room = room_with_floor(Point::new(40, 148), 16);
    let legacy = Simulation::new(room.clone());
    let mut explicit_legacy = Simulation::new(room.clone());
    explicit_legacy.clear_movement_tuning();
    let mut retained = Simulation::new(room);
    retained.set_movement_tuning(MovementTuning::LAB_DEFAULT);

    assert_eq!(legacy, explicit_legacy);
    assert_eq!(legacy.digest(), explicit_legacy.digest());
    assert_ne!(legacy.digest(), retained.digest());
    assert_eq!(
        retained.movement_tuning(),
        Some(MovementTuning::LAB_DEFAULT)
    );
}

#[test]
fn dash_uses_all_eight_directions_with_fixed_integer_velocities() {
    let cases = [
        ((0, -1), DashDirection::Up, Point::new(0, -1_024)),
        ((1, -1), DashDirection::UpRight, Point::new(724, -724)),
        ((1, 0), DashDirection::Right, Point::new(1_024, 0)),
        ((1, 1), DashDirection::DownRight, Point::new(724, 724)),
        ((0, 1), DashDirection::Down, Point::new(0, 1_024)),
        ((-1, 1), DashDirection::DownLeft, Point::new(-724, 724)),
        ((-1, 0), DashDirection::Left, Point::new(-1_024, 0)),
        ((-1, -1), DashDirection::UpLeft, Point::new(-724, -724)),
    ];
    for ((move_x, move_y), direction, velocity) in cases {
        let mut simulation = Simulation::with_abilities(
            room_with_floor(Point::new(150, 80), 16),
            AbilitySet::new(false, true),
        );
        let start = simulation.player().position_subpixels();
        let report = simulation.step(Action {
            move_x,
            move_y,
            dash: true,
            ..Action::default()
        });
        assert_eq!(
            report.events.first(),
            Some(&SimulationEvent::Dashed { direction })
        );
        assert_eq!(simulation.player().velocity_subpixels(), velocity);
        assert_eq!(
            simulation.player().position_subpixels(),
            Point::new(start.x + velocity.x, start.y + velocity.y)
        );
        assert_eq!(simulation.player().dash_direction(), Some(direction));
        assert_eq!(simulation.player().dash_ticks_remaining(), DASH_TICKS - 1);
    }
}

#[test]
fn dash_duration_is_exact_and_it_recharges_only_after_safe_ground_contact() {
    let mut simulation = Simulation::with_abilities(
        room_with_floor(Point::new(40, 40), 16),
        AbilitySet::new(false, true),
    );
    let start_x = simulation.player().position_subpixels().x;
    for tick in 0..DASH_TICKS {
        let report = simulation.step(Action {
            move_x: 1,
            dash: true,
            ..Action::default()
        });
        assert_eq!(
            report.events.contains(&SimulationEvent::Dashed {
                direction: DashDirection::Right,
            }),
            tick == 0
        );
        assert_eq!(
            simulation.player().position_subpixels().x,
            start_x + i32::from(tick + 1) * 1_024
        );
    }
    assert_eq!(simulation.player().dash_ticks_remaining(), 0);
    assert_eq!(simulation.player().dash_direction(), None);
    assert!(!simulation.player().dash_available());

    simulation.step(Action::default());
    let unavailable = simulation.step(Action {
        move_x: 1,
        dash: true,
        ..Action::default()
    });
    assert!(
        !unavailable
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Dashed { .. }))
    );

    while !simulation.player().grounded() {
        simulation.step(Action::default());
    }
    assert!(simulation.player().dash_available());
    simulation.step(Action::default());
    let recharged = simulation.step(Action {
        move_y: -1,
        dash: true,
        ..Action::default()
    });
    assert!(recharged.events.contains(&SimulationEvent::Dashed {
        direction: DashDirection::Up,
    }));
}

#[test]
fn dash_collision_never_penetrates_solids_and_cancels_the_blocked_axis() {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        tiles[y * WIDTH + 15] = Tile::Solid;
    }
    let room = Room::new(
        "dash-wall",
        "Dash wall",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(140, 80),
        vec![],
    )
    .unwrap();
    let mut simulation = Simulation::with_abilities(room, AbilitySet::new(false, true));
    simulation.step(Action {
        move_x: 1,
        dash: true,
        ..Action::default()
    });

    assert_eq!(simulation.player().position_subpixels().x, 142 * 256);
    assert_eq!(simulation.player().bounds().right(), 150);
    assert_eq!(simulation.player().velocity_subpixels().x, 0);
    assert_eq!(simulation.player().dash_ticks_remaining(), DASH_TICKS - 1);
}

#[test]
fn deterministic_replay_covers_every_ability_loadout_and_dash_edge_state() {
    let room = room_with_floor(Point::new(110, 40), 16);
    for abilities in [
        AbilitySet::NONE,
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::ALL,
    ] {
        let mut left = Simulation::with_abilities(room.clone(), abilities);
        let mut right = left.clone();
        for tick in 0..240 {
            let action = Action {
                move_x: match tick % 90 {
                    0..=29 => -1,
                    30..=59 => 0,
                    _ => 1,
                },
                move_y: match tick % 60 {
                    0..=19 => -1,
                    20..=39 => 0,
                    _ => 1,
                },
                jump: tick % 37 < 8,
                dash: tick % 43 < 3,
                restart: tick == 173,
            };
            assert_eq!(left.step(action), right.step(action), "tick {tick}");
            assert_eq!(left.digest(), right.digest(), "tick {tick}");
            assert_eq!(left, right, "tick {tick}");
        }
    }
}

#[test]
fn one_way_platform_is_ignored_while_rising() {
    let mut simulation = Simulation::new(room_with_one_way(Point::new(40, 148), 13, true));
    simulation.step(Action::default());
    assert!(simulation.player().grounded());

    let mut entirely_above_platform = false;
    for _ in 0..30 {
        simulation.step(Action {
            jump: true,
            ..Action::default()
        });
        if simulation.player().position_subpixels().y + 12 * SUBPIXELS_PER_PIXEL
            <= 130 * SUBPIXELS_PER_PIXEL
        {
            entirely_above_platform = true;
            break;
        }
    }
    assert!(
        entirely_above_platform,
        "the platform blocked its underside"
    );
    assert!(simulation.player().velocity_subpixels().y < 0);
}

#[test]
fn one_way_platform_catches_falling_player_and_remains_stable() {
    let mut simulation = Simulation::new(room_with_one_way(Point::new(40, 40), 10, false));
    while !simulation.player().grounded() {
        simulation.step(Action::default());
    }
    assert_eq!(simulation.player().bounds(), Rect::new(40, 88, 8, 12));
    assert_eq!(simulation.player().velocity_subpixels().y, 0);

    for _ in 0..30 {
        simulation.step(Action::default());
        assert!(simulation.player().grounded());
        assert_eq!(simulation.player().position_subpixels().y, 88 * 256);
    }
}

#[test]
fn down_and_jump_drops_through_one_way_at_an_exact_boundary() {
    let mut simulation = Simulation::new(room_with_one_way(Point::new(40, 88), 10, false));
    simulation.step(Action::default());
    assert!(simulation.player().grounded());

    let report = simulation.step(Action {
        move_y: 1,
        jump: true,
        ..Action::default()
    });
    assert!(
        !report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Jumped(_)))
    );
    assert!(!simulation.player().grounded());
    assert_eq!(
        simulation.player().position_subpixels().y,
        88 * SUBPIXELS_PER_PIXEL + 96
    );
    assert_eq!(simulation.player().velocity_subpixels().y, 96);
    assert_eq!(
        simulation.player().one_way_drop_ticks_remaining(),
        ONE_WAY_DROP_TICKS - 1
    );

    for _ in 0..8 {
        simulation.step(Action {
            move_y: 1,
            jump: true,
            ..Action::default()
        });
    }
    assert!(simulation.player().bounds().y > 100);
}

#[test]
fn room_objects_reject_invalid_data_and_change_content_identity() {
    assert!(matches!(
        TimedHazard::new(Rect::new(0, 0, 10, 10), 0, 1, 0),
        Err(RoomObjectError::ZeroHazardPeriod)
    ));
    assert!(matches!(
        TimedHazard::new(Rect::new(0, 0, 10, 10), 5, 6, 0),
        Err(RoomObjectError::InvalidHazardActiveTicks { .. })
    ));
    assert!(matches!(
        TimedHazard::new(Rect::new(0, 0, 10, 10), 5, 1, 5),
        Err(RoomObjectError::InvalidHazardPhase { .. })
    ));
    assert!(Pickup::new("", Rect::new(0, 0, 1, 1)).is_err());
    assert!(Pickup::new("coin", Rect::new(319, 179, 2, 1)).is_err());

    let plain = room_with_floor(Point::new(40, 148), 16);
    let decorated = plain
        .clone()
        .with_objects(
            vec![TimedHazard::new(Rect::new(80, 148, 10, 12), 10, 3, 2).unwrap()],
            vec![Pickup::new("coin", Rect::new(60, 148, 6, 8)).unwrap()],
        )
        .unwrap();
    assert_ne!(
        Simulation::new(plain).digest(),
        Simulation::new(decorated).digest()
    );
}

#[test]
fn timed_hazard_phase_boundaries_are_exact_and_death_resets_room_clock() {
    let hazard = TimedHazard::new(Rect::new(50, 140, 60, 20), 6, 2, 0).unwrap();
    assert!(hazard.is_active_at(0));
    assert!(hazard.is_active_at(1));
    assert!(!hazard.is_active_at(2));
    assert!(!hazard.is_active_at(5));
    assert!(hazard.is_active_at(6));

    let room = room_with_floor(Point::new(40, 148), 16)
        .with_objects(vec![hazard], vec![])
        .unwrap();
    let mut simulation = Simulation::new(room);
    for expected_tick in 1..6 {
        let report = simulation.step(Action {
            move_x: 1,
            ..Action::default()
        });
        assert_eq!(simulation.room_tick(), expected_tick);
        assert!(
            !report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Died(_)))
        );
    }
    let report = simulation.step(Action {
        move_x: 1,
        ..Action::default()
    });
    assert_eq!(
        report.events,
        vec![
            SimulationEvent::Died(DeathReason::TimedHazard { hazard_index: 0 }),
            SimulationEvent::Reset,
        ]
    );
    assert_eq!(simulation.tick(), 6);
    assert_eq!(simulation.room_tick(), 0);
    assert_eq!(simulation.deaths(), 1);
    assert_eq!(simulation.timed_hazard_is_active(0), Some(true));
    assert_eq!(simulation.timed_hazard_is_active(1), None);
    assert_eq!(simulation.active_timed_hazards().count(), 1);
}

#[test]
fn pickup_collects_once_per_attempt_and_restart_restores_it() {
    let room = room_with_floor(Point::new(40, 148), 16)
        .with_objects(
            vec![],
            vec![Pickup::new("spawn-gem", Rect::new(40, 148, 8, 12)).unwrap()],
        )
        .unwrap();
    let mut simulation = Simulation::new(room);

    let collected = simulation.step(Action::default());
    assert_eq!(
        collected.events,
        vec![
            SimulationEvent::Landed,
            SimulationEvent::PickupCollected {
                id: "spawn-gem".into(),
            },
        ]
    );
    assert_eq!(simulation.pickup_is_collected(0), Some(true));
    assert_eq!(simulation.pickup_is_collected(1), None);
    assert_eq!(
        simulation
            .collected_pickups()
            .map(Pickup::id)
            .collect::<Vec<_>>(),
        vec!["spawn-gem"]
    );
    assert!(
        !simulation
            .step(Action::default())
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::PickupCollected { .. }))
    );

    assert_eq!(
        simulation
            .step(Action {
                restart: true,
                ..Action::default()
            })
            .events,
        vec![SimulationEvent::Reset]
    );
    assert_eq!(simulation.room_tick(), 0);
    assert_eq!(simulation.pickup_is_collected(0), Some(false));
    assert!(simulation.collected_pickups().next().is_none());
    assert!(simulation.step(Action::default()).events.contains(
        &SimulationEvent::PickupCollected {
            id: "spawn-gem".into(),
        }
    ));
}

#[test]
fn replay_determinism_includes_one_way_hazards_pickups_and_room_resets() {
    let room = room_with_one_way(Point::new(40, 88), 10, false)
        .with_objects(
            vec![TimedHazard::new(Rect::new(140, 120, 30, 30), 17, 4, 3).unwrap()],
            vec![
                Pickup::new("a", Rect::new(40, 88, 8, 12)).unwrap(),
                Pickup::new("b", Rect::new(200, 120, 8, 12)).unwrap(),
            ],
        )
        .unwrap();
    let mut left = Simulation::with_abilities(room, AbilitySet::ALL);
    let mut right = left.clone();
    for tick in 0..360 {
        let action = Action {
            move_x: match tick % 75 {
                0..=24 => 1,
                25..=49 => -1,
                _ => 0,
            },
            move_y: i8::from(tick % 41 < 5),
            jump: tick % 41 < 5,
            dash: tick % 67 < 2,
            restart: tick == 203,
        };
        assert_eq!(left.step(action), right.step(action), "tick {tick}");
        assert_eq!(left, right, "tick {tick}");
        assert_eq!(left.digest(), right.digest(), "tick {tick}");
    }
}

#[test]
fn granting_dash_mid_run_is_additive_and_starts_charged() {
    let room = room_with_floor(Point::new(40, 138), 15);
    let mut simulation = Simulation::with_abilities(room, AbilitySet::new(true, false));
    assert_eq!(simulation.abilities(), AbilitySet::new(true, false));
    assert!(!simulation.player().dash_available());

    simulation.grant_abilities(AbilitySet::new(false, true));
    assert_eq!(simulation.abilities(), AbilitySet::ALL);
    assert!(simulation.player().dash_available());

    simulation.grant_abilities(AbilitySet::NONE);
    assert_eq!(simulation.abilities(), AbilitySet::ALL);
}

#[test]
fn current_horizontal_dash_squeezes_through_one_tile_tunnels_and_expands_afterward() {
    let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
    for x in 0..WIDTH {
        tiles[17 * WIDTH + x] = Tile::Solid;
    }
    for x in 5..12 {
        tiles[15 * WIDTH + x] = Tile::Solid;
    }
    let room = Room::new(
        "dash-tunnel",
        "Dash Tunnel",
        WIDTH as u16,
        HEIGHT as u16,
        TILE_SIZE,
        tiles,
        Point::new(30, 158),
        vec![],
    )
    .unwrap();

    let mut walking = Simulation::with_abilities(room.clone(), AbilitySet::new(false, true));
    walking.enable_current_player_movement();
    for _ in 0..30 {
        walking.step(Action {
            move_x: 1,
            ..Action::default()
        });
    }
    assert!(walking.player().bounds().right() <= 50);
    assert!(!walking.player().dash_compressed());

    let mut dashing = Simulation::with_abilities(room, AbilitySet::new(false, true));
    dashing.enable_current_player_movement();
    dashing.step(Action {
        move_x: 1,
        dash: true,
        ..Action::default()
    });
    assert!(dashing.player().dash_compressed());
    for _ in 1..DASH_TICKS {
        dashing.step(Action {
            move_x: 1,
            ..Action::default()
        });
    }
    assert!(dashing.player().bounds().x >= 50);
    assert!(dashing.player().dash_compressed());

    for _ in 0..100 {
        dashing.step(Action {
            move_x: 1,
            ..Action::default()
        });
        if !dashing.player().dash_compressed() {
            break;
        }
    }
    assert!(
        dashing.player().bounds().x >= 120,
        "player stopped at {:?}",
        dashing.player().bounds()
    );
    assert!(!dashing.player().dash_compressed());
    assert_eq!(dashing.player().bounds().height, 12);
}
