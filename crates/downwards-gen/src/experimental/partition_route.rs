//! Independent graph-first recursive-partition terrain experiment.
//!
//! This prototype starts with a BSP derivation, not a finished room layout.
//! The derivation recursively replaces a traversal chamber with two connected
//! children. Vertical cuts put a collision wall across the floor route;
//! horizontal cuts create an upper branch with two independently placed
//! openings, so taking the branch forms a fork/rejoin cycle. A separate,
//! bounded embedding pass assigns tile coordinates and may fail without
//! falling back to another grammar.

use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fmt,
};

use downwards_core::{
    AbilitySet, BoundarySide, Door, DoorError, PLAYER_HEIGHT, PLAYER_WIDTH, Point, Rect, RoomError,
    Tile,
};

use super::{
    BoundaryPort, ChallengeIntent, NodeRole, RoutePlan, RoutePlanSummary, RouteVerb, SupportKind,
    SupportSpec,
    common::{
        FLOOR_ROW, RoomDraft, StableRng, add_route_edge, add_route_node,
        conservative_baseline_transition, reversible_baseline_transition,
    },
};
use crate::{
    AbilityTier, GeneratedLevel, GeneratedMetadata, LayoutFamily, ROOM_HEIGHT, ROOM_WIDTH,
    TILE_SIZE,
};

/// Frozen version of the partition-route seed-to-candidate mapping.
pub const PARTITION_ROUTE_GENERATION_VERSION: u32 = 3;

/// The retry coordinate is part of candidate identity and never advanced
/// implicitly by generation.
pub const PARTITION_ROUTE_MAX_EMBEDDING_ATTEMPT: u8 = 31;

const PARTITION_GRAPH_STREAM: u64 = 0x5041_5254_4752_5031;
const PARTITION_EMBED_STREAM: u64 = 0x5041_5254_454d_4231;
const SIDE_DOOR_DEPTH: i32 = 12;
const BOUNDARY_DOOR_DEPTH: i32 = 12;
const DOOR_SPAN: i32 = 20;
const ROOT_PATH: u32 = 1;
const VERTICAL_SOCKET_CENTERS: [u16; 3] = [6, 16, 26];

/// A graph-level bias. Profiles change rewrite probabilities rather than
/// selecting a whole-room layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartitionRouteProfile {
    MixedBsp,
    Columnar,
    Branching,
}

impl PartitionRouteProfile {
    pub const ALL: [Self; 3] = [Self::MixedBsp, Self::Columnar, Self::Branching];

    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::MixedBsp => "mixed-bsp",
            Self::Columnar => "columnar",
            Self::Branching => "branching",
        }
    }
}

/// Complete regeneration identity for one exact graph and embedding attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartitionRouteKey {
    pub source_seed: u64,
    pub construction_abilities: AbilitySet,
    pub intent: ChallengeIntent,
    pub profile: PartitionRouteProfile,
    pub embedding_attempt: u8,
}

impl PartitionRouteKey {
    #[must_use]
    pub const fn new(
        source_seed: u64,
        construction_abilities: AbilitySet,
        intent: ChallengeIntent,
        profile: PartitionRouteProfile,
    ) -> Self {
        Self {
            source_seed,
            construction_abilities,
            intent,
            profile,
            embedding_attempt: 0,
        }
    }

    #[must_use]
    pub const fn with_embedding_attempt(mut self, embedding_attempt: u8) -> Self {
        self.embedding_attempt = embedding_attempt;
        self
    }

    /// Regenerate exactly this key. No retry or alternate grammar is selected.
    pub fn regenerate(self) -> Result<PartitionRouteCandidate, PartitionRouteGenerationError> {
        generate_partition_route(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartitionSplitAxis {
    Vertical,
    Horizontal,
}

/// Coordinate-free record of one recursive graph rewrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartitionDerivationSplit {
    pub parent_path: u32,
    pub target_spine_rank: u8,
    pub axis: PartitionSplitAxis,
    /// A normalized relative division choice, deliberately not a tile value.
    pub ratio_slot: u8,
    /// A relative vertical/horizontal aperture band, not an absolute row.
    pub gate_band: u8,
    pub cadence: u8,
    pub reverses_before_gate: bool,
    pub branch_runs_forward: bool,
}

/// Relative primary-route rhythm at one partition transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartitionDerivationBeat {
    /// Negative is a rise, positive is a fall, and zero is level.
    pub relative_vertical_delta: i8,
    pub cadence: u8,
    pub horizontal_reversal: bool,
}

/// The grammar output before coordinate embedding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PartitionDerivation {
    pub chamber_count: u8,
    pub splits: Vec<PartitionDerivationSplit>,
    pub primary_beats: Vec<PartitionDerivationBeat>,
    pub fork_rejoin_cycles: u8,
    pub boundary_sides: Vec<BoundarySide>,
    pub pickup_attachment: Option<u32>,
    /// Index into the frozen ceiling/floor socket grid used by the ceiling
    /// port. The realized embedding selects the nearest slot whose connector
    /// corridor preserves the root collision separator.
    pub vertical_socket_slot: u8,
    /// Independently selected floor slot. Technical keys cycle deliberately
    /// through the same finite inventory; intents without a floor port carry
    /// no irrelevant floor coordinate.
    pub floor_socket_slot: Option<u8>,
    /// Pure graph structure: rewrite ancestry/axes, forks, ports, and pickup
    /// attachment. It excludes every embedding and movement-rhythm choice.
    pub graph_topology_signature: u64,
    /// Graph structure plus coordinate-free movement beats and reversals. It
    /// excludes split ratios, gate bands, mirroring, and absolute tiles.
    pub route_derivation_signature: u64,
    /// Complete derivation fingerprint, including normalized split ratios and
    /// gate bands but still excluding seed, retry identity, and coordinates.
    pub derivation_fingerprint: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartitionRouteSummary {
    pub generation_version: u32,
    pub chambers: u8,
    pub partition_cuts: u8,
    pub fork_rejoin_cycles: u8,
    pub boundary_ports: u8,
    pub authored_rises: u8,
    pub authored_falls: u8,
    pub authored_horizontal_reversals: u8,
    pub pickup_node_id: Option<u16>,
    pub interior_terrain_tiles: u16,
    pub graph_topology_signature: u64,
    pub route_derivation_signature: u64,
    pub derivation_fingerprint: u64,
    pub coordinate_route_signature: u64,
}

/// Structurally constructed terrain-only candidate. This type makes no solver
/// reachability claim; bounded solver non-success remains inconclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionRouteCandidate {
    pub key: PartitionRouteKey,
    pub generated: GeneratedLevel,
    pub derivation: PartitionDerivation,
    pub route_plan: RoutePlan,
    pub route_summary: RoutePlanSummary,
    pub boundary_ports: Vec<BoundaryPort>,
    pub summary: PartitionRouteSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartitionRouteFailure {
    UnsupportedEmbeddingAttempt { maximum: u8, actual: u8 },
    GraphExhausted(String),
    EmbeddingExhausted(String),
    PortContract(String),
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for PartitionRouteFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEmbeddingAttempt { maximum, actual } => write!(
                formatter,
                "embedding attempt {actual} exceeds the frozen maximum {maximum}"
            ),
            Self::GraphExhausted(detail) => write!(formatter, "graph rewrite exhausted: {detail}"),
            Self::EmbeddingExhausted(detail) => {
                write!(formatter, "constraint embedding exhausted: {detail}")
            }
            Self::PortContract(detail) => write!(formatter, "port contract failed: {detail}"),
            Self::Room(error) => write!(formatter, "generated room was invalid: {error}"),
            Self::Door(error) => write!(formatter, "generated room doors were invalid: {error}"),
        }
    }
}

impl Error for PartitionRouteFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Room(error) => Some(error),
            Self::Door(error) => Some(error),
            Self::UnsupportedEmbeddingAttempt { .. }
            | Self::GraphExhausted(_)
            | Self::EmbeddingExhausted(_)
            | Self::PortContract(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionRouteGenerationError {
    pub key: PartitionRouteKey,
    pub cause: PartitionRouteFailure,
}

impl fmt::Display for PartitionRouteGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "partition route v{} {} {} {} attempt {} seed {:016x} failed: {}",
            PARTITION_ROUTE_GENERATION_VERSION,
            self.key.profile.slug(),
            ability_slug(self.key.construction_abilities),
            self.key.intent.slug(),
            self.key.embedding_attempt,
            self.key.source_seed,
            self.cause,
        )
    }
}

impl Error for PartitionRouteGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

/// Generate one exact recursive-partition candidate.
pub fn generate_partition_route(
    key: PartitionRouteKey,
) -> Result<PartitionRouteCandidate, PartitionRouteGenerationError> {
    generate(key).map_err(|cause| PartitionRouteGenerationError { key, cause })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphLeaf {
    path: u32,
    horizontal_depth: u8,
}

#[derive(Clone, Debug)]
struct PartitionGraphSpec {
    derivation: PartitionDerivation,
    mirrored: bool,
}

#[derive(Clone, Copy, Debug)]
struct Region {
    start_x: u16,
    end_x: u16,
    start_y: u16,
    end_y: u16,
}

impl Region {
    const fn width(self) -> u16 {
        self.end_x - self.start_x
    }

    const fn height(self) -> u16 {
        self.end_y - self.start_y
    }
}

#[derive(Clone, Copy, Debug)]
enum EmbeddedCut {
    Vertical {
        parent: Region,
        divider_x: u16,
        gate_row: u16,
        reversal: bool,
        cadence: u8,
    },
    Horizontal {
        parent: Region,
        divider_row: u16,
        first_gap_x: u16,
        second_gap_x: u16,
        branch_runs_forward: bool,
        cadence: u8,
    },
}

#[derive(Clone, Debug)]
struct EmbeddedPartition {
    cuts: Vec<EmbeddedCut>,
}

#[derive(Clone, Copy, Debug)]
struct RootSeparator {
    x: u16,
    start_y: u16,
    gate_row: u16,
    end_y: u16,
}

#[derive(Clone, Debug, Default)]
struct RasterReservations {
    /// Bit 0: declared landing material/headroom; bit 1: ceiling connector;
    /// bit 2: validated boundary-door arrival. Multiple provenances may
    /// deliberately name the same cell.
    cells: BTreeMap<(u16, u16), u8>,
}

impl RasterReservations {
    const LANDING: u8 = 1;
    const CEILING_CONNECTOR: u8 = 2;
    const PORT_ARRIVAL: u8 = 4;

    fn reserve(
        &mut self,
        x: u16,
        row: u16,
        provenance: u8,
        root: RootSeparator,
    ) -> Result<(), PartitionRouteFailure> {
        if root.protects(x, row) {
            return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                "raster reservation provenance {provenance} intersects protected root tile ({x}, {row})"
            )));
        }
        *self.cells.entry((x, row)).or_default() |= provenance;
        Ok(())
    }
}

