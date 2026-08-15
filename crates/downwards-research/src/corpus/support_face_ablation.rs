//! Terrain-face segmentation for boundary-connected ablation audits.
//!
//! Connected-component terrain audits deliberately ignore the immutable room
//! shell. That makes a decorative ledge or pillar attached to the floor or a
//! wall disappear into the boundary component. This module instead records
//! exposed maximal support and wall faces whose backing tiles are interior,
//! even when those tiles touch the shell.
//!
//! This is geometric addressability only. A face is not asserted reachable,
//! useful, removable, or necessary. Later ablation passes must rebuild a valid
//! room and replay exact positive controllers before making any positive
//! redundancy observation.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use downwards_ai::{ReachedTarget, Replay, ReplayDivergence, SearchTarget};
use downwards_core::{DeathReason, DoorEntryError, Room, Simulation, SimulationEvent, Tile};
use downwards_validation::BoundedTargetEvidence;
use serde::{Deserialize, Serialize};

use super::{
    CORPUS_ROOM_ANALYSIS_VERSION, CorpusCandidate, CorpusMetricInputV2Error, CorpusRoomAnalysis,
    EvaluatedCorpusRoom, EvaluatedCorpusRoomV2, EvaluationLoadout, LoadoutRouteMatrix,
    resolve_corpus_metric_candidate_v2,
};

/// Version of the face extraction and stable face identities.
pub const SUPPORT_FACE_SEGMENTATION_VERSION: u32 = 1;

/// Interpretation boundary for every segmentation record.
pub const SUPPORT_FACE_SEGMENTATION_DISCLAIMER: &str = "support faces are exposed geometric units backed by interior tiles, including boundary-connected extensions; they are not evidence of reachability, usefulness, removability, redundancy, or necessity";

/// Version of face coalescing, room reconstruction, exact-controller replay,
/// outcome classification, and report ordering.
pub const SUPPORT_FACE_ABLATION_VERSION: u32 = 1;

/// Interpretation boundary for every ablation report.
pub const SUPPORT_FACE_ABLATION_DISCLAIMER: &str = "survival is positive redundancy evidence for one exact stored controller and one removed unit only; changed success is reported separately from behavior-preserving success, and death, wrong-target contact, or replay exhaustion after removal is not evidence that the unit is necessary or that the modified objective is unreachable";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportSurfaceMaterial {
    Solid,
    OneWay,
}

impl SupportSurfaceMaterial {
    const fn from_tile(tile: Tile) -> Option<Self> {
        match tile {
            Tile::Solid => Some(Self::Solid),
            Tile::OneWay => Some(Self::OneWay),
            Tile::Empty
            | Tile::HazardUp
            | Tile::HazardDown
            | Tile::HazardLeft
            | Tile::HazardRight => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainTileCoordinate {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalWallSide {
    Left,
    Right,
}

/// Stable identity of one maximal exposed face.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "orientation", rename_all = "snake_case")]
pub enum SupportFaceId {
    Horizontal {
        row: u16,
        start_x: u16,
        end_x: u16,
        material: SupportSurfaceMaterial,
    },
    Vertical {
        column: u16,
        start_y: u16,
        end_y: u16,
        side: VerticalWallSide,
    },
}

/// Maximal same-material top surface backed only by interior tiles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HorizontalSupportFace {
    pub id: SupportFaceId,
    pub row: u16,
    pub start_x: u16,
    /// Exclusive tile coordinate.
    pub end_x: u16,
    pub material: SupportSurfaceMaterial,
    pub backing_tiles: Vec<TerrainTileCoordinate>,
}

/// Maximal exposed side of a solid wall backed only by interior tiles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerticalWallFace {
    pub id: SupportFaceId,
    pub column: u16,
    pub start_y: u16,
    /// Exclusive tile coordinate.
    pub end_y: u16,
    pub side: VerticalWallSide,
    pub backing_tiles: Vec<TerrainTileCoordinate>,
}

/// Complete deterministic face inventory for one room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceSegmentation {
    pub version: u32,
    pub disclaimer: String,
    pub room_width_tiles: u16,
    pub room_height_tiles: u16,
    pub horizontal_supports: Vec<HorizontalSupportFace>,
    pub vertical_walls: Vec<VerticalWallFace>,
}

/// Extract maximal exposed terrain faces without mutating the room.
///
/// Boundary tiles themselves are never emitted, including solid shell cells
/// surrounding a carved door aperture. Interior backing tiles remain eligible
/// when connected to that shell, which is the distinction from component-only
/// terrain segmentation.
#[must_use]
pub fn segment_support_faces(room: &Room) -> SupportFaceSegmentation {
    let width = room.width();
    let height = room.height();
    let mut horizontal_supports = Vec::new();
    let mut vertical_walls = Vec::new();

    if width >= 3 && height >= 3 {
        for row in 1..height - 1 {
            let mut start = 1;
            while start < width - 1 {
                let material = exposed_top_material(room, start, row);
                let Some(material) = material else {
                    start += 1;
                    continue;
                };
                let mut end = start + 1;
                while end < width - 1 && exposed_top_material(room, end, row) == Some(material) {
                    end += 1;
                }
                let backing_tiles = (start..end)
                    .map(|x| TerrainTileCoordinate { x, y: row })
                    .collect();
                let id = SupportFaceId::Horizontal {
                    row,
                    start_x: start,
                    end_x: end,
                    material,
                };
                horizontal_supports.push(HorizontalSupportFace {
                    id,
                    row,
                    start_x: start,
                    end_x: end,
                    material,
                    backing_tiles,
                });
                start = end;
            }
        }

        for column in 1..width - 1 {
            for side in [VerticalWallSide::Left, VerticalWallSide::Right] {
                let mut start = 1;
                while start < height - 1 {
                    if !exposed_solid_side(room, column, start, side) {
                        start += 1;
                        continue;
                    }
                    let mut end = start + 1;
                    while end < height - 1 && exposed_solid_side(room, column, end, side) {
                        end += 1;
                    }
                    let backing_tiles = (start..end)
                        .map(|y| TerrainTileCoordinate { x: column, y })
                        .collect();
                    let id = SupportFaceId::Vertical {
                        column,
                        start_y: start,
                        end_y: end,
                        side,
                    };
                    vertical_walls.push(VerticalWallFace {
                        id,
                        column,
                        start_y: start,
                        end_y: end,
                        side,
                        backing_tiles,
                    });
                    start = end;
                }
            }
        }
    }

    SupportFaceSegmentation {
        version: SUPPORT_FACE_SEGMENTATION_VERSION,
        disclaimer: SUPPORT_FACE_SEGMENTATION_DISCLAIMER.to_owned(),
        room_width_tiles: width,
        room_height_tiles: height,
        horizontal_supports,
        vertical_walls,
    }
}

fn exposed_top_material(room: &Room, x: u16, y: u16) -> Option<SupportSurfaceMaterial> {
    let material = SupportSurfaceMaterial::from_tile(room.tile(x, y)?)?;
    let covered = room
        .tile(x, y - 1)
        .and_then(SupportSurfaceMaterial::from_tile)
        .is_some();
    (!covered).then_some(material)
}

fn exposed_solid_side(room: &Room, x: u16, y: u16, side: VerticalWallSide) -> bool {
    if room.tile(x, y) != Some(Tile::Solid) {
        return false;
    }
    let neighbor_x = match side {
        VerticalWallSide::Left => x - 1,
        VerticalWallSide::Right => x + 1,
    };
    room.tile(neighbor_x, y) != Some(Tile::Solid)
}

/// Stable identity of one disjoint ablation unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationUnitId {
    pub ablation_version: u32,
    pub ordinal: u32,
    pub anchor: TerrainTileCoordinate,
}

/// Exact material expected at one tile owned by an ablation unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationTile {
    pub coordinate: TerrainTileCoordinate,
    pub material: SupportSurfaceMaterial,
}

/// A conservative, disjoint collection of face-backing tiles.
///
/// Faces which share any backing tile are joined transitively. This prevents
/// a corner tile from being independently removed once as a horizontal
/// support and again as a vertical wall. Tiles which have no exposed top or
/// side face are deliberately absent rather than guessed into a unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationUnit {
    pub id: SupportFaceAblationUnitId,
    pub member_faces: Vec<SupportFaceId>,
    pub tiles: Vec<SupportFaceAblationTile>,
    pub solid_tile_count: usize,
    pub one_way_tile_count: usize,
    pub attached_to_boundary_shell: bool,
}

