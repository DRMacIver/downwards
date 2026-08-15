//! Structural diagnostics for experimental candidates.
//!
//! These descriptors are deliberately weaker than solver certificates.  They
//! can identify an abstract low-demand route or give terrain a plausible
//! constructive provenance, but they cannot prove that a route is physically
//! executable or that terrain absent from a witness is unreachable.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use downwards_core::{AbilitySet, Room, Tile};
use downwards_gen::experimental::{
    BoundaryPort, ExperimentalCandidate, NodeRole, RouteEdge, RoutePlan, RouteVerb, SupportSpec,
};
use downwards_lab::TraversalTrace;

/// Version of the definitions and ordering used by this research descriptor.
pub const STRUCTURAL_DESCRIPTOR_VERSION: u32 = 2;

/// The lexicographic cost used to choose the easiest abstract port path.
///
/// Field order is policy: ability requirements dominate length, length
/// dominates branch decisions, decisions dominate height changes, and height
/// changes dominate movement-vocabulary variety.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructuralPathCost {
    pub required_ability_edges: usize,
    pub edge_count: usize,
    pub decision_nodes: usize,
    pub vertical_transitions: usize,
    pub verb_variety: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequiredAbility {
    WallJump,
    Dash,
}

/// One directionally interpreted edge in an abstract port-to-port path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StructuralPathStep {
    pub edge_index: usize,
    pub from_node_id: u16,
    pub to_node_id: u16,
    /// The verb recorded by the generator, in the edge's declared direction.
    pub declared_verb: RouteVerb,
    /// The verb after interpreting ascent/descent in this ordered direction.
    pub traversal_verb: RouteVerb,
    pub required_ability: Option<RequiredAbility>,
    pub critical: bool,
    pub vertical_transition: bool,
}

/// Research-only description of the easiest abstract route for an ordered
/// door pair.
///
/// Direct-edge flags inspect every edge between the two port nodes, not only
/// the selected path.  This means `one_edge_bypass` may be true when a longer
/// baseline path correctly wins over a direct ability edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuralBypassDescriptor {
    pub source_door_id: String,
    pub target_door_id: String,
    pub source_node_id: u16,
    pub target_node_id: u16,
    pub node_path: Vec<u16>,
    pub steps: Vec<StructuralPathStep>,
    pub cost: StructuralPathCost,
    /// Abilities used by the lexicographically easiest abstract path.
    ///
    /// This is descriptive evidence about `steps`, not a requirement claim:
    /// an alternate path may avoid one or both abilities.
    pub selected_path_abilities: AbilitySet,
    pub selected_path_wall_jump_edges: usize,
    pub selected_path_dash_edges: usize,
    /// Abilities whose corresponding directed traversals disconnect this
    /// ordered port pair when removed from the route graph.
    ///
    /// Each ability is tested independently. Parallel wall-jump and dash
    /// routes therefore make neither individual ability unavoidable even
    /// though a baseline-only route may still be absent.
    pub unavoidable_abilities: AbilitySet,
    pub critical_edges: usize,
    pub direct_run_bypass: bool,
    pub direct_drop_bypass: bool,
    pub one_edge_bypass: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructuralDescriptorError {
    SameDoor(String),
    UnknownDoor(String),
    DuplicateBoundaryDoor(String),
    DuplicateRouteNode(u16),
    MissingPortNode {
        door_id: String,
        node_id: u16,
    },
    DanglingEdge {
        edge_index: usize,
        node_id: u16,
    },
    NoPath {
        source_node_id: u16,
        target_node_id: u16,
    },
}

impl fmt::Display for StructuralDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameDoor(id) => write!(formatter, "ordered port path repeats door {id:?}"),
            Self::UnknownDoor(id) => write!(formatter, "unknown boundary door {id:?}"),
            Self::DuplicateBoundaryDoor(id) => {
                write!(formatter, "duplicate boundary door id {id:?}")
            }
            Self::DuplicateRouteNode(id) => write!(formatter, "duplicate route node id {id}"),
            Self::MissingPortNode { door_id, node_id } => write!(
                formatter,
                "boundary door {door_id:?} references missing route node {node_id}"
            ),
            Self::DanglingEdge {
                edge_index,
                node_id,
            } => write!(
                formatter,
                "route edge {edge_index} references missing node {node_id}"
            ),
            Self::NoPath {
                source_node_id,
                target_node_id,
            } => write!(
                formatter,
                "no abstract path from route node {source_node_id} to {target_node_id}"
            ),
        }
    }
}

impl Error for StructuralDescriptorError {}

/// Describe one ordered pair using the candidate's explicit door-to-node map.
pub fn describe_ordered_port_path(
    candidate: &ExperimentalCandidate,
    source_door_id: &str,
    target_door_id: &str,
) -> Result<StructuralBypassDescriptor, StructuralDescriptorError> {
    describe_port_path(
        &candidate.route_plan,
        &candidate.boundary_ports,
        source_door_id,
        target_door_id,
    )
}