impl RootSeparator {
    const fn protects(self, x: u16, row: u16) -> bool {
        x == self.x
            && ((row >= self.start_y && row < self.gate_row.saturating_sub(2))
                || (row > self.gate_row && row <= self.end_y))
    }
}

fn generate(key: PartitionRouteKey) -> Result<PartitionRouteCandidate, PartitionRouteFailure> {
    if key.embedding_attempt > PARTITION_ROUTE_MAX_EMBEDDING_ATTEMPT {
        return Err(PartitionRouteFailure::UnsupportedEmbeddingAttempt {
            maximum: PARTITION_ROUTE_MAX_EMBEDDING_ATTEMPT,
            actual: key.embedding_attempt,
        });
    }

    let salted_seed =
        key.source_seed ^ u64::from(key.embedding_attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let graph = derive_partition_graph(key, salted_seed)?;
    let embedded = embed_partition(&graph.derivation, salted_seed, graph.mirrored)?;
    realize_candidate(key, graph, embedded)
}

fn derive_partition_graph(
    key: PartitionRouteKey,
    salted_seed: u64,
) -> Result<PartitionGraphSpec, PartitionRouteFailure> {
    let mut rng = StableRng::new(salted_seed, PARTITION_GRAPH_STREAM);
    let chamber_count = match key.intent {
        ChallengeIntent::Gentle => 2 + rng.below(3) as u8,
        ChallengeIntent::Standard => 2 + rng.below(3) as u8,
        ChallengeIntent::Technical => 3 + rng.below(2) as u8,
    };
    let mirrored = rng.coin();
    let mut spine = vec![GraphLeaf {
        path: ROOT_PATH,
        horizontal_depth: 0,
    }];
    let mut splits = Vec::with_capacity(usize::from(chamber_count - 1));
    let mut horizontal_splits = 0_u8;

    for split_index in 0..chamber_count - 1 {
        let eligible = spine
            .iter()
            .enumerate()
            .filter(|(_, leaf)| leaf.horizontal_depth == 0)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            return Err(PartitionRouteFailure::GraphExhausted(
                "no unsplit traversal chamber remained".to_owned(),
            ));
        }
        let selected = if split_index == 0 {
            0
        } else {
            eligible[usize::from(rng.below(eligible.len() as u16))]
        };
        let parent = spine[selected];
        let axis = if split_index == 0 {
            PartitionSplitAxis::Vertical
        } else {
            choose_split_axis(key.profile, horizontal_splits, &mut rng)
        };
        let split = PartitionDerivationSplit {
            parent_path: parent.path,
            target_spine_rank: selected.try_into().expect("four chambers fit u8"),
            axis,
            ratio_slot: rng.below(7) as u8,
            gate_band: rng.below(7) as u8,
            cadence: rng.below(4) as u8,
            reverses_before_gate: rng.coin(),
            branch_runs_forward: rng.coin(),
        };
        splits.push(split);
        let first_child = GraphLeaf {
            path: parent.path << 1,
            horizontal_depth: parent.horizontal_depth,
        };
        let second_child = GraphLeaf {
            path: (parent.path << 1) | 1,
            horizontal_depth: parent.horizontal_depth,
        };
        match axis {
            PartitionSplitAxis::Vertical => {
                spine.splice(selected..=selected, [first_child, second_child]);
            }
            PartitionSplitAxis::Horizontal => {
                horizontal_splits += 1;
                // The upper child is a branch chamber; the lower child stays
                // on the west-to-east spine and may itself be rewritten.
                spine[selected] = GraphLeaf {
                    horizontal_depth: parent.horizontal_depth + 1,
                    ..second_child
                };
            }
        }
    }

    let mut primary_beats = splits
        .iter()
        .filter(|split| split.axis == PartitionSplitAxis::Vertical)
        .scan(6_i8, |previous_band, split| {
            let band = split.gate_band as i8;
            let delta = band - *previous_band;
            *previous_band = band;
            Some(PartitionDerivationBeat {
                relative_vertical_delta: delta,
                cadence: split.cadence,
                horizontal_reversal: split.reverses_before_gate,
            })
        })
        .collect::<Vec<_>>();
    let last_band = splits
        .iter()
        .rev()
        .find(|split| split.axis == PartitionSplitAxis::Vertical)
        .map_or(6_i8, |split| split.gate_band as i8);
    primary_beats.push(PartitionDerivationBeat {
        relative_vertical_delta: 6 - last_band,
        cadence: rng.below(4) as u8,
        horizontal_reversal: false,
    });

    let boundary_sides = match key.intent {
        ChallengeIntent::Gentle => vec![BoundarySide::Left, BoundarySide::Right],
        ChallengeIntent::Standard => vec![
            BoundarySide::Left,
            BoundarySide::Right,
            BoundarySide::Ceiling,
        ],
        ChallengeIntent::Technical => vec![
            BoundarySide::Left,
            BoundarySide::Right,
            BoundarySide::Ceiling,
            BoundarySide::Floor,
        ],
    };
    let pickup_attachment = (rng.below(4) != 0).then(|| {
        if horizontal_splits > 0 && rng.coin() {
            splits
                .iter()
                .find(|split| split.axis == PartitionSplitAxis::Horizontal)
                .map_or(ROOT_PATH, |split| split.parent_path << 1)
        } else {
            spine[usize::from(rng.below(spine.len() as u16))].path
        }
    });
    let mut derivation = PartitionDerivation {
        chamber_count,
        splits,
        primary_beats,
        fork_rejoin_cycles: horizontal_splits,
        boundary_sides,
        pickup_attachment,
        // Ceiling placement depends on the realized top anchor and is filled
        // in after embedding. Zero is the canonical no-ceiling value.
        vertical_socket_slot: 0,
        floor_socket_slot: (key.intent == ChallengeIntent::Technical)
            .then(|| floor_socket_slot(key)),
        graph_topology_signature: 0,
        route_derivation_signature: 0,
        derivation_fingerprint: 0,
    };
    derivation.graph_topology_signature = graph_topology_signature(&derivation);
    derivation.route_derivation_signature = route_derivation_signature(&derivation);
    derivation.derivation_fingerprint = derivation_fingerprint(&derivation);
    Ok(PartitionGraphSpec {
        derivation,
        mirrored,
    })
}

fn choose_split_axis(
    profile: PartitionRouteProfile,
    horizontal_splits: u8,
    rng: &mut StableRng,
) -> PartitionSplitAxis {
    if horizontal_splits >= 2 {
        return PartitionSplitAxis::Vertical;
    }
    let horizontal = match profile {
        PartitionRouteProfile::MixedBsp => rng.coin(),
        PartitionRouteProfile::Columnar => rng.below(4) == 0,
        PartitionRouteProfile::Branching => rng.below(4) != 0,
    };
    if horizontal {
        PartitionSplitAxis::Horizontal
    } else {
        PartitionSplitAxis::Vertical
    }
}

fn embed_partition(
    derivation: &PartitionDerivation,
    salted_seed: u64,
    mirrored: bool,
) -> Result<EmbeddedPartition, PartitionRouteFailure> {
    let mut rng = StableRng::new(salted_seed, PARTITION_EMBED_STREAM);
    let mut regions = BTreeMap::from([(
        ROOT_PATH,
        Region {
            start_x: 1,
            end_x: ROOM_WIDTH - 1,
            start_y: 1,
            end_y: FLOOR_ROW,
        },
    )]);
    let mut cuts = Vec::with_capacity(derivation.splits.len());
    for split in &derivation.splits {
        let parent = regions.remove(&split.parent_path).ok_or_else(|| {
            PartitionRouteFailure::EmbeddingExhausted(format!(
                "derivation path {} had already been consumed",
                split.parent_path
            ))
        })?;
        match split.axis {
            PartitionSplitAxis::Vertical => {
                let left_path = split.parent_path << 1;
                let right_path = left_path | 1;
                let left_required = required_region_width(derivation, left_path);
                let right_required = required_region_width(derivation, right_path);
                let required = left_required + 1 + right_required;
                if parent.width() < required || parent.height() < 7 {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "vertical split path {} needs width {required} but has only {}x{} tiles",
                        split.parent_path,
                        parent.width(),
                        parent.height()
                    )));
                }
                let slack = parent.width() - required;
                let left_width = left_required
                    + (slack * u16::from(split.ratio_slot) + 3) / 6
                    + u16::from(rng.below(2) == 1 && slack > 0);
                let left_width = left_width.min(parent.width() - 1 - right_required);
                let mut divider_x = parent.start_x + left_width;
                let mut reserved_columns = cuts
                    .iter()
                    .flat_map(|cut| match *cut {
                        EmbeddedCut::Horizontal {
                            first_gap_x,
                            second_gap_x,
                            ..
                        } => [first_gap_x, first_gap_x + 1, second_gap_x, second_gap_x + 1]
                            .into_iter()
                            .collect::<Vec<_>>(),
                        EmbeddedCut::Vertical { .. } => Vec::new(),
                    })
                    .collect::<Vec<_>>();
                if split.parent_path == ROOT_PATH
                    && let Some(floor_socket_slot) = derivation.floor_socket_slot
                {
                    let socket_center = VERTICAL_SOCKET_CENTERS[usize::from(floor_socket_slot)];
                    reserved_columns.extend([
                        mirror_tile_x(socket_center - 1, mirrored),
                        mirror_tile_x(socket_center, mirrored),
                    ]);
                }
                if reserved_columns.contains(&divider_x) {
                    let minimum = parent.start_x + left_required;
                    let maximum = parent.end_x - 1 - right_required;
                    let replacement = [
                        divider_x.saturating_add(1),
                        divider_x.saturating_sub(1),
                        divider_x.saturating_add(2),
                        divider_x.saturating_sub(2),
                        divider_x.saturating_add(3),
                        divider_x.saturating_sub(3),
                    ]
                    .into_iter()
                    .find(|candidate| {
                        (*candidate >= minimum && *candidate <= maximum)
                            && !reserved_columns.contains(candidate)
                    });
                    let Some(replacement) = replacement else {
                        return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                            "vertical split path {} cannot preserve prior opening columns {reserved_columns:?}",
                            split.parent_path
                        )));
                    };
                    divider_x = replacement;
                }
                let gate_min = parent.start_y.saturating_add(3).max(11);
                let gate_max = parent.end_y.saturating_sub(2).min(15);
                if gate_min > gate_max {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "vertical split path {} has no conservative gate row",
                        split.parent_path
                    )));
                }
                let gate_span = gate_max - gate_min;
                let gate_row = gate_min + (gate_span * u16::from(split.gate_band) + 3) / 6;
                let left = Region {
                    end_x: divider_x,
                    ..parent
                };
                let right = Region {
                    start_x: divider_x + 1,
                    ..parent
                };
                regions.insert(left_path, left);
                regions.insert(right_path, right);
                cuts.push(EmbeddedCut::Vertical {
                    parent,
                    divider_x,
                    gate_row,
                    reversal: split.reverses_before_gate,
                    cadence: split.cadence,
                });
            }
            PartitionSplitAxis::Horizontal => {
                if parent.height() < 12 || parent.width() < 11 {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "horizontal split path {} has only {}x{} tiles",
                        split.parent_path,
                        parent.width(),
                        parent.height()
                    )));
                }
                let divider_min = parent.start_y + 6;
                let divider_max = parent.end_y.saturating_sub(5).min(10);
                if divider_min > divider_max {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "horizontal split path {} has no branch shelf row",
                        split.parent_path
                    )));
                }
                let span = divider_max - divider_min;
                let divider_row = divider_min + (span * u16::from(split.ratio_slot) + 3) / 6;
                let first_base = parent.start_x + 2;
                let second_base = parent.end_x - 4;
                let inward = u16::from(split.gate_band % 2) * u16::from(parent.width() >= 12);
                let first_gap_x = first_base + inward;
                let second_gap_x = second_base.saturating_sub(inward);
                if second_gap_x < first_gap_x + 4 {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "horizontal split path {} cannot separate two openings",
                        split.parent_path
                    )));
                }
                let upper = Region {
                    end_y: divider_row,
                    ..parent
                };
                let lower = Region {
                    start_y: divider_row + 1,
                    ..parent
                };
                regions.insert(split.parent_path << 1, upper);
                regions.insert((split.parent_path << 1) | 1, lower);
                cuts.push(EmbeddedCut::Horizontal {
                    parent,
                    divider_row,
                    first_gap_x,
                    second_gap_x,
                    branch_runs_forward: split.branch_runs_forward,
                    cadence: split.cadence,
                });
            }
        }
    }
    Ok(EmbeddedPartition { cuts })
}