/// Coalesce intersecting face records into canonical disjoint tile units.
#[must_use]
pub fn derive_support_face_ablation_units(room: &Room) -> Vec<SupportFaceAblationUnit> {
    let segmentation = segment_support_faces(room);
    let mut faces = segmentation
        .horizontal_supports
        .iter()
        .map(|face| (face.id.clone(), face.backing_tiles.as_slice()))
        .chain(
            segmentation
                .vertical_walls
                .iter()
                .map(|face| (face.id.clone(), face.backing_tiles.as_slice())),
        )
        .collect::<Vec<_>>();
    faces.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    if faces.is_empty() {
        return Vec::new();
    }

    let mut parents = (0..faces.len()).collect::<Vec<_>>();
    let mut first_owner = BTreeMap::<TerrainTileCoordinate, usize>::new();
    for (face_index, (_, tiles)) in faces.iter().enumerate() {
        for &tile in *tiles {
            if let Some(&other) = first_owner.get(&tile) {
                union_sets(&mut parents, face_index, other);
            } else {
                first_owner.insert(tile, face_index);
            }
        }
    }

    let mut grouped_faces = BTreeMap::<usize, BTreeSet<SupportFaceId>>::new();
    let mut grouped_tiles = BTreeMap::<usize, BTreeSet<TerrainTileCoordinate>>::new();
    for (face_index, (face_id, tiles)) in faces.iter().enumerate() {
        let root = find_set(&mut parents, face_index);
        grouped_faces
            .entry(root)
            .or_default()
            .insert(face_id.clone());
        grouped_tiles
            .entry(root)
            .or_default()
            .extend(tiles.iter().copied());
    }

    let mut pending = grouped_faces
        .into_iter()
        .map(|(root, member_faces)| {
            let coordinates = grouped_tiles
                .remove(&root)
                .expect("every face group owns backing tiles");
            let anchor = *coordinates
                .first()
                .expect("segmentation never emits an empty face");
            let tiles = coordinates
                .into_iter()
                .map(|coordinate| SupportFaceAblationTile {
                    coordinate,
                    material: SupportSurfaceMaterial::from_tile(
                        room.tile(coordinate.x, coordinate.y)
                            .expect("segmentation coordinates are in bounds"),
                    )
                    .expect("face backing cells are supporting terrain"),
                })
                .collect::<Vec<_>>();
            let solid_tile_count = tiles
                .iter()
                .filter(|tile| tile.material == SupportSurfaceMaterial::Solid)
                .count();
            let one_way_tile_count = tiles.len() - solid_tile_count;
            let attached_to_boundary_shell = tiles.iter().any(|tile| {
                tile_neighbors(room, tile.coordinate)
                    .into_iter()
                    .any(|neighbor| {
                        is_boundary(room, neighbor)
                            && SupportSurfaceMaterial::from_tile(
                                room.tile(neighbor.x, neighbor.y)
                                    .expect("neighbor coordinates are in bounds"),
                            )
                            .is_some()
                    })
            });
            (
                anchor,
                member_faces.into_iter().collect::<Vec<_>>(),
                tiles,
                solid_tile_count,
                one_way_tile_count,
                attached_to_boundary_shell,
            )
        })
        .collect::<Vec<_>>();
    pending.sort_unstable_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    pending
        .into_iter()
        .enumerate()
        .map(
            |(
                ordinal,
                (
                    anchor,
                    member_faces,
                    tiles,
                    solid_tile_count,
                    one_way_tile_count,
                    attached_to_boundary_shell,
                ),
            )| SupportFaceAblationUnit {
                id: SupportFaceAblationUnitId {
                    ablation_version: SUPPORT_FACE_ABLATION_VERSION,
                    ordinal: u32::try_from(ordinal)
                        .expect("a room has fewer than u32::MAX exposed faces"),
                    anchor,
                },
                member_faces,
                tiles,
                solid_tile_count,
                one_way_tile_count,
                attached_to_boundary_shell,
            },
        )
        .collect()
}

fn find_set(parents: &mut [usize], index: usize) -> usize {
    let parent = parents[index];
    if parent != index {
        parents[index] = find_set(parents, parent);
    }
    parents[index]
}

fn union_sets(parents: &mut [usize], left: usize, right: usize) {
    let left_root = find_set(parents, left);
    let right_root = find_set(parents, right);
    if left_root == right_root {
        return;
    }
    let (first, second) = if left_root < right_root {
        (left_root, right_root)
    } else {
        (right_root, left_root)
    };
    parents[second] = first;
}

fn is_boundary(room: &Room, coordinate: TerrainTileCoordinate) -> bool {
    coordinate.x == 0
        || coordinate.y == 0
        || coordinate.x + 1 == room.width()
        || coordinate.y + 1 == room.height()
}

fn tile_neighbors(room: &Room, coordinate: TerrainTileCoordinate) -> Vec<TerrainTileCoordinate> {
    let mut neighbors = Vec::with_capacity(4);
    if coordinate.x > 0 {
        neighbors.push(TerrainTileCoordinate {
            x: coordinate.x - 1,
            y: coordinate.y,
        });
    }
    if coordinate.x + 1 < room.width() {
        neighbors.push(TerrainTileCoordinate {
            x: coordinate.x + 1,
            y: coordinate.y,
        });
    }
    if coordinate.y > 0 {
        neighbors.push(TerrainTileCoordinate {
            x: coordinate.x,
            y: coordinate.y - 1,
        });
    }
    if coordinate.y + 1 < room.height() {
        neighbors.push(TerrainTileCoordinate {
            x: coordinate.x,
            y: coordinate.y + 1,
        });
    }
    neighbors
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportFaceNonConstructibleStage {
    UnitContract,
    RoomValidation,
    ObjectValidation,
    DoorValidation,
    PreservationContract,
}

/// Typed reason why an ablated room was not admitted for replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceNonConstructible {
    pub stage: SupportFaceNonConstructibleStage,
    pub detail: String,
}

/// Rebuild a room after removing one canonical disjoint unit.
///
/// Exact unit identity is checked against a fresh segmentation before any
/// mutation. The reconstruction then proves that all non-unit tiles, every
/// boundary tile, room identity, spawn, exits, doors, pickups, and hazards are
/// unchanged.
pub fn rebuild_without_support_face_unit(
    room: &Room,
    unit: &SupportFaceAblationUnit,
) -> Result<Room, SupportFaceNonConstructible> {
    let canonical = derive_support_face_ablation_units(room);
    if !canonical.iter().any(|candidate| candidate == unit) {
        return Err(non_constructible(
            SupportFaceNonConstructibleStage::UnitContract,
            "unit is not an exact member of the current room's canonical segmentation",
        ));
    }

    let removed = unit
        .tiles
        .iter()
        .map(|tile| tile.coordinate)
        .collect::<BTreeSet<_>>();
    let mut tiles = room.tiles().to_vec();
    for tile in &unit.tiles {
        if is_boundary(room, tile.coordinate) {
            return Err(non_constructible(
                SupportFaceNonConstructibleStage::UnitContract,
                "canonical unit unexpectedly contains a boundary tile",
            ));
        }
        let index = tile_index(room.width(), tile.coordinate);
        tiles[index] = Tile::Empty;
    }

    let rebuilt = Room::new(
        room.id(),
        room.name(),
        room.width(),
        room.height(),
        room.tile_size(),
        tiles,
        room.spawn(),
        room.exits().to_vec(),
    )
    .map_err(|error| {
        non_constructible(
            SupportFaceNonConstructibleStage::RoomValidation,
            error.to_string(),
        )
    })?
    .with_objects(room.timed_hazards().to_vec(), room.pickups().to_vec())
    .map_err(|error| {
        non_constructible(
            SupportFaceNonConstructibleStage::ObjectValidation,
            error.to_string(),
        )
    })?
    .with_doors(room.doors().to_vec())
    .map_err(|error| {
        non_constructible(
            SupportFaceNonConstructibleStage::DoorValidation,
            error.to_string(),
        )
    })?;

    validate_reconstruction(room, &rebuilt, &removed)?;
    Ok(rebuilt)
}

fn validate_reconstruction(
    original: &Room,
    rebuilt: &Room,
    removed: &BTreeSet<TerrainTileCoordinate>,
) -> Result<(), SupportFaceNonConstructible> {
    if original.id() != rebuilt.id()
        || original.name() != rebuilt.name()
        || original.width() != rebuilt.width()
        || original.height() != rebuilt.height()
        || original.tile_size() != rebuilt.tile_size()
        || original.spawn() != rebuilt.spawn()
        || original.exits() != rebuilt.exits()
        || original.doors() != rebuilt.doors()
        || original.pickups() != rebuilt.pickups()
        || original.timed_hazards() != rebuilt.timed_hazards()
    {
        return Err(non_constructible(
            SupportFaceNonConstructibleStage::PreservationContract,
            "room identity, dimensions, spawn, exits, doors, pickups, or hazards changed",
        ));
    }
    for y in 0..original.height() {
        for x in 0..original.width() {
            let coordinate = TerrainTileCoordinate { x, y };
            let expected = if removed.contains(&coordinate) {
                Tile::Empty
            } else {
                original
                    .tile(x, y)
                    .expect("coordinates are within original room")
            };
            if rebuilt.tile(x, y) != Some(expected) {
                return Err(non_constructible(
                    SupportFaceNonConstructibleStage::PreservationContract,
                    format!("tile ({x}, {y}) differs outside the exact removal contract"),
                ));
            }
            if is_boundary(original, coordinate) && rebuilt.tile(x, y) != original.tile(x, y) {
                return Err(non_constructible(
                    SupportFaceNonConstructibleStage::PreservationContract,
                    format!("immutable boundary tile ({x}, {y}) changed"),
                ));
            }
        }
    }
    Ok(())
}

