use downwards_core::{
    AbilitySet, BoundarySide, Door, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect, Tile,
};

use super::{
    BoundaryPort, CandidateParts, ChallengeIntent, ExperimentalGenerationError, NodeRole,
    RoutePlan, RouteVerb, SupportKind, SupportSpec,
};
use crate::{ROOM_HEIGHT, ROOM_WIDTH, TILE_SIZE};

use super::common::{
    FLOOR_ROW, RoomDraft, StableRng, add_route_edge, add_route_node, edge_verb, mirrored_spawn,
    mirrored_support,
};

const PORT_RNG_STREAM: u64 = 0x504f_5254_5259_5448;
const MECHANIC_RNG_STREAM: u64 = 0x4741_5445_535f_5257;
const SIDE_DOOR_DEPTH: i32 = 12;
const CEILING_DOOR_DEPTH: i32 = 12;
const DOOR_SPAN: i32 = 20;

/// One local wall-jump transfer composed in front of the otherwise free-form
/// weave. The boundary port enters beneath a pair of interior shaft walls.
/// Reaching the lip therefore asks for one or more wall jumps, while returning
/// to the port is a simple reversible drop through the lower aperture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WallTransfer {
    inner_wall_x: u16,
    top_row: u16,
    lip_width: u16,
}

impl WallTransfer {
    const OUTER_WALL_X: u16 = 1;

    fn sample(intent: ChallengeIntent, rng: &mut StableRng) -> Self {
        let top_row = match intent {
            ChallengeIntent::Gentle => 13,
            ChallengeIntent::Standard => rng.between(12, 13),
            ChallengeIntent::Technical => rng.between(12, 13),
        };
        Self {
            inner_wall_x: rng.between(6, 8),
            top_row,
            lip_width: rng.between(4, 6),
        }
    }

    const fn start_support(self) -> SupportSpec {
        SupportSpec {
            start_x: Self::OUTER_WALL_X + 1,
            end_x: self.inner_wall_x,
            row: FLOOR_ROW,
            kind: SupportKind::Solid,
        }
    }

    const fn launch_support(self) -> SupportSpec {
        SupportSpec {
            start_x: self.inner_wall_x,
            end_x: self.inner_wall_x + self.lip_width,
            row: self.top_row,
            kind: SupportKind::Solid,
        }
    }

    fn add_geometry(self, draft: &mut RoomDraft, mirrored: bool) {
        let outer_wall_x = if mirrored {
            ROOM_WIDTH - 1 - Self::OUTER_WALL_X
        } else {
            Self::OUTER_WALL_X
        };
        let inner_wall_x = if mirrored {
            ROOM_WIDTH - 1 - self.inner_wall_x
        } else {
            self.inner_wall_x
        };
        // The outer wall stops above the lateral door aperture. Players enter
        // beneath it, then bounce between two interior walls without touching
        // the source-door trigger again on every low wall jump.
        draft.solid_column(outer_wall_x, self.top_row, FLOOR_ROW - 2);
        draft.solid_column(inner_wall_x, self.top_row, FLOOR_ROW);
    }

    fn reserved_shaft(self, mirrored: bool) -> Rect {
        let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
        let width = i32::from(self.inner_wall_x - Self::OUTER_WALL_X + 1) * TILE_SIZE;
        let x = if mirrored {
            room_width - i32::from(self.inner_wall_x + 1) * TILE_SIZE
        } else {
            i32::from(Self::OUTER_WALL_X) * TILE_SIZE
        };
        Rect::new(
            x,
            i32::from(self.top_row) * TILE_SIZE,
            width,
            i32::from(FLOOR_ROW - self.top_row) * TILE_SIZE,
        )
    }
}

