//! Physical mapping for the isolated, unpromoted wall-chimney-v4 key.
//!
//! This is a child of `compositional_route_cut` so it can reuse the frozen
//! mission/support primitives without exposing them or routing v2 through new
//! branches.  Registration is deliberately separate from the v2 entry point.

use super::*;
use crate::experimental::AbilityGateEmbeddingState;
use crate::experimental::wall_chimney_v4::{
    COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION, COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
    CompositionalWallChimneyV4Candidate, CompositionalWallChimneyV4GenerationKey,
    WallChimneyV4Contract, WallChimneyV4EmbeddingSummary, WallChimneyV4ExitSide,
    WallChimneyV4Realization,
};

const V4_RHYTHM_SEARCH_LIMIT: u32 = 500_000;
const V4_RHYTHM_CANDIDATE_LIMIT: u16 = 32;
const V4_SPINE_PLAN_SEARCH_LIMIT: u32 = 50_000;
const V4_FORK_SEARCH_LIMIT: u32 = 100_000;
const V4_RNG_STREAM: u64 = 0x5743_4849_4d4e_4559;

type V4SpineEmbedding = (Vec<u16>, Vec<Option<SupportSpec>>, Vec<RouteCutRealization>);

pub(super) fn embed(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten_mission: AbilityRewrittenMission,
) -> Result<CompositionalWallChimneyV4Candidate, CompositionalAbilityGenerationFailure> {
    let attempt_salt = u64::from(key.base_key.embedding_attempt)
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ u64::from(key.chimney_attempt).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let mut rng = StableRng::new(key.base_key.source_seed ^ attempt_salt, V4_RNG_STREAM);
    let (spine_rows, mut supports, cut_realizations) =
        embed_v4_spine(key, &rewritten_mission, &mut rng)?;
    embed_v4_fork_supports(key, &rewritten_mission, &mut supports, &mut rng)?;
    let mut chimneys = v4_chimney_realizations(key, &rewritten_mission, &supports)?;
    validate_v4_support_contract(key, &rewritten_mission, &supports, &chimneys)?;

    let mission = &rewritten_mission.base_mission;
    let mut route_plan = RoutePlan::default();
    let mut mission_route_nodes = Vec::with_capacity(mission.plan.nodes.len());
    let mut route_by_mission = vec![u16::MAX; mission.plan.nodes.len()];
    for mission_node in &mission.plan.nodes {
        let support = supports
            .get(usize::from(mission_node.id))
            .and_then(|support| *support)
            .ok_or({
                CompositionalAbilityGenerationFailure::BaselineEmbedding(
                    CompositionalRouteCutGenerationFailure::MissingMissionNode {
                        node_id: mission_node.id,
                    },
                )
            })?;
        let route_node_id = add_route_node(&mut route_plan, route_role(mission_node.kind), support);
        route_by_mission[usize::from(mission_node.id)] = route_node_id;
        mission_route_nodes.push(MissionRouteNodeMapping {
            mission_node_id: mission_node.id,
            route_node_id,
        });
    }
    for directed in &rewritten_mission.plan.edges {
        let edge = directed.edge;
        let from = route_by_mission[usize::from(edge.from)];
        let to = route_by_mission[usize::from(edge.to)];
        let from_support = route_plan.nodes[usize::from(from)].support;
        let to_support = route_plan.nodes[usize::from(to)].support;
        let verb = match directed.forward_requirement {
            DirectedTraversalRequirement::Baseline => {
                conservative_baseline_transition(from_support, to_support).ok_or(
                    CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                        phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                        explored: 0,
                    },
                )?
            }
            DirectedTraversalRequirement::Ability(GateAbility::WallJump) => RouteVerb::WallClimb,
            DirectedTraversalRequirement::Ability(GateAbility::Dash) => {
                return Err(CompositionalAbilityGenerationFailure::GateContract {
                    gate_ordinal: 0,
                    violation: AbilityGateGeometryViolation::MissingGateEdge,
                });
            }
        };
        add_route_edge(&mut route_plan, from, to, verb, edge.critical);
    }

    let route_summary = route_plan.summary();
    let socket_columns = socket_columns(&mission.plan, &supports);
    let boundary_ports =
        build_boundary_ports(&mission.plan, &route_by_mission, &supports, &socket_columns)
            .map_err(CompositionalAbilityGenerationFailure::BaselineEmbedding)?;
    validate_embedded_port_contract(&route_plan, &boundary_ports)
        .map_err(CompositionalAbilityGenerationFailure::BaselineEmbedding)?;
    let physical_realizations = chimney_physical_realizations(&chimneys);
    validate_v4_boundary_reservations(&chimneys, &boundary_ports)?;

    let mut draft = RoomDraft::new();
    for node in &route_plan.nodes {
        if node.support.row != FLOOR_ROW {
            draft.platform(node.support);
        }
    }
    rasterize_gate_reservations(&mut draft, &physical_realizations);
    let pickup_route_node = route_by_mission[usize::from(mission.plan.pickup_node_id)];
    draft.pickup_above(
        "wall-chimney-v4-cache",
        route_plan.nodes[usize::from(pickup_route_node)].support,
    );
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }

    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-wall-chimney-v4-{}-e{:02}-r{:03}-c{:02}-{:016x}",
        key.base_key.intent.slug(),
        key.base_key.embedding_attempt,
        key.rewrite_attempt,
        key.chimney_attempt,
        key.base_key.source_seed,
    );
    let name = format!(
        "Experimental unpromoted wall chimney v4 {} embedding {} rewrite {} chimney {} {:016x}",
        key.base_key.intent.slug(),
        key.base_key.embedding_attempt,
        key.rewrite_attempt,
        key.chimney_attempt,
        key.base_key.source_seed,
    );
    let doors = boundary_ports
        .iter()
        .map(|port| port.door.clone())
        .collect();
    let spawn =
        safe_ground_spawn_avoiding_gate_tiles(&supports, &boundary_ports, &physical_realizations)
            .ok_or_else(|| {
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::PortContract(
                    "no collision-free ground spawn outside the v4 chimney remained".to_owned(),
                ),
            )
        })?;
    let room = draft
        .finish_without_exits(id, name, spawn)
        .map_err(|error| {
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::Room(error),
            )
        })?
        .with_doors(doors)
        .map_err(|error| {
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::Door(error),
            )
        })?;
    validate_rasterized_ability_contract(&room, &route_plan, &physical_realizations)?;
    validate_v4_room_objects(&room, &chimneys)?;
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: 34_000 + COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
            seed: key.base_key.source_seed,
            layout_family: LayoutFamily::Chimney,
            ability_tier: AbilityTier::WallJump,
            intended_abilities: AbilitySet::new(true, false),
            stats,
        },
    };

    for chimney in &mut chimneys {
        chimney.physical.from_route_node_id =
            route_by_mission[usize::from(chimney.physical.gate.ascent_from)];
        chimney.physical.to_route_node_id =
            route_by_mission[usize::from(chimney.physical.gate.ascent_to)];
    }
    let (ascent_edges, descent_edges, level_edges) = rhythm_counts(&spine_rows);
    let horizontal_direction_reversals = horizontal_reversals(&mission.plan, &supports)
        .try_into()
        .unwrap_or(u16::MAX);
    let cut_realizations = cut_realizations
        .into_iter()
        .map(|mut realization| {
            realization.route_node_id = route_by_mission[usize::from(realization.mission_node_id)];
            realization.predecessor_route_node_id =
                route_by_mission[usize::from(realization.predecessor_route_node_id)];
            realization.successor_route_node_id =
                route_by_mission[usize::from(realization.successor_route_node_id)];
            realization
        })
        .collect();
    let embedding = WallChimneyV4EmbeddingSummary {
        mapping_version: COMPOSITIONAL_WALL_CHIMNEY_V4_MAPPING_VERSION,
        contract_version: COMPOSITIONAL_WALL_CHIMNEY_V4_CONTRACT_VERSION,
        graph_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
        embedding_attempt: key.base_key.embedding_attempt,
        rewrite_attempt: key.rewrite_attempt,
        chimney_attempt: key.chimney_attempt,
        ascent_edges,
        descent_edges,
        level_edges,
        horizontal_direction_reversals,
        cut_realizations,
        socket_columns: mission
            .plan
            .ports
            .iter()
            .filter_map(|port| socket_columns.get(&port.ordinal).copied())
            .collect(),
        chimney_realizations: chimneys,
    };
    Ok(CompositionalWallChimneyV4Candidate {
        key,
        rewritten_mission,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
        mission_route_nodes,
        embedding,
        evidence_state: AbilityGateEmbeddingState::geometry_embedded_replay_pending(),
    })
}