fn non_constructible(
    stage: SupportFaceNonConstructibleStage,
    detail: impl Into<String>,
) -> SupportFaceNonConstructible {
    SupportFaceNonConstructible {
        stage,
        detail: detail.into(),
    }
}

fn tile_index(width: u16, coordinate: TerrainTileCoordinate) -> usize {
    usize::from(coordinate.y) * usize::from(width) + usize::from(coordinate.x)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SupportFaceControllerTarget {
    Door(String),
    Pickup(String),
}

impl SupportFaceControllerTarget {
    fn search_target(&self) -> SearchTarget {
        match self {
            Self::Door(id) => SearchTarget::door(id),
            Self::Pickup(id) => SearchTarget::pickup(id),
        }
    }

    fn reached_target(&self) -> ReachedTarget {
        match self {
            Self::Door(id) => ReachedTarget::Door(id.clone()),
            Self::Pickup(id) => ReachedTarget::Pickup(id.clone()),
        }
    }
}

/// Where one stored exact controller entered the known-positive inventory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SupportFaceControllerProvenance {
    CanonicalDoorMatrix {
        witness_fingerprint: u64,
        canonical_measurement_index: usize,
    },
    CanonicalPickupMatrix {
        witness_fingerprint: u64,
    },
    DirectController {
        source_batch_index: usize,
        route_index: usize,
        witness_index: usize,
        assessment_version: u32,
        direct_probe_audit_version: u32,
        semantic_trace_version: u32,
        controller_demand_version: u32,
    },
    ExternalCertified {
        stable_id: String,
    },
}

/// One claimed positive controller supplied to the lower-level audit API.
///
/// The claim is not trusted: the replay is verified and its exact typed target
/// is observed on the original room before it can enter any denominator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownPositiveSupportController {
    pub source_door_id: String,
    pub target: SupportFaceControllerTarget,
    pub loadout: EvaluationLoadout,
    pub replay: Replay,
    pub provenance: SupportFaceControllerProvenance,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceControllerId {
    pub identity_version: u32,
    pub source_door_id: String,
    pub target: SupportFaceControllerTarget,
    pub loadout: EvaluationLoadout,
    pub replay_fingerprint: u64,
    pub hash_collision_ordinal: u32,
}

/// Re-certified controller identity retained in the report. Exact duplicate
/// replays from multiple evidence sources are merged without hiding their
/// provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceControllerRecord {
    pub id: SupportFaceControllerId,
    pub provenances: Vec<SupportFaceControllerProvenance>,
    pub replay_ticks: usize,
    pub original_completion_tick: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceControllerInventorySummary {
    pub canonical_door_evidence_records: usize,
    pub canonical_pickup_evidence_records: usize,
    pub direct_controller_evidence_records: usize,
    pub external_evidence_records: usize,
    pub supplied_evidence_records: usize,
    pub unique_exact_controllers: usize,
    pub merged_duplicate_evidence_records: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupportFaceAblationDeathReason {
    Hazard { tile_x: u16, tile_y: u16 },
    TimedHazard { hazard_index: usize },
}

impl From<DeathReason> for SupportFaceAblationDeathReason {
    fn from(reason: DeathReason) -> Self {
        match reason {
            DeathReason::Hazard { tile_x, tile_y } => Self::Hazard { tile_x, tile_y },
            DeathReason::TimedHazard { hazard_index } => Self::TimedHazard { hazard_index },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SupportFaceUnexpectedTerminal {
    Door(String),
    LegacyExit(String),
}

/// Result of applying one exact stored action sequence to one constructible
/// ablated room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SupportFaceControllerAblationOutcome {
    /// Exact typed target reached at the original tick with no difference in
    /// player/attempt state, collected pickups, or emitted events beforehand.
    ExactBehaviorPreservedSuccess { completion_tick: usize },
    /// Exact typed target still reached, but observable behavior changed.
    ChangedBehaviorSuccess {
        completion_tick: usize,
        first_behavior_divergence_tick: usize,
    },
    Died {
        tick: usize,
        reason: SupportFaceAblationDeathReason,
        first_behavior_divergence_tick: Option<usize>,
    },
    WrongTarget {
        tick: usize,
        reached: SupportFaceUnexpectedTerminal,
        first_behavior_divergence_tick: Option<usize>,
    },
    ReplayExhausted {
        frames_replayed: usize,
        first_behavior_divergence_tick: Option<usize>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceControllerAblationObservation {
    pub controller_id: SupportFaceControllerId,
    pub outcome: SupportFaceControllerAblationOutcome,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceControllerOutcomeSummary {
    pub controller_count: usize,
    pub exact_behavior_preserved_successes: usize,
    pub changed_behavior_successes: usize,
    pub positive_redundancy_observations: usize,
    pub died: usize,
    pub wrong_target: usize,
    pub replay_exhausted: usize,
    pub non_success_observations: usize,
}

impl SupportFaceControllerOutcomeSummary {
    fn from_observations(observations: &[SupportFaceControllerAblationObservation]) -> Self {
        let mut summary = Self {
            controller_count: observations.len(),
            ..Self::default()
        };
        for observation in observations {
            match &observation.outcome {
                SupportFaceControllerAblationOutcome::ExactBehaviorPreservedSuccess { .. } => {
                    summary.exact_behavior_preserved_successes += 1;
                }
                SupportFaceControllerAblationOutcome::ChangedBehaviorSuccess { .. } => {
                    summary.changed_behavior_successes += 1;
                }
                SupportFaceControllerAblationOutcome::Died { .. } => summary.died += 1,
                SupportFaceControllerAblationOutcome::WrongTarget { .. } => {
                    summary.wrong_target += 1;
                }
                SupportFaceControllerAblationOutcome::ReplayExhausted { .. } => {
                    summary.replay_exhausted += 1;
                }
            }
        }
        summary.positive_redundancy_observations = summary
            .exact_behavior_preserved_successes
            .saturating_add(summary.changed_behavior_successes);
        summary.non_success_observations = summary
            .died
            .saturating_add(summary.wrong_target)
            .saturating_add(summary.replay_exhausted);
        summary
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "construction", rename_all = "snake_case")]
pub enum SupportFaceUnitAblationResult {
    Constructible {
        controller_observations: Vec<SupportFaceControllerAblationObservation>,
        controller_summary: SupportFaceControllerOutcomeSummary,
    },
    NonConstructible {
        reason: SupportFaceNonConstructible,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceUnitAblationAudit {
    pub unit: SupportFaceAblationUnit,
    pub result: SupportFaceUnitAblationResult,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationSummary {
    pub unit_count: usize,
    pub constructible_units: usize,
    pub non_constructible_units: usize,
    pub constructible_units_without_controllers: usize,
    pub units_all_controllers_exact_behavior_preserved: usize,
    pub units_all_controllers_still_succeeded: usize,
    pub units_with_changed_success: usize,
    pub units_with_non_success: usize,
    pub controller_experiments: usize,
    pub exact_behavior_preserved_successes: usize,
    pub changed_behavior_successes: usize,
    pub positive_redundancy_observations: usize,
    pub non_success_observations: usize,
}

impl SupportFaceAblationSummary {
    fn from_units(units: &[SupportFaceUnitAblationAudit]) -> Self {
        let mut summary = Self {
            unit_count: units.len(),
            ..Self::default()
        };
        for unit in units {
            match &unit.result {
                SupportFaceUnitAblationResult::NonConstructible { .. } => {
                    summary.non_constructible_units += 1;
                }
                SupportFaceUnitAblationResult::Constructible {
                    controller_summary, ..
                } => {
                    summary.constructible_units += 1;
                    if controller_summary.controller_count == 0 {
                        summary.constructible_units_without_controllers += 1;
                    } else {
                        summary.units_all_controllers_exact_behavior_preserved += usize::from(
                            controller_summary.exact_behavior_preserved_successes
                                == controller_summary.controller_count,
                        );
                        summary.units_all_controllers_still_succeeded += usize::from(
                            controller_summary.positive_redundancy_observations
                                == controller_summary.controller_count,
                        );
                    }
                    summary.units_with_changed_success +=
                        usize::from(controller_summary.changed_behavior_successes > 0);
                    summary.units_with_non_success +=
                        usize::from(controller_summary.non_success_observations > 0);
                    summary.controller_experiments = summary
                        .controller_experiments
                        .saturating_add(controller_summary.controller_count);
                    summary.exact_behavior_preserved_successes = summary
                        .exact_behavior_preserved_successes
                        .saturating_add(controller_summary.exact_behavior_preserved_successes);
                    summary.changed_behavior_successes = summary
                        .changed_behavior_successes
                        .saturating_add(controller_summary.changed_behavior_successes);
                    summary.positive_redundancy_observations = summary
                        .positive_redundancy_observations
                        .saturating_add(controller_summary.positive_redundancy_observations);
                    summary.non_success_observations = summary
                        .non_success_observations
                        .saturating_add(controller_summary.non_success_observations);
                }
            }
        }
        summary
    }
}

/// Audit work only. Solver effort used to discover inherited controllers is
/// intentionally absent and remains in its source evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationOperationalCost {
    pub original_replay_verifications: usize,
    pub original_replay_verification_ticks: usize,
    pub original_target_observation_ticks: usize,
    pub room_reconstruction_attempts: usize,
    pub constructible_room_variants: usize,
    pub non_constructible_room_variants: usize,
    pub controller_ablation_attempts: usize,
    pub original_comparison_ticks: usize,
    pub ablated_simulation_ticks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportFaceAblationReport {
    pub version: u32,
    pub disclaimer: String,
    pub room_id: String,
    pub segmentation: SupportFaceSegmentation,
    pub controller_inventory: Vec<SupportFaceControllerRecord>,
    pub controller_inventory_summary: SupportFaceControllerInventorySummary,
    pub units: Vec<SupportFaceUnitAblationAudit>,
    pub summary: SupportFaceAblationSummary,
    pub operational_cost: SupportFaceAblationOperationalCost,
}

#[derive(Clone)]
struct CertifiedController {
    id: SupportFaceControllerId,
    replay: Replay,
    original_completion_tick: usize,
}

#[derive(Clone)]
struct PendingController {
    source_door_id: String,
    target: SupportFaceControllerTarget,
    loadout: EvaluationLoadout,
    replay: Replay,
    provenances: Vec<SupportFaceControllerProvenance>,
}

/// Audit canonical disjoint units using caller-supplied claimed positives.
/// Every controller is independently re-certified on `room` first.
pub fn audit_support_face_ablations_for_controllers(
    room: &Room,
    controllers: &[KnownPositiveSupportController],
) -> Result<SupportFaceAblationReport, SupportFaceAblationError> {
    let segmentation = segment_support_faces(room);
    let units = derive_support_face_ablation_units(room);
    audit_support_face_units(room, segmentation, units, controllers)
}

/// Collect every known positive canonical door/pickup matrix witness and every
/// retained direct-controller witness, preserving its exact loadout, then run
/// the face-unit audit.
pub fn audit_known_support_face_ablations(
    evaluated: &EvaluatedCorpusRoom,
    analysis: &CorpusRoomAnalysis,
) -> Result<SupportFaceAblationReport, SupportFaceAblationError> {
    if evaluated.generated.id != analysis.room_id {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "evaluated room {} does not match analysis room {}",
                evaluated.generated.id.0, analysis.room_id.0
            ),
        });
    }
    let candidate = evaluated.generated.variants.first().ok_or_else(|| {
        SupportFaceAblationError::InvalidEvidence {
            detail: "evaluated room has no canonical candidate".to_owned(),
        }
    })?;
    validate_matrix_coordinates(&candidate.generated.room, &evaluated.matrices)?;
    let controllers = collect_known_controllers(&evaluated.matrices, analysis)?;
    audit_support_face_ablations_for_controllers(&candidate.generated.room, &controllers)
}

/// Run the known-controller support-face audit for a final-path physical room
/// using its validated post-feasibility canonical native candidate.
pub fn audit_known_support_face_ablations_v2(
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
) -> Result<SupportFaceAblationReport, SupportFaceAblationError> {
    let candidate = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        SupportFaceAblationError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    if analysis.room_id != evaluated.generated.id {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "evaluated room {} does not match analysis room {}",
                evaluated.generated.id.0, analysis.room_id.0
            ),
        });
    }
    validate_matrix_coordinates(&candidate.generated().room, &evaluated.matrices)?;
    let controllers = collect_known_controllers(&evaluated.matrices, analysis)?;
    audit_support_face_ablations_for_controllers(&candidate.generated().room, &controllers)
}

/// Explicit-candidate final-path entry point. The supplied candidate must
/// exactly equal the validated retained canonical candidate.
pub fn audit_known_support_face_ablations_for_corpus_candidate(
    candidate: &CorpusCandidate,
    evaluated: &EvaluatedCorpusRoomV2,
    analysis: &CorpusRoomAnalysis,
) -> Result<SupportFaceAblationReport, SupportFaceAblationError> {
    let canonical = resolve_corpus_metric_candidate_v2(evaluated).map_err(|source| {
        SupportFaceAblationError::CorpusV2Identity {
            source: Box::new(source),
        }
    })?;
    if candidate != canonical {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "explicit support-face candidate does not equal the validated canonical candidate for {}",
                evaluated.generated.id.0
            ),
        });
    }
    if analysis.room_id != evaluated.generated.id {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "evaluated room {} does not match analysis room {}",
                evaluated.generated.id.0, analysis.room_id.0
            ),
        });
    }
    validate_matrix_coordinates(&candidate.generated().room, &evaluated.matrices)?;
    let controllers = collect_known_controllers(&evaluated.matrices, analysis)?;
    audit_support_face_ablations_for_controllers(&candidate.generated().room, &controllers)
}

