use downwards_ai::{
    DifficultyConfig, ReachedTarget, Replay, SearchStats, SearchTarget, ShakyHandConfig, Solution,
    TargetSolution, analyze_solution, evaluate_shaky_hand,
};
use downwards_core::{
    AbilitySet, Action, BoundarySide, Door, Exit, Pickup, Point, Rect, Room, Simulation,
    SimulationEvent, Tile, TimedHazard,
};
use downwards_lab::{
    BoundaryMask, CollisionCell, CollisionTopologyDescriptor, ObservationFeature, RoomAblationKind,
    SemanticAction, SemanticEvent, SimulationGeometryDescriptor, StaticVisualDescriptor,
    TraversalGrid, WitnessObservationError, collision_topology_distance,
    observation_feature_vector, observe_solution, observe_successful_replay,
    room_ablation_variants, route_difficulty_vector, route_diversity, semantic_action_distance,
    static_visual_distance, traversal_distance,
};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

fn floor_tiles() -> Vec<Tile> {
    let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
    for y in 16..HEIGHT {
        for x in 0..WIDTH {
            tiles[usize::from(y) * usize::from(WIDTH) + usize::from(x)] = Tile::Solid;
        }
    }
    tiles
}

fn flat_room(id: &str, tiles: Vec<Tile>) -> Room {
    Room::new(
        id,
        format!("Room {id}"),
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(20, 148),
        vec![Exit {
            id: "right".to_owned(),
            bounds: Rect::new(100, 125, 10, 35),
            destination: None,
            destination_entrance: None,
        }],
    )
    .unwrap()
}

fn canonical_room_variant(alternate: bool) -> Room {
    let mut exits = vec![
        Exit {
            id: if alternate { "alpha" } else { "right" }.to_owned(),
            bounds: Rect::new(100, 125, 10, 35),
            destination: None,
            destination_entrance: None,
        },
        Exit {
            id: if alternate { "omega" } else { "left" }.to_owned(),
            bounds: Rect::new(0, 120, 10, 40),
            destination: None,
            destination_entrance: None,
        },
    ];
    let mut pickups = vec![
        Pickup::new(
            if alternate { "ruby" } else { "coin" },
            Rect::new(180, 130, 6, 6),
        )
        .unwrap(),
        Pickup::new(
            if alternate { "opal" } else { "cache" },
            Rect::new(250, 100, 6, 6),
        )
        .unwrap(),
    ];
    let mut hazards = vec![
        TimedHazard::new(
            Rect::new(210, 140, 6, 20),
            if alternate { 240 } else { 90 },
            if alternate { 80 } else { 15 },
            if alternate { 120 } else { 5 },
        )
        .unwrap(),
        TimedHazard::new(
            Rect::new(280, 140, 6, 20),
            if alternate { 75 } else { 180 },
            if alternate { 20 } else { 60 },
            if alternate { 40 } else { 90 },
        )
        .unwrap(),
    ];
    if alternate {
        exits.reverse();
        pickups.reverse();
        hazards.reverse();
    }
    Room::new(
        if alternate { "other-id" } else { "first-id" },
        if alternate {
            "Other name"
        } else {
            "First name"
        },
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        floor_tiles(),
        Point::new(20, 148),
        exits,
    )
    .unwrap()
    .with_objects(hazards, pickups)
    .unwrap()
}

fn replay_until_exit(initial: &Simulation, mut input: impl FnMut(usize) -> Action) -> Replay {
    let mut probe = initial.clone();
    let mut actions = Vec::new();
    for tick in 0..240 {
        let action = input(tick);
        let report = probe.step(action);
        actions.push(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::ExitReached { .. }))
        {
            return Replay::record(initial, actions);
        }
    }
    panic!("fixture did not reach its exit");
}

fn right_action(jump: bool) -> Action {
    Action {
        move_x: 1,
        jump,
        ..Action::default()
    }
}

#[test]
fn static_visual_descriptor_is_canonical_and_excludes_timing_and_ids() {
    let first = canonical_room_variant(false);
    let alternate = canonical_room_variant(true);

    assert_ne!(first, alternate);
    let first_descriptor = StaticVisualDescriptor::from_room(&first);
    let alternate_descriptor = StaticVisualDescriptor::from_room(&alternate);
    assert_eq!(first_descriptor, alternate_descriptor);
    assert_eq!(
        static_visual_distance(&first_descriptor, &alternate_descriptor).combined,
        0.0
    );
}

