use downwards_core::{AbilitySet, BoundarySide, Door, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect};

use super::{
    BoundaryPort, CandidateParts, ChallengeIntent, ExperimentalGenerationError, NodeRole,
    RoutePlan, SupportKind, SupportSpec,
    common::{
        FLOOR_ROW, StableRng, add_route_edge, add_route_node, edge_verb, mirrored_spawn,
        mirrored_support,
    },
};
use crate::{ROOM_HEIGHT, ROOM_WIDTH, TILE_SIZE};

const ENTRY_CENTER_X: u16 = 6;
const MERGE_CENTER_X: u16 = 26;
const ANCHOR_ROW: u16 = 15;
const RNG_STREAM: u64 = 0x6379_636c_6963_7631;
const PORT_RNG_STREAM: u64 = 0x706f_7274_6379_6331;
const SIDE_DOOR_DEPTH: i32 = 12;
const CEILING_DOOR_DEPTH: i32 = 12;
const DOOR_SPAN: i32 = 20;

#[derive(Clone, Copy)]
struct Profile {
    lower_interior_min: u16,
    lower_interior_extra: u16,
    upper_interior_min: u16,
    upper_interior_extra: u16,
    peak_row_min: u16,
    peak_row_extra: u16,
    support_width_min: u16,
    support_width_extra: u16,
    detours: u16,
    optional_detour: bool,
    timed_hazards: u16,
    recovery_pockets: u16,
}

impl Profile {
    const fn for_intent(intent: ChallengeIntent) -> Self {
        match intent {
            ChallengeIntent::Gentle => Self {
                lower_interior_min: 3,
                lower_interior_extra: 2,
                upper_interior_min: 4,
                upper_interior_extra: 2,
                peak_row_min: 11,
                peak_row_extra: 1,
                support_width_min: 4,
                support_width_extra: 2,
                detours: 0,
                optional_detour: true,
                timed_hazards: 1,
                recovery_pockets: 2,
            },
            ChallengeIntent::Standard => Self {
                lower_interior_min: 4,
                lower_interior_extra: 2,
                upper_interior_min: 6,
                upper_interior_extra: 1,
                peak_row_min: 9,
                peak_row_extra: 1,
                support_width_min: 3,
                support_width_extra: 2,
                detours: 1,
                optional_detour: true,
                timed_hazards: 2,
                recovery_pockets: 1,
            },
            ChallengeIntent::Technical => Self {
                lower_interior_min: 5,
                lower_interior_extra: 2,
                upper_interior_min: 7,
                upper_interior_extra: 1,
                peak_row_min: 7,
                peak_row_extra: 1,
                support_width_min: 2,
                support_width_extra: 2,
                detours: 2,
                optional_detour: true,
                timed_hazards: 3,
                recovery_pockets: 0,
            },
        }
    }
}