fn audit_support_face_units(
    room: &Room,
    segmentation: SupportFaceSegmentation,
    units: Vec<SupportFaceAblationUnit>,
    controllers: &[KnownPositiveSupportController],
) -> Result<SupportFaceAblationReport, SupportFaceAblationError> {
    let mut operational_cost = SupportFaceAblationOperationalCost::default();
    let (certified, controller_inventory, controller_inventory_summary) =
        certify_controllers(room, controllers, &mut operational_cost)?;
    let mut audits = Vec::with_capacity(units.len());
    for unit in units {
        operational_cost.room_reconstruction_attempts += 1;
        match rebuild_without_support_face_unit(room, &unit) {
            Err(reason) => {
                operational_cost.non_constructible_room_variants += 1;
                audits.push(SupportFaceUnitAblationAudit {
                    unit,
                    result: SupportFaceUnitAblationResult::NonConstructible { reason },
                });
            }
            Ok(ablated) => {
                operational_cost.constructible_room_variants += 1;
                let mut observations = Vec::with_capacity(certified.len());
                for controller in &certified {
                    operational_cost.controller_ablation_attempts += 1;
                    observations.push(replay_controller_on_ablation(
                        room,
                        &ablated,
                        controller,
                        &mut operational_cost,
                    )?);
                }
                let controller_summary =
                    SupportFaceControllerOutcomeSummary::from_observations(&observations);
                audits.push(SupportFaceUnitAblationAudit {
                    unit,
                    result: SupportFaceUnitAblationResult::Constructible {
                        controller_observations: observations,
                        controller_summary,
                    },
                });
            }
        }
    }
    let summary = SupportFaceAblationSummary::from_units(&audits);
    Ok(SupportFaceAblationReport {
        version: SUPPORT_FACE_ABLATION_VERSION,
        disclaimer: SUPPORT_FACE_ABLATION_DISCLAIMER.to_owned(),
        room_id: room.id().to_owned(),
        segmentation,
        controller_inventory,
        controller_inventory_summary,
        units: audits,
        summary,
        operational_cost,
    })
}