/// Minimum width needed by the remaining rewrite subtree. This is derived
/// from graph structure before embedding, so a parent reserves enough space
/// for both of its descendants rather than relying on an implicit retry.
fn required_region_width(derivation: &PartitionDerivation, path: u32) -> u16 {
    let Some(split) = derivation
        .splits
        .iter()
        .find(|split| split.parent_path == path)
    else {
        return 5;
    };
    let first = required_region_width(derivation, path << 1);
    let second = required_region_width(derivation, (path << 1) | 1);
    match split.axis {
        PartitionSplitAxis::Vertical => first + 1 + second,
        // Two separated two-tile branch openings and three solid shelf
        // segments need eleven tiles, regardless of absolute placement.
        PartitionSplitAxis::Horizontal => first.max(second).max(11),
    }
}

fn realize_candidate(
    key: PartitionRouteKey,
    mut graph: PartitionGraphSpec,
    embedded: EmbeddedPartition,
) -> Result<PartitionRouteCandidate, PartitionRouteFailure> {
    let mut draft = RoomDraft::new();
    let root_separator = root_separator(&embedded, graph.mirrored)?;

    let west_support = SupportSpec {
        start_x: 1,
        end_x: 5,
        row: FLOOR_ROW,
        kind: SupportKind::Solid,
    };
    let east_support = SupportSpec {
        start_x: ROOM_WIDTH - 5,
        end_x: ROOM_WIDTH - 1,
        row: FLOOR_ROW,
        kind: SupportKind::Solid,
    };
    let mut route_plan = RoutePlan::default();
    let west_id = add_route_node(&mut route_plan, NodeRole::Port, west_support);
    // Move along the boundary floor before beginning any ascent so the first
    // one-way step cannot overlap the west door arrival rectangle.
    let west_launch_center = root_separator.x.saturating_sub(2).clamp(3, 9);
    let west_launch_support = support_around(west_launch_center, FLOOR_ROW, 3);
    let west_launch = add_route_node(&mut route_plan, NodeRole::Landing, west_launch_support);
    add_route_edge(
        &mut route_plan,
        west_id,
        west_launch,
        conservative_verb(west_support, west_launch_support),
        true,
    );
    let mut primary_nodes = vec![west_id, west_launch];
    let mut current_id = west_launch;
    let mut vertical_cuts = embedded
        .cuts
        .iter()
        .filter_map(|cut| match *cut {
            EmbeddedCut::Vertical {
                divider_x,
                gate_row,
                reversal,
                cadence,
                ..
            } => Some((divider_x, gate_row, reversal, cadence)),
            EmbeddedCut::Horizontal { .. } => None,
        })
        .collect::<Vec<_>>();
    vertical_cuts.sort_unstable_by_key(|cut| cut.0);
    if graph.mirrored {
        vertical_cuts.reverse();
    }
    let mut authored_reversals = 0_u8;
    let mut passed_divider_x = 0_u16;
    for (base_divider_x, gate_row, reversal, cadence) in vertical_cuts {
        let divider_x = mirror_tile_x(base_divider_x, graph.mirrored);
        let approach_center = divider_x.saturating_sub(3).max(2);
        if reversal {
            let current = route_plan.nodes[usize::from(current_id)].support;
            let reverse_center = current
                .center_x()
                .saturating_sub(3)
                .max(passed_divider_x.saturating_add(2))
                .max(2);
            if reverse_center < current.center_x() {
                let reverse_row = step_row_toward(current.row, gate_row, 2);
                current_id = append_target(
                    &mut route_plan,
                    &mut draft,
                    current_id,
                    support_around(reverse_center, reverse_row, 3),
                    false,
                    cadence,
                );
                primary_nodes.push(current_id);
                authored_reversals += 1;
            }
        }
        current_id = append_target(
            &mut route_plan,
            &mut draft,
            current_id,
            support_around(approach_center, gate_row, 3 + u16::from(cadence % 2)),
            true,
            cadence,
        );
        primary_nodes.push(current_id);
        current_id = append_target(
            &mut route_plan,
            &mut draft,
            current_id,
            support_around(divider_x, gate_row, 5),
            true,
            cadence,
        );
        primary_nodes.push(current_id);
        let landing_center = (divider_x + 3).min(ROOM_WIDTH - 3);
        current_id = append_target(
            &mut route_plan,
            &mut draft,
            current_id,
            support_around(landing_center, gate_row, 3 + u16::from((cadence + 1) % 2)),
            true,
            cadence,
        );
        primary_nodes.push(current_id);
        passed_divider_x = divider_x;
    }

    // Symmetric floor approach protects the east arrival from the final
    // descending step.
    let east_landing_center = (ROOM_WIDTH - 10)
        .max(passed_divider_x.saturating_add(2))
        .min(ROOM_WIDTH - 3);
    let east_landing = append_target(
        &mut route_plan,
        &mut draft,
        current_id,
        support_around(east_landing_center, FLOOR_ROW, 3),
        true,
        1,
    );
    primary_nodes.push(east_landing);
    let east_id = add_route_node(&mut route_plan, NodeRole::Port, east_support);
    let east_landing_support = route_plan.nodes[usize::from(east_landing)].support;
    add_route_edge(
        &mut route_plan,
        east_landing,
        east_id,
        conservative_verb(east_landing_support, east_support),
        true,
    );
    primary_nodes.push(east_id);

    let mut branch_nodes = Vec::new();
    let branch_anchors = primary_nodes
        .iter()
        .copied()
        .filter(|id| route_plan.nodes[usize::from(*id)].role != NodeRole::Port)
        .collect::<Vec<_>>();
    for cut in &embedded.cuts {
        let EmbeddedCut::Horizontal {
            divider_row,
            first_gap_x,
            second_gap_x,
            branch_runs_forward,
            cadence,
            ..
        } = *cut
        else {
            continue;
        };
        let mut first_x = mirror_gap_center(first_gap_x, graph.mirrored);
        let mut second_x = mirror_gap_center(second_gap_x, graph.mirrored);
        if first_x > second_x {
            std::mem::swap(&mut first_x, &mut second_x);
        }
        let (entry_x, exit_x) = if branch_runs_forward {
            (first_x, second_x)
        } else {
            (second_x, first_x)
        };
        let entry_anchor = closest_node_by_x(&route_plan, &branch_anchors, entry_x);
        let exit_anchor =
            closest_distinct_node_by_x(&route_plan, &branch_anchors, exit_x, entry_anchor);
        route_plan.nodes[usize::from(entry_anchor)].role = NodeRole::Junction;
        route_plan.nodes[usize::from(exit_anchor)].role = NodeRole::Junction;
        let under_row = (divider_row + 2).min(FLOOR_ROW - 1);
        let mut branch_id = append_target(
            &mut route_plan,
            &mut draft,
            entry_anchor,
            support_around(entry_x, under_row, 2),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around(entry_x, divider_row, 2),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around(entry_x, divider_row.saturating_sub(2), 3),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        let peak_row = divider_row.saturating_sub(3 + u16::from(cadence % 2));
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around((entry_x + exit_x) / 2, peak_row.max(3), 3),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around(exit_x, divider_row.saturating_sub(2), 3),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around(exit_x, divider_row, 2),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        // Descend through the authored two-tile shelf opening before
        // drifting back toward the lower-spine rejoin. Interpolating sideways
        // while still inside the shelf height would put a declared landing
        // underneath the solid shelf segment.
        branch_id = append_target(
            &mut route_plan,
            &mut draft,
            branch_id,
            support_around(exit_x, under_row, 2),
            false,
            cadence,
        );
        branch_nodes.push(branch_id);
        connect_to_existing(
            &mut route_plan,
            &mut draft,
            branch_id,
            exit_anchor,
            false,
            cadence,
        );
    }

    let mut pickup_node_id = None;
    if graph.derivation.pickup_attachment.is_some() {
        let selected = branch_nodes
            .iter()
            .copied()
            .min_by_key(|id| route_plan.nodes[usize::from(*id)].support.row)
            .or_else(|| {
                let anchor = primary_nodes[primary_nodes.len() / 2];
                let support = route_plan.nodes[usize::from(anchor)].support;
                let detour_row = support.row.saturating_sub(2).max(3);
                let detour = [
                    (support.center_x() + 3).min(ROOM_WIDTH - 3),
                    support.center_x().saturating_sub(3).max(2),
                ]
                .into_iter()
                .map(|center_x| support_around(center_x, detour_row, 3))
                .find(|&target| {
                    reversible_baseline_transition(support, target)
                        && support_has_partition_clearance(target, &embedded, graph.mirrored)
                });
                detour.map_or(Some(anchor), |target| {
                    Some(append_target(
                        &mut route_plan,
                        &mut draft,
                        anchor,
                        target,
                        false,
                        0,
                    ))
                })
            });
        if let Some(node_id) = selected {
            route_plan.nodes[usize::from(node_id)].role = NodeRole::Pickup;
            pickup_node_id = Some(node_id);
        }
    }

    // Resolve every graph-derived landing against the immutable cut raster
    // before choosing a ceiling anchor. The ceiling then chooses from actual
    // safe top supports rather than coordinates that a later constraint pass
    // would move.
    let floor_socket_center = graph
        .derivation
        .floor_socket_slot
        .map(|slot| VERTICAL_SOCKET_CENTERS[usize::from(slot)]);
    constrain_route_supports(
        &mut route_plan,
        &embedded,
        graph.mirrored,
        floor_socket_center,
    )?;

    let mut boundary_ports = vec![
        wall_port("port-west", west_id, west_support, BoundarySide::Left),
        wall_port("port-east", east_id, east_support, BoundarySide::Right),
    ];
    let mut ceiling_connector = None;
    if graph
        .derivation
        .boundary_sides
        .contains(&BoundarySide::Ceiling)
    {
        let (source, vertical_socket_slot) = choose_ceiling_connection(
            &route_plan,
            &embedded,
            graph.mirrored,
            root_separator,
        )
        .ok_or_else(|| {
            PartitionRouteFailure::EmbeddingExhausted(
                "no embedded route anchor has a cut-preserving ceiling connector in the frozen socket inventory"
                    .to_owned(),
            )
        })?;
        graph.derivation.vertical_socket_slot = vertical_socket_slot;
        let vertical_center = VERTICAL_SOCKET_CENTERS[usize::from(vertical_socket_slot)];
        let ceiling_support = support_around(vertical_center, 3, 4);
        let ceiling_id = add_route_node(&mut route_plan, NodeRole::Port, ceiling_support);
        connect_to_existing(&mut route_plan, &mut draft, source, ceiling_id, false, 2);
        reserve_connector_corridor(&route_plan, source, ceiling_id, root_separator)?;
        ceiling_connector = Some((source, ceiling_id));
        boundary_ports.push(ceiling_port(ceiling_id, ceiling_support));
    }
    if graph
        .derivation
        .boundary_sides
        .contains(&BoundarySide::Floor)
    {
        let floor_socket_slot = graph.derivation.floor_socket_slot.ok_or_else(|| {
            PartitionRouteFailure::EmbeddingExhausted(
                "technical floor port has no recorded socket slot".to_owned(),
            )
        })?;
        let floor_center = VERTICAL_SOCKET_CENTERS[usize::from(floor_socket_slot)];
        let floor_arrival_x =
            floor_port_arrival(&route_plan, &embedded, graph.mirrored, floor_center).ok_or_else(
                || {
                    PartitionRouteFailure::EmbeddingExhausted(
                        "no clear floor-port arrival among the frozen socket grid".to_owned(),
                    )
                },
            )?;
        let floor_arrival_center = ((floor_arrival_x + PLAYER_WIDTH / 2) / TILE_SIZE)
            .clamp(2, i32::from(ROOM_WIDTH - 3)) as u16;
        let floor_support = support_around(floor_arrival_center, FLOOR_ROW, 2);
        let floor_id = add_route_node(&mut route_plan, NodeRole::Port, floor_support);
        let floor_anchors = primary_nodes
            .iter()
            .copied()
            .filter(|id| route_plan.nodes[usize::from(*id)].support.row == FLOOR_ROW)
            .collect::<Vec<_>>();
        let anchor = closest_node_by_x(&route_plan, &floor_anchors, floor_arrival_center);
        connect_to_existing(&mut route_plan, &mut draft, anchor, floor_id, false, 0);
        boundary_ports.push(floor_port(floor_id, floor_center, floor_arrival_x));
    }

    constrain_route_supports(
        &mut route_plan,
        &embedded,
        graph.mirrored,
        floor_socket_center,
    )?;
    for node in &route_plan.nodes {
        draw_support(&mut draft, node.support);
    }
    if let Some(node_id) = pickup_node_id {
        draft.pickup_above(
            "partition-cache",
            route_plan.nodes[usize::from(node_id)].support,
        );
    }

    let reservations = build_raster_reservations(
        &route_plan,
        &boundary_ports,
        ceiling_connector,
        root_separator,
    )?;
    validate_raster_reservations(&embedded, graph.mirrored, &reservations, &route_plan)?;
    draw_partition_terrain(&mut draft, &embedded, graph.mirrored);
    validate_partition_raster(&draft, &embedded, graph.mirrored)?;

    for port in &boundary_ports {
        if port.door.side == BoundarySide::Ceiling {
            draft.carve_ceiling_aperture(port.door.trigger_bounds);
        }
        draft.carve_boundary(port.door.trigger_bounds);
    }
    validate_port_contract(&route_plan, &boundary_ports)?;
    validate_realized_route_supports(&draft, &route_plan)?;

    // The full derivation includes the route-aware ceiling slot and the
    // independent floor slot, so it is finalized only after realization.
    graph.derivation.derivation_fingerprint = derivation_fingerprint(&graph.derivation);

    let route_summary = route_plan.summary();
    let stats = draft.stats(route_summary.node_count);
    let id = format!(
        "experimental-partition-route-v{}-{}-{}-{}-a{:02}-{:016x}",
        PARTITION_ROUTE_GENERATION_VERSION,
        key.profile.slug(),
        ability_slug(key.construction_abilities),
        key.intent.slug(),
        key.embedding_attempt,
        key.source_seed,
    );
    let name = format!(
        "Experimental partition route v{} {} {} {} attempt {} {:016x}",
        PARTITION_ROUTE_GENERATION_VERSION,
        key.profile.slug(),
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
        .finish_without_exits(
            id,
            name,
            Point::new(2 * TILE_SIZE, FLOOR_ROW as i32 * TILE_SIZE - PLAYER_HEIGHT),
        )
        .map_err(PartitionRouteFailure::Room)?
        .with_doors(doors)
        .map_err(PartitionRouteFailure::Door)?;
    let generated = GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: 30_000 + PARTITION_ROUTE_GENERATION_VERSION,
            seed: key.source_seed,
            layout_family: LayoutFamily::TerracedAscent,
            ability_tier: AbilityTier::from_abilities(key.construction_abilities),
            intended_abilities: key.construction_abilities,
            stats,
        },
    };
    let authored_rises = graph
        .derivation
        .primary_beats
        .iter()
        .filter(|beat| beat.relative_vertical_delta < 0)
        .count()
        .try_into()
        .unwrap_or(u8::MAX);
    let authored_falls = graph
        .derivation
        .primary_beats
        .iter()
        .filter(|beat| beat.relative_vertical_delta > 0)
        .count()
        .try_into()
        .unwrap_or(u8::MAX);
    let summary = PartitionRouteSummary {
        generation_version: PARTITION_ROUTE_GENERATION_VERSION,
        chambers: graph.derivation.chamber_count,
        partition_cuts: graph
            .derivation
            .splits
            .len()
            .try_into()
            .expect("three splits fit u8"),
        fork_rejoin_cycles: graph.derivation.fork_rejoin_cycles,
        boundary_ports: boundary_ports.len().try_into().expect("four ports fit u8"),
        authored_rises,
        authored_falls,
        authored_horizontal_reversals: authored_reversals,
        pickup_node_id,
        interior_terrain_tiles: generated.metadata.stats.interior_solid_tiles
            + generated.metadata.stats.one_way_tiles,
        graph_topology_signature: graph.derivation.graph_topology_signature,
        route_derivation_signature: graph.derivation.route_derivation_signature,
        derivation_fingerprint: graph.derivation.derivation_fingerprint,
        coordinate_route_signature: route_summary.signature,
    };
    Ok(PartitionRouteCandidate {
        key,
        generated,
        derivation: graph.derivation,
        route_plan,
        route_summary,
        boundary_ports,
        summary,
    })
}

