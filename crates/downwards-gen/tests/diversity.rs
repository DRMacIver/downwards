use std::collections::{HashMap, HashSet};

use downwards_core::{BoundarySide, Exit, Pickup, Point, Rect, Room, Tile, TimedHazard};
use downwards_gen::{AbilityTier, ROOM_HEIGHT, ROOM_WIDTH, TILE_SIZE, v6::generate_uncurated};

const CATALOGUE_SIZE: u64 = 1_000;

// These floors deliberately require a large step beyond generator v5. Exact
// visual uniqueness by itself is too easy to inflate by moving a pickup or a
// timed hazard, so the tile field and collision topology have independent
// requirements as well.
const MIN_UNIQUE_VISUALS: usize = 900;
const MIN_UNIQUE_TILE_FIELDS: usize = 850;
const MIN_UNIQUE_COLLISION_TOPOLOGIES: usize = 750;
const MIN_UNIQUE_ROUTE_SIGNATURES: usize = 900;
const MAX_EXACT_VISUAL_REPETITION: usize = 4;
const MIN_VARIABLE_TILE_CELLS: usize = 100;
const MIN_SAMPLED_TILE_HAMMING_P10: usize = 60;
const MIN_SAMPLED_TILE_HAMMING_MEDIAN: usize = 90;
const MIN_NEAREST_TILE_HAMMING_P10: usize = 20;
const MIN_NEAREST_TILE_HAMMING_MEDIAN: usize = 35;

const ALL_TIERS: [AbilityTier; 4] = [
    AbilityTier::Baseline,
    AbilityTier::WallJump,
    AbilityTier::Dash,
    AbilityTier::WallJumpAndDash,
];

type VisualRect = (i32, i32, i32, i32);

/// Everything visible in a static room preview, with deliberately no room
/// identity, object identity, destination, or timed-hazard schedule data.
///
/// Lists of objects are sorted because their storage order is not visual.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StaticVisualDescriptor {
    width: u16,
    height: u16,
    tile_size: i32,
    spawn: (i32, i32),
    tiles: Vec<u8>,
    exits: Vec<VisualRect>,
    doors: Vec<(BoundarySide, VisualRect)>,
    pickups: Vec<VisualRect>,
    timed_hazards: Vec<VisualRect>,
}