fn validate_matrix_coordinates(
    room: &Room,
    matrices: &[LoadoutRouteMatrix],
) -> Result<(), SupportFaceAblationError> {
    let door_ids = room
        .doors()
        .iter()
        .map(|door| door.id.clone())
        .collect::<BTreeSet<_>>();
    if door_ids.len() != room.doors().len() {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: "the exact room contains duplicate door IDs".to_owned(),
        });
    }
    let pickup_ids = room
        .pickups()
        .iter()
        .map(|pickup| pickup.id().to_owned())
        .collect::<BTreeSet<_>>();
    if pickup_ids.len() != room.pickups().len() {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: "the exact room contains duplicate pickup IDs".to_owned(),
        });
    }
    let expected_doors = door_ids
        .iter()
        .flat_map(|source| {
            door_ids
                .iter()
                .filter(move |target| *target != source)
                .map(move |target| (source.clone(), target.clone()))
        })
        .collect::<BTreeSet<_>>();
    let expected_pickups = door_ids
        .iter()
        .flat_map(|source| {
            pickup_ids
                .iter()
                .map(move |pickup| (source.clone(), pickup.clone()))
        })
        .collect::<BTreeSet<_>>();

    for loadout in EvaluationLoadout::ALL {
        let matching = matrices
            .iter()
            .filter(|matrix| matrix.loadout == loadout)
            .collect::<Vec<_>>();
        let [matrix] = matching.as_slice() else {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!(
                    "expected one {} canonical matrix, found {}",
                    loadout.slug(),
                    matching.len()
                ),
            });
        };
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!("{} matrix has a different physics loadout", loadout.slug()),
            });
        }
        let actual_doors = matrix
            .evidence
            .door_routes()
            .iter()
            .map(|row| (row.source_door_id.clone(), row.target_door_id.clone()))
            .collect::<BTreeSet<_>>();
        if actual_doors.len() != matrix.evidence.door_routes().len()
            || actual_doors != expected_doors
        {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!(
                    "{} matrix door rows do not exactly equal the room's directed door coordinates",
                    loadout.slug()
                ),
            });
        }
        let actual_pickups = matrix
            .evidence
            .pickup_routes()
            .iter()
            .map(|row| (row.source_door_id.clone(), row.required_pickup_id.clone()))
            .collect::<BTreeSet<_>>();
        if actual_pickups.len() != matrix.evidence.pickup_routes().len()
            || actual_pickups != expected_pickups
        {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!(
                    "{} matrix pickup rows do not exactly equal the room's source/pickup coordinates",
                    loadout.slug()
                ),
            });
        }
    }
    Ok(())
}

fn collect_known_controllers(
    matrices: &[LoadoutRouteMatrix],
    analysis: &CorpusRoomAnalysis,
) -> Result<Vec<KnownPositiveSupportController>, SupportFaceAblationError> {
    if analysis.version != CORPUS_ROOM_ANALYSIS_VERSION {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "room analysis version {} is unsupported; expected {}",
                analysis.version, CORPUS_ROOM_ANALYSIS_VERSION
            ),
        });
    }
    let mut controllers = Vec::new();
    let mut consumed_measurements = BTreeSet::new();
    for loadout in EvaluationLoadout::ALL {
        let matching_matrices = matrices
            .iter()
            .enumerate()
            .filter(|(_, matrix)| matrix.loadout == loadout)
            .collect::<Vec<_>>();
        let [(_, matrix)] = matching_matrices.as_slice() else {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!(
                    "expected one {} canonical matrix, found {}",
                    loadout.slug(),
                    matching_matrices.len()
                ),
            });
        };
        if matrix.evidence.loadout() != loadout.abilities() {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!("{} matrix has a different physics loadout", loadout.slug()),
            });
        }
        for row in matrix.evidence.door_routes() {
            let BoundedTargetEvidence::Positive(positive) = &row.evidence else {
                continue;
            };
            let target = SupportFaceControllerTarget::Door(row.target_door_id.clone());
            validate_solution_contract(
                &row.source_door_id,
                &target,
                positive.solution().target.clone(),
                positive.solution().reached.clone(),
            )?;
            let measurement_matches = analysis
                .canonical_route_measurements
                .iter()
                .enumerate()
                .filter(|(_, measurement)| {
                    measurement.loadout == loadout
                        && measurement.source_door_id == row.source_door_id
                        && measurement.target_door_id == row.target_door_id
                        && measurement.witness_fingerprint == positive.witness_fingerprint()
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [canonical_measurement_index] = measurement_matches.as_slice() else {
                return Err(SupportFaceAblationError::InvalidEvidence {
                    detail: format!(
                        "canonical positive {} -> {} under {} has {} matching landing/route measurements",
                        row.source_door_id,
                        row.target_door_id,
                        loadout.slug(),
                        measurement_matches.len()
                    ),
                });
            };
            consumed_measurements.insert(*canonical_measurement_index);
            controllers.push(KnownPositiveSupportController {
                source_door_id: row.source_door_id.clone(),
                target,
                loadout,
                replay: positive.solution().replay.clone(),
                provenance: SupportFaceControllerProvenance::CanonicalDoorMatrix {
                    witness_fingerprint: positive.witness_fingerprint().as_u64(),
                    canonical_measurement_index: *canonical_measurement_index,
                },
            });
        }
        for row in matrix.evidence.pickup_routes() {
            let BoundedTargetEvidence::Positive(positive) = &row.evidence else {
                continue;
            };
            let target = SupportFaceControllerTarget::Pickup(row.required_pickup_id.clone());
            validate_solution_contract(
                &row.source_door_id,
                &target,
                positive.solution().target.clone(),
                positive.solution().reached.clone(),
            )?;
            controllers.push(KnownPositiveSupportController {
                source_door_id: row.source_door_id.clone(),
                target,
                loadout,
                replay: positive.solution().replay.clone(),
                provenance: SupportFaceControllerProvenance::CanonicalPickupMatrix {
                    witness_fingerprint: positive.witness_fingerprint().as_u64(),
                },
            });
        }
    }
    if consumed_measurements.len() != analysis.canonical_route_measurements.len() {
        return Err(SupportFaceAblationError::InvalidEvidence {
            detail: format!(
                "{} of {} canonical landing/route measurements matched canonical positives",
                consumed_measurements.len(),
                analysis.canonical_route_measurements.len()
            ),
        });
    }

    for (source_batch_index, batch) in analysis.source_route_assessments.iter().enumerate() {
        for (route_index, route) in batch.routes.iter().enumerate() {
            if route.source_door_id != batch.source_door_id {
                return Err(SupportFaceAblationError::InvalidEvidence {
                    detail: format!(
                        "direct route {} in source batch {} has a different source",
                        route_index, source_batch_index
                    ),
                });
            }
            if route.policy != batch.policy {
                return Err(SupportFaceAblationError::InvalidEvidence {
                    detail: format!(
                        "direct route {} in source batch {} has a different policy identity",
                        route_index, source_batch_index
                    ),
                });
            }
            for (witness_index, witness) in route.easiest_first_witnesses.iter().enumerate() {
                controllers.push(KnownPositiveSupportController {
                    source_door_id: route.source_door_id.clone(),
                    target: SupportFaceControllerTarget::Door(route.target_door_id.clone()),
                    loadout: witness.loadout,
                    replay: witness.replay.clone(),
                    provenance: SupportFaceControllerProvenance::DirectController {
                        source_batch_index,
                        route_index,
                        witness_index,
                        assessment_version: route.policy.assessment_version,
                        direct_probe_audit_version: route.policy.direct_probe_audit_version,
                        semantic_trace_version: route.policy.semantic_trace_version,
                        controller_demand_version: route.policy.controller_demand_version,
                    },
                });
            }
        }
    }
    Ok(controllers)
}

fn validate_solution_contract(
    source_door_id: &str,
    target: &SupportFaceControllerTarget,
    reported_target: SearchTarget,
    reported_reached: ReachedTarget,
) -> Result<(), SupportFaceAblationError> {
    if reported_target == target.search_target() && reported_reached == target.reached_target() {
        return Ok(());
    }
    Err(SupportFaceAblationError::InvalidEvidence {
        detail: format!(
            "canonical controller from {source_door_id:?} claims {reported_target:?}/{reported_reached:?}, expected {:?}/{:?}",
            target.search_target(),
            target.reached_target()
        ),
    })
}

fn certify_controllers(
    room: &Room,
    controllers: &[KnownPositiveSupportController],
    operational_cost: &mut SupportFaceAblationOperationalCost,
) -> Result<
    (
        Vec<CertifiedController>,
        Vec<SupportFaceControllerRecord>,
        SupportFaceControllerInventorySummary,
    ),
    SupportFaceAblationError,