fn draw_partition_terrain(draft: &mut RoomDraft, embedded: &EmbeddedPartition, mirrored: bool) {
    for cut in &embedded.cuts {
        match *cut {
            EmbeddedCut::Vertical {
                parent,
                divider_x,
                gate_row,
                ..
            } => {
                let divider_x = mirror_tile_x(divider_x, mirrored);
                if parent.start_y < gate_row.saturating_sub(2) {
                    draft.solid_column(divider_x, parent.start_y, gate_row - 2);
                }
                if gate_row + 1 < parent.end_y {
                    draft.solid_column(divider_x, gate_row + 1, parent.end_y);
                }
            }
            EmbeddedCut::Horizontal {
                parent,
                divider_row,
                first_gap_x,
                second_gap_x,
                ..
            } => {
                for support in [
                    solid_segment(parent.start_x, first_gap_x, divider_row),
                    solid_segment(first_gap_x + 2, second_gap_x, divider_row),
                    solid_segment(second_gap_x + 2, parent.end_x, divider_row),
                ]
                .into_iter()
                .flatten()
                {
                    draw_support(draft, mirror_support(support, mirrored));
                }
            }
        }
    }
}

fn authored_cut_cells(cut: EmbeddedCut, mirrored: bool) -> Vec<(u16, u16)> {
    match cut {
        EmbeddedCut::Vertical {
            parent,
            divider_x,
            gate_row,
            ..
        } => {
            let divider_x = mirror_tile_x(divider_x, mirrored);
            (parent.start_y..gate_row.saturating_sub(2))
                .chain(gate_row + 1..parent.end_y)
                .map(|row| (divider_x, row))
                .collect()
        }
        EmbeddedCut::Horizontal {
            parent,
            divider_row,
            first_gap_x,
            second_gap_x,
            ..
        } => (parent.start_x..first_gap_x)
            .chain(first_gap_x + 2..second_gap_x)
            .chain(second_gap_x + 2..parent.end_x)
            .map(|x| (mirror_tile_x(x, mirrored), divider_row))
            .collect(),
    }
}