pub(super) fn generate(
    seed: u64,
    _abilities: AbilitySet,
    intent: ChallengeIntent,
) -> Result<CandidateParts, ExperimentalGenerationError> {
    let profile = Profile::for_intent(intent);
    let mut rng = StableRng::new(seed, RNG_STREAM);
    let mut port_rng = StableRng::new(seed, PORT_RNG_STREAM);
    let mirrored = rng.coin();

    // These four supports are deliberately separate from the generated arcs.
    // The lateral supports are physical boundary-port anchors, while entry
    // and merge are stable attachment points for the cyclic interior.
    let start_support = mirrored_support(
        SupportSpec {
            start_x: 1,
            end_x: 6,
            row: FLOOR_ROW,
            kind: SupportKind::Solid,
        },
        mirrored,
    );
    let entry_support = mirrored_support(
        support_around(ENTRY_CENTER_X, ANCHOR_ROW, 5, SupportKind::OneWay),
        mirrored,
    );
    let merge_support = mirrored_support(
        support_around(MERGE_CENTER_X, ANCHOR_ROW, 5, SupportKind::OneWay),
        mirrored,
    );
    let goal_support = mirrored_support(
        SupportSpec {
            start_x: 27,
            end_x: 31,
            row: ANCHOR_ROW,
            kind: SupportKind::Solid,
        },
        mirrored,
    );

    let mut route_plan = RoutePlan::default();
    let start = add_route_node(&mut route_plan, NodeRole::Port, start_support);
    let entry = add_route_node(&mut route_plan, NodeRole::Junction, entry_support);
    let merge = add_route_node(&mut route_plan, NodeRole::Junction, merge_support);
    let goal = add_route_node(&mut route_plan, NodeRole::Port, goal_support);
    add_baseline_edge(&mut route_plan, start, entry, true);
    add_baseline_edge(&mut route_plan, merge, goal, true);

    let lower_count = profile.lower_interior_min + rng.below(profile.lower_interior_extra + 1);
    let upper_count = profile.upper_interior_min + rng.below(profile.upper_interior_extra + 1);
    let peak_row = profile.peak_row_min + rng.below(profile.peak_row_extra + 1);

    // Both arcs begin as a single abstract edge. Repeated interval subdivision
    // supplies their internal nodes; row composition is a separate pass. This
    // keeps topology independent of its embedding instead of selecting a room
    // template and perturbing it.
    let lower_centers = subdivided_centers(ENTRY_CENTER_X, MERGE_CENTER_X, lower_count, &mut rng);
    let upper_centers = subdivided_centers(ENTRY_CENTER_X, MERGE_CENTER_X, upper_count, &mut rng);
    let lower_rows = lower_rows(lower_count, &mut rng);
    let upper_rows = upper_rows(upper_count, peak_row, &mut rng);

    let upper_path = add_arc(
        &mut route_plan,
        entry,
        merge,
        &upper_centers,
        &upper_rows,
        profile,
        mirrored,
        false,
        &mut rng,
    );
    let lower_path = add_arc(
        &mut route_plan,
        entry,
        merge,
        &lower_centers,
        &lower_rows,
        profile,
        mirrored,
        true,
        &mut rng,
    );

    // A detour is another compositional rewrite: replace a sub-path by a
    // fork, a lifted sequence, and a rejoin while retaining the original.
    // Technical rooms recursively apply the rewrite to the newly added path,
    // producing genuinely nested cycles rather than a list of prefab lanes.
    let requested_detours =
        profile.detours + u16::from(profile.optional_detour && rng.below(3) == 0);
    let mut branch_basis = lower_path.clone();
    for depth in 0..requested_detours {
        let Some(detour) = add_detour(&mut route_plan, &branch_basis, profile, depth, &mut rng)
        else {
            break;
        };
        branch_basis = detour;
    }

    let (west_node, west_support, east_node, east_support) = if mirrored {
        (goal, goal_support, start, start_support)
    } else {
        (start, start_support, goal, goal_support)
    };
    let mut boundary_ports = vec![
        wall_port(west_node, west_support, BoundarySide::Left),
        wall_port(east_node, east_support, BoundarySide::Right),
    ];

    // Optional ports are graph rewrites too: a ceiling port grows a short
    // reversible spur from the upper arc, while a floor port promotes a
    // lower-arc landing beside a clear drop shaft. Port count is sampled on a
    // separate stream so adding dungeon-facing structure does not perturb the
    // underlying cyclic grammar.
    let wants_ceiling = match intent {
        ChallengeIntent::Gentle => port_rng.below(3) == 0,
        ChallengeIntent::Standard => true,
        ChallengeIntent::Technical => true,
    };
    if wants_ceiling {
        boundary_ports.push(add_ceiling_port(
            &mut route_plan,
            &upper_path,
            &mut port_rng,
        ));
    }

    let mut draft = super::common::RoomDraft::new();
    for node in &route_plan.nodes {
        if node.support.row != FLOOR_ROW {
            draft.platform(node.support);
        }
    }

    let pickup_node = select_pickup_node(&route_plan);
    route_plan.nodes[usize::from(pickup_node)].role = NodeRole::Pickup;
    draft.pickup_above(
        "coin-cycle",
        route_plan.nodes[usize::from(pickup_node)].support,
    );

    let recovery_nodes = mark_recovery_nodes(
        &mut route_plan,
        &lower_path,
        profile.recovery_pockets,
        pickup_node,
    );
    let wants_floor = match intent {
        ChallengeIntent::Gentle => port_rng.below(4) == 0,
        ChallengeIntent::Standard => port_rng.coin(),
        ChallengeIntent::Technical => true,
    };
    let floor_safe_range = wants_floor
        .then(|| add_floor_port(&mut route_plan, &lower_path, pickup_node, &mut port_rng))
        .flatten()
        .map(|(port, safe_range)| {
            boundary_ports.push(port);
            safe_range
        });
    let mut safe_ranges = vec![
        (start_support.start_x, start_support.end_x),
        (goal_support.start_x, goal_support.end_x),
    ];
    safe_ranges.extend(recovery_nodes.iter().map(|&node| {
        let support = route_plan.nodes[usize::from(node)].support;
        (support.start_x, support.end_x)
    }));
    safe_ranges.extend(floor_safe_range);
    draft.hazard_floor_except(&safe_ranges);

    let mut reserved_arrivals = boundary_ports
        .iter()
        .map(|port| {
            Rect::new(
                port.door.arrival.x,
                port.door.arrival.y,
                PLAYER_WIDTH,
                PLAYER_HEIGHT,
            )
        })
        .collect::<Vec<_>>();
    for port in &boundary_ports {
        if port.door.side == BoundarySide::Floor {
            let support = route_plan.nodes[usize::from(port.node_id)].support;
            let corridor_y = i32::from(support.row) * TILE_SIZE;
            reserved_arrivals.push(Rect::new(
                port.door.trigger_bounds.x,
                corridor_y,
                port.door.trigger_bounds.width,
                i32::from(ROOM_HEIGHT) * TILE_SIZE - corridor_y,
            ));
        }
    }
    place_timed_hazards(
        &mut draft,
        &route_plan,
        &lower_path,
        profile.timed_hazards,
        &reserved_arrivals,
        &mut rng,
    );

    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    Ok(CandidateParts {
        draft,
        spawn: mirrored_spawn(2, mirrored),
        boundary_ports,
        route_plan,
    })
}