> {
    let mut inventory_summary = SupportFaceControllerInventorySummary {
        supplied_evidence_records: controllers.len(),
        ..SupportFaceControllerInventorySummary::default()
    };
    for controller in controllers {
        match &controller.provenance {
            SupportFaceControllerProvenance::CanonicalDoorMatrix { .. } => {
                inventory_summary.canonical_door_evidence_records += 1;
            }
            SupportFaceControllerProvenance::CanonicalPickupMatrix { .. } => {
                inventory_summary.canonical_pickup_evidence_records += 1;
            }
            SupportFaceControllerProvenance::DirectController { .. } => {
                inventory_summary.direct_controller_evidence_records += 1;
            }
            SupportFaceControllerProvenance::ExternalCertified { .. } => {
                inventory_summary.external_evidence_records += 1;
            }
        }
    }

    let mut ordered = controllers.to_vec();
    ordered.sort_unstable_by(|left, right| {
        left.source_door_id
            .cmp(&right.source_door_id)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.loadout.cmp(&right.loadout))
            .then_with(|| left.provenance.cmp(&right.provenance))
    });
    let mut seen_provenance = BTreeSet::new();
    let mut pending = Vec::<PendingController>::new();
    for controller in ordered {
        if !seen_provenance.insert(controller.provenance.clone()) {
            return Err(SupportFaceAblationError::InvalidEvidence {
                detail: format!(
                    "duplicate controller provenance {:?}",
                    controller.provenance
                ),
            });
        }
        if let Some(existing) = pending.iter_mut().find(|existing| {
            existing.source_door_id == controller.source_door_id
                && existing.target == controller.target
                && existing.loadout == controller.loadout
                && existing.replay == controller.replay
        }) {
            existing.provenances.push(controller.provenance);
        } else {
            pending.push(PendingController {
                source_door_id: controller.source_door_id,
                target: controller.target,
                loadout: controller.loadout,
                replay: controller.replay,
                provenances: vec![controller.provenance],
            });
        }
    }
    pending.sort_unstable_by(|left, right| {
        left.source_door_id
            .cmp(&right.source_door_id)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.loadout.cmp(&right.loadout))
            .then_with(|| replay_fingerprint(&left.replay).cmp(&replay_fingerprint(&right.replay)))
            .then_with(|| left.provenances.cmp(&right.provenances))
    });

    let mut collision_counts =
        BTreeMap::<(String, SupportFaceControllerTarget, EvaluationLoadout, u64), u32>::new();
    let mut certified = Vec::with_capacity(pending.len());
    let mut records = Vec::with_capacity(pending.len());
    for pending in pending {
        validate_controller_target_exists(room, &pending)?;
        let fingerprint = replay_fingerprint(&pending.replay);
        let collision_key = (
            pending.source_door_id.clone(),
            pending.target.clone(),
            pending.loadout,
            fingerprint,
        );
        let collision_count = collision_counts.entry(collision_key).or_default();
        let collision_ordinal = *collision_count;
        *collision_count += 1;
        let id = SupportFaceControllerId {
            identity_version: SUPPORT_FACE_ABLATION_VERSION,
            source_door_id: pending.source_door_id.clone(),
            target: pending.target.clone(),
            loadout: pending.loadout,
            replay_fingerprint: fingerprint,
            hash_collision_ordinal: collision_ordinal,
        };
        let initial = Simulation::enter_via_door(
            room.clone(),
            pending.loadout.abilities(),
            &pending.source_door_id,
        )
        .map_err(|source| SupportFaceAblationError::DoorEntry {
            controller: Box::new(id.clone()),
            source,
        })?;
        operational_cost.original_replay_verifications += 1;
        operational_cost.original_replay_verification_ticks = operational_cost
            .original_replay_verification_ticks
            .saturating_add(pending.replay.frames.len());
        pending.replay.verify(&initial).map_err(|source| {
            SupportFaceAblationError::OriginalReplayDiverged {
                controller: Box::new(id.clone()),
                source: Box::new(source),
            }
        })?;
        let original_completion_tick = observe_original_completion(
            &initial,
            &pending.replay,
            &id,
            &mut operational_cost.original_target_observation_ticks,
        )?;
        records.push(SupportFaceControllerRecord {
            id: id.clone(),
            provenances: pending.provenances,
            replay_ticks: pending.replay.frames.len(),
            original_completion_tick,
        });
        certified.push(CertifiedController {
            id,
            replay: pending.replay,
            original_completion_tick,
        });
    }
    inventory_summary.unique_exact_controllers = certified.len();
    inventory_summary.merged_duplicate_evidence_records = inventory_summary
        .supplied_evidence_records
        .saturating_sub(certified.len());
    Ok((certified, records, inventory_summary))
}

fn validate_controller_target_exists(
    room: &Room,
    controller: &PendingController,
) -> Result<(), SupportFaceAblationError> {
    let source_exists = room
        .doors()
        .iter()
        .any(|door| door.id == controller.source_door_id);
    let target_exists = match &controller.target {
        SupportFaceControllerTarget::Door(id) => room.doors().iter().any(|door| door.id == *id),
        SupportFaceControllerTarget::Pickup(id) => {
            room.pickups().iter().any(|pickup| pickup.id() == id)
        }
    };
    if source_exists && target_exists {
        return Ok(());
    }
    Err(SupportFaceAblationError::InvalidEvidence {
        detail: format!(
            "controller source {:?} exists={source_exists}, target {:?} exists={target_exists}",
            controller.source_door_id, controller.target
        ),
    })
}

fn observe_original_completion(
    initial: &Simulation,
    replay: &Replay,
    controller: &SupportFaceControllerId,
    observed_ticks: &mut usize,
) -> Result<usize, SupportFaceAblationError> {
    let mut simulation = initial.clone();
    for (frame_index, frame) in replay.frames.iter().enumerate() {
        let tick = frame_index + 1;
        let report = simulation.step(frame.action);
        *observed_ticks = observed_ticks.saturating_add(1);
        if let Some(reason) = first_death(&report.events) {
            return Err(SupportFaceAblationError::OriginalControllerDied {
                controller: Box::new(controller.clone()),
                tick,
                reason,
            });
        }
        if target_reached(&simulation, &controller.target) {
            return Ok(tick);
        }
        if let Some(reached) = wrong_terminal_target(&simulation, &controller.target) {
            return Err(SupportFaceAblationError::OriginalControllerHitWrongTarget {
                controller: Box::new(controller.clone()),
                tick,
                reached,
            });
        }
    }
    Err(SupportFaceAblationError::OriginalControllerMissedTarget {
        controller: Box::new(controller.clone()),
    })
}

fn replay_controller_on_ablation(
    original_room: &Room,
    ablated_room: &Room,
    controller: &CertifiedController,
    operational_cost: &mut SupportFaceAblationOperationalCost,
) -> Result<SupportFaceControllerAblationObservation, SupportFaceAblationError> {
    let abilities = controller.id.loadout.abilities();
    let mut original = Simulation::enter_via_door(
        original_room.clone(),
        abilities,
        &controller.id.source_door_id,
    )
    .map_err(|source| SupportFaceAblationError::DoorEntry {
        controller: Box::new(controller.id.clone()),
        source,
    })?;
    let mut ablated = Simulation::enter_via_door(
        ablated_room.clone(),
        abilities,
        &controller.id.source_door_id,
    )
    .map_err(|source| SupportFaceAblationError::DoorEntry {
        controller: Box::new(controller.id.clone()),
        source,
    })?;
    let mut first_behavior_divergence_tick =
        (!same_observable_state(&original, &ablated)).then_some(0);
    for (frame_index, frame) in controller.replay.frames.iter().enumerate() {
        let tick = frame_index + 1;
        let original_report = original.step(frame.action);
        let ablated_report = ablated.step(frame.action);
        operational_cost.original_comparison_ticks += 1;
        operational_cost.ablated_simulation_ticks += 1;
        if first_behavior_divergence_tick.is_none()
            && (original_report.events != ablated_report.events
                || !same_observable_state(&original, &ablated))
        {
            first_behavior_divergence_tick = Some(tick);
        }
        if let Some(reason) = first_death(&ablated_report.events) {
            return Ok(SupportFaceControllerAblationObservation {
                controller_id: controller.id.clone(),
                outcome: SupportFaceControllerAblationOutcome::Died {
                    tick,
                    reason: reason.into(),
                    first_behavior_divergence_tick,
                },
            });
        }
        if target_reached(&ablated, &controller.id.target) {
            let outcome = if first_behavior_divergence_tick.is_none()
                && tick == controller.original_completion_tick
            {
                SupportFaceControllerAblationOutcome::ExactBehaviorPreservedSuccess {
                    completion_tick: tick,
                }
            } else {
                SupportFaceControllerAblationOutcome::ChangedBehaviorSuccess {
                    completion_tick: tick,
                    first_behavior_divergence_tick: first_behavior_divergence_tick.expect(
                        "a changed target-completion tick changes observable exit/pickup state",
                    ),
                }
            };
            return Ok(SupportFaceControllerAblationObservation {
                controller_id: controller.id.clone(),
                outcome,
            });
        }
        if let Some(reached) = wrong_terminal_target(&ablated, &controller.id.target) {
            return Ok(SupportFaceControllerAblationObservation {
                controller_id: controller.id.clone(),
                outcome: SupportFaceControllerAblationOutcome::WrongTarget {
                    tick,
                    reached,
                    first_behavior_divergence_tick,
                },
            });
        }
    }
    Ok(SupportFaceControllerAblationObservation {
        controller_id: controller.id.clone(),
        outcome: SupportFaceControllerAblationOutcome::ReplayExhausted {
            frames_replayed: controller.replay.frames.len(),
            first_behavior_divergence_tick,
        },
    })
}

fn target_reached(simulation: &Simulation, target: &SupportFaceControllerTarget) -> bool {
    match target {
        SupportFaceControllerTarget::Door(id) => simulation.reached_exit() == Some(id),
        SupportFaceControllerTarget::Pickup(id) => simulation
            .room()
            .pickups()
            .iter()
            .position(|pickup| pickup.id() == id)
            .and_then(|index| simulation.pickup_is_collected(index))
            .unwrap_or(false),
    }
}