fn partition_cell_is_solid(embedded: &EmbeddedPartition, mirrored: bool, x: u16, row: u16) -> bool {
    embedded
        .cuts
        .iter()
        .any(|&cut| authored_cut_cells(cut, mirrored).contains(&(x, row)))
}

fn support_has_partition_clearance(
    support: SupportSpec,
    embedded: &EmbeddedPartition,
    mirrored: bool,
) -> bool {
    (support.start_x..support.end_x).all(|x| {
        (support.row.saturating_sub(2)..=support.row)
            .all(|row| !partition_cell_is_solid(embedded, mirrored, x, row))
    })
}

fn constrain_route_supports(
    plan: &mut RoutePlan,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    floor_socket_center: Option<u16>,
) -> Result<(), PartitionRouteFailure> {
    for node_index in 0..plan.nodes.len() {
        if plan.nodes[node_index].role == NodeRole::Port
            || support_has_embedding_clearance(
                plan.nodes[node_index].support,
                embedded,
                mirrored,
                floor_socket_center,
            )
        {
            continue;
        }
        let node_id = plan.nodes[node_index].id;
        let original = plan.nodes[node_index].support;
        let mut replacement = None;
        let mut rows = vec![original.row];
        for row in [
            original.row.saturating_add(1).min(FLOOR_ROW - 1),
            original.row.saturating_sub(1).max(3),
            original.row.saturating_add(2).min(FLOOR_ROW - 1),
            original.row.saturating_sub(2).max(3),
        ] {
            if !rows.contains(&row) {
                rows.push(row);
            }
        }
        for row in rows {
            for width in (2..=original.width()).rev() {
                for offset in [0_i16, -1, 1, -2, 2, -3, 3] {
                    let center = i32::from(original.center_x()) + i32::from(offset);
                    if !(2..=i32::from(ROOM_WIDTH - 3)).contains(&center) {
                        continue;
                    }
                    let candidate = support_around(center as u16, row, width);
                    let preserves_incident_edges = plan.edges.iter().all(|edge| {
                        let adjacent = if edge.from == node_id {
                            Some(edge.to)
                        } else if edge.to == node_id {
                            Some(edge.from)
                        } else {
                            None
                        };
                        adjacent.is_none_or(|adjacent| {
                            reversible_baseline_transition(
                                candidate,
                                plan.nodes[usize::from(adjacent)].support,
                            )
                        })
                    });
                    if support_has_embedding_clearance(
                        candidate,
                        embedded,
                        mirrored,
                        floor_socket_center,
                    ) && preserves_incident_edges
                    {
                        replacement = Some(candidate);
                        break;
                    }
                }
                if replacement.is_some() {
                    break;
                }
            }
            if replacement.is_some() {
                break;
            }
        }
        plan.nodes[node_index].support = replacement.ok_or_else(|| {
            PartitionRouteFailure::EmbeddingExhausted(format!(
                "route node {} cannot place a two-tile landing near {original:?} without intersecting an authored cut",
                node_id
            ))
        })?;
    }
    for edge in &mut plan.edges {
        let from = plan.nodes[usize::from(edge.from)].support;
        let to = plan.nodes[usize::from(edge.to)].support;
        let verb = conservative_baseline_transition(from, to).ok_or_else(|| {
            PartitionRouteFailure::EmbeddingExhausted(format!(
                "constrained route edge {}->{} left the conservative baseline envelope: {from:?} -> {to:?}",
                edge.from, edge.to
            ))
        })?;
        if !reversible_baseline_transition(from, to) {
            return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                "constrained route edge {}->{} is not reversible under the conservative baseline contract: {from:?} -> {to:?}",
                edge.from, edge.to
            )));
        }
        edge.verb = verb;
    }
    Ok(())
}

fn support_has_embedding_clearance(
    support: SupportSpec,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    floor_socket_center: Option<u16>,
) -> bool {
    support_has_partition_clearance(support, embedded, mirrored)
        && floor_socket_center.is_none_or(|center| {
            support.row != FLOOR_ROW || !support_intersects_floor_trigger(support, center)
        })
}

fn validate_raster_reservations(
    embedded: &EmbeddedPartition,
    mirrored: bool,
    reservations: &RasterReservations,
    plan: &RoutePlan,
) -> Result<(), PartitionRouteFailure> {
    for (&(x, row), &provenance) in &reservations.cells {
        if partition_cell_is_solid(embedded, mirrored, x, row) {
            let route_node = plan.nodes.iter().find(|node| {
                (node.support.start_x..node.support.end_x).contains(&x)
                    && (node.support.row.saturating_sub(2)..=node.support.row).contains(&row)
            });
            let cuts = embedded
                .cuts
                .iter()
                .enumerate()
                .filter(|(_, cut)| authored_cut_cells(**cut, mirrored).contains(&(x, row)))
                .collect::<Vec<_>>();
            return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                "raster reservation provenance {provenance} conflicts with authored partition tile ({x}, {row}); route-node={route_node:?}; cuts={cuts:?}"
            )));
        }
    }
    Ok(())
}

fn validate_partition_raster(
    draft: &RoomDraft,
    embedded: &EmbeddedPartition,
    mirrored: bool,
) -> Result<(), PartitionRouteFailure> {
    for (cut_index, &cut) in embedded.cuts.iter().enumerate() {
        let cells = authored_cut_cells(cut, mirrored);
        if cells.is_empty() {
            return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                "partition cut {cut_index} has no authored collision tiles"
            )));
        }
        for (x, row) in cells {
            if draft.tile(x, row) != Tile::Solid {
                return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                    "partition cut {cut_index} lost authored solid tile ({x}, {row})"
                )));
            }
        }
    }
    Ok(())
}

fn solid_segment(start_x: u16, end_x: u16, row: u16) -> Option<SupportSpec> {
    (start_x < end_x).then_some(SupportSpec {
        start_x,
        end_x,
        row,
        kind: SupportKind::Solid,
    })
}

fn append_target(
    plan: &mut RoutePlan,
    draft: &mut RoomDraft,
    from_id: u16,
    target: SupportSpec,
    critical: bool,
    cadence: u8,
) -> u16 {
    let target_id = add_route_node(plan, NodeRole::Landing, target);
    connect_to_existing(plan, draft, from_id, target_id, critical, cadence);
    target_id
}

fn connect_to_existing(
    plan: &mut RoutePlan,
    _draft: &mut RoomDraft,
    from_id: u16,
    target_id: u16,
    critical: bool,
    cadence: u8,
) {
    let from = plan.nodes[usize::from(from_id)].support;
    let target = plan.nodes[usize::from(target_id)].support;
    let mut previous_id = from_id;
    for support in connector_intermediate_supports(from, target, cadence) {
        let node_id = add_route_node(plan, NodeRole::Landing, support);
        let previous = plan.nodes[usize::from(previous_id)].support;
        add_route_edge(
            plan,
            previous_id,
            node_id,
            conservative_verb(previous, support),
            critical,
        );
        previous_id = node_id;
    }
    let previous = plan.nodes[usize::from(previous_id)].support;
    add_route_edge(
        plan,
        previous_id,
        target_id,
        conservative_verb(previous, target),
        critical,
    );
}

fn connector_intermediate_supports(
    from: SupportSpec,
    target: SupportSpec,
    cadence: u8,
) -> Vec<SupportSpec> {
    let horizontal = from.center_x().abs_diff(target.center_x());
    let vertical = from.row.abs_diff(target.row);
    let horizontal_stride = 3 + u16::from(cadence % 3);
    let horizontal_steps = horizontal.div_ceil(horizontal_stride);
    let vertical_steps = vertical.div_ceil(2);
    let steps = horizontal_steps.max(vertical_steps).max(1);
    (1..steps)
        .map(|step| {
            let center_x = lerp_u16(from.center_x(), target.center_x(), step, steps);
            let row = lerp_u16(from.row, target.row, step, steps);
            support_around(center_x, row, 3)
        })
        .collect()
}

fn conservative_verb(from: SupportSpec, to: SupportSpec) -> RouteVerb {
    conservative_baseline_transition(from, to).unwrap_or_else(|| {
        if to.row > from.row {
            RouteVerb::Drop
        } else if from.row != to.row || supports_separated(from, to) {
            RouteVerb::Jump
        } else {
            RouteVerb::Run
        }
    })
}

const fn supports_separated(first: SupportSpec, second: SupportSpec) -> bool {
    first.end_x < second.start_x || second.end_x < first.start_x
}

fn support_around(center_x: u16, row: u16, width: u16) -> SupportSpec {
    let width = width.clamp(2, 5);
    let start_x = center_x
        .saturating_sub(width / 2)
        // Keep one interior tile clear beside each lateral boundary for the
        // validated door arrival rectangles. Boundary-floor port supports are
        // authored separately and therefore unaffected.
        .clamp(2, ROOM_WIDTH - 2 - width);
    SupportSpec {
        start_x,
        end_x: start_x + width,
        row,
        kind: if row == FLOOR_ROW {
            SupportKind::Solid
        } else {
            SupportKind::OneWay
        },
    }
}

fn draw_support(draft: &mut RoomDraft, support: SupportSpec) {
    if support.row != FLOOR_ROW {
        draft.platform(support);
    }
}

fn lerp_u16(start: u16, end: u16, step: u16, steps: u16) -> u16 {
    let start = i32::from(start);
    let delta = i32::from(end) - start;
    (start + delta * i32::from(step) / i32::from(steps)) as u16
}

fn step_row_toward(start: u16, target: u16, distance: u16) -> u16 {
    if target < start {
        start.saturating_sub(distance.min(start - target))
    } else {
        start + distance.min(target - start)
    }
}

