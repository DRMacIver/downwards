use std::collections::HashSet;

use downwards_core::{AbilitySet, BoundarySide, Door, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect};

use super::{
    BoundaryPort, CandidateParts, ChallengeIntent, ExperimentalGenerationError, GenerationStrategy,
    NodeRole, RoutePlan, RouteVerb, SupportKind, SupportSpec,
    common::{
        FLOOR_ROW, RoomDraft, StableRng, add_route_edge, add_route_node, edge_verb,
        mirrored_support,
    },
};

const RNG_STREAM: u64 = 0x7265_6163_682d_6772;
const MIN_PLATFORM_ROW: u16 = 3;
const MAX_PLATFORM_ROW: u16 = FLOOR_ROW - 2;

#[derive(Clone, Copy)]
struct GrowthProfile {
    minimum_nodes: u16,
    extra_nodes: u16,
    minimum_width: u16,
    maximum_width: u16,
    ascent_bias: u16,
    frontier_bias: u16,
    maximum_baseline_gap: u16,
    maximum_dash_gap: u16,
    maximum_dash_rise: u16,
    safe_islands: u16,
    safe_island_variation: u16,
    minimum_hazards: u16,
    extra_hazards: u16,
    timed_hazard_height: i32,
    solid_frequency: u16,
}

impl GrowthProfile {
    const fn for_intent(intent: ChallengeIntent) -> Self {
        match intent {
            ChallengeIntent::Gentle => Self {
                minimum_nodes: 8,
                extra_nodes: 3,
                minimum_width: 4,
                maximum_width: 7,
                ascent_bias: 42,
                frontier_bias: 42,
                maximum_baseline_gap: 3,
                maximum_dash_gap: 4,
                maximum_dash_rise: 2,
                safe_islands: 3,
                safe_island_variation: 3,
                minimum_hazards: 0,
                extra_hazards: 2,
                timed_hazard_height: 8,
                solid_frequency: 6,
            },
            ChallengeIntent::Standard => Self {
                minimum_nodes: 11,
                extra_nodes: 3,
                minimum_width: 3,
                maximum_width: 6,
                ascent_bias: 57,
                frontier_bias: 58,
                maximum_baseline_gap: 3,
                maximum_dash_gap: 5,
                maximum_dash_rise: 3,
                safe_islands: 1,
                safe_island_variation: 3,
                minimum_hazards: 1,
                extra_hazards: 2,
                timed_hazard_height: 13,
                solid_frequency: 4,
            },
            ChallengeIntent::Technical => Self {
                minimum_nodes: 14,
                extra_nodes: 3,
                minimum_width: 2,
                maximum_width: 5,
                ascent_bias: 70,
                frontier_bias: 72,
                maximum_baseline_gap: 3,
                maximum_dash_gap: 6,
                maximum_dash_rise: 4,
                safe_islands: 0,
                safe_island_variation: 2,
                minimum_hazards: 2,
                extra_hazards: 2,
                timed_hazard_height: 18,
                solid_frequency: 3,
            },
        }
    }

    fn target_nodes(self, rng: &mut StableRng) -> usize {
        usize::from(self.minimum_nodes + rng.below(self.extra_nodes))
    }

    const fn maximum_gap(self, abilities: AbilitySet) -> u16 {
        if abilities.dash {
            self.maximum_dash_gap
        } else {
            self.maximum_baseline_gap
        }
    }