fn chimney_physical_realizations(
    chimneys: &[WallChimneyV4Realization],
) -> Vec<AbilityGateRealization> {
    chimneys
        .iter()
        .map(|chimney| chimney.physical.clone())
        .collect()
}

fn embed_v4_spine(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    rng: &mut StableRng,
) -> Result<V4SpineEmbedding, CompositionalAbilityGenerationFailure> {
    let plan = &rewritten.base_mission.plan;
    let transition_count = plan.spine.len() - 1;
    let constrained_edges = plan
        .cuts
        .iter()
        .flat_map(|cut| {
            let index = usize::from(cut.spine_index);
            [index - 1, index]
        })
        .collect::<Vec<_>>();
    let mut rows = vec![FLOOR_ROW, FLOOR_ROW];
    let mut search = V4SpineSearch {
        key,
        rewritten,
        transition_count,
        constrained_edges,
        rng,
        rhythm_explored: 0,
        rhythm_candidates: 0,
        spine_explored: 0,
        result: None,
    };
    search.complete(1, FLOOR_ROW, v4_row_bit(FLOOR_ROW), 0, &mut rows);
    if let Some(result) = search.result {
        return Ok(result);
    }
    let (phase, explored) = if search.rhythm_candidates == 0 {
        (
            CompositionalAbilityEmbeddingPhase::Rhythm,
            search.rhythm_explored,
        )
    } else {
        (
            CompositionalAbilityEmbeddingPhase::Spine,
            search.spine_explored,
        )
    };
    Err(CompositionalAbilityGenerationFailure::ConstraintSearchExhausted { phase, explored })
}