pub(super) fn generate(
    seed: u64,
    abilities: AbilitySet,
    intent: ChallengeIntent,
) -> Result<CandidateParts, ExperimentalGenerationError> {
    let mut rhythm_rng = StableRng::new(seed, 0x0052_5954_484d);
    let mut geometry_rng = StableRng::new(seed, 0x4745_4f4d_4554_5259);
    let mut decoration_rng = StableRng::new(seed, 0x4445_434f_5241_5445);
    let mut port_rng = StableRng::new(seed, PORT_RNG_STREAM);
    let mut mechanic_rng = StableRng::new(seed, MECHANIC_RNG_STREAM);
    let mirrored = rhythm_rng.coin();
    let (minimum_beats, extra_beats, lane_separation) = match intent {
        ChallengeIntent::Gentle => (3, 1, 2),
        ChallengeIntent::Standard => (4, 2, 3),
        ChallengeIntent::Technical => (5, 3, 4),
    };
    let beat_counts = [
        minimum_beats + rhythm_rng.below(extra_beats + 1),
        minimum_beats + rhythm_rng.below(extra_beats + 1),
    ];

    // A dash loadout receives a shared upward transfer immediately before the
    // goal. It is a local movement affordance rather than a whole-room shape;
    // all earlier rhythm and embedding decisions remain free to vary.
    let required_wall = abilities.wall_jump;
    let required_dash = abilities.dash;
    let sampled_wall_transfer =
        required_wall.then(|| WallTransfer::sample(intent, &mut mechanic_rng));
    let goal_row = if required_dash {
        match (required_wall, intent) {
            // With both gates present, keep the wall lip below the dash launch
            // band. The route then serializes the two transfers instead of
            // letting the wall module accidentally emerge above the dash goal.
            (true, ChallengeIntent::Gentle) => geometry_rng.between(6, 7),
            (true, ChallengeIntent::Standard) => geometry_rng.between(5, 6),
            (true, ChallengeIntent::Technical) => geometry_rng.between(4, 5),
            (false, ChallengeIntent::Gentle) => geometry_rng.between(8, 9),
            (false, ChallengeIntent::Standard) => geometry_rng.between(6, 8),
            (false, ChallengeIntent::Technical) => geometry_rng.between(4, 7),
        }
    } else {
        match (required_wall, intent) {
            // Leave readable headroom above the wall lip. Packing the gentle
            // weave into rows 13/14 creates sub-player-height shelves when its
            // two lanes cross; a small follow-on rise is both clearer and
            // more reliable while the shorter rhythm remains gentle.
            (true, ChallengeIntent::Gentle) => geometry_rng.between(11, 12),
            (_, ChallengeIntent::Gentle) => geometry_rng.between(13, 14),
            (_, ChallengeIntent::Standard) => geometry_rng.between(9, 12),
            (_, ChallengeIntent::Technical) => geometry_rng.between(5, 9),
        }
    };
    let pre_goal_row = if required_dash {
        // Five rows is deliberately beyond the conservative baseline jump
        // envelope. This makes the local transfer a dash gate, rather than a
        // route label applied to geometry a sufficiently precise jump can beat.
        goal_row + 5
    } else {
        goal_row
    };
    let wall_transfer = sampled_wall_transfer.map(|mut transfer| {
        if required_dash {
            transfer.top_row = transfer.top_row.max(pre_goal_row);
        }
        transfer
    });

    let start_support = wall_transfer.map_or(
        SupportSpec {
            start_x: 1,
            end_x: 5,
            row: FLOOR_ROW,
            kind: SupportKind::Solid,
        },
        WallTransfer::start_support,
    );
    let launch_support = wall_transfer.map_or(
        SupportSpec {
            start_x: 3,
            end_x: 7,
            row: 15,
            kind: SupportKind::OneWay,
        },
        WallTransfer::launch_support,
    );
    let goal_support = SupportSpec {
        start_x: 27 - geometry_rng.below(2),
        end_x: 31,
        row: goal_row,
        kind: if required_dash {
            // A dash-up may approach through the underside. Keeping this lip
            // directional makes both mirrored embeddings reliable without
            // prescribing the rest of the room's material palette.
            SupportKind::OneWay
        } else if geometry_rng.coin() {
            SupportKind::Solid
        } else {
            SupportKind::OneWay
        },
    };

    let mut paths = Vec::with_capacity(2);
    for (lane, &beat_count) in beat_counts.iter().enumerate() {
        paths.push(build_rhythm_path(
            lane,
            beat_count,
            launch_support,
            pre_goal_row,
            lane_separation,
            required_dash.then_some(pre_goal_row),
            intent,
            &mut rhythm_rng,
            &mut geometry_rng,
        ));
    }

    let pickup_path = usize::from(rhythm_rng.coin());
    let pickup_index = pickup_node_index(&paths[pickup_path]);

    let mut draft = RoomDraft::new();
    let mut plan = RoutePlan::default();
    let start = mirrored_support(start_support, mirrored);
    let launch = mirrored_support(launch_support, mirrored);
    let goal = mirrored_support(goal_support, mirrored);
    let start_id = add_route_node(&mut plan, NodeRole::Port, start);
    let launch_id = add_route_node(&mut plan, NodeRole::Junction, launch);
    let goal_id = add_route_node(&mut plan, NodeRole::Port, goal);
    add_route_edge(
        &mut plan,
        start_id,
        launch_id,
        if required_wall {
            RouteVerb::WallClimb
        } else {
            edge_verb(start, launch, AbilitySet::NONE)
        },
        true,
    );
    draft.platform(launch);
    draft.platform(goal);
    if let Some(transfer) = wall_transfer {
        transfer.add_geometry(&mut draft, mirrored);
    }
    if required_wall && !required_dash && intent != ChallengeIntent::Gentle {
        // A low one-way recovery shelf makes the reverse traversal robust:
        // misses while descending the weave land outside the shaft, from
        // where the lip is a baseline-safe jump and the port is a drop. It
        // cannot bypass the wall gate in the upward direction because the
        // solid inner wall still separates the boundary arrival from it.
        let recovery = mirrored_support(
            SupportSpec {
                start_x: launch_support.start_x,
                end_x: ROOM_WIDTH - 1,
                row: 14,
                kind: SupportKind::OneWay,
            },
            mirrored,
        );
        draft.platform(recovery);
        let recovery_id = add_route_node(&mut plan, NodeRole::Recovery, recovery);
        add_route_edge(&mut plan, launch_id, recovery_id, RouteVerb::Jump, false);
    }

    let mut pickup_support = None;
    for (lane, path) in paths.into_iter().enumerate() {
        let path_len = path.len();
        let mut previous_id = launch_id;
        let mut previous_support = launch;
        for (index, support) in path.into_iter().enumerate() {
            let support = mirrored_support(support, mirrored);
            draft.platform(support);
            let is_pickup = lane == pickup_path && index == pickup_index;
            let role = if is_pickup {
                pickup_support = Some(support);
                NodeRole::Pickup
            } else if index == 0 || index + 1 == path_len {
                NodeRole::Junction
            } else {
                NodeRole::Landing
            };
            let node_id = add_route_node(&mut plan, role, support);
            add_route_edge(
                &mut plan,
                previous_id,
                node_id,
                edge_verb(previous_support, support, abilities),
                lane == 0,
            );
            previous_id = node_id;
            previous_support = support;
        }
        add_route_edge(
            &mut plan,
            previous_id,
            goal_id,
            if required_dash {
                RouteVerb::DashUp
            } else {
                edge_verb(previous_support, goal, abilities)
            },
            lane == 0,
        );
    }

    let pickup_support = pickup_support.unwrap_or(goal);
    draft.pickup_above("optional-cache", pickup_support);

    let (west_node, west_support, east_node, east_support) = if mirrored {
        (goal_id, goal, start_id, start)
    } else {
        (start_id, start, goal_id, goal)
    };
    let mut boundary_ports = vec![
        wall_port(west_node, west_support, BoundarySide::Left),
        wall_port(east_node, east_support, BoundarySide::Right),
    ];
    // The wall-only experiment isolates its mechanic on the two lateral
    // sockets. The combined kit retains the full 2--4-port topology, so its
    // route matrix exercises the serialized wall and dash gates as well as
    // ceiling/floor branches.
    let wants_ceiling = !required_wall || required_dash;
    let wants_ceiling = wants_ceiling
        && match intent {
            ChallengeIntent::Gentle => port_rng.below(3) == 0,
            ChallengeIntent::Standard => true,
            ChallengeIntent::Technical => true,
        };
    if wants_ceiling && let Some(port) = add_ceiling_port(&mut plan, &mut draft, &mut port_rng) {
        boundary_ports.push(port);
    }
    let wants_floor = !required_wall || required_dash;
    let wants_floor = wants_floor
        && match intent {
            ChallengeIntent::Gentle => port_rng.below(4) == 0,
            ChallengeIntent::Standard => port_rng.coin(),
            ChallengeIntent::Technical => true,
        };
    let floor_safe_range = wants_floor
        .then(|| add_floor_port(&mut plan, wall_transfer, mirrored, &mut port_rng))
        .flatten()
        .map(|(port, safe_range)| {
            boundary_ports.push(port);
            safe_range
        });

    // Falling off either woven route is consequential, but the starting floor
    // remains a broad readable launch area. A small safe goal-side floor strip
    // provides recovery only when the goal itself is low.
    let start_safe_range = if required_wall {
        // The interior outer wall sits one tile nearer the boundary than the
        // conceptual standing support. That tile is the return corridor to
        // the lateral door and must never be converted into floor spikes.
        if start.start_x < ROOM_WIDTH / 2 {
            (start.start_x - 1, start.end_x)
        } else {
            (start.start_x, start.end_x + 1)
        }
    } else {
        (start.start_x, start.end_x)
    };
    let mut safe_ranges = vec![start_safe_range];
    if goal.row >= 13 {
        safe_ranges.push((goal.start_x.saturating_sub(1), (goal.end_x + 1).min(31)));
    }
    safe_ranges.extend(floor_safe_range);
    if intent == ChallengeIntent::Gentle {
        let offset = decoration_rng.below(2);
        for start in [7 + offset, 15 - offset, 23 + offset] {
            hazard_run_except(&mut draft, start, start + 2, &safe_ranges);
        }
    } else {
        draft.hazard_floor_except(&safe_ranges);
    }
    add_visual_rhythm_accents(
        &mut draft,
        intent,
        mirrored,
        &boundary_ports,
        &mut decoration_rng,
    );
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
    if let Some(transfer) = wall_transfer {
        reserved_arrivals.push(transfer.reserved_shaft(mirrored));
    }
    for port in &boundary_ports {
        if port.door.side == BoundarySide::Floor {
            let support = plan.nodes[usize::from(port.node_id)].support;
            let corridor_y = i32::from(support.row) * TILE_SIZE;
            reserved_arrivals.push(Rect::new(
                port.door.trigger_bounds.x,
                corridor_y,
                port.door.trigger_bounds.width,
                i32::from(ROOM_HEIGHT) * TILE_SIZE - corridor_y,
            ));
        }
    }
    place_timed_hazard(&mut draft, &reserved_arrivals, &mut decoration_rng);
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    Ok(CandidateParts {
        draft,
        spawn: mirrored_spawn(2, mirrored),
        boundary_ports,
        route_plan: plan,
    })
}