/// Lower-level entry point useful to research adapters and focused fixtures.
pub fn describe_port_path(
    plan: &RoutePlan,
    ports: &[BoundaryPort],
    source_door_id: &str,
    target_door_id: &str,
) -> Result<StructuralBypassDescriptor, StructuralDescriptorError> {
    if source_door_id == target_door_id {
        return Err(StructuralDescriptorError::SameDoor(
            source_door_id.to_owned(),
        ));
    }
    let nodes = index_nodes(plan)?;
    let source_node_id = resolve_port(ports, source_door_id)?;
    let target_node_id = resolve_port(ports, target_door_id)?;
    for (door_id, node_id) in [
        (source_door_id, source_node_id),
        (target_door_id, target_node_id),
    ] {
        if !nodes.contains_key(&node_id) {
            return Err(StructuralDescriptorError::MissingPortNode {
                door_id: door_id.to_owned(),
                node_id,
            });
        }
    }
    let adjacency = adjacency(plan, &nodes)?;
    let direct = adjacency
        .get(&source_node_id)
        .into_iter()
        .flatten()
        .filter(|entry| entry.other == target_node_id)
        .map(|entry| {
            directional_verb(
                &plan.edges[entry.edge_index],
                source_node_id,
                nodes[&source_node_id].support,
                nodes[&target_node_id].support,
            )
        })
        .collect::<Vec<_>>();
    let one_edge_bypass = !direct.is_empty();
    let direct_run_bypass = direct.contains(&RouteVerb::Run);
    let direct_drop_bypass = direct.contains(&RouteVerb::Drop);

    let initial_state = (source_node_id, 0_u8);
    let mut frontier = BTreeSet::from([(StructuralPathCost::default(), initial_state)]);
    let mut distances = BTreeMap::from([(initial_state, StructuralPathCost::default())]);
    let mut predecessors = BTreeMap::<(u16, u8), Predecessor>::new();
    let target_state = loop {
        let Some((cost, state)) = frontier.pop_first() else {
            return Err(StructuralDescriptorError::NoPath {
                source_node_id,
                target_node_id,
            });
        };
        if distances.get(&state) != Some(&cost) {
            continue;
        }
        if state.0 == target_node_id {
            break state;
        }
        for entry in &adjacency[&state.0] {
            let edge = &plan.edges[entry.edge_index];
            let from = nodes[&state.0].support;
            let to = nodes[&entry.other].support;
            let verb = directional_verb(edge, state.0, from, to);
            let requirement = ability_requirement(verb);
            let next_mask = state.1 | verb_bit(verb);
            let next_state = (entry.other, next_mask);
            let next_cost = StructuralPathCost {
                required_ability_edges: cost.required_ability_edges
                    + usize::from(requirement.is_some()),
                edge_count: cost.edge_count + 1,
                decision_nodes: cost.decision_nodes
                    + usize::from(state.0 != source_node_id && adjacency[&state.0].len() > 2),
                vertical_transitions: cost.vertical_transitions + usize::from(from.row != to.row),
                verb_variety: next_mask.count_ones() as usize,
            };
            if distances
                .get(&next_state)
                .is_some_and(|known| *known <= next_cost)
            {
                continue;
            }
            if let Some(old) = distances.insert(next_state, next_cost) {
                frontier.remove(&(old, next_state));
            }
            predecessors.insert(
                next_state,
                Predecessor {
                    previous: state,
                    edge_index: entry.edge_index,
                    traversal_verb: verb,
                },
            );
            frontier.insert((next_cost, next_state));
        }
    };

    let mut reversed_steps = Vec::new();
    let mut cursor = target_state;
    while cursor != initial_state {
        let predecessor = predecessors[&cursor];
        let edge = &plan.edges[predecessor.edge_index];
        let from = predecessor.previous.0;
        let to = cursor.0;
        reversed_steps.push(StructuralPathStep {
            edge_index: predecessor.edge_index,
            from_node_id: from,
            to_node_id: to,
            declared_verb: edge.verb,
            traversal_verb: predecessor.traversal_verb,
            required_ability: ability_requirement(predecessor.traversal_verb),
            critical: edge.critical,
            vertical_transition: nodes[&from].support.row != nodes[&to].support.row,
        });
        cursor = predecessor.previous;
    }
    reversed_steps.reverse();
    let mut node_path = Vec::with_capacity(reversed_steps.len() + 1);
    node_path.push(source_node_id);
    node_path.extend(reversed_steps.iter().map(|step| step.to_node_id));
    let selected_path_wall_jump_edges = reversed_steps
        .iter()
        .filter(|step| step.required_ability == Some(RequiredAbility::WallJump))
        .count();
    let selected_path_dash_edges = reversed_steps
        .iter()
        .filter(|step| step.required_ability == Some(RequiredAbility::Dash))
        .count();
    let critical_edges = reversed_steps.iter().filter(|step| step.critical).count();
    let unavoidable_abilities = AbilitySet::new(
        !is_reachable_avoiding_ability(
            plan,
            &nodes,
            &adjacency,
            source_node_id,
            target_node_id,
            RequiredAbility::WallJump,
        ),
        !is_reachable_avoiding_ability(
            plan,
            &nodes,
            &adjacency,
            source_node_id,
            target_node_id,
            RequiredAbility::Dash,
        ),
    );

    Ok(StructuralBypassDescriptor {
        source_door_id: source_door_id.to_owned(),
        target_door_id: target_door_id.to_owned(),
        source_node_id,
        target_node_id,
        node_path,
        steps: reversed_steps,
        cost: distances[&target_state],
        selected_path_abilities: AbilitySet::new(
            selected_path_wall_jump_edges > 0,
            selected_path_dash_edges > 0,
        ),
        selected_path_wall_jump_edges,
        selected_path_dash_edges,
        unavoidable_abilities,
        critical_edges,
        direct_run_bypass,
        direct_drop_bypass,
        one_edge_bypass,
    })
}