fn wrong_terminal_target(
    simulation: &Simulation,
    target: &SupportFaceControllerTarget,
) -> Option<SupportFaceUnexpectedTerminal> {
    let reached = simulation.reached_exit()?;
    if matches!(target, SupportFaceControllerTarget::Door(id) if id == reached) {
        return None;
    }
    if simulation
        .room()
        .doors()
        .iter()
        .any(|door| door.id == reached)
    {
        Some(SupportFaceUnexpectedTerminal::Door(reached.to_owned()))
    } else {
        Some(SupportFaceUnexpectedTerminal::LegacyExit(
            reached.to_owned(),
        ))
    }
}

fn first_death(events: &[SimulationEvent]) -> Option<DeathReason> {
    events.iter().find_map(|event| match event {
        SimulationEvent::Died(reason) => Some(*reason),
        _ => None,
    })
}

fn same_observable_state(left: &Simulation, right: &Simulation) -> bool {
    left.player() == right.player()
        && left.room_tick() == right.room_tick()
        && left.deaths() == right.deaths()
        && left.reached_exit() == right.reached_exit()
        && left
            .collected_pickups()
            .map(|pickup| pickup.id())
            .eq(right.collected_pickups().map(|pickup| pickup.id()))
}

fn replay_fingerprint(replay: &Replay) -> u64 {
    let mut hash = StableReplayHash::new();
    hash.u64(replay.initial_digest.0);
    hash.u64(replay.frames.len() as u64);
    for frame in &replay.frames {
        hash.byte(frame.action.move_x as u8);
        hash.byte(frame.action.move_y as u8);
        hash.byte(u8::from(frame.action.jump));
        hash.byte(u8::from(frame.action.dash));
        hash.byte(u8::from(frame.action.restart));
        hash.u64(frame.expected_digest.0);
        hash.u64(frame.expected_event_digest.0);
    }
    hash.finish()
}

struct StableReplayHash(u64);

impl StableReplayHash {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
pub enum SupportFaceAblationError {
    CorpusV2Identity {
        source: Box<CorpusMetricInputV2Error>,
    },
    InvalidEvidence {
        detail: String,
    },
    DoorEntry {
        controller: Box<SupportFaceControllerId>,
        source: DoorEntryError,
    },
    OriginalReplayDiverged {
        controller: Box<SupportFaceControllerId>,
        source: Box<ReplayDivergence>,
    },
    OriginalControllerDied {
        controller: Box<SupportFaceControllerId>,
        tick: usize,
        reason: DeathReason,
    },
    OriginalControllerHitWrongTarget {
        controller: Box<SupportFaceControllerId>,
        tick: usize,
        reached: SupportFaceUnexpectedTerminal,
    },
    OriginalControllerMissedTarget {
        controller: Box<SupportFaceControllerId>,
    },
}

impl fmt::Display for SupportFaceAblationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorpusV2Identity { source } => source.fmt(formatter),
            Self::InvalidEvidence { detail } => write!(formatter, "invalid evidence: {detail}"),
            Self::DoorEntry { controller, source } => write!(
                formatter,
                "cannot recreate source arrival for controller {controller:?}: {source}"
            ),
            Self::OriginalReplayDiverged { controller, source } => write!(
                formatter,
                "controller {controller:?} no longer verifies on the original room: {source}"
            ),
            Self::OriginalControllerDied {
                controller,
                tick,
                reason,
            } => write!(
                formatter,
                "controller {controller:?} died at tick {tick} on the original room: {reason:?}"
            ),
            Self::OriginalControllerHitWrongTarget {
                controller,
                tick,
                reached,
            } => write!(
                formatter,
                "controller {controller:?} hit {reached:?} at tick {tick} on the original room"
            ),
            Self::OriginalControllerMissedTarget { controller } => write!(
                formatter,
                "controller {controller:?} exhausted without its target on the original room"
            ),
        }
    }
}