fn wall_port(node_id: u16, support: SupportSpec, side: BoundarySide) -> BoundaryPort {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    // All strategies share a tile-aligned aperture grid so catalogue rooms
    // can be joined across strategy boundaries.
    let trigger_y =
        (i32::from(support.row) * TILE_SIZE - DOOR_SPAN).clamp(0, room_height - DOOR_SPAN);
    let (id, trigger_x, arrival_x) = match side {
        BoundarySide::Left => ("port-west", 0, i32::from(support.start_x) * TILE_SIZE + 2),
        BoundarySide::Right => (
            "port-east",
            room_width - SIDE_DOOR_DEPTH,
            (i32::from(support.end_x) * TILE_SIZE - 2 - PLAYER_WIDTH)
                .min(room_width - TILE_SIZE - 2 - PLAYER_WIDTH),
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

fn add_ceiling_port(
    plan: &mut RoutePlan,
    draft: &mut RoomDraft,
    rng: &mut StableRng,
) -> Option<BoundaryPort> {
    let minimum_row = plan
        .nodes
        .iter()
        .filter(|node| node.role != NodeRole::Port && node.support.row > 4)
        .map(|node| node.support.row)
        .min()?;
    let apexes = plan
        .nodes
        .iter()
        .filter(|node| node.role != NodeRole::Port && node.support.row == minimum_row)
        .map(|node| node.id)
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
        draft.platform(support);
        let role = if row == 4 {
            NodeRole::Port
        } else {
            NodeRole::Landing
        };
        let node = add_route_node(plan, role, support);
        let previous_support = plan.nodes[usize::from(previous)].support;
        add_route_edge(
            plan,
            previous,
            node,
            edge_verb(previous_support, support, AbilitySet::NONE),
            false,
        );
        previous = node;
    }

    let support = plan.nodes[usize::from(previous)].support;
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let trigger_x = (i32::from(support.center_x()) * TILE_SIZE - DOOR_SPAN / 2)
        .clamp(TILE_SIZE, room_width - TILE_SIZE - DOOR_SPAN);
    Some(BoundaryPort {
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

fn add_floor_port(
    plan: &mut RoutePlan,
    wall_transfer: Option<WallTransfer>,
    mirrored: bool,
    rng: &mut StableRng,
) -> Option<(BoundaryPort, (u16, u16))> {
    let blocked_column = wall_transfer.map(|transfer| {
        (
            if mirrored {
                ROOM_WIDTH - 1 - transfer.inner_wall_x
            } else {
                transfer.inner_wall_x
            },
            transfer.top_row,
        )
    });
    let mut candidates = plan
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.role,
                NodeRole::Landing | NodeRole::Junction | NodeRole::Recovery
            ) && blocked_column.is_none_or(|(x, start_row)| {
                node.support.row < start_row || x < node.support.start_x || x >= node.support.end_x
            })
        })
        .map(|node| node.id)
        .collect::<Vec<_>>();
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
    candidates.sort_by_key(|&node| u16::MAX - plan.nodes[usize::from(node)].support.row);

    for node in candidates {
        let support = plan.nodes[usize::from(node)].support;
        // Share the ceiling connector's 5..=24 aperture inventory so a floor
        // opening never becomes an unpairable dungeon socket.
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
        let Some(arrival_x) = clear_arrival_x(plan, node, blocked_column) else {
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

fn clear_arrival_x(
    plan: &RoutePlan,
    anchor: u16,
    blocked_column: Option<(u16, u16)>,
) -> Option<i32> {
    let support = plan.nodes[usize::from(anchor)].support;
    let y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let minimum_x = i32::from(support.start_x) * TILE_SIZE;
    let maximum_x = i32::from(support.end_x) * TILE_SIZE - PLAYER_WIDTH;
    let preferred_x = i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2;
    (minimum_x..=maximum_x)
        .filter(|&x| {
            let arrival = Rect::new(x, y, PLAYER_WIDTH, PLAYER_HEIGHT);
            let avoids_route_supports = plan.nodes.iter().all(|node| {
                node.id == anchor
                    || !arrival.intersects(Rect::new(
                        i32::from(node.support.start_x) * TILE_SIZE,
                        i32::from(node.support.row) * TILE_SIZE,
                        i32::from(node.support.width()) * TILE_SIZE,
                        TILE_SIZE,
                    ))
            });
            let avoids_wall = blocked_column.is_none_or(|(column, start_row)| {
                !arrival.intersects(Rect::new(
                    i32::from(column) * TILE_SIZE,
                    i32::from(start_row) * TILE_SIZE,
                    TILE_SIZE,
                    i32::from(FLOOR_ROW - start_row) * TILE_SIZE,
                ))
            });
            avoids_route_supports && avoids_wall
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

fn hazard_run_except(draft: &mut RoomDraft, start: u16, end: u16, safe_ranges: &[(u16, u16)]) {
    let mut unsafe_start = None;
    for x in start..end {
        let safe = safe_ranges
            .iter()
            .any(|&(safe_start, safe_end)| (safe_start..safe_end).contains(&x));
        match (unsafe_start, safe) {
            (None, false) => unsafe_start = Some(x),
            (Some(run_start), true) => {
                draft.hazard_run(run_start, x);
                unsafe_start = None;
            }
            _ => {}
        }
    }
    if let Some(run_start) = unsafe_start {
        draft.hazard_run(run_start, end);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_rhythm_path(
    lane: usize,
    beat_count: u16,
    initial_support: SupportSpec,
    final_row: u16,
    separation: u16,
    minimum_row: Option<u16>,
    intent: ChallengeIntent,
    rhythm_rng: &mut StableRng,
    geometry_rng: &mut StableRng,
) -> Vec<SupportSpec> {
    let mut supports = Vec::with_capacity(usize::from(beat_count));
    let mut previous_row = initial_support.row;
    let mut previous_end = initial_support.end_x;
    for beat in 0..beat_count {
        let remaining = beat_count - beat - 1;
        let progress_numerator = u32::from(beat + 1);
        let progress_denominator = u32::from(beat_count + 1);
        let base_row = i32::from(initial_support.row)
            + ((i32::from(final_row) - i32::from(initial_support.row))
                * i32::try_from(progress_numerator).unwrap())
                / i32::try_from(progress_denominator).unwrap();
        let triangular = progress_numerator.min(progress_denominator - progress_numerator);
        let lane_offset = i32::from(separation) * i32::try_from(triangular).unwrap()
            / i32::try_from((progress_denominator / 2).max(1)).unwrap();
        let target_row = if lane == 0 {
            base_row + lane_offset
        } else {
            base_row - lane_offset
        };
        let row = choose_bridged_row(
            previous_row,
            final_row,
            remaining,
            target_row,
            minimum_row,
            rhythm_rng,
        );

        let nominal_center = 4 + ((23 * (beat + 1)) / (beat_count + 1));
        let jitter = i32::from(geometry_rng.below(3)) - 1;
        let center = (i32::from(nominal_center) + jitter).clamp(6, 26) as u16;
        let (minimum_width, maximum_width) = match intent {
            ChallengeIntent::Gentle => (5, 7),
            ChallengeIntent::Standard => (4, 6),
            ChallengeIntent::Technical => (3, 5),
        };
        let width = geometry_rng.between(minimum_width, maximum_width);
        let mut start = center.saturating_sub(width / 2).clamp(2, 28);
        let mut end = (start + width).min(30);
        start = end.saturating_sub(width);
        // The wall-jump module owns the space between the boundary and its
        // launch lip. Keeping the independent weave on the far side preserves
        // a clear shaft instead of accidentally inserting baseline footholds.
        if start < initial_support.start_x {
            start = initial_support.start_x;
            end = (start + width).min(30);
        }

        // Keep the intended route inside the conservative 30px support-gap
        // contract. The rhythm may still vary widths, overlap, and cadence.
        if start > previous_end + 3 {
            start = previous_end + 3;
            end = (start + width).min(30);
        }
        // The last beat feeds a goal whose left edge is tile 26 or 27. Keep
        // at least the same three-tile conservative gap contract at the
        // rejoin instead of only applying it while growing left-to-right.
        if remaining == 0 && end < 24 {
            start += 24 - end;
            end = 24;
        }
        // Woven lanes frequently cross in screen space. Directional one-way
        // supports preserve both routes instead of turning the other lane into
        // an accidental ceiling. Solid structures remain available to local
        // mechanic affordances and decoration passes.
        let kind = SupportKind::OneWay;
        supports.push(SupportSpec {
            start_x: start,
            end_x: end,
            row,
            kind,
        });
        previous_row = row;
        previous_end = end;
    }
    supports
}

fn choose_bridged_row(
    previous: u16,
    final_row: u16,
    remaining_steps: u16,
    target_row: i32,
    minimum_row: Option<u16>,
    rng: &mut StableRng,
) -> u16 {
    let mut candidates = (5_u16..=15)
        .filter(|&row| minimum_row.is_none_or(|minimum| row >= minimum))
        .filter(|&row| previous.abs_diff(row) <= 2)
        .filter(|&row| row.abs_diff(final_row) <= remaining_steps * 2)
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|&row| {
        (
            (i32::from(row) - target_row).unsigned_abs(),
            row.abs_diff(final_row),
            row,
        )
    });
    let choice_count = candidates.len().min(3) as u16;
    candidates[usize::from(rng.below(choice_count))]
}

fn pickup_node_index(path: &[SupportSpec]) -> usize {
    path.iter()
        .enumerate()
        .min_by_key(|(index, support)| (support.row, usize::MAX - *index))
        .map_or(0, |(index, _)| index)
}

fn add_visual_rhythm_accents(
    draft: &mut RoomDraft,
    intent: ChallengeIntent,
    mirrored: bool,
    boundary_ports: &[BoundaryPort],
    rng: &mut StableRng,
) {
    let count = match intent {
        ChallengeIntent::Gentle => 1,
        ChallengeIntent::Standard => 2,
        ChallengeIntent::Technical => 3,
    };
    for _ in 0..count {
        let x = rng.between(3, 28);
        let height = rng.between(1, 3);
        let from_left = if mirrored { ROOM_WIDTH - 1 - x } else { x };
        let column_bounds = Rect::new(
            i32::from(from_left) * TILE_SIZE,
            0,
            TILE_SIZE,
            i32::from(height + 1) * TILE_SIZE,
        );
        if boundary_ports.iter().any(|port| {
            port.door.side == BoundarySide::Ceiling
                && column_bounds.intersects(Rect::new(
                    port.door.trigger_bounds.x,
                    0,
                    port.door.trigger_bounds.width,
                    column_bounds.height,
                ))
        }) {
            continue;
        }
        // Short hanging teeth alter silhouettes and jump timing without
        // reaching down into the conservative route band.
        for row in 1..=height {
            if draft.is_empty(from_left, row) {
                draft.solid_column(from_left, row, row + 1);
            }
        }
    }
}

fn place_timed_hazard(draft: &mut RoomDraft, reserved: &[Rect], rng: &mut StableRng) {
    // Search deterministic empty pockets rather than tying the object to a
    // fixed whole-room location. The object is cosmetic/challenge content;
    // reachability certification remains authoritative.
    for _ in 0..64 {
        let width = rng.between(4, 8);
        let height = rng.between(14, 34);
        let x = i32::from(rng.between(2, 29)) * TILE_SIZE + 1;
        let y = i32::from(rng.between(3, 10)) * TILE_SIZE + 1;
        let bounds = Rect::new(
            x,
            y,
            i32::from(width),
            i32::from(height).min(i32::from(ROOM_HEIGHT) * TILE_SIZE - y - 1),
        );
        if !reserved.iter().any(|area| area.intersects(bounds))
            && draft.try_timed_hazard(bounds, rng)
        {
            return;
        }
    }

    // Find a one-cell fallback pocket without changing route geometry.
    for row in 2..8 {
        for x in 2..ROOM_WIDTH - 2 {
            if draft.tile(x, row) == Tile::Empty {
                let bounds = Rect::new(
                    i32::from(x) * TILE_SIZE + 2,
                    i32::from(row) * TILE_SIZE + 1,
                    5,
                    8,
                );
                if !reserved.iter().any(|area| area.intersects(bounds))
                    && draft.try_timed_hazard(bounds, rng)
                {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn rhythm_weave_constructs_broadly_and_retains_a_cycle() {
        for seed in 0..256 {
            for intent in ChallengeIntent::ALL {
                let candidate = generate(seed, AbilitySet::ALL, intent).unwrap();
                let summary = candidate.route_plan.summary();
                assert!(summary.node_count >= 8);
                assert!(summary.cycle_rank >= 1);
                assert!(summary.dash_edges >= 2);
                assert_eq!(summary.wall_edges, 1);
                assert_eq!(summary.port_count, candidate.boundary_ports.len() as u16);
            }
        }
    }

    #[test]
    fn ports_are_unique_connected_boundary_anchors_with_carved_apertures() {
        for abilities in [
            AbilitySet::NONE,
            AbilitySet::new(true, false),
            AbilitySet::new(false, true),
            AbilitySet::ALL,
        ] {
            for intent in ChallengeIntent::ALL {
                for seed in 0..128 {
                    let parts = generate(seed, abilities, intent).expect("generation succeeds");
                    assert!((2..=4).contains(&parts.boundary_ports.len()));
                    let mut sides = HashSet::new();
                    let mut ids = HashSet::new();
                    for port in &parts.boundary_ports {
                        assert!(
                            sides.insert(port.door.side),
                            "{abilities:?} {intent:?} {seed}"
                        );
                        assert!(ids.insert(port.door.id.as_str()));
                        assert_eq!(
                            parts.route_plan.nodes[usize::from(port.node_id)].role,
                            NodeRole::Port
                        );
                        assert!(node_reaches_entire_plan(&parts.route_plan, port.node_id));
                        assert!(boundary_aperture_is_carved(&parts.draft, &port.door));
                    }
                    assert!(sides.contains(&BoundarySide::Left));
                    assert!(sides.contains(&BoundarySide::Right));

                    for edge in &parts.route_plan.edges {
                        let from = parts.route_plan.nodes[usize::from(edge.from)].support;
                        let to = parts.route_plan.nodes[usize::from(edge.to)].support;
                        let gap = to
                            .start_x
                            .saturating_sub(from.end_x)
                            .max(from.start_x.saturating_sub(to.end_x));
                        assert!(gap <= 3, "{abilities:?} {intent:?} {seed}: {edge:?}");
                        match edge.verb {
                            RouteVerb::DashUp => {
                                assert!(abilities.dash);
                                assert_eq!(from.row.saturating_sub(to.row), 5);
                            }
                            RouteVerb::WallClimb => {
                                assert!(abilities.wall_jump);
                                assert!(
                                    from.row.saturating_sub(to.row) >= 4,
                                    "{abilities:?} {intent:?} {seed}: {from:?} -> {to:?}"
                                );
                            }
                            RouteVerb::Run | RouteVerb::Jump => {
                                assert!(
                                    from.row.abs_diff(to.row) <= 2,
                                    "{abilities:?} {intent:?} {seed}: {edge:?}, {from:?} -> {to:?}"
                                );
                            }
                            RouteVerb::DashAcross | RouteVerb::Drop => panic!(
                                "rhythm links must remain reversible jumps or explicit gates: {edge:?}"
                            ),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn mechanic_transfers_structurally_dominate_distinct_boundary_ports() {
        for abilities in [
            AbilitySet::NONE,
            AbilitySet::new(true, false),
            AbilitySet::new(false, true),
            AbilitySet::ALL,
        ] {
            for intent in ChallengeIntent::ALL {
                for seed in 0..256 {
                    let parts = generate(seed, abilities, intent).expect("generation succeeds");
                    let wall_port = dominated_port(&parts, |verb| verb == RouteVerb::WallClimb);
                    let dash_port = dominated_port(&parts, |verb| {
                        matches!(verb, RouteVerb::DashAcross | RouteVerb::DashUp)
                    });
                    assert_eq!(wall_port.is_some(), abilities.wall_jump);
                    assert_eq!(dash_port.is_some(), abilities.dash);

                    if let Some(port) = wall_port {
                        assert!(!node_reaches_while_omitting(
                            &parts.route_plan,
                            port,
                            |verb| verb == RouteVerb::WallClimb,
                        ));
                    }
                    if let Some(port) = dash_port {
                        assert!(!node_reaches_while_omitting(
                            &parts.route_plan,
                            port,
                            |verb| matches!(verb, RouteVerb::DashAcross | RouteVerb::DashUp),
                        ));
                    }
                    if abilities == AbilitySet::ALL {
                        let wall_port = wall_port.expect("ALL has a wall-dominated port");
                        let dash_port = dash_port.expect("ALL has a dash-dominated port");
                        assert_ne!(wall_port, dash_port);
                        assert!(nodes_are_connected(
                            &parts.route_plan,
                            wall_port,
                            dash_port,
                            |_| false,
                        ));
                        assert!(!nodes_are_connected(
                            &parts.route_plan,
                            wall_port,
                            dash_port,
                            |verb| verb == RouteVerb::WallClimb,
                        ));
                        assert!(!nodes_are_connected(
                            &parts.route_plan,
                            wall_port,
                            dash_port,
                            |verb| matches!(verb, RouteVerb::DashAcross | RouteVerb::DashUp),
                        ));
                    }
                }
            }
        }
    }

    #[test]
    fn wall_transfer_is_a_varied_clear_continuous_shaft() {
        let mut profiles = HashSet::new();
        for intent in ChallengeIntent::ALL {
            for seed in 0..256 {
                let parts = generate(seed, AbilitySet::new(true, false), intent)
                    .expect("generation succeeds");
                let edge = parts
                    .route_plan
                    .edges
                    .iter()
                    .find(|edge| edge.verb == RouteVerb::WallClimb)
                    .expect("wall loadout composes one transfer");
                let bottom = parts.route_plan.nodes[usize::from(edge.from)].support;
                let lip = parts.route_plan.nodes[usize::from(edge.to)].support;
                assert_eq!(bottom.row, FLOOR_ROW);
                assert!(bottom.row - lip.row >= 4);

                let (side, outer_wall_x, inner_wall_x, shaft_start, shaft_end) =
                    if bottom.start_x < ROOM_WIDTH / 2 {
                        (
                            BoundarySide::Left,
                            WallTransfer::OUTER_WALL_X,
                            lip.start_x,
                            WallTransfer::OUTER_WALL_X + 1,
                            lip.start_x,
                        )
                    } else {
                        (
                            BoundarySide::Right,
                            ROOM_WIDTH - 1 - WallTransfer::OUTER_WALL_X,
                            lip.end_x - 1,
                            lip.end_x,
                            ROOM_WIDTH - 1 - WallTransfer::OUTER_WALL_X,
                        )
                    };
                assert!(
                    i32::from(shaft_end - shaft_start) * TILE_SIZE >= PLAYER_WIDTH + 3 * TILE_SIZE
                );
                for row in lip.row..FLOOR_ROW {
                    assert_eq!(parts.draft.tile(inner_wall_x, row), Tile::Solid);
                }
                for row in lip.row..FLOOR_ROW - 2 {
                    assert_eq!(parts.draft.tile(outer_wall_x, row), Tile::Solid);
                }
                for row in lip.row + 1..FLOOR_ROW - 1 {
                    for x in shaft_start..shaft_end {
                        assert_eq!(
                            parts.draft.tile(x, row),
                            Tile::Empty,
                            "shaft obstruction for {intent:?} seed {seed} at {x},{row}"
                        );
                    }
                }
                profiles.insert((side, lip.row, shaft_end - shaft_start, lip.width()));
            }
        }
        assert!(
            profiles.len() >= 24,
            "wall profiles were too repetitive: {profiles:?}"
        );
        assert!(
            profiles
                .iter()
                .any(|profile| profile.0 == BoundarySide::Left)
        );
        assert!(
            profiles
                .iter()
                .any(|profile| profile.0 == BoundarySide::Right)
        );
    }

    #[test]
    fn seed_space_varies_routes_and_port_topologies() {
        let mut all_port_counts = HashSet::new();
        for intent in ChallengeIntent::ALL {
            let mut signatures = HashSet::new();
            let mut node_counts = HashSet::new();
            let mut ceiling_positions = HashSet::new();
            for seed in 0..128 {
                let parts = generate(seed, AbilitySet::ALL, intent).expect("generation succeeds");
                let summary = parts.route_plan.summary();
                signatures.insert(summary.signature);
                node_counts.insert(summary.node_count);
                all_port_counts.insert(summary.port_count);
                if let Some(port) = parts
                    .boundary_ports
                    .iter()
                    .find(|port| port.door.side == BoundarySide::Ceiling)
                {
                    ceiling_positions.insert(port.door.trigger_bounds.x);
                }
            }
            assert!(signatures.len() >= 120, "{intent:?}: {signatures:?}");
            assert!(node_counts.len() >= 3, "{intent:?}: {node_counts:?}");
            if intent != ChallengeIntent::Gentle {
                assert!(
                    ceiling_positions.len() >= 8,
                    "{intent:?}: {ceiling_positions:?}"
                );
            }
        }
        assert!(all_port_counts.len() >= 3, "{all_port_counts:?}");
    }

    #[test]
    fn broad_seed_batch_finishes_core_room_and_door_validation() {
        for abilities in [
            AbilitySet::NONE,
            AbilitySet::new(true, false),
            AbilitySet::new(false, true),
            AbilitySet::ALL,
        ] {
            for intent in ChallengeIntent::ALL {
                for seed in 0..256 {
                    let parts = generate(seed, abilities, intent).expect("generation succeeds");
                    let port_count = parts.boundary_ports.len();
                    let doors = parts
                        .boundary_ports
                        .iter()
                        .map(|port| port.door.clone())
                        .collect();
                    let room = parts
                        .draft
                        .finish_without_exits(
                            format!("rhythm-test-{abilities:?}-{intent:?}-{seed}"),
                            "Rhythm test".to_owned(),
                            parts.spawn,
                        )
                        .expect("candidate satisfies core room invariants")
                        .with_doors(doors)
                        .expect("candidate doors satisfy core invariants");
                    assert!(room.exits().is_empty());
                    assert_eq!(room.doors().len(), port_count);
                }
            }
        }
    }

    fn dominated_port(parts: &CandidateParts, is_gate: impl Fn(RouteVerb) -> bool) -> Option<u16> {
        parts.boundary_ports.iter().find_map(|port| {
            let incident = parts
                .route_plan
                .edges
                .iter()
                .filter(|edge| edge.from == port.node_id || edge.to == port.node_id)
                .collect::<Vec<_>>();
            (!incident.is_empty() && incident.iter().all(|edge| is_gate(edge.verb)))
                .then_some(port.node_id)
        })
    }

    fn node_reaches_while_omitting(
        plan: &RoutePlan,
        start: u16,
        omit: impl Fn(RouteVerb) -> bool,
    ) -> bool {
        plan.nodes
            .iter()
            .all(|node| nodes_are_connected(plan, start, node.id, &omit))
    }

    fn nodes_are_connected(
        plan: &RoutePlan,
        start: u16,
        target: u16,
        omit: impl Fn(RouteVerb) -> bool,
    ) -> bool {
        let mut seen = vec![false; plan.nodes.len()];
        seen[usize::from(start)] = true;
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if node == target {
                return true;
            }
            for edge in &plan.edges {
                if omit(edge.verb) {
                    continue;
                }
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
        false
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
