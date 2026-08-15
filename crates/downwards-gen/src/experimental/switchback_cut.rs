//! Independent terrain-only shelf-return room experiment.
//!
//! The compositional v6 rooms can look vertically elaborate while their
//! continuous boundary floor remains an easier route. This grammar starts
//! from a collision cut instead: a long solid shelf joins the entry-side wall
//! and leaves its only opening away from both the entry and upper route. An
//! ascent must therefore travel out through the opening, then reverse back
//! over the shelf. Descending follows the same cut with drops instead of the
//! authored ascent jumps, preserving useful directional asymmetry.
//!
//! This is deliberately not a v6 strategy or feature transform. Its typed key
//! is a complete regeneration identity and its room IDs/version namespace are
//! independent from legacy [`crate::StagedCompositionalKey`] artifacts.

use std::{collections::HashSet, error::Error, fmt};

use downwards_core::{
    AbilitySet, BoundarySide, Door, DoorError, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect, RoomError,
};

use super::{
    BoundaryPort, ChallengeIntent, NodeRole, RoutePlan, RoutePlanSummary, RouteVerb, SupportKind,
    SupportSpec,
    common::{
        FLOOR_ROW, RoomDraft, StableRng, add_route_edge, add_route_node, mirrored_spawn,
        mirrored_support,
    },
};
use crate::{
    AbilityTier, GeneratedLevel, GeneratedMetadata, LayoutFamily, ROOM_HEIGHT, ROOM_WIDTH,
    TILE_SIZE,
};

/// Version of the independent switchback-cut seed-to-room mapping.
pub const SWITCHBACK_CUT_GENERATION_VERSION: u32 = 1;

/// Every accepted retry index has a stable, independently regenerable map.
///
/// Generation never silently advances this value. Offline overgeneration may
/// try another index, but the selected index must be retained in the key.
pub const SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT: u8 = 15;

const RNG_STREAM: u64 = 0x5357_4954_4348_4231;
const SIDE_DOOR_DEPTH: i32 = 12;
const CEILING_DOOR_DEPTH: i32 = 12;
const DOOR_SPAN: i32 = 20;
const CUT_ROWS: [u16; 1] = [14];
const CEILING_SUPPORT_ROW: u16 = 3;

/// Frozen physical grammar encoded by [`SwitchbackCutKey`].
///
/// Future rewrites add variants rather than changing an existing seed map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SwitchbackCutGrammar {
    /// One side-anchored cut forces an out-and-back shelf traversal.
    ShelfReturnV1,
}

impl SwitchbackCutGrammar {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::ShelfReturnV1 => "shelf-return-v1",
        }
    }
}

/// Complete stable identity for one switchback-cut room.
///
/// `embedding_attempt` is an explicit deterministic salt, not an implicit
/// retry counter. This lets a corpus record exactly which alternate embedding
/// was selected without depending on a search policy or a v6 source key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SwitchbackCutKey {
    pub source_seed: u64,
    pub construction_abilities: AbilitySet,
    pub intent: ChallengeIntent,
    pub grammar: SwitchbackCutGrammar,
    pub embedding_attempt: u8,
}

impl SwitchbackCutKey {
    #[must_use]
    pub const fn new(
        source_seed: u64,
        construction_abilities: AbilitySet,
        intent: ChallengeIntent,
    ) -> Self {
        Self {
            source_seed,
            construction_abilities,
            intent,
            grammar: SwitchbackCutGrammar::ShelfReturnV1,
            embedding_attempt: 0,
        }
    }

    /// Select an exact grammar and retry identity without changing the seed.
    #[must_use]
    pub const fn with_embedding(
        mut self,
        grammar: SwitchbackCutGrammar,
        embedding_attempt: u8,
    ) -> Self {
        self.grammar = grammar;
        self.embedding_attempt = embedding_attempt;
        self
    }