impl Error for SupportFaceAblationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CorpusV2Identity { source } => Some(source.as_ref()),
            Self::DoorEntry { source, .. } => Some(source),
            Self::OriginalReplayDiverged { source, .. } => Some(source.as_ref()),
            Self::InvalidEvidence { .. }
            | Self::OriginalControllerDied { .. }
            | Self::OriginalControllerHitWrongTarget { .. }
            | Self::OriginalControllerMissedTarget { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::{DifficultyConfig, Replay, SolverConfig};
    use downwards_core::{
        AbilitySet, Action, BoundarySide, Door, Pickup, Point, Rect, TimedHazard,
    };

    use crate::corpus::{
        CorpusBuildConfigV1, CorpusRoomAnalysisConfig, analyze_corpus_room,
        evaluate_route_matrices, generate_seed_block,
    };

    use super::*;

    fn boundary_connected_fixture() -> Room {
        let width = 32_u16;
        let height = 18_u16;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        let mut set = |x: u16, y: u16, tile: Tile| {
            tiles[usize::from(y) * usize::from(width) + usize::from(x)] = tile;
        };
        for x in 0..width {
            set(x, 0, Tile::Solid);
            set(x, height - 1, Tile::Solid);
        }
        for y in 1..height - 1 {
            set(0, y, Tile::Solid);
            set(width - 1, y, Tile::Solid);
        }
        // A pillar joined to the floor boundary.
        for y in 12..height - 1 {
            set(14, y, Tile::Solid);
        }
        // A ledge joined to the left wall boundary.
        for x in 1..6 {
            set(x, 10, Tile::Solid);
        }
        // A separate one-way support.
        for x in 20..24 {
            set(x, 8, Tile::OneWay);
        }
        Room::new(
            "face-segmentation-fixture",
            "Face segmentation fixture",
            width,
            height,
            10,
            tiles,
            Point::new(20, 150),
            vec![],
        )
        .unwrap()
        .with_objects(
            vec![TimedHazard::new(Rect::new(100, 20, 10, 10), 60, 20, 0).unwrap()],
            vec![Pickup::new("fixture-cache", Rect::new(150, 30, 6, 6)).unwrap()],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".to_owned(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 130, 4, 40),
                arrival: Point::new(20, 150),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".to_owned(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 130, 4, 40),
                arrival: Point::new(292, 150),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    #[test]
    fn boundary_connected_extensions_remain_separately_addressable() {
        let room = boundary_connected_fixture();
        let report = segment_support_faces(&room);

        assert_eq!(report.version, SUPPORT_FACE_SEGMENTATION_VERSION);
        assert!(report.horizontal_supports.iter().any(|face| {
            face.id
                == (SupportFaceId::Horizontal {
                    row: 12,
                    start_x: 14,
                    end_x: 15,
                    material: SupportSurfaceMaterial::Solid,
                })
        }));
        assert!(report.horizontal_supports.iter().any(|face| {
            face.id
                == (SupportFaceId::Horizontal {
                    row: 10,
                    start_x: 1,
                    end_x: 6,
                    material: SupportSurfaceMaterial::Solid,
                })
        }));
        assert!(report.horizontal_supports.iter().any(|face| {
            face.id
                == (SupportFaceId::Horizontal {
                    row: 8,
                    start_x: 20,
                    end_x: 24,
                    material: SupportSurfaceMaterial::OneWay,
                })
        }));
        assert!(report.vertical_walls.iter().any(|face| {
            face.id
                == (SupportFaceId::Vertical {
                    column: 14,
                    start_y: 12,
                    end_y: 17,
                    side: VerticalWallSide::Left,
                })
        }));
    }

    #[test]
    fn immutable_boundary_shell_is_never_emitted() {
        let room = boundary_connected_fixture();
        let report = segment_support_faces(&room);

        assert!(report.horizontal_supports.iter().all(|face| {
            face.row > 0
                && face.row < room.height() - 1
                && face.start_x > 0
                && face.end_x < room.width()
        }));
        assert!(report.vertical_walls.iter().all(|face| {
            face.column > 0
                && face.column < room.width() - 1
                && face.start_y > 0
                && face.end_y < room.height()
        }));
        assert!(!report.horizontal_supports.iter().any(|face| face.row == 17));
    }

    #[test]
    fn segmentation_is_byte_stable_and_nonmutating() {
        let room = boundary_connected_fixture();
        let before = room.tiles().to_vec();
        let first = segment_support_faces(&room);
        let second = segment_support_faces(&room);

        assert_eq!(first, second);
        assert_eq!(room.tiles(), before);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
    }

    #[test]
    fn intersecting_faces_coalesce_into_disjoint_units_and_rebuild_exactly() {
        let room = boundary_connected_fixture();
        let units = derive_support_face_ablation_units(&room);
        assert_eq!(units.len(), 3);

        let mut all_tiles = BTreeSet::new();
        for unit in &units {
            assert!(!unit.member_faces.is_empty());
            assert!(!unit.tiles.is_empty());
            assert_eq!(
                unit.solid_tile_count + unit.one_way_tile_count,
                unit.tiles.len()
            );
            for tile in &unit.tiles {
                assert!(
                    all_tiles.insert(tile.coordinate),
                    "a backing tile must belong to exactly one coalesced unit"
                );
            }
            let rebuilt = rebuild_without_support_face_unit(&room, unit).unwrap();
            assert_eq!(rebuilt.id(), room.id());
            assert_eq!(rebuilt.doors(), room.doors());
            assert_eq!(rebuilt.pickups(), room.pickups());
            assert_eq!(rebuilt.timed_hazards(), room.timed_hazards());
            for x in 0..room.width() {
                assert_eq!(rebuilt.tile(x, 0), room.tile(x, 0));
                assert_eq!(
                    rebuilt.tile(x, room.height() - 1),
                    room.tile(x, room.height() - 1)
                );
            }
            for y in 0..room.height() {
                assert_eq!(rebuilt.tile(0, y), room.tile(0, y));
                assert_eq!(
                    rebuilt.tile(room.width() - 1, y),
                    room.tile(room.width() - 1, y)
                );
            }
        }

        let ledge = units
            .iter()
            .find(|unit| unit.id.anchor == TerrainTileCoordinate { x: 1, y: 10 })
            .unwrap();
        assert_eq!(ledge.tiles.len(), 5);
        assert!(ledge.attached_to_boundary_shell);
        let pillar = units
            .iter()
            .find(|unit| unit.id.anchor == TerrainTileCoordinate { x: 14, y: 12 })
            .unwrap();
        assert_eq!(pillar.tiles.len(), 5);
        assert!(pillar.attached_to_boundary_shell);

        let mut stale = ledge.clone();
        stale.tiles[0].coordinate = TerrainTileCoordinate { x: 0, y: 10 };
        let failure = rebuild_without_support_face_unit(&room, &stale).unwrap_err();
        assert_eq!(
            failure.stage,
            SupportFaceNonConstructibleStage::UnitContract
        );
    }

    #[test]
    fn exact_survival_changed_success_and_non_constructible_are_separate() {
        let room = controller_fixture();
        let initial = Simulation::enter_via_door(room.clone(), AbilitySet::NONE, "west").unwrap();
        let replay = replay_to_east(&initial);
        let controllers = vec![KnownPositiveSupportController {
            source_door_id: "west".to_owned(),
            target: SupportFaceControllerTarget::Door("east".to_owned()),
            loadout: EvaluationLoadout::Baseline,
            replay,
            provenance: SupportFaceControllerProvenance::ExternalCertified {
                stable_id: "walking-east".to_owned(),
            },
        }];
        let report = audit_support_face_ablations_for_controllers(&room, &controllers).unwrap();
        assert_eq!(report.controller_inventory.len(), 1);
        assert_eq!(report.summary.unit_count, 2);
        assert_eq!(report.summary.constructible_units, 2);
        assert_eq!(report.summary.non_constructible_units, 0);

        let decorative = report
            .units
            .iter()
            .find(|audit| audit.unit.id.anchor == TerrainTileCoordinate { x: 1, y: 5 })
            .unwrap();
        let SupportFaceUnitAblationResult::Constructible {
            controller_observations,
            controller_summary,
        } = &decorative.result
        else {
            panic!("canonical decorative unit must reconstruct");
        };
        assert_eq!(controller_summary.exact_behavior_preserved_successes, 1);
        assert!(matches!(
            &controller_observations[0].outcome,
            SupportFaceControllerAblationOutcome::ExactBehaviorPreservedSuccess { .. }
        ));

        let route_support = report
            .units
            .iter()
            .find(|audit| audit.unit.id.anchor == TerrainTileCoordinate { x: 1, y: 15 })
            .unwrap();
        let SupportFaceUnitAblationResult::Constructible {
            controller_observations,
            controller_summary,
        } = &route_support.result
        else {
            panic!("canonical route-support unit must reconstruct");
        };
        assert_eq!(
            controller_summary.changed_behavior_successes, 1,
            "route-support outcome: {:?}",
            controller_observations[0].outcome
        );
        assert!(matches!(
            &controller_observations[0].outcome,
            SupportFaceControllerAblationOutcome::ChangedBehaviorSuccess { .. }
        ));
        assert_eq!(report.summary.positive_redundancy_observations, 2);
        assert_eq!(report.operational_cost.controller_ablation_attempts, 2);

        let segmentation = segment_support_faces(&room);
        let mut stale_units = derive_support_face_ablation_units(&room);
        stale_units[0].tiles[0].coordinate = TerrainTileCoordinate { x: 0, y: 5 };
        let stale_report =
            audit_support_face_units(&room, segmentation, stale_units, &controllers).unwrap();
        assert_eq!(stale_report.summary.non_constructible_units, 1);
        assert_eq!(
            stale_report.summary.controller_experiments,
            stale_report.summary.constructible_units
        );
        assert_eq!(
            stale_report
                .operational_cost
                .non_constructible_room_variants,
            1
        );
        assert_eq!(
            stale_report.operational_cost.controller_ablation_attempts,
            stale_report.summary.controller_experiments
        );
        assert!(matches!(
            &stale_report.units[0].result,
            SupportFaceUnitAblationResult::NonConstructible { .. }
        ));
    }

    #[test]
    fn real_generated_room_audits_all_known_controller_sources_repeatably() {
        let mut generated =
            generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        let room = generated
            .rooms
            .iter()
            .find(|room| {
                room.variants[0].generated.room.doors().len() == 2
                    && !derive_support_face_ablation_units(&room.variants[0].generated.room)
                        .is_empty()
            })
            .cloned()
            .expect("seed zero has a two-door room with addressable faces");
        generated.rooms = vec![room];
        let evaluated = evaluate_route_matrices(generated)
            .unwrap()
            .rooms
            .pop()
            .unwrap();
        let analysis = analyze_corpus_room(
            &evaluated,
            &CorpusRoomAnalysisConfig {
                direct_controller_solver: SolverConfig {
                    max_expanded_nodes: 10_000,
                    max_simulated_ticks: 2_000_000,
                    max_ticks_per_path: 240,
                    ..SolverConfig::default()
                },
                canonical_witness_difficulty: DifficultyConfig::default(),
            },
        )
        .unwrap();
        let first = audit_known_support_face_ablations(&evaluated, &analysis).unwrap();
        let second = audit_known_support_face_ablations(&evaluated, &analysis).unwrap();
        assert_eq!(first, second);
        assert!(!first.units.is_empty());
        assert!(
            first
                .controller_inventory_summary
                .canonical_door_evidence_records
                > 0
        );
        assert!(
            first
                .controller_inventory_summary
                .direct_controller_evidence_records
                > 0
        );
        assert_eq!(
            first.summary.constructible_units + first.summary.non_constructible_units,
            first.summary.unit_count
        );
        assert_eq!(
            first.operational_cost.controller_ablation_attempts,
            first.summary.controller_experiments
        );
        assert_eq!(
            first.summary.positive_redundancy_observations + first.summary.non_success_observations,
            first.summary.controller_experiments
        );
    }

    fn controller_fixture() -> Room {
        let width = 32_u16;
        let height = 18_u16;
        let mut tiles = vec![Tile::Empty; usize::from(width) * usize::from(height)];
        let mut set = |x: u16, y: u16, tile: Tile| {
            tiles[usize::from(y) * usize::from(width) + usize::from(x)] = tile;
        };
        for x in 0..width {
            set(x, 0, Tile::Solid);
            set(x, height - 1, Tile::Solid);
        }
        for y in 1..height - 1 {
            set(0, y, Tile::Solid);
            set(width - 1, y, Tile::Solid);
        }
        for y in 13..=16 {
            set(0, y, Tile::Empty);
            set(width - 1, y, Tile::Empty);
        }
        for x in 1..6 {
            set(x, 5, Tile::Solid);
        }
        for x in 1..width - 1 {
            set(x, 15, Tile::OneWay);
        }
        Room::new(
            "support-controller-fixture",
            "Support controller fixture",
            width,
            height,
            10,
            tiles,
            Point::new(20, 138),
            vec![],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".to_owned(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 130, 4, 40),
                arrival: Point::new(20, 138),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".to_owned(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 130, 4, 40),
                arrival: Point::new(292, 138),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    fn replay_to_east(initial: &Simulation) -> Replay {
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        for _ in 0..400 {
            let action = Action {
                move_x: 1,
                ..Action::default()
            };
            simulation.step(action);
            actions.push(action);
            if simulation.reached_exit() == Some("east") {
                // Preserve a deterministic tail so the same controller can
                // still demonstrate changed-but-successful behavior after a
                // support removal delays its horizontal progress.
                actions.extend(std::iter::repeat_n(action, 80));
                return Replay::record(initial, actions);
            }
        }
        panic!("fixture controller did not reach east");
    }
}