fn support_around(center_x: u16, row: u16, width: u16, kind: SupportKind) -> SupportSpec {
    let mut start_x = center_x.saturating_sub(width / 2).max(1);
    let mut end_x = start_x + width;
    if end_x >= ROOM_WIDTH {
        end_x = ROOM_WIDTH - 1;
        start_x = end_x - width;
    }
    SupportSpec {
        start_x,
        end_x,
        row,
        kind,
    }
}

fn wall_port(node_id: u16, support: SupportSpec, side: BoundarySide) -> BoundaryPort {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    // Keep lateral apertures on the same tile-aligned socket grid as the
    // other generators. Arrival geometry may differ internally, but adjacent
    // rooms can only be tiled when opposite openings share an exact offset.
    let trigger_y =
        (i32::from(support.row) * TILE_SIZE - DOOR_SPAN).clamp(0, room_height - DOOR_SPAN);
    let (id, trigger_x, arrival_x) = match side {
        BoundarySide::Left => ("port-west", 0, TILE_SIZE + 2),
        BoundarySide::Right => (
            "port-east",
            room_width - SIDE_DOOR_DEPTH,
            room_width - TILE_SIZE - 2 - PLAYER_WIDTH,
        ),
        BoundarySide::Ceiling | BoundarySide::Floor => {
            unreachable!("wall_port only constructs lateral ports")
        }
    };
    BoundaryPort {
        node_id,
        door: Door {
            id: id.to_owned(),
            side,
            trigger_bounds: Rect::new(trigger_x, trigger_y, SIDE_DOOR_DEPTH, DOOR_SPAN),
            arrival: Point::new(arrival_x, standing_y),
            destination_room: None,
            destination_door: None,
        },
    }
}

fn add_ceiling_port(plan: &mut RoutePlan, upper_path: &[u16], rng: &mut StableRng) -> BoundaryPort {
    let minimum_row = upper_path
        .iter()
        .map(|&node| plan.nodes[usize::from(node)].support.row)
        .min()
        .expect("the upper arc has endpoints");
    let apexes = upper_path
        .iter()
        .copied()
        .filter(|&node| plan.nodes[usize::from(node)].support.row == minimum_row)
        .collect::<Vec<_>>();
    let attach = apexes[usize::from(
        rng.below(
            apexes
                .len()
                .try_into()
                .expect("the short apex list fits u16"),
        ),
    )];
    let attach_support = plan.nodes[usize::from(attach)].support;
    let center = attach_support.center_x().clamp(3, ROOM_WIDTH - 3);
    let mut previous = attach;
    let mut row = attach_support.row;
    while row > 4 {
        row = row.saturating_sub(2).max(4);
        let support = support_around(center, row, 3 + rng.below(2), SupportKind::OneWay);
        let role = if row == 4 {
            NodeRole::Port
        } else {
            NodeRole::Landing
        };
        let node = add_route_node(plan, role, support);
        add_baseline_edge(plan, previous, node, false);
        previous = node;
    }

    let support = plan.nodes[usize::from(previous)].support;
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let trigger_x = (i32::from(support.center_x()) * TILE_SIZE - DOOR_SPAN / 2)
        .clamp(TILE_SIZE, room_width - TILE_SIZE - DOOR_SPAN);
    BoundaryPort {
        node_id: previous,
        door: Door {
            id: "port-ceiling".to_owned(),
            side: BoundarySide::Ceiling,
            trigger_bounds: Rect::new(trigger_x, 0, DOOR_SPAN, CEILING_DOOR_DEPTH),
            arrival: Point::new(
                i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2,
                TILE_SIZE + 2,
            ),
            destination_room: None,
            destination_door: None,
        },
    }
}

