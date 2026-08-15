use std::collections::HashSet;

use downwards_core::{Door, Pickup, Point, Rect, Room, RoomError, Tile, TimedHazard};

use crate::{GenerationStats, ROOM_HEIGHT, ROOM_WIDTH, TILE_SIZE};

pub(crate) const FLOOR_ROW: u16 = ROOM_HEIGHT - 1;
pub(crate) const WALKING_ROW: u16 = FLOOR_ROW - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeRole {
    Port,
    Start,
    Exit,
    Landing,
    Junction,
    Pickup,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RouteVerb {
    Run,
    Jump,
    Drop,
    WallClimb,
    DashAcross,
    DashUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SupportKind {
    Solid,
    OneWay,
}

impl SupportKind {
    pub(crate) const fn tile(self) -> Tile {
        match self {
            Self::Solid => Tile::Solid,
            Self::OneWay => Tile::OneWay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SupportSpec {
    pub start_x: u16,
    pub end_x: u16,
    pub row: u16,
    pub kind: SupportKind,
}

impl SupportSpec {
    #[must_use]
    pub const fn width(self) -> u16 {
        self.end_x - self.start_x
    }

    #[must_use]
    pub const fn center_x(self) -> u16 {
        self.start_x + self.width() / 2
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RouteNode {
    pub id: u16,
    pub role: NodeRole,
    pub support: SupportSpec,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RouteEdge {
    pub from: u16,
    pub to: u16,
    pub verb: RouteVerb,
    pub critical: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RoutePlan {
    pub nodes: Vec<RouteNode>,
    pub edges: Vec<RouteEdge>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RoutePlanSummary {
    pub node_count: u16,
    pub edge_count: u16,
    pub port_count: u16,
    pub cycle_rank: u16,
    pub branch_nodes: u16,
    pub vertical_span_rows: u16,
    pub wall_edges: u16,
    pub dash_edges: u16,
    pub signature: u64,
}

impl RoutePlan {
    #[must_use]
    pub fn summary(&self) -> RoutePlanSummary {
        let mut degree = vec![0_u16; self.nodes.len()];
        let mut wall_edges = 0_u16;
        let mut dash_edges = 0_u16;
        let mut signature = Signature::new();
        for node in &self.nodes {
            signature.byte(node.role as u8);
            signature.u16(node.support.start_x);
            signature.u16(node.support.end_x);
            signature.u16(node.support.row);
            signature.byte(node.support.kind as u8);
        }
        for edge in &self.edges {
            if let Some(value) = degree.get_mut(usize::from(edge.from)) {
                *value = value.saturating_add(1);
            }
            if let Some(value) = degree.get_mut(usize::from(edge.to)) {
                *value = value.saturating_add(1);
            }
            wall_edges += u16::from(edge.verb == RouteVerb::WallClimb);
            dash_edges += u16::from(matches!(
                edge.verb,
                RouteVerb::DashAcross | RouteVerb::DashUp
            ));
            signature.u16(edge.from);
            signature.u16(edge.to);
            signature.byte(edge.verb as u8);
            signature.byte(u8::from(edge.critical));
        }
        let min_row = self
            .nodes
            .iter()
            .map(|node| node.support.row)
            .min()
            .unwrap_or_default();
        let max_row = self
            .nodes
            .iter()
            .map(|node| node.support.row)
            .max()
            .unwrap_or_default();
        let components = connected_components(self);
        let cycle_rank = self
            .edges
            .len()
            .saturating_add(components)
            .saturating_sub(self.nodes.len());
        RoutePlanSummary {
            node_count: self.nodes.len().try_into().unwrap_or(u16::MAX),
            edge_count: self.edges.len().try_into().unwrap_or(u16::MAX),
            port_count: self
                .nodes
                .iter()
                .filter(|node| node.role == NodeRole::Port)
                .count()
                .try_into()
                .unwrap_or(u16::MAX),
            cycle_rank: cycle_rank.try_into().unwrap_or(u16::MAX),
            branch_nodes: degree
                .into_iter()
                .filter(|&value| value >= 3)
                .count()
                .try_into()
                .unwrap_or(u16::MAX),
            vertical_span_rows: max_row.saturating_sub(min_row),
            wall_edges,
            dash_edges,
            signature: signature.finish(),
        }
    }
}

fn connected_components(plan: &RoutePlan) -> usize {
    let mut remaining = (0..plan.nodes.len()).collect::<HashSet<_>>();
    let mut components = 0;
    while let Some(&first) = remaining.iter().next() {
        components += 1;
        remaining.remove(&first);
        let mut stack = vec![first];
        while let Some(node) = stack.pop() {
            for edge in &plan.edges {
                let adjacent = if usize::from(edge.from) == node {
                    Some(usize::from(edge.to))
                } else if usize::from(edge.to) == node {
                    Some(usize::from(edge.from))
                } else {
                    None
                };
                if let Some(adjacent) = adjacent
                    && remaining.remove(&adjacent)
                {
                    stack.push(adjacent);
                }
            }
        }
    }
    components
}

/// A physical boundary door and the route-plan node it attaches to.
///
/// Keeping this association explicit lets offline validation relate a failed
/// door pair to the generated graph rather than reverse-engineering geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundaryPort {
    pub node_id: u16,
    pub door: Door,
}

pub struct CandidateParts {
    pub(crate) draft: RoomDraft,
    pub(crate) spawn: Point,
    pub boundary_ports: Vec<BoundaryPort>,
    pub route_plan: RoutePlan,
}

pub(crate) struct RoomDraft {
    tiles: Vec<Tile>,
    timed_hazards: Vec<TimedHazard>,
    pickups: Vec<Pickup>,
    hazard_clusters: u16,
}

impl RoomDraft {
    pub(crate) fn new() -> Self {
        let mut draft = Self {
            tiles: vec![Tile::Empty; usize::from(ROOM_WIDTH) * usize::from(ROOM_HEIGHT)],
            timed_hazards: Vec::new(),
            pickups: Vec::new(),
            hazard_clusters: 0,
        };
        draft.line(0, ROOM_WIDTH, 0, Tile::Solid);
        draft.line(0, ROOM_WIDTH, FLOOR_ROW, Tile::Solid);
        for row in 1..FLOOR_ROW {
            draft.set(0, row, Tile::Solid);
            draft.set(ROOM_WIDTH - 1, row, Tile::Solid);
        }
        draft
    }

    pub(crate) fn platform(&mut self, support: SupportSpec) {
        debug_assert!(support.start_x > 0);
        debug_assert!(support.start_x < support.end_x && support.end_x < ROOM_WIDTH);
        debug_assert!((1..FLOOR_ROW).contains(&support.row));
        self.line(
            support.start_x,
            support.end_x,
            support.row,
            support.kind.tile(),
        );
    }

    /// Clear only boundary tiles intersecting a door trigger.
    ///
    /// Door triggers extend inward so collision-free rooms can activate them
    /// before the player leaves the viewport. Carving the boundary portion
    /// makes the connection visually read as an opening while retaining every
    /// interior support or wall tile.
    pub(crate) fn carve_boundary(&mut self, bounds: Rect) {
        for y in 0..ROOM_HEIGHT {
            for x in 0..ROOM_WIDTH {
                if x != 0 && x != ROOM_WIDTH - 1 && y != 0 && y != FLOOR_ROW {
                    continue;
                }
                let tile_bounds = Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(y) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
                if bounds.intersects(tile_bounds) {
                    let index = usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x);
                    self.tiles[index] = Tile::Empty;
                }
            }
        }
    }

    /// Clear the shallow interior of a ceiling aperture.
    ///
    /// Partition experiments use a closed finite ceiling/floor socket grid.
    /// A recursive collision wall can otherwise intersect a chosen ceiling
    /// socket even though its boundary tile was carved. Clearing exactly the
    /// two interior rows occupied by a door arrival preserves the partition
    /// below while making the validated aperture and arrival agree.
    pub(crate) fn carve_ceiling_aperture(&mut self, bounds: Rect) {
        let shaft = Rect::new(bounds.x, 0, bounds.width, 3 * TILE_SIZE);
        for y in 0_u16..3 {
            for x in 0_u16..ROOM_WIDTH {
                let tile_bounds = Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(y) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
                if shaft.intersects(tile_bounds) {
                    let index = usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x);
                    self.tiles[index] = Tile::Empty;
                }
            }
        }
    }

    pub(crate) fn solid_column(&mut self, x: u16, start_row: u16, end_row: u16) {
        debug_assert!(x > 0 && x < ROOM_WIDTH - 1);
        debug_assert!(start_row < end_row && end_row <= FLOOR_ROW);
        for row in start_row..end_row {
            self.set(x, row, Tile::Solid);
        }
    }

    /// Remove both hazard layers while returning their exact former sizes.
    ///
    /// This is used only by the opt-in terrain-constraint experiment.  It is
    /// intentionally not part of feature staging, whose legacy transform is
    /// frozen independently in `v6`.
    pub(crate) fn clear_hazards(&mut self) -> (u16, u16) {
        let mut static_tiles = 0_u16;
        for tile in &mut self.tiles {
            if tile.is_hazard() {
                *tile = Tile::Empty;
                static_tiles = static_tiles.saturating_add(1);
            }
        }
        let timed = self.timed_hazards.len().try_into().unwrap_or(u16::MAX);
        self.timed_hazards.clear();
        self.hazard_clusters = 0;
        (static_tiles, timed)
    }

    /// Ground a narrow slice of an existing route support into the boundary
    /// floor.  The support itself remains the authored landing; the pier only
    /// closes the otherwise universal floor bypass beneath it.
    pub(crate) fn ground_support_pier(&mut self, support: SupportSpec, start_x: u16, end_x: u16) {
        debug_assert!(support.start_x <= start_x);
        debug_assert!(start_x < end_x && end_x <= support.end_x);
        for x in start_x..end_x {
            self.solid_column(x, support.row, FLOOR_ROW);
        }
    }

    pub(crate) fn hazard_run(&mut self, start_x: u16, end_x: u16) {
        debug_assert!(start_x > 0 && start_x < end_x && end_x < ROOM_WIDTH);
        for x in start_x..end_x {
            self.set(x, WALKING_ROW, Tile::HazardUp);
        }
        self.hazard_clusters = self.hazard_clusters.saturating_add(1);
    }

    pub(crate) fn hazard_floor_except(&mut self, safe_ranges: &[(u16, u16)]) {
        let mut in_hazard = false;
        for x in 1..ROOM_WIDTH - 1 {
            let safe = safe_ranges
                .iter()
                .any(|&(start, end)| (start..end).contains(&x));
            if safe {
                in_hazard = false;
                continue;
            }
            self.set(x, WALKING_ROW, Tile::HazardUp);
            if !in_hazard {
                self.hazard_clusters = self.hazard_clusters.saturating_add(1);
                in_hazard = true;
            }
        }
    }

    pub(crate) fn pickup_above(&mut self, id: &str, support: SupportSpec) {
        let x = i32::from(support.center_x()) * TILE_SIZE - 3;
        let bounds = Rect::new(x, i32::from(support.row) * TILE_SIZE - 18, 6, 6);
        self.pickups.push(
            Pickup::new(id, bounds)
                .expect("experimental pickup identifiers and bounds remain valid"),
        );
    }

    pub(crate) fn try_timed_hazard(&mut self, bounds: Rect, rng: &mut StableRng) -> bool {
        if self.rect_hits_tiles(bounds)
            || self
                .pickups
                .iter()
                .any(|pickup| pickup.bounds().intersects(bounds))
        {
            return false;
        }
        let period_ticks = 150 + u32::from(rng.below(6)) * 30;
        let active_ticks = 24 + u32::from(rng.below(5)) * 8;
        let inactive_ticks = period_ticks - active_ticks;
        let phase_ticks = active_ticks + rng.below_u32(inactive_ticks);
        let Ok(hazard) = TimedHazard::new(bounds, period_ticks, active_ticks, phase_ticks) else {
            return false;
        };
        self.timed_hazards.push(hazard);
        true
    }

    pub(crate) fn tile(&self, x: u16, y: u16) -> Tile {
        self.tiles[usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x)]
    }

    pub(crate) fn is_empty(&self, x: u16, y: u16) -> bool {
        self.tile(x, y) == Tile::Empty
    }

    pub(crate) fn stats(&self, route_waypoints: u16) -> GenerationStats {
        let mut solid_tiles = 0_u16;
        let mut boundary_solid_tiles = 0_u16;
        let mut one_way_tiles = 0_u16;
        let mut hazard_tiles = 0_u16;
        for y in 0..ROOM_HEIGHT {
            for x in 0..ROOM_WIDTH {
                match self.tile(x, y) {
                    Tile::Solid => {
                        solid_tiles = solid_tiles.saturating_add(1);
                        if x == 0 || x == ROOM_WIDTH - 1 || y == 0 || y == FLOOR_ROW {
                            boundary_solid_tiles = boundary_solid_tiles.saturating_add(1);
                        }
                    }
                    Tile::OneWay => one_way_tiles = one_way_tiles.saturating_add(1),
                    Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight => {
                        hazard_tiles = hazard_tiles.saturating_add(1)
                    }
                    Tile::Empty => {}
                }
            }
        }
        GenerationStats {
            solid_tiles,
            boundary_solid_tiles,
            interior_solid_tiles: solid_tiles - boundary_solid_tiles,
            one_way_tiles,
            hazard_tiles,
            hazard_clusters: self.hazard_clusters,
            timed_hazards: self.timed_hazards.len().try_into().unwrap_or(u16::MAX),
            pickups: self.pickups.len().try_into().unwrap_or(u16::MAX),
            route_waypoints,
        }
    }

    pub(crate) fn finish_without_exits(
        self,
        id: String,
        name: String,
        spawn: Point,
    ) -> Result<downwards_core::Room, RoomError> {
        Room::new(
            id,
            name,
            ROOM_WIDTH,
            ROOM_HEIGHT,
            TILE_SIZE,
            self.tiles,
            spawn,
            Vec::new(),
        )?
        .with_objects(self.timed_hazards, self.pickups)
    }

    fn line(&mut self, start_x: u16, end_x: u16, row: u16, tile: Tile) {
        for x in start_x..end_x {
            self.set(x, row, tile);
        }
    }

    fn set(&mut self, x: u16, y: u16, tile: Tile) {
        let index = usize::from(y) * usize::from(ROOM_WIDTH) + usize::from(x);
        let existing = self.tiles[index];
        self.tiles[index] = match (existing, tile) {
            (Tile::Solid, _) | (_, Tile::Solid) => Tile::Solid,
            (Tile::OneWay, replacement) if replacement.is_hazard() => Tile::OneWay,
            (_, replacement) => replacement,
        };
    }

    fn rect_hits_tiles(&self, bounds: Rect) -> bool {
        (0..ROOM_HEIGHT).any(|y| {
            (0..ROOM_WIDTH).any(|x| {
                self.tile(x, y) != Tile::Empty
                    && bounds.intersects(Rect::new(
                        i32::from(x) * TILE_SIZE,
                        i32::from(y) * TILE_SIZE,
                        TILE_SIZE,
                        TILE_SIZE,
                    ))
            })
        })
    }
}

pub(crate) fn ground_spawn(tile_x: u16) -> Point {
    Point::new(
        i32::from(tile_x) * TILE_SIZE,
        i32::from(FLOOR_ROW) * TILE_SIZE - downwards_core::PLAYER_HEIGHT,
    )
}

pub(crate) fn mirrored_support(mut support: SupportSpec, mirrored: bool) -> SupportSpec {
    if mirrored {
        let start = ROOM_WIDTH - support.end_x;
        let end = ROOM_WIDTH - support.start_x;
        support.start_x = start;
        support.end_x = end;
    }
    support
}

pub(crate) fn mirrored_spawn(tile_x: u16, mirrored: bool) -> Point {
    ground_spawn(if mirrored {
        ROOM_WIDTH - tile_x - 1
    } else {
        tile_x
    })
}

pub(crate) fn add_route_node(plan: &mut RoutePlan, role: NodeRole, support: SupportSpec) -> u16 {
    let id = plan.nodes.len().try_into().expect("route plan fits u16");
    plan.nodes.push(RouteNode { id, role, support });
    id
}

pub(crate) fn add_route_edge(
    plan: &mut RoutePlan,
    from: u16,
    to: u16,
    verb: RouteVerb,
    critical: bool,
) {
    plan.edges.push(RouteEdge {
        from,
        to,
        verb,
        critical,
    });
}

pub(crate) fn edge_verb(
    from: SupportSpec,
    to: SupportSpec,
    abilities: downwards_core::AbilitySet,
) -> RouteVerb {
    let rise_rows = from.row.saturating_sub(to.row);
    let horizontal_gap = to
        .start_x
        .saturating_sub(from.end_x)
        .max(from.start_x.saturating_sub(to.end_x));
    if abilities.dash && rise_rows >= 4 {
        RouteVerb::DashUp
    } else if abilities.dash && horizontal_gap >= 4 {
        RouteVerb::DashAcross
    } else if to.row.saturating_sub(from.row) > 2 {
        RouteVerb::Drop
    } else if rise_rows > 0 || horizontal_gap > 0 {
        RouteVerb::Jump
    } else {
        RouteVerb::Run
    }
}

/// Frozen conservative envelope for a baseline support-to-support transfer.
///
/// This is a construction contract, not the authoritative physics proof.
/// Every generated edge still needs replay evidence.  In particular, callers
/// must not use [`edge_verb`] alone as reachability evidence: that function
/// classifies an already accepted edge and deliberately does not reject a
/// distant landing.
pub(crate) const BASELINE_MAX_RISE_ROWS: u16 = 2;
pub(crate) const BASELINE_MAX_SUPPORT_GAP_TILES: u16 = 3;

#[must_use]
pub(crate) const fn horizontal_support_gap(first: SupportSpec, second: SupportSpec) -> u16 {
    if first.end_x < second.start_x {
        second.start_x - first.end_x
    } else {
        first.start_x.saturating_sub(second.end_x)
    }
}

/// Classify one directed transfer only when it lies inside the conservative
/// baseline envelope.
#[must_use]
pub(crate) fn conservative_baseline_transition(
    from: SupportSpec,
    to: SupportSpec,
) -> Option<RouteVerb> {
    if from == to
        || from.row.saturating_sub(to.row) > BASELINE_MAX_RISE_ROWS
        || horizontal_support_gap(from, to) > BASELINE_MAX_SUPPORT_GAP_TILES
    {
        return None;
    }
    let rises = from.row > to.row;
    let descends = from.row < to.row;
    let overlaps_horizontally = from.start_x < to.end_x && to.start_x < from.end_x;
    // Solid landings cannot be crossed from their underside. One-way
    // supports intentionally permit the ordinary platformer ascent.
    if rises && overlaps_horizontally && to.kind == SupportKind::Solid {
        return None;
    }
    // Descending onto an overlapping support cannot be achieved by merely
    // running: the player must intentionally drop through the source. Solid
    // sources do not support that operation, so reject them rather than
    // pretending the pair is reversible.
    if descends && overlaps_horizontally {
        return (from.kind == SupportKind::OneWay).then_some(RouteVerb::Drop);
    }
    Some(edge_verb(from, to, downwards_core::AbilitySet::NONE))
}

/// Whether the same physical support pair has a conservative baseline
/// transfer in both directions.
#[must_use]
pub(crate) fn reversible_baseline_transition(first: SupportSpec, second: SupportSpec) -> bool {
    conservative_baseline_transition(first, second).is_some()
        && conservative_baseline_transition(second, first).is_some()
}

pub(crate) struct StableRng {
    state: u64,
}

impl StableRng {
    pub(crate) const fn new(seed: u64, stream: u64) -> Self {
        Self {
            state: seed ^ stream.wrapping_mul(0xd6e8_feb8_6659_fd93),
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    pub(crate) fn below(&mut self, upper_exclusive: u16) -> u16 {
        debug_assert!(upper_exclusive > 0);
        (self.next_u64() % u64::from(upper_exclusive)) as u16
    }

    pub(crate) fn below_u32(&mut self, upper_exclusive: u32) -> u32 {
        debug_assert!(upper_exclusive > 0);
        (self.next_u64() % u64::from(upper_exclusive)) as u32
    }

    pub(crate) fn between(&mut self, inclusive_min: u16, inclusive_max: u16) -> u16 {
        debug_assert!(inclusive_min <= inclusive_max);
        inclusive_min + self.below(inclusive_max - inclusive_min + 1)
    }

    pub(crate) fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

struct Signature(u64);

impl Signature {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn u16(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn support(start_x: u16, end_x: u16, row: u16, kind: SupportKind) -> SupportSpec {
        SupportSpec {
            start_x,
            end_x,
            row,
            kind,
        }
    }

    #[test]
    fn conservative_baseline_contract_checks_horizontal_and_vertical_geometry() {
        let lower = support(1, 5, 10, SupportKind::OneWay);
        let valid_upper = support(8, 12, 8, SupportKind::OneWay);
        assert_eq!(horizontal_support_gap(lower, valid_upper), 3);
        assert_eq!(
            conservative_baseline_transition(lower, valid_upper),
            Some(RouteVerb::Jump)
        );
        assert!(reversible_baseline_transition(lower, valid_upper));

        let too_far = support(9, 13, 8, SupportKind::OneWay);
        assert_eq!(horizontal_support_gap(lower, too_far), 4);
        assert_eq!(conservative_baseline_transition(lower, too_far), None);

        let too_high = support(8, 12, 7, SupportKind::OneWay);
        assert_eq!(conservative_baseline_transition(lower, too_high), None);
    }

    #[test]
    fn conservative_baseline_contract_rejects_solid_underside_overlap() {
        let lower = support(3, 8, 10, SupportKind::OneWay);
        let solid_upper = support(4, 9, 8, SupportKind::Solid);
        assert_eq!(conservative_baseline_transition(lower, solid_upper), None);

        let one_way_upper = support(4, 9, 8, SupportKind::OneWay);
        assert_eq!(
            conservative_baseline_transition(lower, one_way_upper),
            Some(RouteVerb::Jump)
        );
        assert!(reversible_baseline_transition(lower, one_way_upper));
    }

    #[test]
    fn conservative_baseline_contract_records_small_overlapping_drop_throughs() {
        let upper_one_way = support(4, 9, 8, SupportKind::OneWay);
        let lower = support(3, 10, 10, SupportKind::OneWay);
        assert_eq!(
            conservative_baseline_transition(upper_one_way, lower),
            Some(RouteVerb::Drop)
        );
        assert_eq!(
            conservative_baseline_transition(lower, upper_one_way),
            Some(RouteVerb::Jump)
        );
        assert!(reversible_baseline_transition(upper_one_way, lower));

        let upper_solid = support(4, 9, 8, SupportKind::Solid);
        assert_eq!(conservative_baseline_transition(upper_solid, lower), None);
        assert!(!reversible_baseline_transition(upper_solid, lower));
    }
}