#[test]
fn simulation_geometry_identity_includes_timing_but_excludes_labels_and_order() {
    let first = canonical_room_variant(false);
    let alternate = canonical_room_variant(true);

    let first_static = StaticVisualDescriptor::from_room(&first);
    let alternate_static = StaticVisualDescriptor::from_room(&alternate);
    assert_eq!(first_static, alternate_static);

    let first_simulation = SimulationGeometryDescriptor::from_room(&first);
    let alternate_simulation = SimulationGeometryDescriptor::from_room(&alternate);
    assert_ne!(first_simulation, alternate_simulation);
    assert_ne!(
        first_simulation.stable_digest(),
        alternate_simulation.stable_digest()
    );

    let same_physics = canonical_room_variant(false);
    let same_physics = SimulationGeometryDescriptor::from_room(&same_physics);
    assert_eq!(first_simulation, same_physics);
    assert_eq!(
        first_simulation.stable_digest(),
        same_physics.stable_digest()
    );
}

#[test]
fn collision_topology_ignores_hazard_paint_but_preserves_directional_collision() {
    let empty = flat_room("empty", floor_tiles());
    let mut hazard_tiles = floor_tiles();
    hazard_tiles[10 * usize::from(WIDTH) + 20] = Tile::HazardUp;
    let hazard = flat_room("hazard", hazard_tiles);

    let empty_visual = StaticVisualDescriptor::from_room(&empty);
    let hazard_visual = StaticVisualDescriptor::from_room(&hazard);
    assert!(static_visual_distance(&empty_visual, &hazard_visual).tiles > 0.0);
    let mut downward_tiles = floor_tiles();
    downward_tiles[10 * usize::from(WIDTH) + 20] = Tile::HazardDown;
    let downward_hazard = flat_room("downward-hazard", downward_tiles);
    let downward_visual = StaticVisualDescriptor::from_room(&downward_hazard);
    assert_ne!(hazard_visual, downward_visual);

    let empty_collision = CollisionTopologyDescriptor::from_room(&empty);
    let hazard_collision = CollisionTopologyDescriptor::from_room(&hazard);
    let downward_collision = CollisionTopologyDescriptor::from_room(&downward_hazard);
    assert_eq!(empty_collision, hazard_collision);
    assert_eq!(empty_collision, downward_collision);

    let mut one_way_tiles = floor_tiles();
    one_way_tiles[10 * usize::from(WIDTH) + 20] = Tile::OneWay;
    let one_way = flat_room("one-way", one_way_tiles);
    let one_way_collision = CollisionTopologyDescriptor::from_room(&one_way);
    assert_eq!(
        one_way_collision.cells[10 * usize::from(WIDTH) + 20],
        CollisionCell::OneWay
    );
    assert!(collision_topology_distance(&empty_collision, &one_way_collision).combined > 0.0);
}

#[test]
fn collision_descriptor_reports_exposed_faces_and_separate_regions() {
    let mut tiles = floor_tiles();
    for y in 0..16 {
        tiles[y * usize::from(WIDTH) + 16] = Tile::Solid;
    }
    let descriptor = CollisionTopologyDescriptor::from_room(&flat_room("split", tiles));

    assert_eq!(descriptor.regions.len(), 2);
    assert!(
        descriptor
            .regions
            .iter()
            .all(|region| region.boundaries.touches(BoundaryMask::TOP))
    );
    assert!(
        descriptor
            .faces
            .iter()
            .any(|face| face.length_cells() == 16)
    );
}

#[test]
fn successful_replay_produces_coarse_space_and_semantic_action_traces() {
    let initial = Simulation::new(flat_room("trace", floor_tiles()));
    let replay = replay_until_exit(&initial, |_| Action {
        move_x: 9,
        ..Action::default()
    });
    let observation =
        observe_successful_replay(&initial, &replay, TraversalGrid::default()).unwrap();

    assert_eq!(observation.reached_exit_id, "right");
    assert_eq!(observation.completion_ticks, replay.frames.len());
    assert_eq!(
        observation.traversal.sample_count,
        observation.completion_ticks + 1
    );
    assert!(observation.traversal.visited_cells.len() > 1);
    assert_eq!(observation.actions.spans.len(), 1);
    assert_eq!(
        observation.actions.spans[0].action,
        SemanticAction {
            move_x: 1,
            ..SemanticAction::default()
        }
    );
    assert!(
        observation
            .actions
            .events
            .iter()
            .any(|event| event.event == SemanticEvent::Exit)
    );
}