fn closest_node_by_x(plan: &RoutePlan, candidates: &[u16], x: u16) -> u16 {
    *candidates
        .iter()
        .min_by_key(|id| plan.nodes[usize::from(**id)].support.center_x().abs_diff(x))
        .expect("the primary path always has boundary nodes")
}

fn closest_distinct_node_by_x(plan: &RoutePlan, candidates: &[u16], x: u16, excluded: u16) -> u16 {
    candidates
        .iter()
        .copied()
        .filter(|id| *id != excluded)
        .min_by_key(|id| plan.nodes[usize::from(*id)].support.center_x().abs_diff(x))
        .unwrap_or(excluded)
}

fn choose_ceiling_connection(
    plan: &RoutePlan,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    root: RootSeparator,
) -> Option<(u16, u8)> {
    let mut anchors = plan
        .nodes
        .iter()
        .filter(|node| node.role != NodeRole::Port)
        .map(|node| node.id)
        .collect::<Vec<_>>();
    anchors.sort_unstable_by_key(|&node_id| {
        let node = &plan.nodes[usize::from(node_id)];
        (node.support.row, node.id)
    });
    anchors.into_iter().find_map(|source_id| {
        choose_ceiling_socket_slot(plan, source_id, embedded, mirrored, root)
            .map(|slot| (source_id, slot))
    })
}

fn choose_ceiling_socket_slot(
    plan: &RoutePlan,
    source_id: u16,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    root: RootSeparator,
) -> Option<u8> {
    let source = plan.nodes[usize::from(source_id)].support;
    let mut slots = [0_u8, 1, 2];
    slots.sort_unstable_by_key(|&slot| {
        (
            source
                .center_x()
                .abs_diff(VERTICAL_SOCKET_CENTERS[usize::from(slot)]),
            slot,
        )
    });
    slots.into_iter().find(|&slot| {
        let target = support_around(VERTICAL_SOCKET_CENTERS[usize::from(slot)], 3, 4);
        connector_preserves_root(source, target, 2, root)
            && connector_preserves_partition(source, target, 2, embedded, mirrored)
    })
}

fn connector_preserves_partition(
    from: SupportSpec,
    target: SupportSpec,
    cadence: u8,
    embedded: &EmbeddedPartition,
    mirrored: bool,
) -> bool {
    let chain = connector_support_chain(from, target, cadence);
    if chain.iter().any(|&support| {
        (support.start_x..support.end_x).any(|x| {
            (support.row.saturating_sub(2)..=support.row)
                .any(|row| partition_cell_is_solid(embedded, mirrored, x, row))
        })
    }) {
        return false;
    }
    chain.windows(2).all(|pair| {
        sampled_connector_points(pair[0], pair[1]).all(|(center_x, row)| {
            let start_x = center_x.saturating_sub(1).max(1);
            let end_x = (center_x + 1).min(ROOM_WIDTH - 2);
            (start_x..=end_x).all(|x| {
                (row.saturating_sub(2)..=row)
                    .all(|y| !partition_cell_is_solid(embedded, mirrored, x, y))
            })
        })
    })
}

fn connector_support_chain(
    from: SupportSpec,
    target: SupportSpec,
    cadence: u8,
) -> Vec<SupportSpec> {
    std::iter::once(from)
        .chain(connector_intermediate_supports(from, target, cadence))
        .chain(std::iter::once(target))
        .collect()
}

fn connector_preserves_root(
    from: SupportSpec,
    target: SupportSpec,
    cadence: u8,
    root: RootSeparator,
) -> bool {
    let chain = connector_support_chain(from, target, cadence);
    if chain.iter().any(|&support| {
        (support.start_x..support.end_x)
            .any(|x| (support.row.saturating_sub(2)..=support.row).any(|row| root.protects(x, row)))
    }) {
        return false;
    }
    chain.windows(2).all(|pair| {
        sampled_connector_points(pair[0], pair[1]).all(|(center_x, row)| {
            let start_x = center_x.saturating_sub(1).max(1);
            let end_x = (center_x + 1).min(ROOM_WIDTH - 2);
            (start_x..=end_x).all(|x| (row.saturating_sub(2)..=row).all(|y| !root.protects(x, y)))
        })
    })
}

fn sampled_connector_points(
    from: SupportSpec,
    to: SupportSpec,
) -> impl Iterator<Item = (u16, u16)> {
    let steps = from
        .center_x()
        .abs_diff(to.center_x())
        .max(from.row.abs_diff(to.row))
        .max(1);
    (0..=steps).map(move |step| {
        (
            lerp_u16(from.center_x(), to.center_x(), step, steps),
            lerp_u16(from.row, to.row, step, steps),
        )
    })
}

fn reserve_connector_corridor(
    plan: &RoutePlan,
    source_id: u16,
    target_id: u16,
    root: RootSeparator,
) -> Result<(), PartitionRouteFailure> {
    let source = plan.nodes[usize::from(source_id)].support;
    let target = plan.nodes[usize::from(target_id)].support;
    if !connector_preserves_root(source, target, 2, root) {
        return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
            "ceiling connector {source_id}->{target_id} intersects the protected root separator"
        )));
    }
    Ok(())
}

fn build_raster_reservations(
    plan: &RoutePlan,
    ports: &[BoundaryPort],
    ceiling_connector: Option<(u16, u16)>,
    root: RootSeparator,
) -> Result<RasterReservations, PartitionRouteFailure> {
    let mut reservations = RasterReservations::default();
    for node in &plan.nodes {
        let support = node.support;
        for x in support.start_x..support.end_x {
            if support.row < FLOOR_ROW {
                reservations
                    .reserve(x, support.row, RasterReservations::LANDING, root)
                    .map_err(|_| {
                        PartitionRouteFailure::EmbeddingExhausted(format!(
                            "route node {} material conflicts with protected root tile ({x}, {})",
                            node.id, support.row
                        ))
                    })?;
            }
            for row in support.row.saturating_sub(2).max(1)..support.row {
                reservations
                    .reserve(x, row, RasterReservations::LANDING, root)
                    .map_err(|_| {
                        PartitionRouteFailure::EmbeddingExhausted(format!(
                            "route node {} standing clearance conflicts with protected root tile ({x}, {row})",
                            node.id
                        ))
                    })?;
            }
        }
    }
    if let Some((source_id, target_id)) = ceiling_connector {
        let source = plan.nodes[usize::from(source_id)].support;
        let target = plan.nodes[usize::from(target_id)].support;
        let chain = connector_support_chain(source, target, 2);
        for pair in chain.windows(2) {
            for (center_x, row) in sampled_connector_points(pair[0], pair[1]) {
                let start_x = center_x.saturating_sub(1).max(1);
                let end_x = (center_x + 1).min(ROOM_WIDTH - 2);
                for x in start_x..=end_x {
                    for y in row.saturating_sub(2).max(1)..=row.min(FLOOR_ROW - 1) {
                        reservations.reserve(x, y, RasterReservations::CEILING_CONNECTOR, root)?;
                    }
                }
            }
        }
    }
    for port in ports {
        let arrival = Rect::new(
            port.door.arrival.x,
            port.door.arrival.y,
            PLAYER_WIDTH,
            PLAYER_HEIGHT,
        );
        for row in 1..FLOOR_ROW {
            for x in 1..ROOM_WIDTH - 1 {
                let tile = Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                );
                if arrival.intersects(tile) {
                    reservations.reserve(x, row, RasterReservations::PORT_ARRIVAL, root)?;
                }
            }
        }
    }
    Ok(reservations)
}

fn validate_realized_route_supports(
    draft: &RoomDraft,
    plan: &RoutePlan,
) -> Result<(), PartitionRouteFailure> {
    for node in &plan.nodes {
        for x in node.support.start_x..node.support.end_x {
            let actual = draft.tile(x, node.support.row);
            if actual != node.support.kind.tile() {
                return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                    "route node {} declares {:?} at ({x}, {}) but realized {actual:?}",
                    node.id, node.support.kind, node.support.row
                )));
            }
            for row in node.support.row.saturating_sub(2).max(1)..node.support.row {
                if draft.tile(x, row) == Tile::Solid {
                    return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                        "route node {} has solid standing obstruction at ({x}, {row})",
                        node.id
                    )));
                }
            }
        }
    }
    for edge in &plan.edges {
        let from = plan.nodes[usize::from(edge.from)].support;
        let to = plan.nodes[usize::from(edge.to)].support;
        let declared = conservative_baseline_transition(from, to);
        if declared != Some(edge.verb) || !reversible_baseline_transition(from, to) {
            return Err(PartitionRouteFailure::EmbeddingExhausted(format!(
                "route edge {}->{} has verb {:?} but reversible conservative baseline classification is {declared:?}: {from:?} -> {to:?}",
                edge.from, edge.to, edge.verb
            )));
        }
    }
    Ok(())
}

fn floor_trigger(center_x: u16) -> Rect {
    Rect::new(
        i32::from(center_x) * TILE_SIZE - DOOR_SPAN / 2,
        i32::from(ROOM_HEIGHT) * TILE_SIZE - BOUNDARY_DOOR_DEPTH,
        DOOR_SPAN,
        BOUNDARY_DOOR_DEPTH,
    )
}

fn support_intersects_floor_trigger(support: SupportSpec, center_x: u16) -> bool {
    let trigger = floor_trigger(center_x);
    (support.start_x..support.end_x).any(|x| {
        trigger.intersects(Rect::new(
            i32::from(x) * TILE_SIZE,
            i32::from(FLOOR_ROW) * TILE_SIZE,
            TILE_SIZE,
            TILE_SIZE,
        ))
    })
}

fn floor_port_arrival(
    plan: &RoutePlan,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    center_x: u16,
) -> Option<i32> {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let trigger = floor_trigger(center_x);
    let near_left = trigger.x - PLAYER_WIDTH - 2;
    let near_right = trigger.right() + 2;
    let west_safe = TILE_SIZE + 2;
    let east_safe = room_width - TILE_SIZE - 2 - PLAYER_WIDTH;
    [west_safe, east_safe, near_left, near_right]
        .into_iter()
        .find(|&arrival_x| {
            let arrival = Rect::new(
                arrival_x,
                i32::from(FLOOR_ROW) * TILE_SIZE - PLAYER_HEIGHT,
                PLAYER_WIDTH,
                PLAYER_HEIGHT,
            );
            arrival_x >= TILE_SIZE
                && arrival_x + PLAYER_WIDTH <= room_width - TILE_SIZE
                && !arrival.intersects(trigger)
                && floor_arrival_is_clear(plan, embedded, mirrored, arrival)
        })
}