    const fn maximum_rise(self, abilities: AbilitySet) -> u16 {
        if abilities.dash {
            self.maximum_dash_rise
        } else {
            2
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct GrownNode {
    support: SupportSpec,
    primary_parent: Option<u16>,
    port_side: Option<BoundarySide>,
}

#[derive(Clone, Copy, Debug)]
struct PortSeed {
    side: BoundarySide,
    support: SupportSpec,
}

pub(super) fn generate(
    seed: u64,
    abilities: AbilitySet,
    intent: ChallengeIntent,
) -> Result<CandidateParts, ExperimentalGenerationError> {
    let profile = GrowthProfile::for_intent(intent);
    let mut rng = StableRng::new(seed, RNG_STREAM);
    let mirrored = rng.coin();
    let port_seeds = seed_boundary_ports(intent, profile, &mut rng);
    let port_count = port_seeds.len();
    let mut nodes = port_seeds
        .iter()
        .map(|port| GrownNode {
            support: port.support,
            primary_parent: None,
            port_side: Some(port.side),
        })
        .collect::<Vec<_>>();

    connect_port_components(&mut nodes, port_count, profile, abilities, &mut rng).ok_or_else(
        || {
            let labels = support_component_labels(&nodes, profile, abilities);
            embedding_error(&format!(
                "could not merge every boundary port component after growth {nodes:?}, labels {labels:?}"
            ))
        },
    )?;

    // Add a seed-dependent nucleus after all ports share one reversible
    // network. The cycle closer below therefore enriches a multi-entry room
    // instead of merely joining otherwise disconnected entrances.
    let nucleus_size = (port_count + usize::from(3 + rng.below(3))).max(7);
    while nodes.len() < nucleus_size {
        grow_one(&mut nodes, None, None, profile, abilities, &mut rng)
            .ok_or_else(|| embedding_error("could not grow a reachable nucleus"))?;
    }

    let existing_optional =
        choose_optional_cycle_node(&nodes, port_count, profile, abilities, &mut rng);
    let (pickup_index, injected_rejoin) = if let Some(pickup) = existing_optional {
        (pickup, None)
    } else {
        let mut cycle = inject_fork_rejoin(&mut nodes, profile, abilities, &mut rng);
        for _ in 0..8 {
            if cycle.is_some() {
                break;
            }
            grow_one(&mut nodes, None, None, profile, abilities, &mut rng)
                .ok_or_else(|| embedding_error("could not expand the nucleus toward a cycle"))?;
            cycle = inject_fork_rejoin(&mut nodes, profile, abilities, &mut rng);
        }
        let (pickup, rejoin) = cycle.ok_or_else(|| {
            embedding_error(&format!(
                "could not close a conservative fork/rejoin cycle from {nodes:?}"
            ))
        })?;
        (pickup, Some(rejoin))
    };

    // A newly injected rejoin receives an onward route so the diamond reads
    // as part of the network. Incidental cycles already have downstream graph
    // context and do not need another forced support.
    if let Some(rejoin_index) = injected_rejoin {
        let cycle_anchor = nodes[usize::from(rejoin_index)]
            .primary_parent
            .expect("cycle rejoin always has its direct anchor");
        let onward = grow_one(
            &mut nodes,
            Some(rejoin_index),
            Some(pickup_index),
            profile,
            abilities,
            &mut rng,
        )
        .or_else(|| {
            grow_one(
                &mut nodes,
                Some(cycle_anchor),
                Some(pickup_index),
                profile,
                abilities,
                &mut rng,
            )
        })
        .or_else(|| {
            grow_one(
                &mut nodes,
                None,
                Some(pickup_index),
                profile,
                abilities,
                &mut rng,
            )
        });
        onward.ok_or_else(|| embedding_error("could not continue beyond the cycle"))?;
    }
    if injected_rejoin.is_none() && !support_network_has_branch(&nodes, profile, abilities) {
        grow_one(
            &mut nodes,
            None,
            Some(pickup_index),
            profile,
            abilities,
            &mut rng,
        )
        .ok_or_else(|| embedding_error("could not grow a junction from the existing cycle"))?;
    }

    let target_nodes = profile.target_nodes(&mut rng).max(nodes.len());
    let mut failed_growths = 0_u16;
    while nodes.len() < target_nodes && failed_growths < 24 {
        if grow_one(
            &mut nodes,
            None,
            Some(pickup_index),
            profile,
            abilities,
            &mut rng,
        )
        .is_some()
        {
            failed_growths = 0;
        } else {
            failed_growths += 1;
        }
    }
    if nodes.len() < target_nodes {
        return Err(embedding_error(&format!(
            "support growth stopped at {} of {target_nodes} nodes",
            nodes.len()
        )));
    }

    let mut route_plan = reconstruct_route_plan(
        &nodes,
        port_count,
        pickup_index,
        mirrored,
        profile,
        abilities,
    );
    assign_structural_roles(&mut route_plan, port_count, pickup_index);

    let summary = route_plan.summary();
    if summary.port_count != u16::try_from(port_count).unwrap_or(u16::MAX)
        || summary.cycle_rank == 0
        || summary.branch_nodes == 0
        || !route_plan_is_connected(&route_plan)
    {
        return Err(embedding_error(&format!(
            "reconstructed support network lost a port or its fork/rejoin cycle: {summary:?}, connected {}, pickup {pickup_index}, nodes {nodes:?}",
            route_plan_is_connected(&route_plan)
        )));
    }

    let boundary_ports = build_boundary_ports(&port_seeds, &route_plan, mirrored);

    let mut draft = RoomDraft::new();
    for node in &route_plan.nodes {
        draft.platform(node.support);
    }

    let safe_ranges = floor_safe_ranges(&boundary_ports, mirrored, profile, &mut rng);
    draft.hazard_floor_except(&safe_ranges);
    let pickup_support = route_plan.nodes[usize::from(pickup_index)].support;
    draft.pickup_above("growth-cache", pickup_support);
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    let spawn = boundary_ports[0].door.arrival;
    add_timed_hazards(
        &mut draft,
        spawn,
        &boundary_ports,
        mirrored,
        profile,
        &mut rng,
    );

    Ok(CandidateParts {
        draft,
        spawn,
        boundary_ports,
        route_plan,
    })
}

const ROOM_WIDTH_INTERIOR_END: u16 = crate::ROOM_WIDTH - 1;

fn seed_boundary_ports(
    intent: ChallengeIntent,
    profile: GrowthProfile,
    rng: &mut StableRng,
) -> Vec<PortSeed> {
    let count = usize::from(2 + rng.below(3));
    let mut available = vec![
        BoundarySide::Left,
        BoundarySide::Right,
        BoundarySide::Ceiling,
        BoundarySide::Floor,
    ];
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        let index = usize::from(
            rng.below(
                available
                    .len()
                    .try_into()
                    .expect("four boundary sides fit u16"),
            ),
        );
        let side = available.remove(index);
        let width = rng.between(profile.minimum_width.max(4), profile.maximum_width.max(5));
        let support = match side {
            BoundarySide::Left => SupportSpec {
                start_x: 1,
                end_x: 1 + width,
                row: side_port_row(intent, rng),
                kind: SupportKind::Solid,
            },
            BoundarySide::Right => SupportSpec {
                start_x: ROOM_WIDTH_INTERIOR_END - width,
                end_x: ROOM_WIDTH_INTERIOR_END,
                row: side_port_row(intent, rng),
                kind: SupportKind::Solid,
            },
            BoundarySide::Ceiling => {
                let start_x = rng.between(8, 24 - width);
                SupportSpec {
                    start_x,
                    end_x: start_x + width,
                    row: 3 + rng.below(2),
                    kind: SupportKind::OneWay,
                }
            }
            BoundarySide::Floor => {
                // Reserve two tiles on the chosen run-off side while staying
                // clear of the widest possible west/east port ledges.
                let mut start_x = rng.between(10, 22 - width);
                // An exactly centered ledge is invariant under mirroring but
                // its left/right run-off choice is not. Nudge it one tile so
                // mirroring always mirrors the reserved shaft as well.
                if start_x + width / 2 == crate::ROOM_WIDTH / 2 {
                    start_x = if start_x > 10 {
                        start_x - 1
                    } else {
                        start_x + 1
                    };
                }
                SupportSpec {
                    start_x,
                    end_x: start_x + width,
                    row: MAX_PLATFORM_ROW,
                    kind: SupportKind::Solid,
                }
            }
        };
        ports.push(PortSeed { side, support });
    }
    ports
}

fn side_port_row(intent: ChallengeIntent, rng: &mut StableRng) -> u16 {
    match intent {
        ChallengeIntent::Gentle => rng.between(11, MAX_PLATFORM_ROW),
        ChallengeIntent::Standard => rng.between(8, MAX_PLATFORM_ROW),
        ChallengeIntent::Technical => rng.between(5, MAX_PLATFORM_ROW),
    }
}

fn connect_port_components(
    nodes: &mut Vec<GrownNode>,
    port_count: usize,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<()> {
    for _ in 0..40 {
        let labels = support_component_labels(nodes, profile, abilities);
        let first_port_component = labels[0];
        if labels[..port_count]
            .iter()
            .all(|&component| component == first_port_component)
        {
            return Some(());
        }

        let mut bridges = Vec::new();
        for first in 0..nodes.len() {
            for second in first + 1..nodes.len() {
                if labels[first] == labels[second] {
                    continue;
                }
                bridges.push((
                    support_distance(nodes[first].support, nodes[second].support),
                    first,
                    second,
                ));
            }
        }
        bridges.sort_unstable_by_key(|&(distance, first, second)| (distance, first, second));
        let search_width = bridges.len().min(12);
        if search_width == 0 {
            return None;
        }
        if let Some((parent, support)) =
            sample_component_bridge(nodes, &bridges, profile, abilities, rng)
        {
            nodes.push(GrownNode {
                support,
                primary_parent: Some(parent.try_into().ok()?),
                port_side: None,
            });
            continue;
        }
        let offset = usize::from(
            rng.below(
                search_width
                    .try_into()
                    .expect("bridge search width fits u16"),
            ),
        );
        let mut grown = false;
        for bridge_offset in 0..search_width {
            let (_, mut from, mut toward) = bridges[(offset + bridge_offset) % search_width];
            if rng.coin() {
                std::mem::swap(&mut from, &mut toward);
            }
            if grow_toward(nodes, from, toward, profile, abilities, rng).is_some() {
                grown = true;
                break;
            }
        }
        if !grown {
            return None;
        }
    }
    None
}

fn sample_component_bridge(
    nodes: &[GrownNode],
    cross_component_pairs: &[(u16, usize, usize)],
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<(usize, SupportSpec)> {
    let mut candidates = Vec::new();
    for &(_, first, second) in cross_component_pairs.iter().take(24) {
        for (parent_index, target_index) in [(first, second), (second, first)] {
            let parent = nodes[parent_index].support;
            let target = nodes[target_index].support;
            for row in MIN_PLATFORM_ROW..=MAX_PLATFORM_ROW {
                for candidate in
                    candidates_from_parent(nodes, parent, row, false, profile, abilities)
                {
                    if reversible_pair(candidate, target, profile, abilities) {
                        candidates.push((parent_index, candidate));
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let index = usize::from(
        rng.below(
            candidates
                .len()
                .min(usize::from(u16::MAX))
                .try_into()
                .expect("bounded bridge candidates fit u16"),
        ),
    );
    Some(candidates[index])
}

fn grow_toward(
    nodes: &mut Vec<GrownNode>,
    parent_index: usize,
    target_index: usize,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<u16> {
    let parent = nodes[parent_index].support;
    let target = nodes[target_index].support;
    let maximum_step = profile.maximum_rise(abilities);
    let direct_row = if parent.row > target.row {
        parent.row - (parent.row - target.row).min(maximum_step)
    } else {
        parent.row + (target.row - parent.row).min(maximum_step)
    };
    let mut rows = vec![
        direct_row,
        parent.row.clamp(MIN_PLATFORM_ROW, MAX_PLATFORM_ROW),
        parent
            .row
            .saturating_sub(maximum_step)
            .max(MIN_PLATFORM_ROW),
        parent
            .row
            .saturating_add(maximum_step)
            .min(MAX_PLATFORM_ROW),
    ];
    if direct_row > MIN_PLATFORM_ROW {
        rows.push(direct_row - 1);
    }
    if direct_row < MAX_PLATFORM_ROW {
        rows.push(direct_row + 1);
    }
    rows.sort_unstable();
    rows.dedup();

    let current_distance = support_distance(parent, target);
    let mut candidates = Vec::new();
    for row in rows {
        candidates.extend(candidates_from_parent(
            nodes, parent, row, false, profile, abilities,
        ));
    }
    candidates.retain(|candidate| {
        support_distance(*candidate, target)
            <= current_distance + profile.maximum_gap(abilities) * 4 + 8
            || reversible_pair(*candidate, target, profile, abilities)
    });
    candidates.sort_unstable_by_key(|candidate| {
        (
            support_distance(*candidate, target),
            candidate.row,
            candidate.start_x,
            candidate.end_x,
        )
    });
    let choice_count = candidates.len().min(5);
    if choice_count == 0 {
        return None;
    }
    let choice = usize::from(
        rng.below(
            choice_count
                .try_into()
                .expect("growth choice count fits u16"),
        ),
    );
    let id = nodes.len().try_into().ok()?;
    nodes.push(GrownNode {
        support: candidates[choice],
        primary_parent: Some(parent_index.try_into().ok()?),
        port_side: None,
    });
    Some(id)
}

fn support_component_labels(
    nodes: &[GrownNode],
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> Vec<usize> {
    let mut labels = vec![usize::MAX; nodes.len()];
    let mut next_label = 0;
    for start in 0..nodes.len() {
        if labels[start] != usize::MAX {
            continue;
        }
        labels[start] = next_label;
        let mut stack = vec![start];
        while let Some(current) = stack.pop() {
            for adjacent in 0..nodes.len() {
                if labels[adjacent] == usize::MAX
                    && reversible_pair(
                        nodes[current].support,
                        nodes[adjacent].support,
                        profile,
                        abilities,
                    )
                {
                    labels[adjacent] = next_label;
                    stack.push(adjacent);
                }
            }
        }
        next_label += 1;
    }
    labels
}

fn support_distance(first: SupportSpec, second: SupportSpec) -> u16 {
    horizontal_gap(first, second) * 3 + first.row.abs_diff(second.row) * 4
}

fn embedding_error(detail: &str) -> ExperimentalGenerationError {
    ExperimentalGenerationError::EmbeddingExhausted {
        strategy: GenerationStrategy::ReachabilityGrowth,
        detail: detail.to_owned(),
    }
}

fn grow_one(
    nodes: &mut Vec<GrownNode>,
    preferred_parent: Option<u16>,
    excluded_parent: Option<u16>,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<u16> {
    for _ in 0..96 {
        let parent_index =
            preferred_parent.unwrap_or_else(|| choose_parent(nodes, excluded_parent, profile, rng));
        if Some(parent_index) == excluded_parent {
            continue;
        }
        let parent = nodes[usize::from(parent_index)].support;
        let target_row = sample_target_row(parent.row, profile, abilities, rng);
        let want_solid = !nodes
            .iter()
            .skip(1)
            .any(|node| node.support.kind == SupportKind::Solid)
            || rng.below(profile.solid_frequency) == 0;
        let mut candidates =
            candidates_from_parent(nodes, parent, target_row, want_solid, profile, abilities);
        if candidates.is_empty() && want_solid {
            candidates =
                candidates_from_parent(nodes, parent, target_row, false, profile, abilities);
        }
        let Some(support) = choose_spatial_candidate(&candidates, nodes, profile, rng) else {
            continue;
        };
        let id = nodes.len().try_into().expect("experimental route fits u16");
        nodes.push(GrownNode {
            support,
            primary_parent: Some(parent_index),
            port_side: None,
        });
        return Some(id);
    }
    None
}

fn choose_parent(
    nodes: &[GrownNode],
    excluded: Option<u16>,
    profile: GrowthProfile,
    rng: &mut StableRng,
) -> u16 {
    let eligible = (0..nodes.len())
        .filter_map(|index| {
            let id = u16::try_from(index).ok()?;
            (Some(id) != excluded).then_some(id)
        })
        .collect::<Vec<_>>();
    if rng.below(100) < profile.frontier_bias {
        let highest_row = eligible
            .iter()
            .map(|&id| nodes[usize::from(id)].support.row)
            .min()
            .expect("growth always retains an eligible root");
        let frontier = eligible
            .iter()
            .copied()
            .filter(|&id| nodes[usize::from(id)].support.row <= highest_row + 2)
            .collect::<Vec<_>>();
        return frontier[usize::from(rng.below(frontier.len().try_into().unwrap_or(u16::MAX)))];
    }
    eligible[usize::from(rng.below(eligible.len().try_into().unwrap_or(u16::MAX)))]
}

fn sample_target_row(
    parent_row: u16,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> u16 {
    let roll = rng.below(100);
    let sampled = if roll < profile.ascent_bias && parent_row > MIN_PLATFORM_ROW {
        let maximum = profile
            .maximum_rise(abilities)
            .min(parent_row - MIN_PLATFORM_ROW);
        parent_row - rng.between(1, maximum.max(1))
    } else if roll < profile.ascent_bias + 15 {
        parent_row.min(MAX_PLATFORM_ROW)
    } else if parent_row < MAX_PLATFORM_ROW {
        parent_row + rng.between(1, (MAX_PLATFORM_ROW - parent_row).min(3))
    } else {
        parent_row.saturating_sub(rng.between(1, 2))
    };
    sampled.clamp(MIN_PLATFORM_ROW, MAX_PLATFORM_ROW)
}

fn candidates_from_parent(
    nodes: &[GrownNode],
    parent: SupportSpec,
    target_row: u16,
    solid: bool,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> Vec<SupportSpec> {
    let kind = if solid {
        SupportKind::Solid
    } else {
        SupportKind::OneWay
    };
    let mut candidates = Vec::new();
    for width in profile.minimum_width..=profile.maximum_width {
        for start_x in 1..=ROOM_WIDTH_INTERIOR_END - width {
            let candidate = SupportSpec {
                start_x,
                end_x: start_x + width,
                row: target_row,
                kind,
            };
            if support_conflicts(candidate, nodes) {
                continue;
            }
            if conservative_edge(parent, candidate, profile, abilities).is_some()
                && conservative_edge(candidate, parent, profile, abilities).is_some()
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn choose_spatial_candidate(
    candidates: &[SupportSpec],
    nodes: &[GrownNode],
    profile: GrowthProfile,
    rng: &mut StableRng,
) -> Option<SupportSpec> {
    if candidates.is_empty() {
        return None;
    }
    let mut chosen =
        candidates[usize::from(rng.below(candidates.len().try_into().unwrap_or(u16::MAX)))];
    // A small tournament favours unexplored screen regions without making a
    // deterministic packing pattern. Technical intent samples more widely;
    // gentle intent keeps denser, easier-to-read platform neighborhoods.
    let trials = if profile.minimum_nodes >= 14 { 6 } else { 3 };
    for _ in 1..trials {
        let contender =
            candidates[usize::from(rng.below(candidates.len().try_into().unwrap_or(u16::MAX)))];
        let chosen_novelty = spatial_novelty(chosen, nodes);
        let contender_novelty = spatial_novelty(contender, nodes);
        let prefer_novel = profile.minimum_nodes >= 11;
        if (prefer_novel && contender_novelty > chosen_novelty)
            || (!prefer_novel && contender_novelty < chosen_novelty)
        {
            chosen = contender;
        }
    }
    Some(chosen)
}

fn spatial_novelty(candidate: SupportSpec, nodes: &[GrownNode]) -> u16 {
    nodes
        .iter()
        .map(|node| {
            candidate.center_x().abs_diff(node.support.center_x())
                + candidate.row.abs_diff(node.support.row) * 2
        })
        .min()
        .unwrap_or_default()
}

fn choose_optional_cycle_node(
    nodes: &[GrownNode],
    port_count: usize,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<u16> {
    let mut degree = vec![0_u16; nodes.len()];
    let mut edge_count = 0_usize;
    for first in 0..nodes.len() {
        for second in first + 1..nodes.len() {
            if reversible_pair(
                nodes[first].support,
                nodes[second].support,
                profile,
                abilities,
            ) {
                degree[first] += 1;
                degree[second] += 1;
                edge_count += 1;
            }
        }
    }
    if edge_count < nodes.len() {
        return None;
    }
    let candidates = (port_count..nodes.len())
        .filter(|&index| {
            degree[index] >= 2
                && support_network_connected_without(nodes, index, profile, abilities)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let chosen = usize::from(
        rng.below(
            candidates
                .len()
                .try_into()
                .expect("optional cycle candidates fit u16"),
        ),
    );
    candidates[chosen].try_into().ok()
}

fn support_network_has_branch(
    nodes: &[GrownNode],
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> bool {
    (0..nodes.len()).any(|node| {
        (0..nodes.len())
            .filter(|&adjacent| {
                adjacent != node
                    && reversible_pair(
                        nodes[node].support,
                        nodes[adjacent].support,
                        profile,
                        abilities,
                    )
            })
            .take(3)
            .count()
            >= 3
    })
}

fn support_network_connected_without(
    nodes: &[GrownNode],
    excluded: usize,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> bool {
    let Some(start) = (0..nodes.len()).find(|&index| index != excluded) else {
        return false;
    };
    let mut visited = HashSet::from([start]);
    let mut stack = vec![start];
    while let Some(current) = stack.pop() {
        for adjacent in 0..nodes.len() {
            if adjacent == excluded || visited.contains(&adjacent) {
                continue;
            }
            if reversible_pair(
                nodes[current].support,
                nodes[adjacent].support,
                profile,
                abilities,
            ) {
                visited.insert(adjacent);
                stack.push(adjacent);
            }
        }
    }
    visited.len() == nodes.len() - 1
}

fn inject_fork_rejoin(
    nodes: &mut Vec<GrownNode>,
    profile: GrowthProfile,
    abilities: AbilitySet,
    rng: &mut StableRng,
) -> Option<(u16, u16)> {
    let non_leaves = nodes
        .iter()
        .filter_map(|node| node.primary_parent)
        .collect::<HashSet<_>>();
    let leaf_candidates = (1..nodes.len())
        .filter(|&index| {
            let node = &nodes[index];
            node.primary_parent.is_some()
                && !non_leaves.contains(&u16::try_from(index).unwrap_or(u16::MAX))
        })
        .collect::<Vec<_>>();
    let branch_candidates = if leaf_candidates.is_empty() {
        (1..nodes.len())
            .filter(|&index| nodes[index].primary_parent.is_some())
            .collect::<Vec<_>>()
    } else {
        leaf_candidates
    };

    let mut candidates = Vec::new();
    for branch_index in branch_candidates {
        let branch_id = u16::try_from(branch_index).ok()?;
        let anchor_id = nodes[branch_index].primary_parent?;
        let branch = nodes[branch_index].support;
        let anchor = nodes[usize::from(anchor_id)].support;
        for width in profile.minimum_width..=profile.maximum_width {
            for row in MIN_PLATFORM_ROW..=MAX_PLATFORM_ROW {
                for start_x in 1..=ROOM_WIDTH_INTERIOR_END - width {
                    for kind in [SupportKind::OneWay, SupportKind::Solid] {
                        let support = SupportSpec {
                            start_x,
                            end_x: start_x + width,
                            row,
                            kind,
                        };
                        if support_conflicts(support, nodes)
                            || !reversible_pair(anchor, support, profile, abilities)
                            || !reversible_pair(branch, support, profile, abilities)
                        {
                            continue;
                        }
                        candidates.push((branch_id, anchor_id, support));
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let (pickup_id, anchor_id, support) =
        candidates[usize::from(rng.below(candidates.len().try_into().unwrap_or(u16::MAX)))];
    let rejoin_id = nodes.len().try_into().ok()?;
    nodes.push(GrownNode {
        support,
        // The direct anchor -> rejoin edge is the mandatory route. The
        // anchor -> pickup -> rejoin side is therefore genuinely optional.
        primary_parent: Some(anchor_id),
        port_side: None,
    });
    Some((pickup_id, rejoin_id))
}

fn support_conflicts(candidate: SupportSpec, nodes: &[GrownNode]) -> bool {
    nodes.iter().any(|node| {
        let existing = node.support;
        if candidate == existing {
            return true;
        }
        let overlaps = intervals_overlap(
            candidate.start_x,
            candidate.end_x,
            existing.start_x,
            existing.end_x,
        );
        if node.port_side == Some(BoundarySide::Floor) && candidate.row == MAX_PLATFORM_ROW {
            let (shaft_start, shaft_end) = floor_shaft_tiles(existing);
            if intervals_overlap(candidate.start_x, candidate.end_x, shaft_start, shaft_end) {
                return true;
            }
        }
        if candidate.row == existing.row {
            // Do not let two graph nodes rasterize into one platform.
            return overlaps
                || candidate.end_x == existing.start_x
                || existing.end_x == candidate.start_x;
        }
        if existing.row == FLOOR_ROW
            && candidate.kind == SupportKind::Solid
            && candidate.row >= FLOOR_ROW - 2
            && overlaps
        {
            return true;
        }
        // Door arrivals are validated against every tile, including one-way
        // platforms. Preserve a full player-height pocket above every seeded
        // port support; ordinary route supports may retain two-row one-way
        // overlaps that the simulation intentionally permits from below.
        if node.primary_parent.is_none() && candidate.row + 2 == existing.row && overlaps {
            return true;
        }
        // Ten vertical pixels never provide standing room, even under the
        // simulation's permissive one-way collision rules.
        candidate.row.abs_diff(existing.row) == 1 && overlaps
    })
}

fn floor_shaft_tiles(support: SupportSpec) -> (u16, u16) {
    if support.center_x() < crate::ROOM_WIDTH / 2 {
        (support.end_x, support.end_x + 2)
    } else {
        (support.start_x - 2, support.start_x)
    }
}

fn reversible_pair(
    first: SupportSpec,
    second: SupportSpec,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> bool {
    conservative_edge(first, second, profile, abilities).is_some()
        && conservative_edge(second, first, profile, abilities).is_some()
}

fn conservative_edge(
    from: SupportSpec,
    to: SupportSpec,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> Option<RouteVerb> {
    if from == to {
        return None;
    }
    let rise = from.row.saturating_sub(to.row);
    let drop = to.row.saturating_sub(from.row);
    if rise > profile.maximum_rise(abilities) || drop > 6 {
        return None;
    }
    let gap = horizontal_gap(from, to);
    let mut maximum_gap = profile.maximum_gap(abilities);
    if rise >= 2 {
        maximum_gap = maximum_gap.saturating_sub(rise - 2);
    } else if drop >= 3 {
        maximum_gap = maximum_gap.saturating_add(1);
    }
    if gap > maximum_gap {
        return None;
    }
    let horizontal_overlap = intervals_overlap(from.start_x, from.end_x, to.start_x, to.end_x);
    // A solid platform cannot be approached through its underside. One-way
    // platforms deliberately permit this common platformer movement.
    if rise > 0 && horizontal_overlap && to.kind == SupportKind::Solid {
        return None;
    }
    Some(edge_verb(from, to, abilities))
}

const fn horizontal_gap(first: SupportSpec, second: SupportSpec) -> u16 {
    if first.end_x < second.start_x {
        second.start_x - first.end_x
    } else {
        first.start_x.saturating_sub(second.end_x)
    }
}

const fn intervals_overlap(
    first_start: u16,
    first_end: u16,
    second_start: u16,
    second_end: u16,
) -> bool {
    first_start < second_end && second_start < first_end
}

fn reconstruct_route_plan(
    nodes: &[GrownNode],
    port_count: usize,
    pickup_index: u16,
    mirrored: bool,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> RoutePlan {
    let mut plan = RoutePlan::default();
    for (index, node) in nodes.iter().enumerate() {
        let id = u16::try_from(index).expect("experimental route fits u16");
        let role = if index < port_count {
            NodeRole::Port
        } else if id == pickup_index {
            NodeRole::Pickup
        } else {
            NodeRole::Landing
        };
        add_route_node(&mut plan, role, mirrored_support(node.support, mirrored));
    }

    let critical_edges = critical_backbone(nodes, pickup_index, profile, abilities);
    for from in 0..nodes.len() {
        for to in from + 1..nodes.len() {
            let canonical_from = nodes[from].support;
            let canonical_to = nodes[to].support;
            if !reversible_pair(canonical_from, canonical_to, profile, abilities) {
                continue;
            }
            let from_id = u16::try_from(from).expect("experimental route fits u16");
            let to_id = u16::try_from(to).expect("experimental route fits u16");
            let displayed_from = plan.nodes[from].support;
            let displayed_to = plan.nodes[to].support;
            add_route_edge(
                &mut plan,
                from_id,
                to_id,
                edge_verb(displayed_from, displayed_to, abilities),
                critical_edges.contains(&(from_id, to_id)),
            );
        }
    }
    plan
}

fn critical_backbone(
    nodes: &[GrownNode],
    pickup_index: u16,
    profile: GrowthProfile,
    abilities: AbilitySet,
) -> HashSet<(u16, u16)> {
    let pickup = usize::from(pickup_index);
    let mut visited = HashSet::from([0_usize]);
    let mut backbone = HashSet::new();
    while visited.len() < nodes.len() - 1 {
        let mut next = None;
        for from in 0..nodes.len() {
            if !visited.contains(&from) || from == pickup {
                continue;
            }
            for to in 0..nodes.len() {
                if to == pickup || visited.contains(&to) {
                    continue;
                }
                if reversible_pair(nodes[from].support, nodes[to].support, profile, abilities) {
                    next = Some((from, to));
                    break;
                }
            }
            if next.is_some() {
                break;
            }
        }
        let Some((from, to)) = next else {
            break;
        };
        visited.insert(to);
        let pair = if from < to { (from, to) } else { (to, from) };
        backbone.insert((
            pair.0.try_into().expect("experimental route fits u16"),
            pair.1.try_into().expect("experimental route fits u16"),
        ));
    }
    backbone
}

fn assign_structural_roles(plan: &mut RoutePlan, port_count: usize, pickup_index: u16) {
    let mut degree = vec![0_u16; plan.nodes.len()];
    for edge in &plan.edges {
        degree[usize::from(edge.from)] += 1;
        degree[usize::from(edge.to)] += 1;
    }
    for (index, node) in plan.nodes.iter_mut().enumerate() {
        let id = u16::try_from(index).expect("experimental route fits u16");
        if index < port_count {
            node.role = NodeRole::Port;
        } else if id == pickup_index {
            node.role = NodeRole::Pickup;
        } else if degree[index] >= 3 {
            node.role = NodeRole::Junction;
        } else if node.support.row >= MAX_PLATFORM_ROW {
            node.role = NodeRole::Recovery;
        }
    }
}

fn route_plan_is_connected(plan: &RoutePlan) -> bool {
    if plan.nodes.is_empty() {
        return false;
    }
    let mut visited = HashSet::from([0_u16]);
    let mut stack = vec![0_u16];
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
                && visited.insert(adjacent)
            {
                stack.push(adjacent);
            }
        }
    }
    visited.len() == plan.nodes.len()
}

fn build_boundary_ports(
    seeds: &[PortSeed],
    route_plan: &RoutePlan,
    mirrored: bool,
) -> Vec<BoundaryPort> {
    seeds
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            let node_id = u16::try_from(index).expect("four boundary ports fit u16");
            let side = mirrored_side(seed.side, mirrored);
            let support = route_plan.nodes[index].support;
            BoundaryPort {
                node_id,
                door: door_for_support(side, support),
            }
        })
        .collect()
}

const fn mirrored_side(side: BoundarySide, mirrored: bool) -> BoundarySide {
    if !mirrored {
        return side;
    }
    match side {
        BoundarySide::Left => BoundarySide::Right,
        BoundarySide::Right => BoundarySide::Left,
        BoundarySide::Ceiling => BoundarySide::Ceiling,
        BoundarySide::Floor => BoundarySide::Floor,
    }
}

fn door_for_support(side: BoundarySide, support: SupportSpec) -> Door {
    let center_x = i32::from(support.center_x()) * crate::TILE_SIZE;
    let standing_y = i32::from(support.row) * crate::TILE_SIZE - PLAYER_HEIGHT;
    let (id, trigger_bounds, arrival) = match side {
        BoundarySide::Left => (
            "port-west",
            Rect::new(0, i32::from(support.row) * crate::TILE_SIZE - 20, 4, 20),
            Point::new(crate::TILE_SIZE, standing_y),
        ),
        BoundarySide::Right => (
            "port-east",
            Rect::new(
                i32::from(crate::ROOM_WIDTH) * crate::TILE_SIZE - 4,
                i32::from(support.row) * crate::TILE_SIZE - 20,
                4,
                20,
            ),
            Point::new(
                i32::from(crate::ROOM_WIDTH) * crate::TILE_SIZE - crate::TILE_SIZE - PLAYER_WIDTH,
                standing_y,
            ),
        ),
        BoundarySide::Ceiling => (
            "port-ceiling",
            Rect::new(center_x - 10, 0, 20, 4),
            Point::new(center_x - PLAYER_WIDTH / 2, crate::TILE_SIZE),
        ),
        BoundarySide::Floor => (
            "port-floor",
            {
                let (shaft_start, _) = floor_shaft_tiles(support);
                Rect::new(
                    i32::from(shaft_start) * crate::TILE_SIZE,
                    i32::from(crate::ROOM_HEIGHT) * crate::TILE_SIZE - 20,
                    20,
                    20,
                )
            },
            Point::new(center_x - PLAYER_WIDTH / 2, standing_y),
        ),
    };
    Door {
        id: id.to_owned(),
        side,
        trigger_bounds,
        arrival,
        destination_room: None,
        destination_door: None,
    }
}

fn floor_safe_ranges(
    boundary_ports: &[BoundaryPort],
    mirrored: bool,
    profile: GrowthProfile,
    rng: &mut StableRng,
) -> Vec<(u16, u16)> {
    let mut ranges = boundary_ports
        .iter()
        .filter(|port| port.door.side == BoundarySide::Floor)
        .map(|port| {
            let start = (port.door.trigger_bounds.x / crate::TILE_SIZE).max(1) as u16;
            let end = ((port.door.trigger_bounds.right() + crate::TILE_SIZE - 1) / crate::TILE_SIZE)
                .min(i32::from(ROOM_WIDTH_INTERIOR_END)) as u16;
            (start, end)
        })
        .collect::<Vec<_>>();
    let islands = profile.safe_islands + rng.below(profile.safe_island_variation);
    for _ in 0..islands {
        let width = rng.between(2, if profile.minimum_nodes <= 8 { 6 } else { 4 });
        let start_x = rng.between(1, ROOM_WIDTH_INTERIOR_END - width);
        let range = if mirrored {
            (
                crate::ROOM_WIDTH - start_x - width,
                crate::ROOM_WIDTH - start_x,
            )
        } else {
            (start_x, start_x + width)
        };
        ranges.push(range);
    }
    ranges.sort_unstable();
    ranges
}

fn add_timed_hazards(
    draft: &mut RoomDraft,
    spawn: downwards_core::Point,
    boundary_ports: &[BoundaryPort],
    mirrored: bool,
    profile: GrowthProfile,
    rng: &mut StableRng,
) {
    let target = profile.minimum_hazards + rng.below(profile.extra_hazards);
    let spawn_bounds = Rect::new(spawn.x, spawn.y, PLAYER_WIDTH, PLAYER_HEIGHT);
    let mut placed = 0;
    for _ in 0..160 {
        if placed >= target {
            break;
        }
        let tile_x = rng.between(1, crate::ROOM_WIDTH - 2);
        let row = rng.between(1, MAX_PLATFORM_ROW);
        let mut bounds = Rect::new(
            i32::from(tile_x) * crate::TILE_SIZE + 2,
            i32::from(row) * crate::TILE_SIZE + 1,
            6,
            profile.timed_hazard_height,
        );
        if mirrored {
            bounds.x = i32::from(crate::ROOM_WIDTH) * crate::TILE_SIZE - bounds.x - bounds.width;
        }
        if bounds.y + bounds.height >= i32::from(FLOOR_ROW) * crate::TILE_SIZE
            || bounds.intersects(spawn_bounds)
            || boundary_ports.iter().any(|port| {
                bounds.intersects(port.door.trigger_bounds)
                    || bounds.intersects(Rect::new(
                        port.door.arrival.x,
                        port.door.arrival.y,
                        PLAYER_WIDTH,
                        PLAYER_HEIGHT,
                    ))
            })
        {
            continue;
        }
        if draft.try_timed_hazard(bounds, rng) {
            placed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_parts(parts: CandidateParts, id: String) -> downwards_core::Room {
        let CandidateParts {
            draft,
            spawn,
            boundary_ports,
            ..
        } = parts;
        let doors = boundary_ports.into_iter().map(|port| port.door).collect();
        draft
            .finish_without_exits(id, "Growth test".to_owned(), spawn)
            .unwrap()
            .with_doors(doors)
            .unwrap()
    }

    #[test]
    fn growth_builds_multi_port_cyclic_networks_across_loadouts() {
        for abilities in [
            AbilitySet::NONE,
            AbilitySet::new(true, false),
            AbilitySet::new(false, true),
            AbilitySet::ALL,
        ] {
            for intent in ChallengeIntent::ALL {
                for seed in 0..1_000 {
                    let parts = generate(seed, abilities, intent).unwrap_or_else(|error| {
                        panic!("{abilities:?} {intent:?} seed {seed}: {error}")
                    });
                    let summary = parts.route_plan.summary();
                    assert!(summary.node_count >= 8);
                    assert!((2..=4).contains(&summary.port_count));
                    assert!(summary.cycle_rank >= 1);
                    assert!(summary.branch_nodes >= 1);
                    assert!(route_plan_is_connected(&parts.route_plan));
                    assert_eq!(usize::from(summary.port_count), parts.boundary_ports.len());
                    let mut sides = HashSet::new();
                    for port in &parts.boundary_ports {
                        assert!(sides.insert(port.door.side));
                        assert_eq!(
                            parts.route_plan.nodes[usize::from(port.node_id)].role,
                            NodeRole::Port
                        );
                        if port.door.side == BoundarySide::Floor {
                            assert_eq!(port.door.trigger_bounds.height, 20);
                            let support = parts.route_plan.nodes[usize::from(port.node_id)].support;
                            let (shaft_start, shaft_end) = floor_shaft_tiles(support);
                            assert!(!intervals_overlap(
                                support.start_x,
                                support.end_x,
                                shaft_start,
                                shaft_end
                            ));
                            for column in shaft_start..shaft_end {
                                assert_eq!(
                                    parts.draft.tile(column, MAX_PLATFORM_ROW),
                                    downwards_core::Tile::Empty,
                                    "{abilities:?} {intent:?} seed {seed}: blocked floor shaft column {column}, support {support:?}, nodes {:?}",
                                    parts.route_plan.nodes
                                );
                            }
                        }
                        for row in 0..crate::ROOM_HEIGHT {
                            for column in 0..crate::ROOM_WIDTH {
                                if column != 0
                                    && column != crate::ROOM_WIDTH - 1
                                    && row != 0
                                    && row != FLOOR_ROW
                                {
                                    continue;
                                }
                                let tile_bounds = Rect::new(
                                    i32::from(column) * crate::TILE_SIZE,
                                    i32::from(row) * crate::TILE_SIZE,
                                    crate::TILE_SIZE,
                                    crate::TILE_SIZE,
                                );
                                if port.door.trigger_bounds.intersects(tile_bounds) {
                                    assert_eq!(
                                        parts.draft.tile(column, row),
                                        downwards_core::Tile::Empty,
                                        "{abilities:?} {intent:?} seed {seed}: sealed {}",
                                        port.door.id
                                    );
                                }
                            }
                        }
                    }
                    assert_eq!(
                        parts
                            .route_plan
                            .nodes
                            .iter()
                            .filter(|node| node.role == NodeRole::Pickup)
                            .count(),
                        1
                    );
                    let pickup_id = parts
                        .route_plan
                        .nodes
                        .iter()
                        .find(|node| node.role == NodeRole::Pickup)
                        .expect("growth always assigns a pickup node")
                        .id;
                    assert!(
                        parts
                            .route_plan
                            .edges
                            .iter()
                            .filter(|edge| edge.critical)
                            .all(|edge| edge.from != pickup_id && edge.to != pickup_id)
                    );
                    let room = finish_parts(parts, format!("growth-test-{seed}"));
                    assert!(room.exits().is_empty());
                    assert!((2..=4).contains(&room.doors().len()));
                }
            }
        }
    }

    #[test]
    fn every_recorded_connection_is_conservative_and_reversible() {
        for seed in 0..64 {
            let profile = GrowthProfile::for_intent(ChallengeIntent::Technical);
            let parts = generate(seed, AbilitySet::ALL, ChallengeIntent::Technical).unwrap();
            for edge in &parts.route_plan.edges {
                let from = parts.route_plan.nodes[usize::from(edge.from)].support;
                let to = parts.route_plan.nodes[usize::from(edge.to)].support;
                assert!(
                    conservative_edge(from, to, profile, AbilitySet::ALL).is_some(),
                    "seed {seed}, edge {edge:?}"
                );
                assert!(
                    conservative_edge(to, from, profile, AbilitySet::ALL).is_some(),
                    "seed {seed}, reverse edge {edge:?}"
                );
            }
        }
    }

    #[test]
    fn intent_changes_density_and_vertical_character() {
        let mut strictly_denser = 0;
        let mut at_least_as_vertical = 0;
        for seed in 0..128 {
            let gentle = generate(seed, AbilitySet::ALL, ChallengeIntent::Gentle)
                .unwrap()
                .route_plan
                .summary();
            let technical = generate(seed, AbilitySet::ALL, ChallengeIntent::Technical)
                .unwrap()
                .route_plan
                .summary();
            strictly_denser += usize::from(technical.node_count > gentle.node_count);
            at_least_as_vertical +=
                usize::from(technical.vertical_span_rows >= gentle.vertical_span_rows);
        }
        assert!(
            strictly_denser >= 90,
            "technical was denser for only {strictly_denser}/128 seeds"
        );
        assert!(
            at_least_as_vertical >= 80,
            "technical was at least as vertical for only {at_least_as_vertical}/128 seeds"
        );
    }

    #[test]
    fn technical_intent_realizes_more_hazard_pressure() {
        let mut gentle_floor_hazards = 0_usize;
        let mut technical_floor_hazards = 0_usize;
        let mut gentle_timed_hazards = 0_usize;
        let mut technical_timed_hazards = 0_usize;
        for seed in 0..128 {
            for (intent, floor_total, timed_total) in [
                (
                    ChallengeIntent::Gentle,
                    &mut gentle_floor_hazards,
                    &mut gentle_timed_hazards,
                ),
                (
                    ChallengeIntent::Technical,
                    &mut technical_floor_hazards,
                    &mut technical_timed_hazards,
                ),
            ] {
                let CandidateParts {
                    draft,
                    spawn,
                    boundary_ports,
                    ..
                } = generate(seed, AbilitySet::ALL, intent).unwrap();
                let doors = boundary_ports.into_iter().map(|port| port.door).collect();
                let room = draft
                    .finish_without_exits(
                        format!("growth-hazards-{seed}"),
                        "Growth hazards".to_owned(),
                        spawn,
                    )
                    .unwrap()
                    .with_doors(doors)
                    .unwrap();
                *floor_total += room
                    .tiles()
                    .iter()
                    .filter(|&&tile| tile.is_hazard())
                    .count();
                *timed_total += room.timed_hazards().len();
            }
        }
        assert!(technical_floor_hazards > gentle_floor_hazards);
        assert!(technical_timed_hazards > gentle_timed_hazards * 2);
    }

    #[test]
    fn seed_space_produces_broad_route_and_tile_geometry() {
        for intent in ChallengeIntent::ALL {
            let mut route_signatures = HashSet::new();
            let mut tile_fields = HashSet::new();
            for seed in 0..512 {
                let parts = generate(seed, AbilitySet::ALL, intent).unwrap();
                route_signatures.insert(parts.route_plan.summary().signature);
                let room = finish_parts(parts, format!("growth-diversity-{seed}"));
                tile_fields.insert(
                    room.tiles()
                        .iter()
                        .map(|tile| *tile as u8)
                        .collect::<Vec<_>>(),
                );
            }
            assert!(
                route_signatures.len() >= 500,
                "{intent:?}: only {} distinct route graphs",
                route_signatures.len()
            );
            assert!(
                tile_fields.len() >= 500,
                "{intent:?}: only {} distinct tile fields",
                tile_fields.len()
            );
        }
    }
}