#[test]
fn route_difficulty_vector_assembles_evidence_and_marks_noise_missing() {
    let initial = Simulation::new(flat_room("difficulty-vector", floor_tiles()));
    let replay = replay_until_exit(&initial, |_| right_action(false));
    let solution = Solution {
        exit_id: "right".to_owned(),
        replay,
        stats: SearchStats {
            expanded_nodes: 123,
            generated_nodes: 456,
            simulated_ticks: 789,
            deepest_path_ticks: 90,
        },
    };
    let witness = observe_solution(&initial, &solution, TraversalGrid::default()).unwrap();
    let difficulty = analyze_solution(&initial, &solution, &DifficultyConfig::default()).unwrap();
    let vector = route_difficulty_vector(&witness, &difficulty, None).unwrap();

    assert_eq!(vector.target_id, "right");
    assert_eq!(vector.traversal.completion_ticks, witness.completion_ticks);
    assert!(vector.traversal.coarse_path_length_cells > 0.0);
    assert_eq!(vector.control.accepted_movement.total_jumps(), 0);
    assert_eq!(vector.hazards.pressure, 0.0);
    assert_eq!(
        vector.operational_solver_cost.expanded_nodes,
        solution.stats.expanded_nodes
    );
    assert!(matches!(
        vector.timing.shaky_hand,
        downwards_lab::ShakyHandEvidence::Missing { .. }
    ));
}

#[test]
fn route_difficulty_vector_preserves_shaky_hand_curve_applicability() {
    let initial = Simulation::new(flat_room("difficulty-noise", floor_tiles()));
    let replay = replay_until_exit(&initial, |_| right_action(false));
    let solution = Solution {
        exit_id: "right".to_owned(),
        replay: replay.clone(),
        stats: SearchStats::default(),
    };
    let target_solution = TargetSolution {
        target: SearchTarget::exit("right"),
        reached: ReachedTarget::Exit("right".to_owned()),
        replay,
        stats: SearchStats::default(),
    };
    let witness = observe_solution(&initial, &solution, TraversalGrid::default()).unwrap();
    let difficulty = analyze_solution(&initial, &solution, &DifficultyConfig::default()).unwrap();
    let shaky_hand = evaluate_shaky_hand(
        &initial,
        &target_solution,
        ShakyHandConfig {
            trials_per_curve_point: 4,
            ..ShakyHandConfig::default()
        },
    )
    .unwrap();
    let vector = route_difficulty_vector(&witness, &difficulty, Some(&shaky_hand)).unwrap();

    let downwards_lab::ShakyHandEvidence::Observed { curves, .. } = vector.timing.shaky_hand else {
        panic!("a supplied shaky-hand report must remain observed evidence");
    };
    assert_eq!(
        curves.len(),
        downwards_lab::ROUTE_DIFFICULTY_NOISE_POINTS.len()
    );
    assert!(curves.iter().any(|curve| matches!(
        curve.evidence,
        downwards_lab::TimingPointEvidence::NotApplicable { .. }
    )));
    assert!(curves.iter().any(|curve| matches!(
        curve.evidence,
        downwards_lab::TimingPointEvidence::Observed(_)
    )));
}

#[test]
fn behavior_distance_distinguishes_solutions_when_room_hamming_is_zero() {
    let room = flat_room("behavior", floor_tiles());
    let initial = Simulation::new(room.clone());
    let straight = replay_until_exit(&initial, |_| right_action(false));
    let jumping = replay_until_exit(&initial, |tick| right_action(tick < 10));
    let grid = TraversalGrid::default();
    let straight = observe_successful_replay(&initial, &straight, grid).unwrap();
    let jumping = observe_successful_replay(&initial, &jumping, grid).unwrap();

    let visual = StaticVisualDescriptor::from_room(&room);
    let collision = CollisionTopologyDescriptor::from_room(&room);
    assert_eq!(static_visual_distance(&visual, &visual).combined, 0.0);
    assert_eq!(
        collision_topology_distance(&collision, &collision).combined,
        0.0
    );
    assert!(semantic_action_distance(&straight.actions, &jumping.actions).combined > 0.0);
    assert!(traversal_distance(&straight.traversal, &jumping.traversal).combined > 0.0);
}