fn floor_arrival_is_clear(
    plan: &RoutePlan,
    embedded: &EmbeddedPartition,
    mirrored: bool,
    arrival: Rect,
) -> bool {
    let cut_clear = embedded.cuts.iter().all(|&cut| {
        authored_cut_cells(cut, mirrored)
            .into_iter()
            .all(|(x, row)| {
                !arrival.intersects(Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                ))
            })
    });
    let route_clear = plan.nodes.iter().all(|node| {
        node.support.row == FLOOR_ROW
            || (node.support.start_x..node.support.end_x).all(|x| {
                !arrival.intersects(Rect::new(
                    i32::from(x) * TILE_SIZE,
                    i32::from(node.support.row) * TILE_SIZE,
                    TILE_SIZE,
                    TILE_SIZE,
                ))
            })
    });
    cut_clear && route_clear
}

fn wall_port(id: &str, node_id: u16, support: SupportSpec, side: BoundarySide) -> BoundaryPort {
    let room_width = i32::from(ROOM_WIDTH) * TILE_SIZE;
    let standing_y = i32::from(support.row) * TILE_SIZE - PLAYER_HEIGHT;
    let trigger_y = (i32::from(support.row) * TILE_SIZE - DOOR_SPAN)
        .clamp(0, i32::from(ROOM_HEIGHT) * TILE_SIZE - DOOR_SPAN);
    let (trigger_x, arrival_x) = match side {
        BoundarySide::Left => (0, TILE_SIZE + 2),
        BoundarySide::Right => (
            room_width - SIDE_DOOR_DEPTH,
            room_width - TILE_SIZE - 2 - PLAYER_WIDTH,
        ),
        BoundarySide::Ceiling | BoundarySide::Floor => {
            unreachable!("wall port only accepts lateral sides")
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
            trigger_bounds: Rect::new(trigger_x, 0, DOOR_SPAN, BOUNDARY_DOOR_DEPTH),
            arrival: Point::new(
                i32::from(support.center_x()) * TILE_SIZE - PLAYER_WIDTH / 2,
                TILE_SIZE + 2,
            ),
            destination_room: None,
            destination_door: None,
        },
    }
}

fn floor_port(node_id: u16, socket_center: u16, arrival_x: i32) -> BoundaryPort {
    let trigger = floor_trigger(socket_center);
    BoundaryPort {
        node_id,
        door: Door {
            id: "port-floor".to_owned(),
            side: BoundarySide::Floor,
            trigger_bounds: trigger,
            arrival: Point::new(arrival_x, i32::from(FLOOR_ROW) * TILE_SIZE - PLAYER_HEIGHT),
            destination_room: None,
            destination_door: None,
        },
    }
}

fn validate_port_contract(
    plan: &RoutePlan,
    ports: &[BoundaryPort],
) -> Result<(), PartitionRouteFailure> {
    if !(2..=4).contains(&ports.len()) {
        return Err(PartitionRouteFailure::PortContract(format!(
            "expected two to four ports, found {}",
            ports.len()
        )));
    }
    let referenced = ports
        .iter()
        .map(|port| port.node_id)
        .collect::<HashSet<_>>();
    let declared = plan
        .nodes
        .iter()
        .filter(|node| node.role == NodeRole::Port)
        .map(|node| node.id)
        .collect::<HashSet<_>>();
    if referenced.len() != ports.len() || referenced != declared {
        return Err(PartitionRouteFailure::PortContract(
            "physical ports and route-plan port nodes differ".to_owned(),
        ));
    }
    let Some(&first) = referenced.iter().next() else {
        return Err(PartitionRouteFailure::PortContract(
            "no boundary port exists".to_owned(),
        ));
    };
    let mut visited = HashSet::from([first]);
    let mut pending = vec![first];
    while let Some(node) = pending.pop() {
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
                pending.push(adjacent);
            }
        }
    }
    if !referenced.is_subset(&visited) {
        return Err(PartitionRouteFailure::PortContract(
            "not all ports belong to the route component".to_owned(),
        ));
    }
    Ok(())
}

fn root_separator(
    embedded: &EmbeddedPartition,
    mirrored: bool,
) -> Result<RootSeparator, PartitionRouteFailure> {
    let Some(EmbeddedCut::Vertical {
        parent,
        divider_x,
        gate_row,
        ..
    }) = embedded.cuts.first().copied()
    else {
        return Err(PartitionRouteFailure::EmbeddingExhausted(
            "the embedded root is not a vertical collision partition".to_owned(),
        ));
    };
    Ok(RootSeparator {
        x: mirror_tile_x(divider_x, mirrored),
        start_y: parent.start_y,
        gate_row,
        end_y: parent.end_y,
    })
}

fn mirror_tile_x(x: u16, mirrored: bool) -> u16 {
    if mirrored { ROOM_WIDTH - 1 - x } else { x }
}

fn mirror_support(mut support: SupportSpec, mirrored: bool) -> SupportSpec {
    if mirrored {
        let start_x = ROOM_WIDTH - support.end_x;
        support.end_x = ROOM_WIDTH - support.start_x;
        support.start_x = start_x;
    }
    support
}

fn mirror_gap_center(gap_start_x: u16, mirrored: bool) -> u16 {
    mirror_support(
        SupportSpec {
            start_x: gap_start_x,
            end_x: gap_start_x + 2,
            row: 1,
            kind: SupportKind::OneWay,
        },
        mirrored,
    )
    .center_x()
}

fn graph_topology_signature(derivation: &PartitionDerivation) -> u64 {
    let mut hash = PartitionDerivationHasher::new();
    hash.byte(1);
    hash_graph_topology(&mut hash, derivation);
    hash.finish()
}

fn route_derivation_signature(derivation: &PartitionDerivation) -> u64 {
    let mut hash = PartitionDerivationHasher::new();
    hash.byte(2);
    hash_graph_topology(&mut hash, derivation);
    for split in &derivation.splits {
        hash.byte(split.cadence);
        hash.byte(u8::from(split.reverses_before_gate));
        hash.byte(u8::from(split.branch_runs_forward));
    }
    for beat in &derivation.primary_beats {
        hash.byte(beat.relative_vertical_delta as u8);
        hash.byte(beat.cadence);
        hash.byte(u8::from(beat.horizontal_reversal));
    }
    hash.finish()
}

fn derivation_fingerprint(derivation: &PartitionDerivation) -> u64 {
    let mut hash = PartitionDerivationHasher::new();
    hash.byte(3);
    hash_graph_topology(&mut hash, derivation);
    for split in &derivation.splits {
        hash.byte(split.ratio_slot);
        hash.byte(split.gate_band);
        hash.byte(split.cadence);
        hash.byte(u8::from(split.reverses_before_gate));
        hash.byte(u8::from(split.branch_runs_forward));
    }
    for beat in &derivation.primary_beats {
        hash.byte(beat.relative_vertical_delta as u8);
        hash.byte(beat.cadence);
        hash.byte(u8::from(beat.horizontal_reversal));
    }
    hash.byte(derivation.vertical_socket_slot);
    match derivation.floor_socket_slot {
        Some(slot) => {
            hash.byte(1);
            hash.byte(slot);
        }
        None => hash.byte(0),
    }
    hash.finish()
}

fn hash_graph_topology(hash: &mut PartitionDerivationHasher, derivation: &PartitionDerivation) {
    hash.byte(derivation.chamber_count);
    for split in &derivation.splits {
        hash.u32(split.parent_path);
        hash.byte(split.target_spine_rank);
        hash.byte(split.axis as u8);
    }
    hash.byte(derivation.fork_rejoin_cycles);
    for side in &derivation.boundary_sides {
        hash.byte(*side as u8);
    }
    match derivation.pickup_attachment {
        Some(path) => {
            hash.byte(1);
            hash.u32(path);
        }
        None => hash.byte(0),
    }
}

struct PartitionDerivationHasher(u64);

impl PartitionDerivationHasher {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

const fn ability_slug(abilities: AbilitySet) -> &'static str {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => "baseline",
        (true, false) => "wall-jump",
        (false, true) => "dash",
        (true, true) => "both",
    }
}