struct V4SpineSearch<'a> {
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &'a AbilityRewrittenMission,
    transition_count: usize,
    constrained_edges: Vec<usize>,
    rng: &'a mut StableRng,
    rhythm_explored: u32,
    rhythm_candidates: u16,
    spine_explored: u32,
    result: Option<V4SpineEmbedding>,
}

impl V4SpineSearch<'_> {
    fn complete(
        &mut self,
        edge_index: usize,
        current_row: u16,
        visited_rows: u64,
        reserved_band_rows: u64,
        rows: &mut Vec<u16>,
    ) -> bool {
        if edge_index == self.transition_count {
            if current_row != 3
                || !v4_rows_respect_gate_isolation(self.rewritten, rows)
                || self.rhythm_candidates >= V4_RHYTHM_CANDIDATE_LIMIT
            {
                return false;
            }
            self.rhythm_candidates += 1;
            return self.try_spine_supports(rows);
        }
        if self.rhythm_explored >= V4_RHYTHM_SEARCH_LIMIT
            || self.rhythm_candidates >= V4_RHYTHM_CANDIDATE_LIMIT
        {
            return false;
        }
        let mut steps = if gate_for_spine_edge(self.rewritten, edge_index).is_some() {
            vec![
                i16::try_from(WallChimneyV4Contract::CURRENT.ascent_rows)
                    .expect("v4 rise fits i16"),
            ]
        } else if edge_index == 1
            || edge_index + 1 == self.transition_count
            || self.constrained_edges.contains(&edge_index)
        {
            vec![1_i16, 2]
        } else {
            vec![2_i16, 1, 0, -1]
        };
        shuffle(&mut steps, self.rng);
        for step in steps {
            if self.rhythm_explored >= V4_RHYTHM_SEARCH_LIMIT
                || self.rhythm_candidates >= V4_RHYTHM_CANDIDATE_LIMIT
            {
                return false;
            }
            self.rhythm_explored = self.rhythm_explored.saturating_add(1);
            let next_row_i32 = i32::from(current_row) - i32::from(step);
            let Ok(next_row) = u16::try_from(next_row_i32) else {
                continue;
            };
            if !(3..FLOOR_ROW).contains(&next_row) {
                continue;
            }
            let next_row_bit = v4_row_bit(next_row);
            if reserved_band_rows & next_row_bit != 0 {
                continue;
            }
            let mut next_reserved_band_rows = reserved_band_rows;
            if gate_for_spine_edge(self.rewritten, edge_index).is_some() {
                let gate_band_rows = v4_rows_strictly_between(next_row, current_row);
                if visited_rows & gate_band_rows != 0 {
                    continue;
                }
                next_reserved_band_rows |= gate_band_rows;
            }
            rows.push(next_row);
            if self.complete(
                edge_index + 1,
                next_row,
                visited_rows | next_row_bit,
                next_reserved_band_rows,
                rows,
            ) {
                return true;
            }
            rows.pop();
        }
        false
    }

    fn try_spine_supports(&mut self, rows: &[u16]) -> bool {
        let plan = &self.rewritten.base_mission.plan;
        let domains = spine_support_domains(plan, rows, self.rng);
        let mut supports = vec![None; plan.nodes.len()];
        let mut explored = 0;
        if !complete_v4_spine_supports(
            self.key,
            self.rewritten,
            &domains,
            0,
            0,
            false,
            &mut supports,
            &mut explored,
        ) {
            self.spine_explored = self.spine_explored.saturating_add(explored);
            return false;
        }
        self.spine_explored = self.spine_explored.saturating_add(explored);
        let Ok(cuts) = realized_cuts(plan, &supports) else {
            return false;
        };
        self.result = Some((rows.to_vec(), supports, cuts));
        true
    }
}

fn v4_row_bit(row: u16) -> u64 {
    1_u64
        .checked_shl(u32::from(row))
        .expect("compositional room rows fit the v4 state mask")
}

fn v4_rows_strictly_between(first: u16, second: u16) -> u64 {
    let lower = first.min(second);
    let upper = first.max(second);
    ((lower + 1)..upper).fold(0_u64, |mask, row| mask | v4_row_bit(row))
}