fn is_reachable_avoiding_ability(
    plan: &RoutePlan,
    nodes: &BTreeMap<u16, &downwards_gen::experimental::RouteNode>,
    adjacency: &BTreeMap<u16, Vec<AdjacencyEntry>>,
    source_node_id: u16,
    target_node_id: u16,
    avoided: RequiredAbility,
) -> bool {
    let mut frontier = vec![source_node_id];
    let mut visited = BTreeSet::from([source_node_id]);
    while let Some(from_node_id) = frontier.pop() {
        for entry in &adjacency[&from_node_id] {
            let edge = &plan.edges[entry.edge_index];
            let traversal_verb = directional_verb(
                edge,
                from_node_id,
                nodes[&from_node_id].support,
                nodes[&entry.other].support,
            );
            if ability_requirement(traversal_verb) == Some(avoided) {
                continue;
            }
            if entry.other == target_node_id {
                return true;
            }
            if visited.insert(entry.other) {
                frontier.push(entry.other);
            }
        }
    }
    false
}

#[derive(Clone, Copy)]
struct AdjacencyEntry {
    edge_index: usize,
    other: u16,
}

#[derive(Clone, Copy)]
struct Predecessor {
    previous: (u16, u8),
    edge_index: usize,
    traversal_verb: RouteVerb,
}

fn index_nodes(
    plan: &RoutePlan,
) -> Result<BTreeMap<u16, &downwards_gen::experimental::RouteNode>, StructuralDescriptorError> {
    let mut nodes = BTreeMap::new();
    for node in &plan.nodes {
        if nodes.insert(node.id, node).is_some() {
            return Err(StructuralDescriptorError::DuplicateRouteNode(node.id));
        }
    }
    Ok(nodes)
}

fn resolve_port(ports: &[BoundaryPort], door_id: &str) -> Result<u16, StructuralDescriptorError> {
    let mut matches = ports
        .iter()
        .filter(|port| port.door.id == door_id)
        .map(|port| port.node_id);
    let Some(node_id) = matches.next() else {
        return Err(StructuralDescriptorError::UnknownDoor(door_id.to_owned()));
    };
    if matches.next().is_some() {
        return Err(StructuralDescriptorError::DuplicateBoundaryDoor(
            door_id.to_owned(),
        ));
    }
    Ok(node_id)
}

fn adjacency(
    plan: &RoutePlan,
    nodes: &BTreeMap<u16, &downwards_gen::experimental::RouteNode>,
) -> Result<BTreeMap<u16, Vec<AdjacencyEntry>>, StructuralDescriptorError> {
    let mut result = nodes
        .keys()
        .copied()
        .map(|id| (id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for (edge_index, edge) in plan.edges.iter().enumerate() {
        for node_id in [edge.from, edge.to] {
            if !nodes.contains_key(&node_id) {
                return Err(StructuralDescriptorError::DanglingEdge {
                    edge_index,
                    node_id,
                });
            }
        }
        result.get_mut(&edge.from).unwrap().push(AdjacencyEntry {
            edge_index,
            other: edge.to,
        });
        result.get_mut(&edge.to).unwrap().push(AdjacencyEntry {
            edge_index,
            other: edge.from,
        });
    }
    Ok(result)
}

fn directional_verb(
    edge: &RouteEdge,
    traversal_from: u16,
    from: SupportSpec,
    to: SupportSpec,
) -> RouteVerb {
    let follows_declaration = traversal_from == edge.from;
    match edge.verb {
        RouteVerb::WallClimb | RouteVerb::DashUp if follows_declaration => edge.verb,
        RouteVerb::WallClimb | RouteVerb::DashUp => RouteVerb::Drop,
        RouteVerb::DashAcross => RouteVerb::DashAcross,
        _ if follows_declaration => edge.verb,
        RouteVerb::Run | RouteVerb::Jump | RouteVerb::Drop => ordinary_verb(from, to),
    }
}

fn ordinary_verb(from: SupportSpec, to: SupportSpec) -> RouteVerb {
    let rise_rows = from.row.saturating_sub(to.row);
    let drop_rows = to.row.saturating_sub(from.row);
    let horizontal_gap = to
        .start_x
        .saturating_sub(from.end_x)
        .max(from.start_x.saturating_sub(to.end_x));
    let overlaps_horizontally = from.start_x < to.end_x && to.start_x < from.end_x;
    if drop_rows > 2 || (drop_rows > 0 && overlaps_horizontally) {
        RouteVerb::Drop
    } else if rise_rows > 0 || horizontal_gap > 0 {
        RouteVerb::Jump
    } else {
        RouteVerb::Run
    }
}

const fn ability_requirement(verb: RouteVerb) -> Option<RequiredAbility> {
    match verb {
        RouteVerb::WallClimb => Some(RequiredAbility::WallJump),
        RouteVerb::DashAcross | RouteVerb::DashUp => Some(RequiredAbility::Dash),
        RouteVerb::Run | RouteVerb::Jump | RouteVerb::Drop => None,
    }
}

const fn verb_bit(verb: RouteVerb) -> u8 {
    1 << match verb {
        RouteVerb::Run => 0,
        RouteVerb::Jump => 1,
        RouteVerb::Drop => 2,
        RouteVerb::WallClimb => 3,
        RouteVerb::DashAcross => 4,
        RouteVerb::DashUp => 5,
    }
}

/// One room-tile coordinate in a deterministic terrain component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TerrainCell {
    pub x: u16,
    pub y: u16,
}

/// Inclusive tile-space bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainBounds {
    pub min_x: u16,
    pub min_y: u16,
    pub max_x: u16,
    pub max_y: u16,
}