fn floor_socket_slot(key: PartitionRouteKey) -> u8 {
    let profile = match key.profile {
        PartitionRouteProfile::MixedBsp => 0_u64,
        PartitionRouteProfile::Columnar => 1,
        PartitionRouteProfile::Branching => 2,
    };
    let abilities = u64::from(key.construction_abilities.wall_jump)
        | (u64::from(key.construction_abilities.dash) << 1);
    // Linear cycling is deliberate: every three consecutive seeds provide
    // all opposite-side mates in the finite vertical inventory for each
    // profile/loadout/retry coordinate.
    ((key.source_seed + profile + abilities + u64::from(key.embedding_attempt))
        % VERTICAL_SOCKET_CENTERS.len() as u64) as u8
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_core::Tile;

    use super::*;

    fn key(seed: u64) -> PartitionRouteKey {
        PartitionRouteKey::new(
            seed,
            AbilitySet::NONE,
            ChallengeIntent::Standard,
            PartitionRouteProfile::MixedBsp,
        )
    }

    #[test]
    fn exact_key_is_deterministic_and_retry_is_bounded() {
        for profile in PartitionRouteProfile::ALL {
            for seed in 0..32 {
                let key =
                    PartitionRouteKey::new(seed, AbilitySet::ALL, ChallengeIntent::Gentle, profile);
                assert_eq!(key.regenerate(), key.regenerate());
            }
        }
        let invalid = key(0).with_embedding_attempt(PARTITION_ROUTE_MAX_EMBEDDING_ATTEMPT + 1);
        assert!(matches!(
            invalid.regenerate(),
            Err(PartitionRouteGenerationError {
                cause: PartitionRouteFailure::UnsupportedEmbeddingAttempt { .. },
                ..
            })
        ));
    }

    #[test]
    fn one_hundred_twenty_eight_seed_batch_reports_each_diversity_layer_honestly() {
        let mut static_signatures = HashSet::new();
        let mut graph_signatures = HashSet::new();
        let mut route_signatures = HashSet::new();
        let mut derivation_fingerprints = HashSet::new();
        let mut constructed = 0_usize;
        let mut failures = Vec::new();
        for seed in 0..128 {
            let exact_key = key(seed);
            let outcome = exact_key.regenerate();
            assert_eq!(outcome, exact_key.regenerate());
            let candidate = match outcome {
                Ok(candidate) => candidate,
                Err(error) => {
                    failures.push((seed, error.cause));
                    continue;
                }
            };
            constructed += 1;
            let static_signature = candidate.generated.room.tiles().iter().fold(
                0xcbf2_9ce4_8422_2325_u64,
                |hash, tile| {
                    (hash
                        ^ match tile {
                            Tile::Empty => 0_u64,
                            Tile::Solid => 1,
                            Tile::Hazard => 2,
                            Tile::OneWay => 3,
                        })
                    .wrapping_mul(0x0000_0100_0000_01b3)
                },
            );
            static_signatures.insert(static_signature);
            graph_signatures.insert(candidate.derivation.graph_topology_signature);
            route_signatures.insert(candidate.derivation.route_derivation_signature);
            derivation_fingerprints.insert(candidate.derivation.derivation_fingerprint);
        }
        eprintln!(
            "partition batch: constructed={constructed}/128 failures={failures:?} uniqueness: static={} graph={} route={} full={}",
            static_signatures.len(),
            graph_signatures.len(),
            route_signatures.len(),
            derivation_fingerprints.len()
        );
        assert!(
            constructed >= 116,
            "only {constructed} / 128 keys constructed"
        );
        assert!(
            static_signatures.len() >= 116,
            "only {} / 128 distinct static geometries",
            static_signatures.len()
        );
        assert!(
            route_signatures.len() >= 116,
            "only {} / 128 distinct coordinate-free route derivations",
            route_signatures.len()
        );
        assert!(
            derivation_fingerprints.len() >= 116,
            "only {} / 128 distinct full derivation fingerprints",
            derivation_fingerprints.len()
        );
    }

    #[test]
    fn partitions_are_terrain_only_close_the_floor_bypass_and_keep_conservative_links() {
        let mut attempted = 0_usize;
        let mut constructed = 0_usize;
        let mut failures = Vec::new();
        for profile in PartitionRouteProfile::ALL {
            for intent in ChallengeIntent::ALL {
                for seed in 0..64 {
                    attempted += 1;
                    let exact_key = PartitionRouteKey::new(seed, AbilitySet::ALL, intent, profile);
                    let outcome = exact_key.regenerate();
                    assert_eq!(outcome, exact_key.regenerate());
                    let candidate = match outcome {
                        Ok(candidate) => candidate,
                        Err(error) => {
                            failures.push((profile, intent, seed, error.cause));
                            continue;
                        }
                    };
                    constructed += 1;
                    assert!((2..=4).contains(&candidate.summary.chambers));
                    assert!((2..=4).contains(&candidate.summary.boundary_ports));
                    assert!(candidate.summary.fork_rejoin_cycles <= 2);
                    assert!(candidate.generated.room.timed_hazards().is_empty());
                    assert!(
                        candidate
                            .generated
                            .room
                            .tiles()
                            .iter()
                            .all(|tile| *tile != Tile::Hazard)
                    );

                    let graph = derive_partition_graph(exact_key, seed).unwrap();
                    let embedded =
                        embed_partition(&graph.derivation, seed, graph.mirrored).unwrap();
                    let EmbeddedCut::Vertical {
                        divider_x,
                        gate_row,
                        ..
                    } = embedded.cuts[0]
                    else {
                        panic!("the root partition must be vertical")
                    };
                    let divider_x = mirror_tile_x(divider_x, graph.mirrored);
                    // The root cut is a continuous collision separator from
                    // the boundary floor up to an elevated doorway. This is
                    // stronger than merely finding an unrelated floor tile.
                    assert_eq!(
                        candidate.generated.room.tile(divider_x, FLOOR_ROW),
                        Some(Tile::Solid)
                    );
                    for row in gate_row + 1..FLOOR_ROW {
                        assert_eq!(
                            candidate.generated.room.tile(divider_x, row),
                            Some(Tile::Solid),
                            "root separator broke at ({divider_x}, {row})"
                        );
                    }
                    assert_ne!(
                        candidate.generated.room.tile(divider_x, gate_row - 1),
                        Some(Tile::Solid),
                        "root separator lacks its elevated opening; a one-way authored step is allowed"
                    );
                    assert!(gate_row < FLOOR_ROW - 1);
                    for (cut_index, &cut) in embedded.cuts.iter().enumerate() {
                        for (x, row) in authored_cut_cells(cut, graph.mirrored) {
                            assert_eq!(
                                candidate.generated.room.tile(x, row),
                                Some(Tile::Solid),
                                "authored cut {cut_index} changed at ({x}, {row}) for {exact_key:?}"
                            );
                        }
                    }
                    for node in &candidate.route_plan.nodes {
                        for x in node.support.start_x..node.support.end_x {
                            assert_eq!(
                                candidate.generated.room.tile(x, node.support.row),
                                Some(node.support.kind.tile()),
                                "route node {} material diverged for {exact_key:?}",
                                node.id
                            );
                            for row in node.support.row.saturating_sub(2).max(1)..node.support.row {
                                assert_ne!(
                                    candidate.generated.room.tile(x, row),
                                    Some(Tile::Solid),
                                    "route node {} standing clearance blocked at ({x}, {row}) for {exact_key:?}",
                                    node.id
                                );
                            }
                        }
                    }
                    for edge in &candidate.route_plan.edges {
                        let from = candidate.route_plan.nodes[usize::from(edge.from)].support;
                        let to = candidate.route_plan.nodes[usize::from(edge.to)].support;
                        assert_eq!(
                            conservative_baseline_transition(from, to),
                            Some(edge.verb),
                            "misclassified/non-conservative edge {edge:?}: {from:?} -> {to:?}"
                        );
                        assert!(
                            reversible_baseline_transition(from, to),
                            "non-reversible baseline edge {edge:?}: {from:?} -> {to:?}"
                        );
                        assert!(!matches!(
                            edge.verb,
                            RouteVerb::WallClimb | RouteVerb::DashAcross | RouteVerb::DashUp
                        ));
                    }
                }
            }
        }
        eprintln!(
            "partition invariant batch: constructed={constructed}/{attempted} failures={failures:?}"
        );
        assert_eq!(attempted, 576);
        assert!(
            constructed >= 519,
            "only {constructed} / {attempted} exact attempt-zero keys constructed"
        );
    }

    #[test]
    fn seventy_two_key_socket_inventory_is_closed_by_construction() {
        let mut sockets = Vec::new();
        let mut attempted = 0_usize;
        let mut rooms = 0_usize;
        let mut failures = Vec::new();
        for profile in PartitionRouteProfile::ALL {
            for seed in 0..8 {
                for intent in ChallengeIntent::ALL {
                    attempted += 1;
                    let exact_key = PartitionRouteKey::new(seed, AbilitySet::NONE, intent, profile);
                    let outcome = exact_key.regenerate();
                    assert_eq!(outcome, exact_key.regenerate());
                    let candidate = match outcome {
                        Ok(candidate) => candidate,
                        Err(error) => {
                            failures.push((profile, intent, seed, error.cause));
                            continue;
                        }
                    };
                    let room_index = rooms;
                    rooms += 1;
                    sockets.extend(
                        candidate
                            .boundary_ports
                            .iter()
                            .map(|port| (room_index, port.door.socket())),
                    );
                }
            }
        }
        eprintln!(
            "partition socket batch: constructed={rooms}/{attempted} occurrences={} failures={failures:?}",
            sockets.len()
        );
        assert_eq!(attempted, 72);
        assert!(
            rooms >= 65,
            "only {rooms} / {attempted} socket keys constructed"
        );
        for &(room_index, socket) in &sockets {
            assert!(
                sockets.iter().any(|&(candidate_room, candidate)| {
                    candidate_room != room_index && socket.matches(candidate)
                }),
                "closed partition-route v3 inventory lacks a different-room mate for room {room_index} socket {socket:?}"
            );
        }
    }

    #[test]
    fn signatures_separate_graph_rhythm_and_embedding_choices() {
        let derivation = PartitionDerivation {
            chamber_count: 2,
            splits: vec![PartitionDerivationSplit {
                parent_path: ROOT_PATH,
                target_spine_rank: 0,
                axis: PartitionSplitAxis::Vertical,
                ratio_slot: 3,
                gate_band: 4,
                cadence: 2,
                reverses_before_gate: true,
                branch_runs_forward: false,
            }],
            primary_beats: vec![PartitionDerivationBeat {
                relative_vertical_delta: -2,
                cadence: 2,
                horizontal_reversal: true,
            }],
            fork_rejoin_cycles: 0,
            boundary_sides: vec![BoundarySide::Left, BoundarySide::Right],
            pickup_attachment: None,
            vertical_socket_slot: 1,
            floor_socket_slot: Some(1),
            graph_topology_signature: 0,
            route_derivation_signature: 0,
            derivation_fingerprint: 0,
        };
        let graph = graph_topology_signature(&derivation);
        let route = route_derivation_signature(&derivation);
        let full = derivation_fingerprint(&derivation);

        let mut embedding_change = derivation.clone();
        embedding_change.splits[0].ratio_slot = 6;
        embedding_change.splits[0].gate_band = 1;
        assert_eq!(graph, graph_topology_signature(&embedding_change));
        assert_eq!(route, route_derivation_signature(&embedding_change));
        assert_ne!(full, derivation_fingerprint(&embedding_change));

        let mut socket_change = derivation.clone();
        socket_change.vertical_socket_slot = 2;
        assert_eq!(graph, graph_topology_signature(&socket_change));
        assert_eq!(route, route_derivation_signature(&socket_change));
        assert_ne!(full, derivation_fingerprint(&socket_change));

        let mut floor_socket_change = derivation.clone();
        floor_socket_change.floor_socket_slot = Some(2);
        assert_eq!(graph, graph_topology_signature(&floor_socket_change));
        assert_eq!(route, route_derivation_signature(&floor_socket_change));
        assert_ne!(full, derivation_fingerprint(&floor_socket_change));

        let mut rhythm_change = derivation;
        rhythm_change.primary_beats[0].horizontal_reversal = false;
        assert_eq!(graph, graph_topology_signature(&rhythm_change));
        assert_ne!(route, route_derivation_signature(&rhythm_change));
    }
}