fn add_floor_port(
    plan: &mut RoutePlan,
    lower_path: &[u16],
    pickup_node: u16,
    rng: &mut StableRng,
) -> Option<(BoundaryPort, (u16, u16))> {
    let mut candidates = lower_path[1..lower_path.len().saturating_sub(1)].to_vec();
    for index in (1..candidates.len()).rev() {
        let swap_with = usize::from(
            rng.below(
                (index + 1)
                    .try_into()
                    .expect("the short floor-port candidate list fits u16"),
            ),
        );
        candidates.swap(index, swap_with);
    }

    for node in candidates {
        if node == pickup_node
            || !matches!(
                plan.nodes[usize::from(node)].role,
                NodeRole::Landing | NodeRole::Recovery
            )
        {
            continue;
        }
        let support = plan.nodes[usize::from(node)].support;
        // Ceiling connectors span offsets 5..=24 on the shared socket grid;
        // constrain floor shafts to that same inventory so every opening can
        // be paired by the dungeon assembler.
        let right_shaft = (support.end_x + 2 <= ROOM_WIDTH - 3)
            .then_some((support.end_x, support.end_x + 2))
            .filter(|&(start, _)| (5..=24).contains(&start));
        let left_shaft = (support.start_x >= 5)
            .then_some((support.start_x - 2, support.start_x))
            .filter(|&(start, _)| (5..=24).contains(&start));
        let shafts = if rng.coin() {
            [right_shaft, left_shaft]
        } else {
            [left_shaft, right_shaft]
        };
        let Some((shaft_start, shaft_end)) = shafts
            .into_iter()
            .flatten()
            .find(|&(start, end)| floor_shaft_is_clear(plan, node, support.row, start, end))
        else {
            continue;
        };
        let Some(arrival_x) = clear_arrival_x(plan, node) else {
            continue;
        };

        plan.nodes[usize::from(node)].role = NodeRole::Port;
        let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
        let shaft_top = i32::from(support.row) * TILE_SIZE;
        return Some((
            BoundaryPort {
                node_id: node,
                door: Door {
                    id: "port-floor".to_owned(),
                    side: BoundarySide::Floor,
                    trigger_bounds: Rect::new(
                        i32::from(shaft_start) * TILE_SIZE,
                        shaft_top,
                        i32::from(shaft_end - shaft_start) * TILE_SIZE,
                        room_height - shaft_top,
                    ),
                    arrival: Point::new(
                        arrival_x,
                        i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT,
                    ),
                    destination_room: None,
                    destination_door: None,
                },
            },
            (shaft_start, shaft_end),
        ));
    }
    None
}

fn clear_arrival_x(plan: &RoutePlan, anchor: u16) -> Option<i32> {
    let support = plan.nodes[usize::from(anchor)].support;
    let y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    let preferred_x = i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let arrival = Rect::new(x, y, PLAYER_WIDTH, PLAYER_HEIGHT);
            plan.nodes.iter().all(|node| {
                node.id == anchor
                    || !arrival.intersects(Rect::new(
                        i32::from(node.support.start_x) * TILE_SIZE,
                        i32::from(node.support.row) * TILE_SIZE,
                        i32::from(node.support.width()) * TILE_SIZE,
                        TILE_SIZE,
                    ))
            })
        })
        .min_by_key(|&x| x.abs_diff(preferred_x))
}

fn floor_shaft_is_clear(
    plan: &RoutePlan,
    anchor: u16,
    anchor_row: u16,
    shaft_start: u16,
    shaft_end: u16,
) -> bool {
    plan.nodes.iter().all(|node| {
        node.id == anchor
            || node.support.row < anchor_row
            || node.support.end_x <= shaft_start
            || shaft_end <= node.support.start_x
    })
}

fn generated_support(
    center_x: u16,
    row: u16,
    profile: Profile,
    mirrored: bool,
    rng: &mut StableRng,
) -> SupportSpec {
    let width = profile.support_width_min + rng.below(profile.support_width_extra + 1);
    let kind = if rng.below(4) == 0 {
        SupportKind::Solid
    } else {
        SupportKind::OneWay
    };
    mirrored_support(support_around(center_x, row, width, kind), mirrored)
}

fn subdivided_centers(start: u16, end: u16, interior_count: u16, rng: &mut StableRng) -> Vec<u16> {
    let mut centers = vec![start, end];
    while centers.len() < usize::from(interior_count) + 2 {
        let widest = centers
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .max()
            .expect("an arc always has its two endpoints");
        let candidates = centers
            .windows(2)
            .enumerate()
            .filter_map(|(index, pair)| {
                let width = pair[1] - pair[0];
                (width + 1 >= widest && width >= 2).then_some(index)
            })
            .collect::<Vec<_>>();
        let selected = candidates[usize::from(
            rng.below(
                candidates
                    .len()
                    .try_into()
                    .expect("the short candidate list fits u16"),
            ),
        )];
        let left = centers[selected];
        let right = centers[selected + 1];
        let midpoint = left + (right - left) / 2;
        let jitter = match rng.below(3) {
            0 if midpoint > left + 1 => -1_i16,
            2 if midpoint + 1 < right => 1_i16,
            _ => 0_i16,
        };
        let split = u16::try_from(i32::from(midpoint) + i32::from(jitter))
            .expect("subdivision remains in the positive room range")
            .clamp(left + 1, right - 1);
        centers.insert(selected + 1, split);
    }
    centers
}