/// Positive attribution and proximity evidence for one four-neighbor interior
/// terrain component.  Solid and one-way tiles are connected to each other;
/// boundary-shell and hazard tiles are excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteriorTerrainComponentDescriptor {
    pub id: usize,
    pub bounds: TerrainBounds,
    pub cells: Box<[TerrainCell]>,
    pub solid_tiles: usize,
    pub one_way_tiles: usize,
    pub route_support_tiles: usize,
    pub recovery_support_tiles: usize,
    /// Tiles inside a one-tile-expanded envelope around an ability edge.
    /// This is geometric correlation, not proof of constructive ownership.
    pub ability_gate_envelope_tiles: usize,
    pub static_attributed_tiles: usize,
    /// Tiles in, or one coarse observation cell from, any supplied certified
    /// traversal trace.  Absence is deliberately not negative evidence.
    pub certified_traversal_near_tiles: usize,
    pub positively_corroborated_tiles: usize,
    pub route_node_ids: Vec<u16>,
    pub recovery_node_ids: Vec<u16>,
    pub ability_gate_edge_indices: Vec<usize>,
    pub nearby_certified_traversal_indices: Vec<usize>,
}

impl InteriorTerrainComponentDescriptor {
    #[must_use]
    pub const fn tile_count(&self) -> usize {
        self.solid_tiles + self.one_way_tiles
    }

    #[must_use]
    pub const fn has_static_attribution(&self) -> bool {
        self.static_attributed_tiles > 0
    }

    #[must_use]
    pub const fn has_positive_corroboration(&self) -> bool {
        self.positively_corroborated_tiles > 0
    }
}

/// Aggregate coverage of connected interior supporting terrain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainUtilityDescriptor {
    pub components: Vec<InteriorTerrainComponentDescriptor>,
    pub interior_terrain_tiles: usize,
    pub route_support_tiles: usize,
    pub recovery_support_tiles: usize,
    pub ability_gate_envelope_tiles: usize,
    pub static_attributed_tiles: usize,
    pub certified_traversal_near_tiles: usize,
    pub positively_corroborated_tiles: usize,
    pub components_without_static_attribution: usize,
    pub tiles_without_static_attribution: usize,
    pub components_without_positive_corroboration: usize,
    pub tiles_without_positive_corroboration: usize,
}

/// Describe supporting terrain using candidate structure and optional positive
/// evidence from replay-certified traces.
///
/// The caller is responsible for supplying only traces whose replays were
/// certified against this candidate.  An empty trace list is valid and makes
/// no claim that statically unattributed terrain is unreachable or unused.
pub fn describe_candidate_terrain(
    candidate: &ExperimentalCandidate,
    certified_traversals: &[&TraversalTrace],
) -> Result<TerrainUtilityDescriptor, StructuralDescriptorError> {
    describe_terrain_utility(
        &candidate.generated.room,
        &candidate.route_plan,
        certified_traversals,
    )
}