    /// Regenerate precisely this key without fallback or selection.
    pub fn regenerate(self) -> Result<SwitchbackCutCandidate, SwitchbackCutGenerationError> {
        generate_switchback_cut(self)
    }
}

/// Constructive facts about the realized shelf-return embedding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchbackCutSummary {
    pub generation_version: u32,
    pub mirrored: bool,
    pub cut_shelves: u16,
    pub boundary_ports: u16,
    pub authored_return_reversals: u16,
    pub pickup_node_id: u16,
    pub interior_terrain_tiles: u16,
    pub route_signature: u64,
}

/// One structurally valid terrain-only experiment candidate.
///
/// Solver acceptance is intentionally not implied. Construction-loadout and
/// complete-kit all-pairs/pickup evidence are mandatory offline gates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchbackCutCandidate {
    pub key: SwitchbackCutKey,
    pub generated: GeneratedLevel,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
    pub summary: SwitchbackCutSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchbackCutFailure {
    UnsupportedEmbeddingAttempt { maximum: u8, actual: u8 },
    PortContract(String),
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for SwitchbackCutFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEmbeddingAttempt { maximum, actual } => write!(
                formatter,
                "embedding attempt {actual} exceeds the frozen maximum {maximum}"
            ),
            Self::PortContract(detail) => write!(formatter, "port contract failed: {detail}"),
            Self::Room(error) => write!(formatter, "generated room was invalid: {error}"),
            Self::Door(error) => write!(formatter, "generated room doors were invalid: {error}"),
        }
    }
}

impl Error for SwitchbackCutFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Room(error) => Some(error),
            Self::Door(error) => Some(error),
            Self::UnsupportedEmbeddingAttempt { .. } | Self::PortContract(_) => None,
        }
    }
}

/// Failure bound to the exact key that was requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchbackCutGenerationError {
    pub key: SwitchbackCutKey,
    pub cause: SwitchbackCutFailure,
}

impl fmt::Display for SwitchbackCutGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "switchback cut v{} {} {} {} attempt {} seed {:016x} failed: {}",
            SWITCHBACK_CUT_GENERATION_VERSION,
            self.key.grammar.slug(),
            ability_slug(self.key.construction_abilities),
            self.key.intent.slug(),
            self.key.embedding_attempt,
            self.key.source_seed,
            self.cause,
        )
    }
}

impl Error for SwitchbackCutGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Construct one exact terrain-only shelf-return room.
pub fn generate_switchback_cut(
    key: SwitchbackCutKey,
) -> Result<SwitchbackCutCandidate, SwitchbackCutGenerationError> {
    generate(key).map_err(|cause| SwitchbackCutGenerationError { key, cause })
}