fn v4_rows_respect_gate_isolation(rewritten: &AbilityRewrittenMission, rows: &[u16]) -> bool {
    let plan = &rewritten.base_mission.plan;
    rewritten.plan.gates.iter().all(|gate| {
        let lower_index = usize::from(gate.spine_edge_index);
        let upper_index = lower_index + 1;
        let lower_row = rows[lower_index];
        let upper_row = rows[upper_index];
        rows.iter().enumerate().all(|(index, &row)| {
            index == lower_index || index == upper_index || !(upper_row < row && row < lower_row)
        }) && plan.spine[lower_index] == gate.ascent_from
            && plan.spine[upper_index] == gate.ascent_to
    })
}

#[allow(clippy::too_many_arguments)]
fn complete_v4_spine_supports(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    domains: &[Vec<SupportSpec>],
    index: usize,
    previous_direction: i8,
    has_reversal: bool,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
) -> bool {
    let plan = &rewritten.base_mission.plan;
    if index == plan.spine.len() {
        return has_reversal
            && v4_chimney_realizations(key, rewritten, supports).is_ok()
            && assigned_v4_route_edges_are_clear(key, rewritten, supports);
    }
    let node_id = plan.spine[index];
    for &candidate in &domains[index] {
        if *explored >= V4_SPINE_PLAN_SEARCH_LIMIT {
            return false;
        }
        *explored = explored.saturating_add(1);
        let (next_direction, next_has_reversal) = if index > 0 {
            let Some(previous) = supports[usize::from(plan.spine[index - 1])] else {
                continue;
            };
            let transition_ok = gate_for_spine_edge(rewritten, index - 1).map_or_else(
                || reversible_baseline_transition(previous, candidate),
                |gate| {
                    v4_chimney_realization_for_supports(
                        key, rewritten, gate, previous, candidate, supports,
                    )
                    .is_ok()
                },
            );
            if !transition_ok {
                continue;
            }
            let direction = match previous.center_x().cmp(&candidate.center_x()) {
                std::cmp::Ordering::Less => 1,
                std::cmp::Ordering::Greater => -1,
                std::cmp::Ordering::Equal => 0,
            };
            (
                if direction == 0 {
                    previous_direction
                } else {
                    direction
                },
                has_reversal
                    || (direction != 0
                        && previous_direction != 0
                        && direction != previous_direction),
            )
        } else {
            (previous_direction, has_reversal)
        };
        if floor_socket_overlaps_assigned_support(plan, node_id, candidate, supports) {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_v4_route_edges_are_clear(key, rewritten, supports)
            && assigned_v4_chimney_reservations_are_clear(key, rewritten, supports)
            && partial_cut_obligations_hold(plan, index, supports)
            && future_v4_cut_obligations_remain_feasible(key, rewritten, domains, index, supports)
            && complete_v4_spine_supports(
                key,
                rewritten,
                domains,
                index + 1,
                next_direction,
                next_has_reversal,
                supports,
                explored,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    false
}

fn future_v4_cut_obligations_remain_feasible(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    domains: &[Vec<SupportSpec>],
    assigned_through_index: usize,
    supports: &[Option<SupportSpec>],
) -> bool {
    let plan = &rewritten.base_mission.plan;
    plan.cuts
        .iter()
        .filter(|cut| usize::from(cut.spine_index) > assigned_through_index)
        .all(|cut| {
            let cut_index = usize::from(cut.spine_index);
            domains[cut_index].iter().copied().any(|shelf| {
                let predecessor = supports[usize::from(plan.spine[cut_index - 1])];
                if predecessor.is_some_and(|predecessor| {
                    let (opening_start, opening_end) = cut_opening(cut.anchor, shelf);
                    predecessor.start_x < opening_start
                        || predecessor.end_x > opening_end
                        || !reversible_baseline_transition(predecessor, shelf)
                }) {
                    return false;
                }
                let mut with_shelf = supports.to_vec();
                with_shelf[usize::from(cut.node_id)] = Some(shelf);
                assigned_support_geometry_is_valid(&with_shelf)
                    && assigned_v4_route_edges_are_clear(key, rewritten, &with_shelf)
                    && assigned_v4_chimney_reservations_are_clear(key, rewritten, &with_shelf)
            })
        })
}

fn v4_chimney_realizations(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> Result<Vec<WallChimneyV4Realization>, CompositionalAbilityGenerationFailure> {
    let realizations = rewritten
        .plan
        .gates
        .iter()
        .map(|gate| {
            let lower = supports
                .get(usize::from(gate.ascent_from))
                .and_then(|support| *support)
                .ok_or_else(|| {
                    gate_contract_failure(gate, AbilityGateGeometryViolation::MissingGateEdge)
                })?;
            let upper = supports
                .get(usize::from(gate.ascent_to))
                .and_then(|support| *support)
                .ok_or_else(|| {
                    gate_contract_failure(gate, AbilityGateGeometryViolation::MissingGateEdge)
                })?;
            v4_chimney_realization_for_supports(key, rewritten, gate, lower, upper, supports)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let physical = chimney_physical_realizations(&realizations);
    if !gate_reservations_are_pairwise_disjoint(&physical) {
        let gate = &realizations
            .first()
            .expect("a rewritten WallJump mission contains a gate")
            .physical
            .gate;
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::ReservedVolumeBlocked,
        ));
    }
    Ok(realizations)
}

fn v4_chimney_realization_for_supports(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    gate: &DirectedAbilityGate,
    lower: SupportSpec,
    upper: SupportSpec,
    supports: &[Option<SupportSpec>],
) -> Result<WallChimneyV4Realization, CompositionalAbilityGenerationFailure> {
    let contract = WallChimneyV4Contract::CURRENT;
    if gate.required_ability != GateAbility::WallJump
        || lower.row <= upper.row
        || lower.row - upper.row != contract.ascent_rows
    {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::AscentEnvelope,
        ));
    }
    if lower.kind != SupportKind::OneWay || upper.kind != SupportKind::OneWay {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::EndpointMaterial,
        ));
    }
    if conservative_baseline_transition(lower, upper).is_some()
        || conservative_baseline_transition(upper, lower).is_none()
    {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::ReverseBaselineContract,
        ));
    }
    if rewritten.base_mission.plan.cuts.iter().any(|cut| {
        supports[usize::from(cut.node_id)]
            .is_some_and(|support| (upper.row + 1..lower.row).contains(&support.row))
    }) {
        return Err(gate_contract_failure(
            gate,
            AbilityGateGeometryViolation::CutRowIntersected,
        ));
    }

    let mut sides = [WallChimneyV4ExitSide::Left, WallChimneyV4ExitSide::Right];
    let mut widths = (contract.minimum_clear_width_tiles..=contract.maximum_clear_width_tiles)
        .collect::<Vec<_>>();
    let ordering = v4_geometry_ordering(key, rewritten, gate);
    if ordering & 1 != 0 {
        sides.reverse();
    }
    let width_rotation = usize::try_from((ordering >> 8) % widths.len() as u64)
        .expect("v4 width-domain rotation fits usize");
    widths.rotate_left(width_rotation);

    for exit_side in sides {
        for &width in &widths {
            let geometry = match exit_side {
                WallChimneyV4ExitSide::Right => {
                    let Some(exit_wall_column) = upper.start_x.checked_sub(1) else {
                        continue;
                    };
                    let Some(shaft_start) = exit_wall_column.checked_sub(width) else {
                        continue;
                    };
                    let Some(continuous_wall_column) = shaft_start.checked_sub(1) else {
                        continue;
                    };
                    (
                        shaft_start,
                        exit_wall_column,
                        continuous_wall_column,
                        exit_wall_column,
                        exit_wall_column + 1,
                    )
                }
                WallChimneyV4ExitSide::Left => {
                    let exit_wall_column = upper.end_x;
                    let Some(shaft_start) = exit_wall_column.checked_add(1) else {
                        continue;
                    };
                    let Some(shaft_end) = shaft_start.checked_add(width) else {
                        continue;
                    };
                    (
                        shaft_start,
                        shaft_end,
                        shaft_end,
                        exit_wall_column,
                        exit_wall_column.saturating_sub(1),
                    )
                }
            };
            let (
                shaft_start,
                shaft_end,
                continuous_wall_column,
                exit_wall_column,
                upper_standing_tile,
            ) = geometry;
            if shaft_start == 0
                || shaft_end >= ROOM_WIDTH - 1
                || continuous_wall_column == 0
                || continuous_wall_column >= ROOM_WIDTH - 1
                || exit_wall_column == 0
                || exit_wall_column >= ROOM_WIDTH - 1
                || lower.start_x > shaft_start
                || lower.end_x < shaft_end
            {
                continue;
            }
            let endpoint_abuts_exit = match exit_side {
                WallChimneyV4ExitSide::Right => upper.start_x == exit_wall_column + 1,
                WallChimneyV4ExitSide::Left => upper.end_x == exit_wall_column,
            };
            if !endpoint_abuts_exit {
                continue;
            }
            let Some(aperture_start_row) = upper.row.checked_sub(contract.side_exit_height_tiles)
            else {
                continue;
            };
            if aperture_start_row == 0 {
                continue;
            }
            let mut required_solid_tiles = (0..lower.row)
                .map(|row| AbilityGateTileCell {
                    x: continuous_wall_column,
                    row,
                })
                .collect::<Vec<_>>();
            required_solid_tiles.extend((0..aperture_start_row).map(|row| AbilityGateTileCell {
                x: exit_wall_column,
                row,
            }));
            required_solid_tiles.extend((upper.row..lower.row).map(|row| AbilityGateTileCell {
                x: exit_wall_column,
                row,
            }));
            let mut required_empty_tiles = (1..lower.row)
                .flat_map(|row| {
                    (shaft_start..shaft_end).map(move |x| AbilityGateTileCell { x, row })
                })
                .collect::<Vec<_>>();
            let side_exit_aperture_tiles = (aperture_start_row..upper.row)
                .map(|row| AbilityGateTileCell {
                    x: exit_wall_column,
                    row,
                })
                .collect::<Vec<_>>();
            required_empty_tiles.extend(side_exit_aperture_tiles.iter().copied());
            let Some(lower_standing_bounds) =
                standing_bounds_at_tile(lower, shaft_start + width / 2)
            else {
                continue;
            };
            let Some(upper_standing_bounds) = standing_bounds_at_tile(upper, upper_standing_tile)
            else {
                continue;
            };
            let physical = AbilityGateRealization {
                gate: gate.clone(),
                from_route_node_id: gate.ascent_from,
                to_route_node_id: gate.ascent_to,
                lower_support: lower,
                upper_support: upper,
                ascent_bounds: Rect::new(
                    i32::from(shaft_start) * TILE_SIZE,
                    i32::from(aperture_start_row) * TILE_SIZE,
                    i32::from(width) * TILE_SIZE,
                    i32::from(lower.row - aperture_start_row) * TILE_SIZE,
                ),
                lower_standing_bounds,
                upper_standing_bounds,
                required_solid_tiles,
                required_empty_tiles,
            };
            if !gate_isolation_avoids_supports(&physical, supports) {
                continue;
            }
            if !gate_reservation_avoids_supports(&physical, supports) {
                continue;
            }
            if !v4_pickup_avoids_solid_reservation(rewritten, supports, &physical) {
                continue;
            }
            return Ok(WallChimneyV4Realization {
                physical,
                contract,
                exit_side,
                continuous_wall_column,
                exit_wall_column,
                side_exit_aperture_tiles,
            });
        }
    }
    Err(gate_contract_failure(
        gate,
        AbilityGateGeometryViolation::AscentEnvelope,
    ))
}