impl StaticVisualDescriptor {
    fn from_room(room: &Room) -> Self {
        let spawn = room.spawn();
        Self {
            width: room.width(),
            height: room.height(),
            tile_size: room.tile_size(),
            spawn: (spawn.x, spawn.y),
            tiles: room.tiles().iter().copied().map(tile_code).collect(),
            exits: sorted_rects(room.exits().iter().map(|exit| exit.bounds)),
            doors: {
                let mut doors = room
                    .doors()
                    .iter()
                    .map(|door| {
                        (
                            door.side,
                            (
                                door.trigger_bounds.x,
                                door.trigger_bounds.y,
                                door.trigger_bounds.width,
                                door.trigger_bounds.height,
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                doors.sort_unstable();
                doors
            },
            pickups: sorted_rects(room.pickups().iter().map(|pickup| pickup.bounds())),
            timed_hazards: sorted_rects(room.timed_hazards().iter().map(|hazard| hazard.bounds())),
        }
    }
}

fn sorted_rects(bounds: impl Iterator<Item = Rect>) -> Vec<VisualRect> {
    let mut rects = bounds
        .map(|bounds| (bounds.x, bounds.y, bounds.width, bounds.height))
        .collect::<Vec<_>>();
    rects.sort_unstable();
    rects
}

const fn tile_code(tile: Tile) -> u8 {
    match tile {
        Tile::Empty => 0,
        Tile::Solid => 1,
        Tile::Hazard => 2,
        Tile::OneWay => 3,
    }
}

/// Geometry relevant to collision routing. Static hazards are intentionally
/// treated as empty here: a catalogue cannot pass this metric merely by
/// repainting the same platforms with different floor hazards.
fn collision_topology(room: &Room) -> Vec<u8> {
    room.tiles()
        .iter()
        .map(|tile| match tile {
            Tile::Solid => 1,
            Tile::OneWay => 2,
            Tile::Empty | Tile::Hazard => 0,
        })
        .collect()
}

#[derive(Debug)]
struct DiversityMetrics {
    unique_visuals: usize,
    unique_tile_fields: usize,
    unique_collision_topologies: usize,
    unique_route_signatures: usize,
    largest_exact_repeat: usize,
    variable_tile_cells: usize,
    sampled_tile_hamming_p10: usize,
    sampled_tile_hamming_median: usize,
    nearest_tile_hamming_p10: usize,
    nearest_tile_hamming_median: usize,
}

fn catalogue_metrics(tier: AbilityTier) -> DiversityMetrics {
    let mut visuals = HashMap::<StaticVisualDescriptor, usize>::new();
    let mut tile_fields = HashSet::<Vec<u8>>::new();
    let mut collision_topologies = HashSet::<Vec<u8>>::new();
    let mut route_signatures = HashSet::<u64>::new();
    let mut tile_states_by_cell = Vec::<u8>::new();
    let mut ordered_tile_fields = Vec::with_capacity(CATALOGUE_SIZE as usize);

    for seed in 0..CATALOGUE_SIZE {
        let level = generate_uncurated(seed, tier.abilities())
            .unwrap_or_else(|error| panic!("{tier:?} seed {seed} failed generation: {error}"));
        let descriptor = StaticVisualDescriptor::from_room(&level.generated.room);
        route_signatures.insert(level.route_summary.signature);

        if tile_states_by_cell.is_empty() {
            tile_states_by_cell.resize(descriptor.tiles.len(), 0);
        }
        assert_eq!(tile_states_by_cell.len(), descriptor.tiles.len());
        for (&tile, states) in descriptor.tiles.iter().zip(&mut tile_states_by_cell) {
            *states |= 1 << tile;
        }

        tile_fields.insert(descriptor.tiles.clone());
        ordered_tile_fields.push(descriptor.tiles.clone());
        collision_topologies.insert(collision_topology(&level.generated.room));
        *visuals.entry(descriptor).or_default() += 1;
    }

    // Exact uniqueness can still hide a catalogue of one-tile nudges. Sample
    // 10,000 deterministic distinct-seed pairs across the full catalogue,
    // then independently inspect every pair in a 256-seed prefix for its
    // nearest neighbour. Both distributions must retain meaningful tile-scale
    // separation.
    let mut sampled_distances = (0_u64..10_000)
        .map(|sample| {
            let left = (mix64(sample) % CATALOGUE_SIZE) as usize;
            let mut right = (mix64(sample ^ 0xa076_1d64_78bd_642f) % (CATALOGUE_SIZE - 1)) as usize;
            if right >= left {
                right += 1;
            }
            tile_hamming(&ordered_tile_fields[left], &ordered_tile_fields[right])
        })
        .collect::<Vec<_>>();
    sampled_distances.sort_unstable();

    let nearest_sample_size = ordered_tile_fields.len().min(256);
    let mut nearest_distances = vec![usize::MAX; nearest_sample_size];
    for left in 0..nearest_sample_size {
        for right in (left + 1)..nearest_sample_size {
            let distance = tile_hamming(&ordered_tile_fields[left], &ordered_tile_fields[right]);
            nearest_distances[left] = nearest_distances[left].min(distance);
            nearest_distances[right] = nearest_distances[right].min(distance);
        }
    }
    nearest_distances.sort_unstable();

    DiversityMetrics {
        unique_visuals: visuals.len(),
        unique_tile_fields: tile_fields.len(),
        unique_collision_topologies: collision_topologies.len(),
        unique_route_signatures: route_signatures.len(),
        largest_exact_repeat: visuals.values().copied().max().unwrap_or_default(),
        variable_tile_cells: tile_states_by_cell
            .iter()
            .filter(|states| states.count_ones() > 1)
            .count(),
        sampled_tile_hamming_p10: percentile(&sampled_distances, 10),
        sampled_tile_hamming_median: percentile(&sampled_distances, 50),
        nearest_tile_hamming_p10: percentile(&nearest_distances, 10),
        nearest_tile_hamming_median: percentile(&nearest_distances, 50),
    }
}

fn tile_hamming(left: &[u8], right: &[u8]) -> usize {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .filter(|(left, right)| left != right)
        .count()
}

fn percentile(sorted: &[usize], percentile: usize) -> usize {
    assert!(!sorted.is_empty());
    sorted[(sorted.len() - 1) * percentile / 100]
}

const fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[test]
fn v6_uncurated_pool_has_substantial_static_visual_diversity() {
    let mut failures = Vec::new();

    for tier in ALL_TIERS {
        let metrics = catalogue_metrics(tier);
        eprintln!("{tier:?}: {metrics:?}");

        if metrics.unique_visuals < MIN_UNIQUE_VISUALS {
            failures.push(format!(
                "{tier:?}: only {} unique static visuals; require {MIN_UNIQUE_VISUALS}",
                metrics.unique_visuals
            ));
        }
        if metrics.unique_tile_fields < MIN_UNIQUE_TILE_FIELDS {
            failures.push(format!(
                "{tier:?}: only {} unique tile fields; require {MIN_UNIQUE_TILE_FIELDS}",
                metrics.unique_tile_fields
            ));
        }
        if metrics.unique_collision_topologies < MIN_UNIQUE_COLLISION_TOPOLOGIES {
            failures.push(format!(
                "{tier:?}: only {} unique collision topologies; require {MIN_UNIQUE_COLLISION_TOPOLOGIES}",
                metrics.unique_collision_topologies
            ));
        }
        if metrics.unique_route_signatures < MIN_UNIQUE_ROUTE_SIGNATURES {
            failures.push(format!(
                "{tier:?}: only {} unique route signatures; require {MIN_UNIQUE_ROUTE_SIGNATURES}",
                metrics.unique_route_signatures
            ));
        }
        if metrics.largest_exact_repeat > MAX_EXACT_VISUAL_REPETITION {
            failures.push(format!(
                "{tier:?}: one static visual repeats {} times; permit at most {MAX_EXACT_VISUAL_REPETITION}",
                metrics.largest_exact_repeat
            ));
        }
        if metrics.variable_tile_cells < MIN_VARIABLE_TILE_CELLS {
            failures.push(format!(
                "{tier:?}: only {} screen tile cells ever vary; require {MIN_VARIABLE_TILE_CELLS}",
                metrics.variable_tile_cells
            ));
        }
        if metrics.sampled_tile_hamming_p10 < MIN_SAMPLED_TILE_HAMMING_P10 {
            failures.push(format!(
                "{tier:?}: sampled tile Hamming p10 is only {}; require {MIN_SAMPLED_TILE_HAMMING_P10}",
                metrics.sampled_tile_hamming_p10
            ));
        }
        if metrics.sampled_tile_hamming_median < MIN_SAMPLED_TILE_HAMMING_MEDIAN {
            failures.push(format!(
                "{tier:?}: sampled tile Hamming median is only {}; require {MIN_SAMPLED_TILE_HAMMING_MEDIAN}",
                metrics.sampled_tile_hamming_median
            ));
        }
        if metrics.nearest_tile_hamming_p10 < MIN_NEAREST_TILE_HAMMING_P10 {
            failures.push(format!(
                "{tier:?}: nearest-neighbour tile Hamming p10 is only {}; require {MIN_NEAREST_TILE_HAMMING_P10}",
                metrics.nearest_tile_hamming_p10
            ));
        }
        if metrics.nearest_tile_hamming_median < MIN_NEAREST_TILE_HAMMING_MEDIAN {
            failures.push(format!(
                "{tier:?}: nearest-neighbour tile Hamming median is only {}; require {MIN_NEAREST_TILE_HAMMING_MEDIAN}",
                metrics.nearest_tile_hamming_median
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "generator catalogue diversity regression:\n{}",
        failures.join("\n")
    );
}

#[test]
fn static_visual_descriptor_ignores_timed_hazard_schedules() {
    let with_schedule = |period_ticks, active_ticks, phase_ticks| {
        let hazards = vec![
            TimedHazard::new(
                Rect::new(100, 100, 6, 20),
                period_ticks,
                active_ticks,
                phase_ticks,
            )
            .unwrap(),
        ];
        Room::new(
            "descriptor-fixture",
            "Descriptor fixture",
            ROOM_WIDTH,
            ROOM_HEIGHT,
            TILE_SIZE,
            vec![Tile::Empty; usize::from(ROOM_WIDTH) * usize::from(ROOM_HEIGHT)],
            Point::new(20, 140),
            vec![Exit {
                id: "down".to_owned(),
                bounds: Rect::new(300, 140, 10, 30),
                destination: None,
                destination_entrance: None,
            }],
        )
        .unwrap()
        .with_objects(
            hazards,
            vec![Pickup::new("cache", Rect::new(150, 80, 6, 6)).unwrap()],
        )
        .unwrap()
    };

    let fast = with_schedule(90, 15, 5);
    let slow = with_schedule(240, 80, 120);
    assert_ne!(fast.timed_hazards(), slow.timed_hazards());
    assert_eq!(
        StaticVisualDescriptor::from_room(&fast),
        StaticVisualDescriptor::from_room(&slow)
    );
}