/// Lower-level terrain entry point useful to research adapters and fixtures.
pub fn describe_terrain_utility(
    room: &Room,
    plan: &RoutePlan,
    certified_traversals: &[&TraversalTrace],
) -> Result<TerrainUtilityDescriptor, StructuralDescriptorError> {
    let nodes = index_nodes(plan)?;
    // Validate edge endpoints even if the room contains no terrain.
    let _ = adjacency(plan, &nodes)?;
    let raw_components = interior_terrain_components(room);
    let mut result = TerrainUtilityDescriptor::default();

    for (id, cells) in raw_components.into_iter().enumerate() {
        let mut solid_tiles = 0;
        let mut one_way_tiles = 0;
        let mut route_support_tiles = 0;
        let mut recovery_support_tiles = 0;
        let mut ability_gate_envelope_tiles = 0;
        let mut static_attributed_tiles = 0;
        let mut certified_traversal_near_tiles = 0;
        let mut positively_corroborated_tiles = 0;
        let mut route_node_ids = BTreeSet::new();
        let mut recovery_node_ids = BTreeSet::new();
        let mut ability_gate_edge_indices = BTreeSet::new();
        let mut nearby_certified_traversal_indices = BTreeSet::new();

        for &cell in &cells {
            match room.tile(cell.x, cell.y) {
                Some(Tile::Solid) => solid_tiles += 1,
                Some(Tile::OneWay) => one_way_tiles += 1,
                _ => unreachable!("component extraction retains supporting terrain only"),
            }
            let cell_route_nodes = nodes
                .values()
                .filter(|node| support_contains(node.support, cell))
                .map(|node| node.id)
                .collect::<Vec<_>>();
            let on_route_support = !cell_route_nodes.is_empty();
            if on_route_support {
                route_support_tiles += 1;
                route_node_ids.extend(cell_route_nodes.iter().copied());
            }
            let cell_recovery_nodes = cell_route_nodes
                .iter()
                .copied()
                .filter(|id| nodes[id].role == NodeRole::Recovery)
                .collect::<Vec<_>>();
            if !cell_recovery_nodes.is_empty() {
                recovery_support_tiles += 1;
                recovery_node_ids.extend(cell_recovery_nodes);
            }
            let cell_gate_edges = plan
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| ability_requirement(edge.verb).is_some())
                .filter(|(_, edge)| {
                    gate_envelope_contains(nodes[&edge.from].support, nodes[&edge.to].support, cell)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let in_gate_envelope = !cell_gate_edges.is_empty();
            if in_gate_envelope {
                ability_gate_envelope_tiles += 1;
                ability_gate_edge_indices.extend(cell_gate_edges);
            }
            let nearby_traces = certified_traversals
                .iter()
                .enumerate()
                .filter(|(_, trace)| traversal_near_cell(room, trace, cell))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let near_certified_traversal = !nearby_traces.is_empty();
            if near_certified_traversal {
                certified_traversal_near_tiles += 1;
                nearby_certified_traversal_indices.extend(nearby_traces);
            }
            let statically_attributed = on_route_support || in_gate_envelope;
            static_attributed_tiles += usize::from(statically_attributed);
            positively_corroborated_tiles +=
                usize::from(statically_attributed || near_certified_traversal);
        }

        let first = cells[0];
        let bounds = cells.iter().fold(
            TerrainBounds {
                min_x: first.x,
                min_y: first.y,
                max_x: first.x,
                max_y: first.y,
            },
            |mut bounds, cell| {
                bounds.min_x = bounds.min_x.min(cell.x);
                bounds.min_y = bounds.min_y.min(cell.y);
                bounds.max_x = bounds.max_x.max(cell.x);
                bounds.max_y = bounds.max_y.max(cell.y);
                bounds
            },
        );
        let component = InteriorTerrainComponentDescriptor {
            id,
            bounds,
            cells: cells.into_boxed_slice(),
            solid_tiles,
            one_way_tiles,
            route_support_tiles,
            recovery_support_tiles,
            ability_gate_envelope_tiles,
            static_attributed_tiles,
            certified_traversal_near_tiles,
            positively_corroborated_tiles,
            route_node_ids: route_node_ids.into_iter().collect(),
            recovery_node_ids: recovery_node_ids.into_iter().collect(),
            ability_gate_edge_indices: ability_gate_edge_indices.into_iter().collect(),
            nearby_certified_traversal_indices: nearby_certified_traversal_indices
                .into_iter()
                .collect(),
        };
        result.interior_terrain_tiles += component.tile_count();
        result.route_support_tiles += component.route_support_tiles;
        result.recovery_support_tiles += component.recovery_support_tiles;
        result.ability_gate_envelope_tiles += component.ability_gate_envelope_tiles;
        result.static_attributed_tiles += component.static_attributed_tiles;
        result.certified_traversal_near_tiles += component.certified_traversal_near_tiles;
        result.positively_corroborated_tiles += component.positively_corroborated_tiles;
        result.components_without_static_attribution +=
            usize::from(!component.has_static_attribution());
        result.tiles_without_static_attribution +=
            component.tile_count() - component.static_attributed_tiles;
        result.components_without_positive_corroboration +=
            usize::from(!component.has_positive_corroboration());
        result.tiles_without_positive_corroboration +=
            component.tile_count() - component.positively_corroborated_tiles;
        result.components.push(component);
    }
    Ok(result)
}

fn interior_terrain_components(room: &Room) -> Vec<Vec<TerrainCell>> {
    let width = room.width();
    let height = room.height();
    let mut remaining = BTreeSet::new();
    if width < 3 || height < 3 {
        return Vec::new();
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            if matches!(room.tile(x, y), Some(Tile::Solid | Tile::OneWay)) {
                remaining.insert(TerrainCell { x, y });
            }
        }
    }
    let mut components = Vec::new();
    while let Some(first) = remaining.pop_first() {
        let mut cells = Vec::new();
        let mut frontier = vec![first];
        while let Some(cell) = frontier.pop() {
            cells.push(cell);
            for adjacent in terrain_neighbors(cell, width, height) {
                if remaining.remove(&adjacent) {
                    frontier.push(adjacent);
                }
            }
        }
        cells.sort_unstable();
        components.push(cells);
    }
    components
}