#[test]
fn route_diversity_counts_material_play_styles_and_reports_pairwise_distance() {
    let room = flat_room("route-diversity", floor_tiles());
    let initial = Simulation::new(room);
    let straight = replay_until_exit(&initial, |_| right_action(false));
    let jumping = replay_until_exit(&initial, |tick| right_action(tick < 10));
    let grid = TraversalGrid::default();
    let straight = observe_successful_replay(&initial, &straight, grid).unwrap();
    let jumping = observe_successful_replay(&initial, &jumping, grid).unwrap();

    let report = route_diversity(&[straight, jumping]);
    assert_eq!(report.route_count, 2);
    assert_eq!(report.reached_target_count, 1);
    assert_eq!(report.spatial_path_classes, 2);
    assert_eq!(report.semantic_controller_classes, 2);
    assert_eq!(report.joint_play_style_classes, 2);
    assert_eq!(report.combined_behavior_distance.comparisons, 1);
    assert!(report.combined_behavior_distance.minimum.unwrap() > 0.0);
    assert_eq!(
        report.combined_behavior_distance.minimum,
        report.combined_behavior_distance.maximum
    );
}

#[test]
fn route_diversity_does_not_count_timing_only_changes_as_new_play_styles() {
    let initial = Simulation::new(flat_room("route-timing", floor_tiles()));
    let replay = replay_until_exit(&initial, |tick| right_action(tick < 10));
    let original = observe_successful_replay(&initial, &replay, TraversalGrid::default()).unwrap();
    let mut retimed = original.clone();
    retimed.completion_ticks += 5;
    retimed.traversal.sample_count += 5;
    retimed.traversal.spans[0].samples += 5;
    retimed.actions.total_ticks += 5;
    retimed.actions.spans[0].ticks += 5;
    for event in &mut retimed.actions.events {
        event.tick += 5;
    }

    let report = route_diversity(&[original, retimed]);
    assert_eq!(report.spatial_path_classes, 1);
    assert_eq!(report.semantic_controller_classes, 1);
    assert_eq!(report.joint_play_style_classes, 1);
    assert!(report.traversal_distance.minimum.unwrap() > 0.0);
    assert!(report.semantic_action_distance.minimum.unwrap() > 0.0);
}

#[test]
fn route_diversity_has_explicit_empty_and_single_route_semantics() {
    let empty = route_diversity(&[]);
    assert_eq!(empty.route_count, 0);
    assert_eq!(empty.joint_play_style_classes, 0);
    assert_eq!(empty.combined_behavior_distance.comparisons, 0);
    assert_eq!(empty.combined_behavior_distance.mean, None);

    let initial = Simulation::new(flat_room("route-single", floor_tiles()));
    let replay = replay_until_exit(&initial, |_| right_action(false));
    let observation =
        observe_successful_replay(&initial, &replay, TraversalGrid::default()).unwrap();
    let single = route_diversity(&[observation]);
    assert_eq!(single.route_count, 1);
    assert_eq!(single.joint_play_style_classes, 1);
    assert_eq!(
        single.combined_behavior_distance.mean_nearest_neighbor,
        None
    );
}

#[test]
fn feature_vector_is_deterministic_bounded_and_strategy_neutral() {
    let room = flat_room("features", floor_tiles());
    let initial = Simulation::with_abilities(room.clone(), AbilitySet::new(false, true));
    let replay = replay_until_exit(&initial, |tick| right_action(tick < 10));
    let witness = observe_successful_replay(&initial, &replay, TraversalGrid::default()).unwrap();
    let visual = StaticVisualDescriptor::from_room(&room);
    let collision = CollisionTopologyDescriptor::from_room(&room);
    let first = observation_feature_vector(&initial, &visual, &collision, &witness);
    let second = observation_feature_vector(&initial, &visual, &collision, &witness);

    assert_eq!(first, second);
    assert!(first.values.iter().all(|value| (0.0..=1.0).contains(value)));
    assert!(first.get(ObservationFeature::SuccessfulJumpRate) > 0.0);
    assert_eq!(first.get(ObservationFeature::WallJumpAbility), 0.0);
    assert_eq!(first.get(ObservationFeature::DashAbility), 1.0);
    assert_eq!(first.normalized_l1_distance(&second), 0.0);
    assert_eq!(first.normalized_l2_distance(&second), 0.0);
}