fn generate(key: SwitchbackCutKey) -> Result<SwitchbackCutCandidate, SwitchbackCutFailure> {
    if key.embedding_attempt > SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT {
        return Err(SwitchbackCutFailure::UnsupportedEmbeddingAttempt {
            maximum: SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT,
            actual: key.embedding_attempt,
        });
    }

    let attempt_seed =
        key.source_seed ^ u64::from(key.embedding_attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut rng = StableRng::new(attempt_seed, RNG_STREAM);
    let mirrored = rng.coin();
    // A wide opening keeps the out-and-back traversal within the
    // authoritative 600-tick path horizon while the shelf remains long
    // enough that its easiest ascent must leave and then return over it.
    let gap_widths = std::array::from_fn::<_, 1, _>(|_| 19 + rng.below(3));

    let bottom_support = mirrored_support(
        SupportSpec {
            start_x: 1,
            end_x: 6,
            row: FLOOR_ROW,
            kind: SupportKind::Solid,
        },
        mirrored,
    );
    let shelves = CUT_ROWS.map(|row| {
        let index = CUT_ROWS
            .iter()
            .position(|&candidate| candidate == row)
            .expect("cut row belongs to the fixed cut array");
        let gap = gap_widths[index];
        let support = if index % 2 == 0 {
            SupportSpec {
                start_x: 1,
                end_x: ROOM_WIDTH - gap,
                row,
                kind: SupportKind::Solid,
            }
        } else {
            SupportSpec {
                start_x: gap,
                end_x: ROOM_WIDTH - 1,
                row,
                kind: SupportKind::Solid,
            }
        };
        mirrored_support(support, mirrored)
    });
    let upper_ascent = [12, 10, 8, 6, 4]
        .map(|row| SupportSpec {
            start_x: 3,
            end_x: 7,
            row,
            kind: SupportKind::OneWay,
        })
        .map(|support| mirrored_support(support, mirrored));
    let ceiling_support = mirrored_support(
        SupportSpec {
            start_x: 1,
            end_x: 5,
            row: CEILING_SUPPORT_ROW,
            kind: SupportKind::OneWay,
        },
        mirrored,
    );

    let mut draft = RoomDraft::new();
    let mut route_plan = RoutePlan::default();
    let bottom_id = add_route_node(&mut route_plan, NodeRole::Port, bottom_support);
    let mut previous_id = bottom_id;
    let mut shelf_ids = [0_u16; CUT_ROWS.len()];
    for (index, support) in shelves.into_iter().enumerate() {
        // The solid shelf is the collision cut. A short one-way step wholly
        // inside its opening splits the around-the-lip ascent into a
        // conservative two-row rise and a one-row rise. It cannot bypass the
        // cut because it never crosses the shelf's sealed horizontal span.
        let transfer = opening_transfer(support);
        draft.platform(transfer);
        let transfer_id = add_route_node(&mut route_plan, NodeRole::Landing, transfer);
        add_route_edge(
            &mut route_plan,
            previous_id,
            transfer_id,
            RouteVerb::Jump,
            true,
        );
        draft.platform(support);
        let node_id = add_route_node(&mut route_plan, NodeRole::Landing, support);
        shelf_ids[index] = node_id;
        add_route_edge(&mut route_plan, transfer_id, node_id, RouteVerb::Jump, true);
        previous_id = node_id;
    }
    let mut upper_ids = [0_u16; 5];
    for (index, support) in upper_ascent.into_iter().enumerate() {
        draft.platform(support);
        let node_id = add_route_node(&mut route_plan, NodeRole::Landing, support);
        upper_ids[index] = node_id;
        add_route_edge(&mut route_plan, previous_id, node_id, RouteVerb::Jump, true);
        previous_id = node_id;
    }
    draft.platform(ceiling_support);
    let ceiling_id = add_route_node(&mut route_plan, NodeRole::Port, ceiling_support);
    add_route_edge(
        &mut route_plan,
        previous_id,
        ceiling_id,
        RouteVerb::Jump,
        true,
    );

    let requested_ports = match key.intent {
        ChallengeIntent::Gentle => 2,
        ChallengeIntent::Standard => 3,
        ChallengeIntent::Technical => 4,
    };
    if requested_ports >= 3 {
        route_plan.nodes[usize::from(shelf_ids[0])].role = NodeRole::Port;
    }
    let far_bottom = mirrored_support(
        SupportSpec {
            start_x: 26,
            end_x: 31,
            row: FLOOR_ROW,
            kind: SupportKind::Solid,
        },
        mirrored,
    );
    let far_bottom_id = (requested_ports >= 4).then(|| {
        let node_id = add_route_node(&mut route_plan, NodeRole::Port, far_bottom);
        add_route_edge(&mut route_plan, bottom_id, node_id, RouteVerb::Run, false);
        node_id
    });

    let pickup_node_id = upper_ids[2];
    route_plan.nodes[usize::from(pickup_node_id)].role = NodeRole::Pickup;
    draft.pickup_above(
        "switchback-cache",
        route_plan.nodes[usize::from(pickup_node_id)].support,
    );

    let bottom_side = mirrored_side(BoundarySide::Left, mirrored);
    let mut boundary_ports = vec![
        wall_port("port-bottom", bottom_id, bottom_support, bottom_side),
        ceiling_port(ceiling_id, ceiling_support),
    ];
    if requested_ports >= 3 {
        boundary_ports.push(wall_port(
            "port-middle",
            shelf_ids[0],
            shelves[0],
            mirrored_side(BoundarySide::Left, mirrored),
        ));
    }
    if let Some(far_bottom_id) = far_bottom_id {
        boundary_ports.push(wall_port(
            "port-turn",
            far_bottom_id,
            far_bottom,
            mirrored_side(BoundarySide::Right, mirrored),
        ));
    }
    for port in &boundary_ports {
        draft.carve_boundary(port.door.trigger_bounds);
    }
    validate_port_contract(&route_plan, &boundary_ports)?;

    let route_summary = route_plan.summary();
    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-switchback-cut-v{}-{}-{}-{}-a{:02}-{:016x}",
        SWITCHBACK_CUT_GENERATION_VERSION,
        key.grammar.slug(),
        ability_slug(key.construction_abilities),
        key.intent.slug(),
        key.embedding_attempt,
        key.source_seed,
    );
    let name = format!(
        "Experimental switchback cut v{} {} {} {} attempt {} {:016x}",
        SWITCHBACK_CUT_GENERATION_VERSION,
        key.grammar.slug(),
        ability_slug(key.construction_abilities),
        key.intent.slug(),
        key.embedding_attempt,
        key.source_seed,
    );
    let doors = boundary_ports
        .iter()
        .map(|port| port.door.clone())
        .collect();
    let room = draft
        .finish_without_exits(id, name, mirrored_spawn(2, mirrored))
        .map_err(SwitchbackCutFailure::Room)?
        .with_doors(doors)
        .map_err(SwitchbackCutFailure::Door)?;
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            // This is intentionally outside both production v5 and
            // compositional v6 experimental namespaces.
            generation_version: 20_000 + SWITCHBACK_CUT_GENERATION_VERSION,
            seed: key.source_seed,
            layout_family: LayoutFamily::TerracedAscent,
            ability_tier: AbilityTier::from_abilities(key.construction_abilities),
            intended_abilities: key.construction_abilities,
            stats,
        },
    };
    let summary = SwitchbackCutSummary {
        generation_version: SWITCHBACK_CUT_GENERATION_VERSION,
        mirrored,
        cut_shelves: CUT_ROWS.len().try_into().expect("one cut fits u16"),
        boundary_ports: boundary_ports.len().try_into().expect("four ports fit u16"),
        authored_return_reversals: 1,
        pickup_node_id,
        interior_terrain_tiles: route_plan
            .nodes
            .iter()
            .filter(|node| node.support.row != FLOOR_ROW)
            .map(|node| node.support.width())
            .sum(),
        route_signature: route_summary.signature,
    };
    Ok(SwitchbackCutCandidate {
        key,
        generated,
        route_plan,
        route_summary,
        boundary_ports,
        summary,
    })
}