fn lower_rows(interior_count: u16, rng: &mut StableRng) -> Vec<u16> {
    let mut rows = Vec::with_capacity(usize::from(interior_count) + 2);
    rows.push(ANCHOR_ROW);
    let mut current = ANCHOR_ROW;
    for _ in 0..interior_count {
        let delta = match rng.below(5) {
            0 => -1_i16,
            4 => 1_i16,
            _ => 0_i16,
        };
        current = u16::try_from(i32::from(current) + i32::from(delta))
            .expect("the lower lane remains in the room")
            .clamp(14, 16);
        rows.push(current);
    }
    rows.push(ANCHOR_ROW);
    rows
}

fn upper_rows(interior_count: u16, peak_row: u16, rng: &mut StableRng) -> Vec<u16> {
    let transitions = interior_count + 1;
    let rise = ANCHOR_ROW - peak_row;
    let minimum_half = rise.div_ceil(2);
    let first_peak_index = minimum_half;
    let last_peak_index = transitions - minimum_half;
    let peak_index = rng.between(first_peak_index, last_peak_index);

    let ascent = composed_steps(rise, peak_index, rng);
    let descent = composed_steps(rise, transitions - peak_index, rng);
    let mut rows = Vec::with_capacity(usize::from(interior_count) + 2);
    let mut row = ANCHOR_ROW;
    rows.push(row);
    for step in ascent {
        row -= step;
        rows.push(row);
    }
    for step in descent {
        row += step;
        rows.push(row);
    }
    debug_assert_eq!(rows.len(), usize::from(interior_count) + 2);
    debug_assert_eq!(rows.last(), Some(&ANCHOR_ROW));
    rows
}

fn composed_steps(total: u16, count: u16, rng: &mut StableRng) -> Vec<u16> {
    debug_assert!(total <= count * 2);
    let mut remaining = total;
    let mut result = Vec::with_capacity(usize::from(count));
    for index in 0..count {
        let slots_after = count - index - 1;
        let minimum = remaining.saturating_sub(slots_after * 2);
        let maximum = remaining.min(2);
        let value = rng.between(minimum, maximum);
        result.push(value);
        remaining -= value;
    }
    debug_assert_eq!(remaining, 0);
    result
}

#[allow(clippy::too_many_arguments)]
fn add_arc(
    plan: &mut RoutePlan,
    entry: u16,
    merge: u16,
    centers: &[u16],
    rows: &[u16],
    profile: Profile,
    mirrored: bool,
    critical: bool,
    rng: &mut StableRng,
) -> Vec<u16> {
    debug_assert_eq!(centers.len(), rows.len());
    let mut path = Vec::with_capacity(centers.len());
    path.push(entry);
    for (&center, &row) in centers[1..centers.len() - 1]
        .iter()
        .zip(&rows[1..rows.len() - 1])
    {
        let support = generated_support(center, row, profile, mirrored, rng);
        path.push(add_route_node(plan, NodeRole::Landing, support));
    }
    path.push(merge);
    add_path_edges(plan, &path, critical);
    path
}

fn add_path_edges(plan: &mut RoutePlan, path: &[u16], critical: bool) {
    for pair in path.windows(2) {
        add_baseline_edge(plan, pair[0], pair[1], critical);
    }
}

fn add_baseline_edge(plan: &mut RoutePlan, from: u16, to: u16, critical: bool) {
    let from_support = plan.nodes[usize::from(from)].support;
    let to_support = plan.nodes[usize::from(to)].support;
    // CyclicGraph intentionally establishes a reversible baseline traversal
    // skeleton. Ability-specific embellishments belong to later decorators.
    let verb = edge_verb(from_support, to_support, AbilitySet::NONE);
    add_route_edge(plan, from, to, verb, critical);
}