#[test]
fn observation_rejects_non_success_and_solution_exit_mismatch() {
    let initial = Simulation::new(flat_room("errors", floor_tiles()));
    let idle = Replay::record(&initial, [Action::default(); 3]);
    assert_eq!(
        observe_successful_replay(&initial, &idle, TraversalGrid::default()),
        Err(WitnessObservationError::DidNotReachExit)
    );

    let replay = replay_until_exit(&initial, |_| right_action(false));
    let solution = Solution {
        exit_id: "not-right".to_owned(),
        replay,
        stats: SearchStats::default(),
    };
    assert!(matches!(
        observe_solution(&initial, &solution, TraversalGrid::default()),
        Err(WitnessObservationError::ReachedUnexpectedExit { .. })
    ));
}

#[test]
fn already_completed_initial_state_has_a_zero_tick_observation() {
    let initial = Simulation::new(flat_room("already-complete", floor_tiles()));
    let path = replay_until_exit(&initial, |_| right_action(false));
    let mut completed = initial.clone();
    for action in path.actions() {
        completed.step(action);
    }
    assert_eq!(completed.reached_exit(), Some("right"));

    let empty = Replay::record(&completed, []);
    let observation =
        observe_successful_replay(&completed, &empty, TraversalGrid::default()).unwrap();
    assert_eq!(observation.completion_ticks, 0);
    assert_eq!(observation.traversal.sample_count, 1);
    assert!(observation.actions.spans.is_empty());
}

#[test]
fn traversal_grid_rejects_zero_dimensions() {
    assert!(TraversalGrid::new(0, 9).is_err());
    assert!(TraversalGrid::new(16, 0).is_err());
    let grid = TraversalGrid::new(16, 9).unwrap();
    assert_eq!(grid, TraversalGrid::default());
    assert_eq!(grid.columns(), 16);
    assert_eq!(grid.rows(), 9);
}

#[test]
fn room_ablation_variants_remove_one_canonical_feature_without_touching_boundary_shell() {
    let mut tiles = floor_tiles();
    for x in 8..=10 {
        tiles[10 * usize::from(WIDTH) + x] = Tile::Solid;
    }
    for x in 20..=21 {
        tiles[15 * usize::from(WIDTH) + x] = Tile::HazardUp;
    }
    let doors = vec![
        Door {
            id: "west".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 120, 4, 30),
            arrival: Point::new(12, 148),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east".to_owned(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 120, 4, 30),
            arrival: Point::new(300, 148),
            destination_room: None,
            destination_door: None,
        },
    ];
    let room = flat_room("ablation", tiles)
        .with_objects(
            vec![TimedHazard::new(Rect::new(250, 130, 5, 30), 120, 30, 20).unwrap()],
            vec![Pickup::new("cache", Rect::new(150, 140, 6, 6)).unwrap()],
        )
        .unwrap()
        .with_doors(doors.clone())
        .unwrap();

    let first = room_ablation_variants(&room).unwrap();
    let second = room_ablation_variants(&room).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);

    let terrain = first
        .iter()
        .find(|variant| {
            matches!(
                variant.kind,
                RoomAblationKind::InteriorTerrainComponent { .. }
            )
        })
        .unwrap();
    assert_eq!(terrain.room.tile(8, 10), Some(Tile::Empty));
    assert_eq!(terrain.room.tile(0, 16), Some(Tile::Solid));
    assert_eq!(terrain.room.timed_hazards().len(), 1);
    assert_eq!(terrain.room.doors(), doors);
    assert_eq!(terrain.room.pickups(), room.pickups());

    let static_hazard = first
        .iter()
        .find(|variant| matches!(variant.kind, RoomAblationKind::StaticHazardComponent { .. }))
        .unwrap();
    assert_eq!(static_hazard.room.tile(20, 15), Some(Tile::Empty));
    assert_eq!(static_hazard.room.tile(8, 10), Some(Tile::Solid));

    let timed_hazard = first
        .iter()
        .find(|variant| matches!(variant.kind, RoomAblationKind::TimedHazard { .. }))
        .unwrap();
    assert!(timed_hazard.room.timed_hazards().is_empty());
    assert_eq!(timed_hazard.room.tile(20, 15), Some(Tile::HazardUp));
}