fn opening_transfer(cut: SupportSpec) -> SupportSpec {
    let (start_x, end_x) = if cut.start_x == 1 {
        (cut.end_x, (cut.end_x + 2).min(ROOM_WIDTH - 2))
    } else {
        (cut.start_x.saturating_sub(2).max(2), cut.start_x)
    };
    SupportSpec {
        start_x,
        end_x,
        row: cut.row + 1,
        kind: SupportKind::OneWay,
    }
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

fn wall_port(id: &str, node_id: u16, support: SupportSpec, side: BoundarySide) -> BoundaryPort {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let room_height = i32::from(ROOM_HEIGHT) * TILE_SIZE;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let trigger_y =
        (i32::from(support.row) * TILE_SIZE - DOOR_SPAN).clamp(0, room_height - DOOR_SPAN);
    let (trigger_x, arrival_x) = match side {
        BoundarySide::Left => (0, TILE_SIZE + 2),
        BoundarySide::Right => (
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

fn ceiling_port(node_id: u16, support: SupportSpec) -> BoundaryPort {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let trigger_x = (i32::from(support.center_x()) * TILE_SIZE - DOOR_SPAN / 2)
        .clamp(TILE_SIZE, room_width - TILE_SIZE - DOOR_SPAN);
    BoundaryPort {
        node_id,
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

fn validate_port_contract(
    route_plan: &RoutePlan,
    boundary_ports: &[BoundaryPort],
) -> Result<(), SwitchbackCutFailure> {
    if !(2..=4).contains(&boundary_ports.len()) {
        return Err(SwitchbackCutFailure::PortContract(format!(
            "expected two to four boundary ports, found {}",
            boundary_ports.len()
        )));
    }
    let referenced = boundary_ports
        .iter()
        .map(|port| port.node_id)
        .collect::<HashSet<_>>();
    if referenced.len() != boundary_ports.len() {
        return Err(SwitchbackCutFailure::PortContract(
            "multiple doors reference the same route node".to_owned(),
        ));
    }
    let declared = route_plan
        .nodes
        .iter()
        .filter(|node| node.role == NodeRole::Port)
        .map(|node| node.id)
        .collect::<HashSet<_>>();
    if declared != referenced {
        return Err(SwitchbackCutFailure::PortContract(
            "route-plan port nodes do not exactly match physical doors".to_owned(),
        ));
    }
    let Some(&first) = referenced.iter().next() else {
        return Err(SwitchbackCutFailure::PortContract(
            "room has no boundary ports".to_owned(),
        ));
    };
    let mut reachable = HashSet::from([first]);
    let mut frontier = vec![first];
    while let Some(node) = frontier.pop() {
        for edge in &route_plan.edges {
            let adjacent = if edge.from == node {
                Some(edge.to)
            } else if edge.to == node {
                Some(edge.from)
            } else {
                None
            };
            if let Some(adjacent) = adjacent
                && reachable.insert(adjacent)
            {
                frontier.push(adjacent);
            }
        }
    }
    if !referenced.is_subset(&reachable) {
        return Err(SwitchbackCutFailure::PortContract(
            "route-plan ports are not in one connected component".to_owned(),
        ));
    }
    Ok(())
}

const fn ability_slug(abilities: AbilitySet) -> &'static str {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => "baseline",
        (true, false) => "wall-jump",
        (false, true) => "dash",
        (true, true) => "both",
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::Tile;

    use super::*;

    fn all_keys() -> impl Iterator<Item = SwitchbackCutKey> {
        [
            AbilitySet::NONE,
            AbilitySet::new(true, false),
            AbilitySet::new(false, true),
            AbilitySet::ALL,
        ]
        .into_iter()
        .flat_map(|abilities| {
            ChallengeIntent::ALL.into_iter().flat_map(move |intent| {
                (0..=SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT).map(move |attempt| {
                    SwitchbackCutKey::new(7, abilities, intent)
                        .with_embedding(SwitchbackCutGrammar::ShelfReturnV1, attempt)
                })
            })
        })
    }

    #[test]
    fn exact_keys_regenerate_deterministically_and_keep_distinct_identity() {
        for key in all_keys() {
            let first = key.regenerate().unwrap();
            let second = key.regenerate().unwrap();
            assert_eq!(first, second);
            assert_eq!(first.key, key);
            assert!(first.generated.room.id().contains("switchback-cut-v1"));
            assert_eq!(first.generated.metadata.generation_version, 20_001);
            assert_eq!(first.generated.metadata.seed, key.source_seed);
            assert_eq!(
                first.generated.metadata.intended_abilities,
                key.construction_abilities
            );
        }
    }

    #[test]
    fn retry_identity_is_bounded_instead_of_silently_falling_back() {
        let key = SwitchbackCutKey::new(0, AbilitySet::NONE, ChallengeIntent::Gentle)
            .with_embedding(
                SwitchbackCutGrammar::ShelfReturnV1,
                SWITCHBACK_CUT_MAX_EMBEDDING_ATTEMPT + 1,
            );
        assert!(matches!(
            key.regenerate(),
            Err(SwitchbackCutGenerationError {
                cause: SwitchbackCutFailure::UnsupportedEmbeddingAttempt { .. },
                ..
            })
        ));
    }

    #[test]
    fn intents_construct_two_three_and_four_ports_without_hazards() {
        for (intent, expected_ports) in [
            (ChallengeIntent::Gentle, 2),
            (ChallengeIntent::Standard, 3),
            (ChallengeIntent::Technical, 4),
        ] {
            for seed in 0..64 {
                let candidate = SwitchbackCutKey::new(seed, AbilitySet::ALL, intent)
                    .regenerate()
                    .unwrap();
                assert_eq!(candidate.boundary_ports.len(), expected_ports);
                assert_eq!(candidate.route_summary.port_count, expected_ports as u16);
                assert_eq!(candidate.generated.room.timed_hazards().len(), 0);
                assert!(
                    candidate
                        .generated
                        .room
                        .tiles()
                        .iter()
                        .all(|&tile| tile != Tile::Hazard)
                );
            }
        }
    }

    #[test]
    fn every_interior_terrain_tile_is_an_authored_route_support() {
        for key in all_keys() {
            let candidate = key.regenerate().unwrap();
            for y in 1..candidate.generated.room.height() - 1 {
                for x in 1..candidate.generated.room.width() - 1 {
                    if !matches!(
                        candidate.generated.room.tile(x, y),
                        Some(Tile::Solid | Tile::OneWay)
                    ) {
                        continue;
                    }
                    assert!(candidate.route_plan.nodes.iter().any(|node| {
                        node.support.row == y
                            && (node.support.start_x..node.support.end_x).contains(&x)
                    }));
                }
            }
        }
    }

    #[test]
    fn solid_shelf_forces_entry_to_leave_and_return_to_the_anchored_side() {
        for key in all_keys() {
            let candidate = key.regenerate().unwrap();
            let cuts = candidate
                .route_plan
                .nodes
                .iter()
                .filter(|node| CUT_ROWS.contains(&node.support.row))
                .map(|node| node.support)
                .collect::<Vec<_>>();
            assert_eq!(cuts.len(), CUT_ROWS.len());
            for support in &cuts {
                assert_eq!(support.kind, SupportKind::Solid);
                assert!(support.start_x == 1 || support.end_x == ROOM_WIDTH - 1);
                assert!(support.width() >= ROOM_WIDTH - 22);
            }
            let cut = cuts[0];
            let bottom = candidate
                .boundary_ports
                .iter()
                .find(|port| port.door.id == "port-bottom")
                .map(|port| candidate.route_plan.nodes[usize::from(port.node_id)].support)
                .unwrap();
            let ceiling = candidate
                .boundary_ports
                .iter()
                .find(|port| port.door.id == "port-ceiling")
                .map(|port| candidate.route_plan.nodes[usize::from(port.node_id)].support)
                .unwrap();
            assert!((cut.start_x..cut.end_x).contains(&bottom.center_x()));
            assert!((cut.start_x..cut.end_x).contains(&ceiling.center_x()));
            let transfer = candidate
                .route_plan
                .nodes
                .iter()
                .find(|node| node.support.row == cut.row + 1)
                .map(|node| node.support)
                .unwrap();
            assert!(transfer.end_x <= cut.start_x || transfer.start_x >= cut.end_x);
            assert_eq!(candidate.summary.authored_return_reversals, 1);
        }
    }
}