fn v4_geometry_ordering(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    gate: &DirectedAbilityGate,
) -> u64 {
    let mut value = key.base_key.source_seed
        ^ rewritten.rewrite_signature().rotate_left(17)
        ^ u64::from(key.base_key.embedding_attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ u64::from(key.rewrite_attempt).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ u64::from(key.chimney_attempt).wrapping_mul(0x94d0_49bb_1331_11eb)
        ^ u64::from(gate.ordinal).rotate_left(41);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn v4_pickup_avoids_solid_reservation(
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
    realization: &AbilityGateRealization,
) -> bool {
    let Some(pickup_support) = supports
        .get(usize::from(rewritten.base_mission.plan.pickup_node_id))
        .and_then(|support| *support)
    else {
        return true;
    };
    let pickup = predicted_pickup_bounds(pickup_support);
    realization.required_solid_tiles.iter().all(|cell| {
        !pickup.intersects(Rect::new(
            i32::from(cell.x) * TILE_SIZE,
            i32::from(cell.row) * TILE_SIZE,
            TILE_SIZE,
            TILE_SIZE,
        ))
    })
}

fn predicted_pickup_bounds(support: SupportSpec) -> Rect {
    Rect::new(
        i32::from(support.center_x()) * TILE_SIZE - 3,
        i32::from(support.row) * TILE_SIZE - 18,
        6,
        6,
    )
}

fn assigned_v4_chimney_reservations_are_clear(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> bool {
    let mut realizations = Vec::new();
    for gate in &rewritten.plan.gates {
        let Some(lower) = supports[usize::from(gate.ascent_from)] else {
            continue;
        };
        let Some(upper) = supports[usize::from(gate.ascent_to)] else {
            continue;
        };
        let Ok(realization) =
            v4_chimney_realization_for_supports(key, rewritten, gate, lower, upper, supports)
        else {
            return false;
        };
        realizations.push(realization.physical);
    }
    gate_reservations_are_pairwise_disjoint(&realizations)
}

fn assigned_v4_route_edges_are_clear(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
) -> bool {
    let assigned = supports.iter().flatten().copied().collect::<Vec<_>>();
    let realizations = rewritten
        .plan
        .gates
        .iter()
        .filter_map(|gate| {
            let lower = supports[usize::from(gate.ascent_from)]?;
            let upper = supports[usize::from(gate.ascent_to)]?;
            v4_chimney_realization_for_supports(key, rewritten, gate, lower, upper, supports)
                .ok()
                .map(|realization| realization.physical)
        })
        .collect::<Vec<_>>();
    rewritten
        .plan
        .edges
        .iter()
        .enumerate()
        .all(|(edge_index, directed)| {
            let edge = directed.edge;
            let Some(first) = supports[usize::from(edge.from)] else {
                return true;
            };
            let Some(second) = supports[usize::from(edge.to)] else {
                return true;
            };
            if let Some(gate) = gate_for_mission_edge(rewritten, edge_index) {
                return v4_chimney_realization_for_supports(
                    key, rewritten, gate, first, second, supports,
                )
                .is_ok();
            }
            reversible_baseline_transition(first, second)
                && support_transfer_headroom_is_clear(first, second, &assigned)
                && v4_transfer_corridor_avoids_chimneys(first, second, &assigned, &realizations)
        })
}

fn v4_transfer_corridor_avoids_chimneys(
    first: SupportSpec,
    second: SupportSpec,
    supports: &[SupportSpec],
    realizations: &[AbilityGateRealization],
) -> bool {
    let first_positions = support_clear_standing_positions(first, supports);
    let second_positions = support_clear_standing_positions(second, supports);
    first_positions.iter().any(|&first_x| {
        second_positions.iter().any(|&second_x| {
            let corridor = standing_transfer_corridor(first, second, first_x, second_x);
            realizations.iter().all(|realization| {
                realization.required_solid_tiles.iter().all(|cell| {
                    !corridor.intersects(Rect::new(
                        i32::from(cell.x) * TILE_SIZE,
                        i32::from(cell.row) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    ))
                })
            })
        })
    })
}

fn standing_transfer_corridor(
    first: SupportSpec,
    second: SupportSpec,
    first_x: i32,
    second_x: i32,
) -> Rect {
    let first_standing_y = i32::from(first.row) * TILE_SIZE - PLAYER_HEIGHT;
    let second_standing_y = i32::from(second.row) * TILE_SIZE - PLAYER_HEIGHT;
    let left = first_x.min(second_x);
    let right = (first_x + PLAYER_WIDTH).max(second_x + PLAYER_WIDTH);
    let top = first_standing_y.min(second_standing_y);
    let bottom = (i32::from(first.row) * TILE_SIZE).max(i32::from(second.row) * TILE_SIZE);
    Rect::new(left, top, right - left, bottom - top)
}

fn embed_v4_fork_supports(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    supports: &mut [Option<SupportSpec>],
    rng: &mut StableRng,
) -> Result<(), CompositionalAbilityGenerationFailure> {
    let plan = &rewritten.base_mission.plan;
    let spine_supports = plan
        .spine
        .iter()
        .filter_map(|&node_id| supports[usize::from(node_id)])
        .collect::<Vec<_>>();
    for fork in &plan.forks {
        let from = supports[usize::from(fork.from)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.from },
            )
        })?;
        let to = supports[usize::from(fork.to)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: fork.to },
            )
        })?;
        let domains = fork_support_domains(fork, from, to, rng);
        let mut explored = 0;
        if !complete_v4_fork_supports(
            key,
            rewritten,
            fork,
            &domains,
            &spine_supports,
            0,
            supports,
            &mut explored,
        ) {
            for &node_id in &fork.branch_nodes {
                supports[usize::from(node_id)] = None;
            }
            return Err(
                CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                    phase: CompositionalAbilityEmbeddingPhase::Fork,
                    explored,
                },
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn complete_v4_fork_supports(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    fork: &MissionFork,
    domains: &[Vec<SupportSpec>],
    spine_supports: &[SupportSpec],
    index: usize,
    supports: &mut [Option<SupportSpec>],
    explored: &mut u32,
) -> bool {
    let plan = &rewritten.base_mission.plan;
    let previous = if index == 0 {
        supports[usize::from(fork.from)].expect("fork source support exists")
    } else {
        supports[usize::from(fork.branch_nodes[index - 1])]
            .expect("previous branch support was assigned")
    };
    if index == fork.branch_nodes.len() {
        let target = supports[usize::from(fork.to)].expect("fork target support exists");
        return reversible_baseline_transition(previous, target)
            && floor_arrivals_remain_clear(plan, supports)
            && assigned_v4_chimney_reservations_are_clear(key, rewritten, supports)
            && assigned_v4_route_edges_are_clear(key, rewritten, supports);
    }
    let node_id = fork.branch_nodes[index];
    let remaining_edges = fork.branch_nodes.len() - index;
    let target = supports[usize::from(fork.to)].expect("fork target support exists");
    for &candidate in &domains[index] {
        if *explored >= V4_FORK_SEARCH_LIMIT {
            return false;
        }
        *explored = explored.saturating_add(1);
        if !reversible_baseline_transition(previous, candidate)
            || candidate.row.abs_diff(target.row)
                > u16::try_from(remaining_edges * 2).unwrap_or(u16::MAX)
            || !has_unique_tile(candidate, spine_supports)
        {
            continue;
        }
        supports[usize::from(node_id)] = Some(candidate);
        if floor_arrivals_remain_clear(plan, supports)
            && assigned_support_geometry_is_valid(supports)
            && assigned_v4_chimney_reservations_are_clear(key, rewritten, supports)
            && assigned_v4_route_edges_are_clear(key, rewritten, supports)
            && complete_v4_fork_supports(
                key,
                rewritten,
                fork,
                domains,
                spine_supports,
                index + 1,
                supports,
                explored,
            )
        {
            return true;
        }
        supports[usize::from(node_id)] = None;
    }
    false
}

fn validate_v4_support_contract(
    key: CompositionalWallChimneyV4GenerationKey,
    rewritten: &AbilityRewrittenMission,
    supports: &[Option<SupportSpec>],
    realizations: &[WallChimneyV4Realization],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    if realizations.len() != rewritten.plan.gates.len()
        || !cut_obligations_hold(&rewritten.base_mission.plan, supports)
        || !assigned_v4_chimney_reservations_are_clear(key, rewritten, supports)
    {
        return Err(
            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                explored: 0,
            },
        );
    }
    for (edge_index, directed) in rewritten.plan.edges.iter().enumerate() {
        let edge = directed.edge;
        let from = supports[usize::from(edge.from)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.from },
            )
        })?;
        let to = supports[usize::from(edge.to)].ok_or({
            CompositionalAbilityGenerationFailure::BaselineEmbedding(
                CompositionalRouteCutGenerationFailure::MissingMissionNode { node_id: edge.to },
            )
        })?;
        match directed.forward_requirement {
            DirectedTraversalRequirement::Baseline => {
                if !reversible_baseline_transition(from, to) {
                    return Err(
                        CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                            phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                            explored: u32::try_from(edge_index + 1).unwrap_or(u32::MAX),
                        },
                    );
                }
            }
            DirectedTraversalRequirement::Ability(GateAbility::WallJump) => {
                let gate = gate_for_mission_edge(rewritten, edge_index).ok_or({
                    CompositionalAbilityGenerationFailure::GateContract {
                        gate_ordinal: 0,
                        violation: AbilityGateGeometryViolation::MissingGateEdge,
                    }
                })?;
                if conservative_baseline_transition(from, to).is_some()
                    || conservative_baseline_transition(to, from).is_none()
                    || !realizations
                        .iter()
                        .any(|realization| realization.physical.gate.ordinal == gate.ordinal)
                {
                    return Err(gate_contract_failure(
                        gate,
                        AbilityGateGeometryViolation::ReverseBaselineContract,
                    ));
                }
            }
            DirectedTraversalRequirement::Ability(GateAbility::Dash) => {
                return Err(CompositionalAbilityGenerationFailure::GateContract {
                    gate_ordinal: 0,
                    violation: AbilityGateGeometryViolation::MissingGateEdge,
                });
            }
        }
    }
    if !assigned_v4_route_edges_are_clear(key, rewritten, supports) {
        return Err(
            CompositionalAbilityGenerationFailure::ConstraintSearchExhausted {
                phase: CompositionalAbilityEmbeddingPhase::FinalGateContract,
                explored: rewritten.plan.edges.len().try_into().unwrap_or(u32::MAX),
            },
        );
    }
    Ok(())
}