fn terrain_neighbors(cell: TerrainCell, width: u16, height: u16) -> Vec<TerrainCell> {
    let mut neighbors = Vec::with_capacity(4);
    if cell.x > 1 {
        neighbors.push(TerrainCell {
            x: cell.x - 1,
            y: cell.y,
        });
    }
    if cell.x + 2 < width {
        neighbors.push(TerrainCell {
            x: cell.x + 1,
            y: cell.y,
        });
    }
    if cell.y > 1 {
        neighbors.push(TerrainCell {
            x: cell.x,
            y: cell.y - 1,
        });
    }
    if cell.y + 2 < height {
        neighbors.push(TerrainCell {
            x: cell.x,
            y: cell.y + 1,
        });
    }
    neighbors
}

const fn support_contains(support: SupportSpec, cell: TerrainCell) -> bool {
    cell.y == support.row && cell.x >= support.start_x && cell.x < support.end_x
}

fn gate_envelope_contains(from: SupportSpec, to: SupportSpec, cell: TerrainCell) -> bool {
    let min_x = from.start_x.min(to.start_x).saturating_sub(1);
    let max_x = from.end_x.max(to.end_x);
    let min_y = from.row.min(to.row).saturating_sub(1);
    let max_y = from.row.max(to.row).saturating_add(1);
    (min_x..=max_x).contains(&cell.x) && (min_y..=max_y).contains(&cell.y)
}

fn traversal_near_cell(room: &Room, trace: &TraversalTrace, cell: TerrainCell) -> bool {
    let x = ((u32::from(cell.x) * 2 + 1) * u32::from(trace.grid.columns())
        / (u32::from(room.width()) * 2)) as u16;
    let y = ((u32::from(cell.y) * 2 + 1) * u32::from(trace.grid.rows())
        / (u32::from(room.height()) * 2)) as u16;
    trace
        .visited_cells
        .iter()
        .any(|visited| visited.x.abs_diff(x) <= 1 && visited.y.abs_diff(y) <= 1)
}

#[cfg(test)]
mod tests {
    use downwards_core::{BoundarySide, Door, Point, Rect};
    use downwards_gen::experimental::{ChallengeIntent, GenerationStrategy, generate_candidate};
    use downwards_gen::experimental::{RouteNode, SupportKind};
    use downwards_lab::{TraversalCell, TraversalGrid};

    use super::*;

    fn support(start_x: u16, end_x: u16, row: u16) -> SupportSpec {
        SupportSpec {
            start_x,
            end_x,
            row,
            kind: SupportKind::Solid,
        }
    }

    fn node(id: u16, role: NodeRole, support: SupportSpec) -> RouteNode {
        RouteNode { id, role, support }
    }

    fn edge(from: u16, to: u16, verb: RouteVerb) -> RouteEdge {
        RouteEdge {
            from,
            to,
            verb,
            critical: false,
        }
    }

    fn port(id: &str, node_id: u16) -> BoundaryPort {
        BoundaryPort {
            node_id,
            door: Door {
                id: id.to_owned(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 0, 10, 20),
                arrival: Point::new(10, 10),
                destination_room: None,
                destination_door: None,
            },
        }
    }