fn add_detour(
    plan: &mut RoutePlan,
    basis: &[u16],
    profile: Profile,
    depth: u16,
    rng: &mut StableRng,
) -> Option<Vec<u16>> {
    let candidates = (0..basis.len().saturating_sub(2))
        .filter(|&index| {
            let from = plan.nodes[usize::from(basis[index])].support.center_x();
            let to = plan.nodes[usize::from(basis[index + 2])].support.center_x();
            from.abs_diff(to) >= 3
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let start_index = candidates[usize::from(
        rng.below(
            candidates
                .len()
                .try_into()
                .expect("the short detour candidate list fits u16"),
        ),
    )];
    let from = basis[start_index];
    let to = basis[start_index + 2];
    let from_support = plan.nodes[usize::from(from)].support;
    let to_support = plan.nodes[usize::from(to)].support;
    let horizontal_span = from_support.center_x().abs_diff(to_support.center_x());
    let new_count: u16 = if horizontal_span >= 6 && depth > 0 {
        2
    } else {
        1
    };
    let lift = 1 + u16::from(depth > 0);
    let mut detour = vec![from];
    let mut previous_row = from_support.row;
    for index in 1..=new_count {
        let denominator = new_count + 1;
        let from_weight = denominator - index;
        let to_weight = index;
        let center = (u32::from(from_support.center_x()) * u32::from(from_weight)
            + u32::from(to_support.center_x()) * u32::from(to_weight))
            / u32::from(denominator);
        let interpolated_row = (u32::from(from_support.row) * u32::from(from_weight)
            + u32::from(to_support.row) * u32::from(to_weight))
            / u32::from(denominator);
        let desired_row = u16::try_from(interpolated_row)
            .expect("an interpolated row fits u16")
            .saturating_sub(lift)
            .max(2);
        let edges_to_goal = denominator - index;
        let minimum_reaching_goal = to_support.row.saturating_sub(edges_to_goal * 2);
        let maximum_reaching_goal = to_support.row.saturating_add(edges_to_goal * 2);
        let minimum = previous_row.saturating_sub(2).max(minimum_reaching_goal);
        let maximum = previous_row
            .saturating_add(2)
            .min(maximum_reaching_goal)
            .min(FLOOR_ROW - 1);
        let row = desired_row.clamp(minimum, maximum);
        let width = profile.support_width_min + rng.below(profile.support_width_extra.min(1) + 1);
        let kind = if rng.coin() {
            SupportKind::OneWay
        } else {
            SupportKind::Solid
        };
        let support = support_around(
            u16::try_from(center).expect("an interpolated center fits u16"),
            row,
            width,
            kind,
        );
        detour.push(add_route_node(plan, NodeRole::Recovery, support));
        previous_row = row;
    }
    detour.push(to);
    if !matches!(
        plan.nodes[usize::from(from)].role,
        NodeRole::Port | NodeRole::Start | NodeRole::Exit
    ) {
        plan.nodes[usize::from(from)].role = NodeRole::Junction;
    }
    if !matches!(
        plan.nodes[usize::from(to)].role,
        NodeRole::Port | NodeRole::Start | NodeRole::Exit
    ) {
        plan.nodes[usize::from(to)].role = NodeRole::Junction;
    }
    add_path_edges(plan, &detour, false);
    Some(detour)
}

fn select_pickup_node(plan: &RoutePlan) -> u16 {
    plan.nodes
        .iter()
        .filter(|node| {
            !matches!(
                node.role,
                NodeRole::Port | NodeRole::Start | NodeRole::Exit | NodeRole::Junction
            )
        })
        .min_by_key(|node| (node.support.row, node.support.center_x()))
        .map_or(1, |node| node.id)
}

fn mark_recovery_nodes(
    plan: &mut RoutePlan,
    lower_path: &[u16],
    count: u16,
    pickup_node: u16,
) -> Vec<u16> {
    if count == 0 || lower_path.len() <= 2 {
        return Vec::new();
    }
    let eligible = lower_path[1..lower_path.len() - 1]
        .iter()
        .copied()
        .filter(|&node| {
            node != pickup_node && plan.nodes[usize::from(node)].role == NodeRole::Landing
        })
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    for index in 0..count {
        let position = (usize::from(index) + 1) * eligible.len() / (usize::from(count) + 1);
        let node = eligible[position.min(eligible.len() - 1)];
        if !result.contains(&node) {
            plan.nodes[usize::from(node)].role = NodeRole::Recovery;
            result.push(node);
        }
    }
    result
}

fn place_timed_hazards(
    draft: &mut super::common::RoomDraft,
    plan: &RoutePlan,
    lower_path: &[u16],
    requested: u16,
    reserved: &[Rect],
    rng: &mut StableRng,
) {
    let mut candidates = lower_path[1..lower_path.len() - 1].to_vec();
    for index in (1..candidates.len()).rev() {
        let swap_with = usize::from(
            rng.below(
                (index + 1)
                    .try_into()
                    .expect("the short hazard candidate list fits u16"),
            ),
        );
        candidates.swap(index, swap_with);
    }

    let mut placed = 0_u16;
    for node in candidates {
        if placed >= requested
            || matches!(
                plan.nodes[usize::from(node)].role,
                NodeRole::Pickup | NodeRole::Port
            )
        {
            continue;
        }
        let support = plan.nodes[usize::from(node)].support;
        let bounds = Rect::new(
            i32::from(support.center_x()) * TILE_SIZE - 3,
            i32::from(support.row) * TILE_SIZE - 7,
            6,
            7,
        );
        if !reserved.iter().any(|area| area.intersects(bounds))
            && draft.try_timed_hazard(bounds, rng)
        {
            placed += 1;
        }
    }

    // Closely woven platforms can occupy every preferred pedestal. Preserve
    // the requested object complexity with small suspended hazards in empty
    // central cells; validation and the gameplay AI decide whether those
    // candidates are interesting enough to retain.
    let scan_offset = rng.below(12);
    for row_offset in 0_u16..8 {
        for x_offset in 0_u16..12 {
            if placed >= requested {
                return;
            }
            let x = 10 + (x_offset + scan_offset) % 12;
            let row = 3 + row_offset;
            let bounds = Rect::new(
                i32::from(x) * TILE_SIZE + 2,
                i32::from(row) * TILE_SIZE + 2,
                6,
                6,
            );
            if !reserved.iter().any(|area| area.intersects(bounds))
                && draft.try_timed_hazard(bounds, rng)
            {
                placed += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::ROOM_HEIGHT;
    use crate::experimental::RouteVerb;
    use downwards_core::Tile;

    #[test]
    fn generated_graphs_are_anchored_cyclic_and_baseline_conservative() {
        for intent in ChallengeIntent::ALL {
            for seed in 0..128 {
                let parts = generate(seed, AbilitySet::ALL, intent).expect("generation succeeds");
                let plan = &parts.route_plan;
                let summary = plan.summary();
                assert!(summary.cycle_rank >= 1, "{intent:?} seed {seed}");
                assert!(summary.branch_nodes >= 2, "{intent:?} seed {seed}");
                assert_eq!(
                    plan.nodes
                        .iter()
                        .filter(|node| node.role == NodeRole::Port)
                        .count(),
                    parts.boundary_ports.len()
                );
                assert!((2..=4).contains(&parts.boundary_ports.len()));
                assert!(plan.edges.iter().all(|edge| !matches!(
                    edge.verb,
                    RouteVerb::Drop
                        | RouteVerb::WallClimb
                        | RouteVerb::DashAcross
                        | RouteVerb::DashUp
                )));
                for edge in &plan.edges {
                    let from = plan.nodes[usize::from(edge.from)].support;
                    let to = plan.nodes[usize::from(edge.to)].support;
                    let gap = to
                        .start_x
                        .saturating_sub(from.end_x)
                        .max(from.start_x.saturating_sub(to.end_x));
                    assert!(
                        from.row.abs_diff(to.row) <= 2 && gap <= 4,
                        "{intent:?} seed {seed}: {edge:?}, {from:?} -> {to:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn ports_are_unique_connected_boundary_anchors_with_carved_apertures() {
        for intent in ChallengeIntent::ALL {
            for seed in 0..128 {
                let parts = generate(seed, AbilitySet::ALL, intent).expect("generation succeeds");
                let mut sides = HashSet::new();
                let mut ids = HashSet::new();
                for port in &parts.boundary_ports {
                    assert!(sides.insert(port.door.side), "{intent:?} seed {seed}");
                    assert!(ids.insert(port.door.id.as_str()), "{intent:?} seed {seed}");
                    assert_eq!(
                        parts.route_plan.nodes[usize::from(port.node_id)].role,
                        NodeRole::Port
                    );
                    assert!(node_reaches_entire_plan(&parts.route_plan, port.node_id));
                    assert!(boundary_aperture_is_carved(&parts.draft, &port.door));
                }
                assert!(sides.contains(&BoundarySide::Left));
                assert!(sides.contains(&BoundarySide::Right));
            }
        }
    }

    #[test]
    fn seed_space_varies_graph_geometry_and_materials() {
        let mut all_port_counts = HashSet::new();
        for intent in ChallengeIntent::ALL {
            let mut signatures = HashSet::new();
            let mut node_counts = HashSet::new();
            let mut edge_counts = HashSet::new();
            let mut port_counts = HashSet::new();
            let mut saw_solid = false;
            let mut saw_one_way = false;
            for seed in 0..128 {
                let parts = generate(seed, AbilitySet::NONE, intent).expect("generation succeeds");
                let summary = parts.route_plan.summary();
                signatures.insert(summary.signature);
                node_counts.insert(summary.node_count);
                edge_counts.insert(summary.edge_count);
                port_counts.insert(summary.port_count);
                all_port_counts.insert(summary.port_count);
                for node in parts.route_plan.nodes {
                    saw_solid |= node.support.kind == SupportKind::Solid;
                    saw_one_way |= node.support.kind == SupportKind::OneWay;
                }
            }
            assert!(signatures.len() >= 120, "{intent:?}: {signatures:?}");
            assert!(node_counts.len() >= 3, "{intent:?}: {node_counts:?}");
            assert!(edge_counts.len() >= 3, "{intent:?}: {edge_counts:?}");
            assert!(!port_counts.is_empty());
            assert!(saw_solid && saw_one_way);
        }
        assert_eq!(all_port_counts, HashSet::from([2, 3, 4]));
    }

    #[test]
    fn intent_increases_typical_structural_and_hazard_complexity() {
        let aggregate = |intent| {
            (0..128)
                .map(|seed| {
                    let parts =
                        generate(seed, AbilitySet::NONE, intent).expect("generation succeeds");
                    let summary = parts.route_plan.summary();
                    let stats = parts.draft.stats(summary.node_count);
                    (
                        u64::from(summary.node_count),
                        u64::from(summary.cycle_rank),
                        u64::from(stats.timed_hazards),
                    )
                })
                .fold((0, 0, 0), |left, right| {
                    (left.0 + right.0, left.1 + right.1, left.2 + right.2)
                })
        };
        let gentle = aggregate(ChallengeIntent::Gentle);
        let standard = aggregate(ChallengeIntent::Standard);
        let technical = aggregate(ChallengeIntent::Technical);
        assert!(gentle.0 < standard.0 && standard.0 < technical.0);
        assert!(gentle.1 < standard.1 && standard.1 < technical.1);
        assert!(gentle.2 < standard.2 && standard.2 < technical.2);
    }

    #[test]
    fn draft_contains_hazard_floor_and_both_platform_materials_across_a_batch() {
        let mut saw_hazard = false;
        let mut saw_one_way = false;
        let mut saw_solid = false;
        for seed in 0..64 {
            let parts = generate(seed, AbilitySet::NONE, ChallengeIntent::Standard)
                .expect("generation succeeds");
            for row in 0..ROOM_HEIGHT {
                for x in 0..ROOM_WIDTH {
                    match parts.draft.tile(x, row) {
                        Tile::Hazard => saw_hazard = true,
                        Tile::OneWay => saw_one_way = true,
                        Tile::Solid => saw_solid = true,
                        Tile::Empty => {}
                    }
                }
            }
        }
        assert!(saw_hazard);
        assert!(saw_one_way);
        assert!(saw_solid);
    }

    #[test]
    fn broad_seed_batch_finishes_core_room_validation() {
        for intent in ChallengeIntent::ALL {
            for seed in 0..512 {
                let parts = generate(seed, AbilitySet::ALL, intent).expect("generation succeeds");
                let doors = parts
                    .boundary_ports
                    .iter()
                    .map(|port| port.door.clone())
                    .collect();
                let room = parts
                    .draft
                    .finish_without_exits(
                        format!("cyclic-test-{intent:?}-{seed}"),
                        "Cyclic test".to_owned(),
                        parts.spawn,
                    )
                    .expect("candidate satisfies core room invariants")
                    .with_doors(doors)
                    .expect("candidate doors satisfy core invariants");
                assert!(room.exits().is_empty());
                assert_eq!(room.doors().len(), parts.boundary_ports.len());
            }
        }
    }

    fn node_reaches_entire_plan(plan: &RoutePlan, start: u16) -> bool {
        let mut seen = vec![false; plan.nodes.len()];
        seen[usize::from(start)] = true;
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for edge in &plan.edges {
                let adjacent = if edge.from == node {
                    Some(edge.to)
                } else if edge.to == node {
                    Some(edge.from)
                } else {
                    None
                };
                if let Some(adjacent) = adjacent
                    && !seen[usize::from(adjacent)]
                {
                    seen[usize::from(adjacent)] = true;
                    stack.push(adjacent);
                }
            }
        }
        seen.into_iter().all(|value| value)
    }

    fn boundary_aperture_is_carved(draft: &super::super::common::RoomDraft, door: &Door) -> bool {
        (0..ROOM_HEIGHT).all(|row| {
            (0..ROOM_WIDTH).all(|x| {
                let on_boundary =
                    x == 0 || x == ROOM_WIDTH - 1 || row == 0 || row == ROOM_HEIGHT - 1;
                let tile_bounds = Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
                !on_boundary
                    || !tile_bounds.intersects(door.trigger_bounds)
                    || draft.tile(x, row) == Tile::Empty
            })
        })
    }
}