fn validate_v4_boundary_reservations(
    chimneys: &[WallChimneyV4Realization],
    ports: &[BoundaryPort],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    let physical = chimney_physical_realizations(chimneys);
    validate_gate_boundary_reservations(&physical, ports)?;
    for chimney in chimneys {
        let collides = ports.iter().any(|port| {
            let arrival = Rect::new(
                port.door.arrival.x,
                port.door.arrival.y,
                PLAYER_WIDTH,
                PLAYER_HEIGHT,
            );
            chimney
                .physical
                .required_solid_tiles
                .iter()
                .chain(&chimney.physical.required_empty_tiles)
                .any(|cell| {
                    let tile = Rect::new(
                        i32::from(cell.x) * TILE_SIZE,
                        i32::from(cell.row) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    );
                    tile.intersects(arrival) || tile.intersects(port.door.trigger_bounds)
                })
        });
        if collides {
            return Err(gate_contract_failure(
                &chimney.physical.gate,
                AbilityGateGeometryViolation::BoundaryArrivalBlocked,
            ));
        }
    }
    Ok(())
}

fn validate_v4_room_objects(
    room: &Room,
    chimneys: &[WallChimneyV4Realization],
) -> Result<(), CompositionalAbilityGenerationFailure> {
    for chimney in chimneys {
        let objects_avoid_solids = room.pickups().iter().all(|pickup| {
            chimney.physical.required_solid_tiles.iter().all(|cell| {
                !pickup.bounds().intersects(Rect::new(
                    i32::from(cell.x) * TILE_SIZE,
                    i32::from(cell.row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                ))
            })
        });
        let object_cells_are_empty = room.pickups().iter().all(|pickup| {
            chimney
                .physical
                .required_empty_tiles
                .iter()
                .filter(|cell| {
                    pickup.bounds().intersects(Rect::new(
                        i32::from(cell.x) * TILE_SIZE,
                        i32::from(cell.row) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    ))
                })
                .all(|cell| room.tile(cell.x, cell.row) == Some(Tile::Empty))
        });
        if !objects_avoid_solids || !object_cells_are_empty {
            return Err(gate_contract_failure(
                &chimney.physical.gate,
                AbilityGateGeometryViolation::RasterMismatch,
            ));
        }
    }
    Ok(())
}