    #[test]
    fn baseline_path_beats_a_direct_ability_edge() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(1, 4, 10)),
                node(1, NodeRole::Landing, support(4, 7, 10)),
                node(2, NodeRole::Port, support(7, 10, 10)),
            ],
            edges: vec![
                edge(0, 2, RouteVerb::DashAcross),
                edge(0, 1, RouteVerb::Run),
                edge(1, 2, RouteVerb::Run),
            ],
        };
        let description =
            describe_port_path(&plan, &[port("a", 0), port("b", 2)], "a", "b").unwrap();

        assert_eq!(description.node_path, [0, 1, 2]);
        assert_eq!(description.cost.required_ability_edges, 0);
        assert_eq!(description.cost.edge_count, 2);
        assert_eq!(description.cost.verb_variety, 1);
        assert_eq!(description.selected_path_abilities, AbilitySet::NONE);
        assert_eq!(description.unavoidable_abilities, AbilitySet::NONE);
        assert!(description.one_edge_bypass);
        assert!(!description.direct_run_bypass);
        assert!(!description.direct_drop_bypass);
    }

    #[test]
    fn upward_gate_is_a_baseline_drop_in_the_reverse_direction() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(2, 5, 15)),
                node(1, NodeRole::Port, support(2, 5, 5)),
            ],
            edges: vec![edge(0, 1, RouteVerb::WallClimb)],
        };
        let ports = [port("bottom", 0), port("top", 1)];

        let ascent = describe_port_path(&plan, &ports, "bottom", "top").unwrap();
        assert_eq!(ascent.cost.required_ability_edges, 1);
        assert_eq!(ascent.selected_path_wall_jump_edges, 1);
        assert_eq!(ascent.unavoidable_abilities, AbilitySet::new(true, false));
        assert!(!ascent.direct_drop_bypass);

        let descent = describe_port_path(&plan, &ports, "top", "bottom").unwrap();
        assert_eq!(descent.cost.required_ability_edges, 0);
        assert_eq!(descent.steps[0].traversal_verb, RouteVerb::Drop);
        assert_eq!(descent.unavoidable_abilities, AbilitySet::NONE);
        assert!(descent.direct_drop_bypass);
        assert!(descent.one_edge_bypass);
    }

    #[test]
    fn reverse_of_a_small_overlapping_jump_is_an_explicit_drop() {
        let lower = SupportSpec {
            start_x: 2,
            end_x: 7,
            row: 10,
            kind: SupportKind::OneWay,
        };
        let upper = SupportSpec {
            start_x: 3,
            end_x: 6,
            row: 8,
            kind: SupportKind::OneWay,
        };
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, lower),
                node(1, NodeRole::Port, upper),
            ],
            edges: vec![edge(0, 1, RouteVerb::Jump)],
        };
        let ports = [port("lower", 0), port("upper", 1)];

        let descent = describe_port_path(&plan, &ports, "upper", "lower").unwrap();
        assert_eq!(descent.steps[0].traversal_verb, RouteVerb::Drop);
        assert!(descent.direct_drop_bypass);
        assert!(!descent.direct_run_bypass);
    }

    #[test]
    fn selected_ability_is_not_required_when_an_alternate_ability_path_exists() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(1, 3, 12)),
                node(1, NodeRole::Landing, support(5, 7, 6)),
                node(2, NodeRole::Landing, support(9, 11, 12)),
                node(3, NodeRole::Port, support(13, 15, 6)),
            ],
            edges: vec![
                edge(0, 1, RouteVerb::WallClimb),
                edge(1, 3, RouteVerb::Run),
                edge(0, 2, RouteVerb::DashAcross),
                edge(2, 3, RouteVerb::DashUp),
            ],
        };
        let description =
            describe_port_path(&plan, &[port("a", 0), port("b", 3)], "a", "b").unwrap();

        assert_eq!(
            description.selected_path_abilities,
            AbilitySet::new(true, false)
        );
        assert_eq!(description.unavoidable_abilities, AbilitySet::NONE);
    }

    #[test]
    fn an_ability_is_unavoidable_when_every_alternate_path_uses_it() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(1, 3, 12)),
                node(1, NodeRole::Landing, support(5, 7, 6)),
                node(2, NodeRole::Landing, support(9, 11, 6)),
                node(3, NodeRole::Port, support(13, 15, 2)),
            ],
            edges: vec![
                edge(0, 1, RouteVerb::WallClimb),
                edge(1, 3, RouteVerb::WallClimb),
                edge(0, 2, RouteVerb::WallClimb),
                edge(2, 3, RouteVerb::Run),
            ],
        };
        let description =
            describe_port_path(&plan, &[port("a", 0), port("b", 3)], "a", "b").unwrap();

        assert_eq!(
            description.unavoidable_abilities,
            AbilitySet::new(true, false)
        );
    }

    #[test]
    fn dash_across_remains_unavoidable_in_both_directions() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(1, 3, 8)),
                node(1, NodeRole::Port, support(12, 14, 8)),
            ],
            edges: vec![edge(0, 1, RouteVerb::DashAcross)],
        };
        let ports = [port("left", 0), port("right", 1)];

        for (source, target) in [("left", "right"), ("right", "left")] {
            let description = describe_port_path(&plan, &ports, source, target).unwrap();
            assert_eq!(
                description.unavoidable_abilities,
                AbilitySet::new(false, true)
            );
        }
    }

    #[test]
    fn dash_up_is_an_unavoidable_dash_only_in_its_declared_direction() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(4, 7, 9)),
                node(1, NodeRole::Port, support(4, 7, 8)),
            ],
            edges: vec![edge(0, 1, RouteVerb::DashUp)],
        };
        let ports = [port("bottom", 0), port("top", 1)];

        let ascent = describe_port_path(&plan, &ports, "bottom", "top").unwrap();
        assert_eq!(ascent.steps[0].traversal_verb, RouteVerb::DashUp);
        assert_eq!(ascent.unavoidable_abilities, AbilitySet::new(false, true));

        let descent = describe_port_path(&plan, &ports, "top", "bottom").unwrap();
        assert_eq!(descent.steps[0].traversal_verb, RouteVerb::Drop);
        assert_eq!(descent.unavoidable_abilities, AbilitySet::NONE);
    }

    #[test]
    fn path_ties_use_decisions_then_vertical_transitions_then_variety() {
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Port, support(1, 3, 10)),
                node(1, NodeRole::Landing, support(3, 5, 10)),
                node(2, NodeRole::Landing, support(3, 5, 8)),
                node(3, NodeRole::Port, support(5, 7, 10)),
                node(4, NodeRole::Landing, support(10, 12, 10)),
            ],
            edges: vec![
                edge(0, 1, RouteVerb::Run),
                edge(1, 3, RouteVerb::Run),
                edge(0, 2, RouteVerb::Jump),
                edge(2, 3, RouteVerb::Drop),
                // Makes node 1 a decision while leaving the alternate path
                // with the same edge count but vertical transitions.
                edge(1, 4, RouteVerb::Jump),
            ],
        };
        let description =
            describe_port_path(&plan, &[port("a", 0), port("b", 3)], "a", "b").unwrap();

        // Decision count precedes vertical transitions in the policy, so the
        // route via node 2 wins despite changing height twice.
        assert_eq!(description.node_path, [0, 2, 3]);
        assert_eq!(description.cost.decision_nodes, 0);
        assert_eq!(description.cost.vertical_transitions, 2);
        assert_eq!(description.cost.verb_variety, 2);
    }

    fn room_with_terrain() -> Room {
        let width: u16 = 32;
        let height: u16 = 18;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        let mut set = |x: u16, y: u16, tile: Tile| {
            tiles[usize::from(y) * usize::from(width) + usize::from(x)] = tile;
        };
        for x in 2..5 {
            set(x, 10, Tile::Solid);
        }
        for x in 8..10 {
            set(x, 8, Tile::OneWay);
        }
        set(20, 5, Tile::Solid);
        set(20, 6, Tile::Solid);
        Room::new(
            "terrain",
            "Terrain",
            width,
            height,
            10,
            tiles,
            Point::new(10, 10),
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn terrain_components_report_static_attribution_without_negative_claims() {
        let room = room_with_terrain();
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Landing, support(2, 5, 10)),
                node(1, NodeRole::Recovery, support(8, 10, 8)),
            ],
            edges: Vec::new(),
        };
        let description = describe_terrain_utility(&room, &plan, &[]).unwrap();

        assert_eq!(description.components.len(), 3);
        assert_eq!(description.interior_terrain_tiles, 7);
        assert_eq!(description.route_support_tiles, 5);
        assert_eq!(description.recovery_support_tiles, 2);
        assert_eq!(description.components_without_static_attribution, 1);
        assert_eq!(description.tiles_without_static_attribution, 2);
        assert_eq!(description.components_without_positive_corroboration, 1);
    }

    #[test]
    fn certified_trace_proximity_is_positive_evidence_for_an_unattributed_component() {
        let room = room_with_terrain();
        let plan = RoutePlan {
            nodes: vec![node(0, NodeRole::Landing, support(2, 5, 10))],
            edges: Vec::new(),
        };
        let trace = TraversalTrace {
            grid: TraversalGrid::new(32, 18).unwrap(),
            sample_count: 1,
            spans: Box::new([]),
            visited_cells: Box::new([TraversalCell { x: 20, y: 5 }]),
        };
        let description = describe_terrain_utility(&room, &plan, &[&trace]).unwrap();
        let isolated = description
            .components
            .iter()
            .find(|component| component.bounds.min_x == 20)
            .unwrap();

        assert!(!isolated.has_static_attribution());
        assert!(isolated.has_positive_corroboration());
        assert_eq!(isolated.nearby_certified_traversal_indices, [0]);
        assert_eq!(isolated.certified_traversal_near_tiles, 2);
        assert!(isolated.certified_traversal_near_tiles <= isolated.tile_count());
        assert!(description.certified_traversal_near_tiles <= description.interior_terrain_tiles);
        assert_eq!(description.components_without_positive_corroboration, 1);
    }

    #[test]
    fn ability_gate_envelope_can_attribute_non_support_wall_tiles() {
        let width: u16 = 32;
        let height: u16 = 18;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        for y in 5_u16..=12 {
            tiles[usize::from(y) * usize::from(width) + 5] = Tile::Solid;
        }
        let room = Room::new(
            "gate",
            "Gate",
            width,
            height,
            10,
            tiles,
            Point::new(10, 10),
            Vec::new(),
        )
        .unwrap();
        let plan = RoutePlan {
            nodes: vec![
                node(0, NodeRole::Landing, support(2, 5, 13)),
                node(1, NodeRole::Landing, support(5, 8, 4)),
            ],
            edges: vec![edge(0, 1, RouteVerb::WallClimb)],
        };
        let description = describe_terrain_utility(&room, &plan, &[]).unwrap();

        assert_eq!(description.components.len(), 1);
        assert_eq!(description.ability_gate_envelope_tiles, 8);
        assert_eq!(description.static_attributed_tiles, 8);
        assert_eq!(description.components[0].ability_gate_edge_indices, [0]);
    }

    #[test]
    fn generated_candidates_satisfy_both_descriptor_contracts() {
        let mut successes = 0;
        for strategy in GenerationStrategy::ALL {
            for intent in ChallengeIntent::ALL {
                for seed in 0..8 {
                    let Ok(candidate) = generate_candidate(seed, AbilitySet::ALL, strategy, intent)
                    else {
                        continue;
                    };
                    successes += 1;
                    for source in &candidate.boundary_ports {
                        for target in &candidate.boundary_ports {
                            if source.door.id == target.door.id {
                                continue;
                            }
                            describe_ordered_port_path(
                                &candidate,
                                &source.door.id,
                                &target.door.id,
                            )
                            .unwrap();
                        }
                    }
                    let terrain = describe_candidate_terrain(&candidate, &[]).unwrap();
                    assert!(terrain.static_attributed_tiles <= terrain.interior_terrain_tiles);
                    assert!(
                        terrain.positively_corroborated_tiles <= terrain.interior_terrain_tiles
                    );
                }
            }
        }
        assert!(successes > 0);
    }
}
