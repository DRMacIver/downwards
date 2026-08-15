//! Deterministic, self-contained SVG contact sheets for human corpus review.
//!
//! The renderer deliberately uses the same fixed 320-by-180 logical room
//! footprint for every card.  It is an inspection artifact, not a gameplay
//! renderer: colors emphasize distinctions that are useful while reviewing a
//! corpus (notably one-way terrain, pickups, door sides, and a selected source
//! side).

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Write as _},
};

use downwards_core::BoundarySide;
use downwards_lab::{
    CollisionTopologyDescriptor, DescriptorRect, StaticVisualDescriptor, VisualTile,
    collision_topology_distance, static_visual_distance,
};

use super::{
    CandidateKeyRecord, EvaluationLoadout, FeatureStageRecord, GeneratedCorpusBatch,
    GeneratedCorpusRoom, RoomId,
};

pub const VISUAL_AUDIT_VERSION: u32 = 1;
pub const DEFAULT_VISUAL_AUDIT_ROOMS_PER_PAGE: usize = 24;
pub const MAX_VISUAL_AUDIT_ROOMS_PER_PAGE: usize = 30;

const DEFAULT_COLUMNS: usize = 4;
const MAX_COLUMNS: usize = 6;
const LOGICAL_ROOM_WIDTH: usize = 320;
const LOGICAL_ROOM_HEIGHT: usize = 180;
const CARD_WIDTH: usize = 348;
const CARD_HEIGHT: usize = 272;
const CARD_GAP: usize = 12;
const PAGE_MARGIN: usize = 20;
const HEADER_HEIGHT: usize = 76;
const FOOTER_HEIGHT: usize = 28;
const ROOM_ID_LABEL_MAX_CHARS: usize = 45;
const STRATEGY_LABEL_MAX_CHARS: usize = 24;
const INTENT_LABEL_MAX_CHARS: usize = 14;
const NEIGHBOR_ID_MAX_CHARS: usize = 17;

/// Stable, owned input to the visual-audit renderer.
///
/// `source_side` is optional because raw generated rooms have no privileged
/// entrance.  A directed-route audit can set it to give every door on the
/// source boundary a white halo while retaining the door-side color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualAuditRecord {
    pub room_id: RoomId,
    pub seed: u64,
    pub construction_loadout: EvaluationLoadout,
    pub strategy: String,
    pub intent: String,
    pub feature_stage: FeatureStageRecord,
    pub source_side: Option<BoundarySide>,
    pub static_visual: StaticVisualDescriptor,
    pub collision_topology: CollisionTopologyDescriptor,
}

impl VisualAuditRecord {
    /// Build one record from the canonical constructive explanation retained
    /// for a generated physical room.
    pub fn from_generated(room: &GeneratedCorpusRoom) -> Result<Self, VisualAuditError> {
        let canonical =
            room.variants
                .first()
                .ok_or_else(|| VisualAuditError::MissingCanonicalVariant {
                    room_id: room.id.clone(),
                })?;
        let key = CandidateKeyRecord::from_staged_key(canonical.key);
        Ok(Self {
            room_id: room.id.clone(),
            seed: key.seed,
            construction_loadout: key.construction_loadout,
            strategy: key.strategy,
            intent: key.intent,
            feature_stage: key.feature_stage,
            source_side: None,
            static_visual: room.static_visual.clone(),
            collision_topology: CollisionTopologyDescriptor::from_room(&canonical.generated.room),
        })
    }

    #[must_use]
    pub const fn with_source_side(mut self, source_side: BoundarySide) -> Self {
        self.source_side = Some(source_side);
        self
    }
}

/// Page geometry.  The default is a four-by-six sheet containing 24 rooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualAuditOptions {
    pub rooms_per_page: usize,
    pub columns: usize,
}

impl Default for VisualAuditOptions {
    fn default() -> Self {
        Self {
            rooms_per_page: DEFAULT_VISUAL_AUDIT_ROOMS_PER_PAGE,
            columns: DEFAULT_COLUMNS,
        }
    }
}

impl VisualAuditOptions {
    pub fn new(rooms_per_page: usize, columns: usize) -> Result<Self, VisualAuditError> {
        let options = Self {
            rooms_per_page,
            columns,
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(self) -> Result<(), VisualAuditError> {
        if !(1..=MAX_VISUAL_AUDIT_ROOMS_PER_PAGE).contains(&self.rooms_per_page) {
            return Err(VisualAuditError::InvalidRoomsPerPage {
                rooms_per_page: self.rooms_per_page,
                maximum: MAX_VISUAL_AUDIT_ROOMS_PER_PAGE,
            });
        }
        if self.columns == 0 || self.columns > MAX_COLUMNS || self.columns > self.rooms_per_page {
            return Err(VisualAuditError::InvalidColumnCount {
                columns: self.columns,
                rooms_per_page: self.rooms_per_page,
                maximum: MAX_COLUMNS,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualAuditNeighborMetric {
    StaticVisual,
    CollisionTopology,
}

impl VisualAuditNeighborMetric {
    const fn label(self) -> &'static str {
        match self {
            Self::StaticVisual => "static",
            Self::CollisionTopology => "collision",
        }
    }
}

/// The nearest room under one lab distance.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualAuditNeighbor {
    pub room_id: RoomId,
    pub distance: f64,
}

/// Both nearest-neighbor views for one room.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualAuditNearestNeighbors {
    pub room_id: RoomId,
    pub static_visual: Option<VisualAuditNeighbor>,
    pub collision_topology: Option<VisualAuditNeighbor>,
}

/// One globally ranked, canonically oriented suspicious pair.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualAuditNeighborPair {
    pub metric: VisualAuditNeighborMetric,
    pub left_room_id: RoomId,
    pub right_room_id: RoomId,
    pub distance: f64,
}

/// One complete SVG page. Page indexes are zero-based in the API and
/// one-based in the rendered title and suggested filename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualAuditPage {
    pub page_index: usize,
    pub page_count: usize,
    pub rendered_rooms: usize,
    pub svg: String,
}

impl VisualAuditPage {
    #[must_use]
    pub fn suggested_file_name(&self) -> String {
        format!(
            "visual-audit-v{VISUAL_AUDIT_VERSION}-page-{:03}-of-{:03}.svg",
            self.page_index + 1,
            self.page_count
        )
    }
}

/// Output of the one-call generated-corpus API.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualAuditBundle {
    pub version: u32,
    pub nearest_neighbors: Vec<VisualAuditNearestNeighbors>,
    pub pages: Vec<VisualAuditPage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisualAuditError {
    MissingCanonicalVariant {
        room_id: RoomId,
    },
    DuplicateRoomId {
        room_id: RoomId,
    },
    InvalidRoomsPerPage {
        rooms_per_page: usize,
        maximum: usize,
    },
    InvalidColumnCount {
        columns: usize,
        rooms_per_page: usize,
        maximum: usize,
    },
    InvalidPairLayout {
        rooms_per_page: usize,
        columns: usize,
    },
    InvalidDescriptor {
        room_id: RoomId,
        detail: String,
    },
    PageOutOfBounds {
        page_index: usize,
        page_count: usize,
    },
}

impl fmt::Display for VisualAuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCanonicalVariant { room_id } => write!(
                formatter,
                "visual-audit room {:?} has no canonical generated variant",
                room_id.0
            ),
            Self::DuplicateRoomId { room_id } => {
                write!(formatter, "duplicate visual-audit room ID {:?}", room_id.0)
            }
            Self::InvalidRoomsPerPage {
                rooms_per_page,
                maximum,
            } => write!(
                formatter,
                "visual-audit rooms-per-page must be in 1..={maximum}, got {rooms_per_page}"
            ),
            Self::InvalidColumnCount {
                columns,
                rooms_per_page,
                maximum,
            } => write!(
                formatter,
                "visual-audit columns must be in 1..={maximum} and no greater than rooms-per-page {rooms_per_page}, got {columns}"
            ),
            Self::InvalidPairLayout {
                rooms_per_page,
                columns,
            } => write!(
                formatter,
                "pair contact sheets need even rooms-per-page and column counts, got {rooms_per_page} rooms in {columns} columns"
            ),
            Self::InvalidDescriptor { room_id, detail } => write!(
                formatter,
                "invalid visual-audit descriptor for room {:?}: {detail}",
                room_id.0
            ),
            Self::PageOutOfBounds {
                page_index,
                page_count,
            } => write!(
                formatter,
                "visual-audit page index {page_index} is out of bounds for {page_count} pages"
            ),
        }
    }
}

impl Error for VisualAuditError {}

/// Convert all generated physical rooms to stable visual-audit records.
pub fn visual_audit_records(
    batch: &GeneratedCorpusBatch,
) -> Result<Vec<VisualAuditRecord>, VisualAuditError> {
    let mut records = batch
        .rooms
        .iter()
        .map(VisualAuditRecord::from_generated)
        .collect::<Result<Vec<_>, _>>()?;
    validate_records(&records)?;
    records.sort_by(compare_records);
    Ok(records)
}

/// Render the catalogue contact sheets and calculate both neighbor lists.
pub fn render_generated_visual_audit(
    batch: &GeneratedCorpusBatch,
    options: VisualAuditOptions,
) -> Result<VisualAuditBundle, VisualAuditError> {
    let records = visual_audit_records(batch)?;
    let nearest_neighbors = list_visual_audit_nearest_neighbors(&records)?;
    let pages = render_catalogue_contact_sheets(&records, &nearest_neighbors, options)?;
    Ok(VisualAuditBundle {
        version: VISUAL_AUDIT_VERSION,
        nearest_neighbors,
        pages,
    })
}

/// List each room's nearest static and collision neighbor.
///
/// Equal distances are resolved by ascending room ID, independently of input
/// order. The output itself follows stable provenance order.
pub fn list_visual_audit_nearest_neighbors(
    records: &[VisualAuditRecord],
) -> Result<Vec<VisualAuditNearestNeighbors>, VisualAuditError> {
    let ordered = validated_order(records)?;
    let mut result = Vec::with_capacity(ordered.len());
    for (index, record) in ordered.iter().enumerate() {
        let mut nearest_static = None::<(f64, &VisualAuditRecord)>;
        let mut nearest_collision = None::<(f64, &VisualAuditRecord)>;
        for (candidate_index, candidate) in ordered.iter().enumerate() {
            if index == candidate_index {
                continue;
            }
            let static_distance =
                static_visual_distance(&record.static_visual, &candidate.static_visual).combined;
            retain_nearer(&mut nearest_static, static_distance, candidate);
            let collision_distance = collision_topology_distance(
                &record.collision_topology,
                &candidate.collision_topology,
            )
            .combined;
            retain_nearer(&mut nearest_collision, collision_distance, candidate);
        }
        result.push(VisualAuditNearestNeighbors {
            room_id: record.room_id.clone(),
            static_visual: nearest_static.map(|(distance, neighbor)| VisualAuditNeighbor {
                room_id: neighbor.room_id.clone(),
                distance,
            }),
            collision_topology: nearest_collision.map(|(distance, neighbor)| VisualAuditNeighbor {
                room_id: neighbor.room_id.clone(),
                distance,
            }),
        });
    }
    Ok(result)
}

/// Rank all room pairs from most to least similar under one lab distance.
///
/// Pair orientation and equal-distance ordering use ascending room IDs. Pass
/// `usize::MAX` to retain every pair.
pub fn rank_visual_audit_neighbor_pairs(
    records: &[VisualAuditRecord],
    metric: VisualAuditNeighborMetric,
    limit: usize,
) -> Result<Vec<VisualAuditNeighborPair>, VisualAuditError> {
    let ordered = validated_order(records)?;
    let mut pairs = Vec::with_capacity(
        ordered
            .len()
            .saturating_mul(ordered.len().saturating_sub(1))
            / 2,
    );
    for left_index in 0..ordered.len() {
        for right_index in left_index + 1..ordered.len() {
            let first = ordered[left_index];
            let second = ordered[right_index];
            let (left, right) = if first.room_id <= second.room_id {
                (first, second)
            } else {
                (second, first)
            };
            let distance = match metric {
                VisualAuditNeighborMetric::StaticVisual => {
                    static_visual_distance(&left.static_visual, &right.static_visual).combined
                }
                VisualAuditNeighborMetric::CollisionTopology => {
                    collision_topology_distance(&left.collision_topology, &right.collision_topology)
                        .combined
                }
            };
            pairs.push(VisualAuditNeighborPair {
                metric,
                left_room_id: left.room_id.clone(),
                right_room_id: right.room_id.clone(),
                distance,
            });
        }
    }
    pairs.sort_by(|left, right| {
        left.distance
            .total_cmp(&right.distance)
            .then_with(|| left.left_room_id.cmp(&right.left_room_id))
            .then_with(|| left.right_room_id.cmp(&right.right_room_id))
    });
    pairs.truncate(limit);
    Ok(pairs)
}

/// Render stable provenance-ordered catalogue pages.
pub fn render_visual_audit_contact_sheets(
    records: &[VisualAuditRecord],
    options: VisualAuditOptions,
) -> Result<Vec<VisualAuditPage>, VisualAuditError> {
    let neighbors = list_visual_audit_nearest_neighbors(records)?;
    render_catalogue_contact_sheets(records, &neighbors, options)
}

fn render_catalogue_contact_sheets(
    records: &[VisualAuditRecord],
    neighbors: &[VisualAuditNearestNeighbors],
    options: VisualAuditOptions,
) -> Result<Vec<VisualAuditPage>, VisualAuditError> {
    options.validate()?;
    let ordered = validated_order(records)?;
    let cards = ordered
        .into_iter()
        .map(|record| AuditCard {
            record,
            context: None,
        })
        .collect::<Vec<_>>();
    render_pages(&cards, neighbors, options, "generated-room catalogue")
}

/// Render one provenance-ordered page, rejecting indexes outside the complete
/// page set instead of silently producing an empty SVG.
pub fn render_visual_audit_page(
    records: &[VisualAuditRecord],
    options: VisualAuditOptions,
    page_index: usize,
) -> Result<VisualAuditPage, VisualAuditError> {
    let pages = render_visual_audit_contact_sheets(records, options)?;
    let page_count = pages.len();
    pages
        .into_iter()
        .nth(page_index)
        .ok_or(VisualAuditError::PageOutOfBounds {
            page_index,
            page_count,
        })
}

/// Render the closest pairs with the two members of every pair adjacent.
///
/// Records may therefore appear more than once. Even page and column counts
/// ensure that a pair is neither split between pages nor wrapped across rows.
pub fn render_visual_audit_suspicious_pair_sheets(
    records: &[VisualAuditRecord],
    metric: VisualAuditNeighborMetric,
    pair_limit: usize,
    options: VisualAuditOptions,
) -> Result<Vec<VisualAuditPage>, VisualAuditError> {
    options.validate()?;
    if !options.rooms_per_page.is_multiple_of(2) || !options.columns.is_multiple_of(2) {
        return Err(VisualAuditError::InvalidPairLayout {
            rooms_per_page: options.rooms_per_page,
            columns: options.columns,
        });
    }
    let ordered = validated_order(records)?;
    let by_id = ordered
        .iter()
        .map(|record| (record.room_id.0.as_str(), *record))
        .collect::<BTreeMap<_, _>>();
    let pairs = rank_visual_audit_neighbor_pairs(records, metric, pair_limit)?;
    let mut cards = Vec::with_capacity(pairs.len().saturating_mul(2));
    for (pair_index, pair) in pairs.iter().enumerate() {
        let context = format!(
            "pair {:03}  {} distance {:.6}",
            pair_index + 1,
            metric.label(),
            pair.distance
        );
        for id in [&pair.left_room_id, &pair.right_room_id] {
            let record = by_id
                .get(id.0.as_str())
                .expect("ranked pairs only reference validated input rooms");
            cards.push(AuditCard {
                record,
                context: Some(context.clone()),
            });
        }
    }
    let title = format!("closest {} pairs", metric.label());
    render_pages(&cards, &[], options, &title)
}

fn retain_nearer<'a>(
    retained: &mut Option<(f64, &'a VisualAuditRecord)>,
    distance: f64,
    candidate: &'a VisualAuditRecord,
) {
    let replace = retained.as_ref().is_none_or(|(best_distance, best)| {
        distance
            .total_cmp(best_distance)
            .then_with(|| candidate.room_id.cmp(&best.room_id))
            == Ordering::Less
    });
    if replace {
        *retained = Some((distance, candidate));
    }
}

fn validated_order(
    records: &[VisualAuditRecord],
) -> Result<Vec<&VisualAuditRecord>, VisualAuditError> {
    validate_records(records)?;
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| compare_records(left, right));
    Ok(ordered)
}

fn compare_records(left: &VisualAuditRecord, right: &VisualAuditRecord) -> Ordering {
    (
        left.seed,
        left.construction_loadout,
        left.strategy.as_str(),
        left.intent.as_str(),
        left.feature_stage,
        &left.room_id,
    )
        .cmp(&(
            right.seed,
            right.construction_loadout,
            right.strategy.as_str(),
            right.intent.as_str(),
            right.feature_stage,
            &right.room_id,
        ))
}

fn validate_records(records: &[VisualAuditRecord]) -> Result<(), VisualAuditError> {
    let mut ids = BTreeSet::new();
    for record in records {
        if !ids.insert(record.room_id.0.as_str()) {
            return Err(VisualAuditError::DuplicateRoomId {
                room_id: record.room_id.clone(),
            });
        }
        validate_descriptor(record)?;
    }
    Ok(())
}

fn validate_descriptor(record: &VisualAuditRecord) -> Result<(), VisualAuditError> {
    let visual = &record.static_visual;
    let expected_tiles = usize::from(visual.width)
        .checked_mul(usize::from(visual.height))
        .ok_or_else(|| invalid_descriptor(record, "static dimensions overflow"))?;
    if visual.width == 0 || visual.height == 0 || visual.tile_size <= 0 {
        return Err(invalid_descriptor(
            record,
            "static dimensions and tile size must be positive",
        ));
    }
    let pixel_width = i64::from(visual.width) * i64::from(visual.tile_size);
    let pixel_height = i64::from(visual.height) * i64::from(visual.tile_size);
    if pixel_width != LOGICAL_ROOM_WIDTH as i64 || pixel_height != LOGICAL_ROOM_HEIGHT as i64 {
        return Err(invalid_descriptor(
            record,
            format!(
                "static preview is {pixel_width}x{pixel_height} pixels, expected fixed {LOGICAL_ROOM_WIDTH}x{LOGICAL_ROOM_HEIGHT}"
            ),
        ));
    }
    if visual.tiles.len() != expected_tiles {
        return Err(invalid_descriptor(
            record,
            format!(
                "static tile count is {}, expected {expected_tiles}",
                visual.tiles.len()
            ),
        ));
    }

    let collision = &record.collision_topology;
    let expected_collision_cells = usize::from(collision.width)
        .checked_mul(usize::from(collision.height))
        .ok_or_else(|| invalid_descriptor(record, "collision dimensions overflow"))?;
    if collision.width == 0 || collision.height == 0 || collision.tile_size <= 0 {
        return Err(invalid_descriptor(
            record,
            "collision dimensions and tile size must be positive",
        ));
    }
    if collision.cells.len() != expected_collision_cells
        || collision.region_labels.len() != expected_collision_cells
    {
        return Err(invalid_descriptor(
            record,
            format!(
                "collision field has {} cells and {} labels, expected {expected_collision_cells} each",
                collision.cells.len(),
                collision.region_labels.len()
            ),
        ));
    }
    if visual.width != collision.width
        || visual.height != collision.height
        || visual.tile_size != collision.tile_size
    {
        return Err(invalid_descriptor(
            record,
            "static and collision dimensions do not match",
        ));
    }
    Ok(())
}

fn invalid_descriptor(record: &VisualAuditRecord, detail: impl Into<String>) -> VisualAuditError {
    VisualAuditError::InvalidDescriptor {
        room_id: record.room_id.clone(),
        detail: detail.into(),
    }
}

struct AuditCard<'a> {
    record: &'a VisualAuditRecord,
    context: Option<String>,
}

fn render_pages(
    cards: &[AuditCard<'_>],
    neighbors: &[VisualAuditNearestNeighbors],
    options: VisualAuditOptions,
    title: &str,
) -> Result<Vec<VisualAuditPage>, VisualAuditError> {
    options.validate()?;
    if cards.is_empty() {
        return Ok(Vec::new());
    }
    let page_count = cards.len().div_ceil(options.rooms_per_page);
    let mut pages = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        let start = page_index * options.rooms_per_page;
        let end = (start + options.rooms_per_page).min(cards.len());
        let svg = render_svg(
            &cards[start..end],
            neighbors,
            options,
            title,
            page_index,
            page_count,
            start,
            cards.len(),
        );
        pages.push(VisualAuditPage {
            page_index,
            page_count,
            rendered_rooms: end - start,
            svg,
        });
    }
    Ok(pages)
}

#[allow(clippy::too_many_arguments)]
fn render_svg(
    cards: &[AuditCard<'_>],
    neighbors: &[VisualAuditNearestNeighbors],
    options: VisualAuditOptions,
    title: &str,
    page_index: usize,
    page_count: usize,
    global_start: usize,
    total_cards: usize,
) -> String {
    let rows = options.rooms_per_page.div_ceil(options.columns);
    let page_width = PAGE_MARGIN * 2
        + options.columns * CARD_WIDTH
        + options.columns.saturating_sub(1) * CARD_GAP;
    let page_height =
        HEADER_HEIGHT + rows * CARD_HEIGHT + rows.saturating_sub(1) * CARD_GAP + FOOTER_HEIGHT;

    let mut svg = String::with_capacity(cards.len().saturating_mul(8_000));
    writeln!(svg, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{page_width}\" height=\"{page_height}\" viewBox=\"0 0 {page_width} {page_height}\" role=\"img\">"
    )
    .unwrap();
    svg.push_str("  <title>");
    push_xml_escaped(
        &mut svg,
        &format!(
            "Downwards visual audit: {title}, page {} of {page_count}",
            page_index + 1
        ),
    );
    svg.push_str("</title>\n");
    writeln!(
        svg,
        "  <metadata>downwards-visual-audit-v{VISUAL_AUDIT_VERSION}</metadata>"
    )
    .unwrap();
    svg.push_str(
        "  <style>\n\
         text{font-family:ui-monospace,SFMono-Regular,Consolas,\"Liberation Mono\",monospace}\n\
         .page-title{fill:#f8fafc;font-size:16px;font-weight:700}\n\
         .legend{fill:#cbd5e1;font-size:10px}\n\
         .room-id{fill:#f8fafc;font-size:11px;font-weight:700}\n\
         .provenance{fill:#aab4c8;font-size:8px}\n\
         .neighbor{fill:#cbd5e1;font-size:8px}\n\
         .footer{fill:#7f8ba3;font-size:9px}\n\
         </style>\n",
    );
    writeln!(
        svg,
        "  <rect width=\"{page_width}\" height=\"{page_height}\" fill=\"#080d19\"/>"
    )
    .unwrap();

    svg.push_str("  <text x=\"20\" y=\"24\" class=\"page-title\">");
    push_xml_escaped(
        &mut svg,
        &format!(
            "Human visual audit / {title} / page {} of {page_count} / cards {}-{} of {total_cards}",
            page_index + 1,
            global_start + 1,
            global_start + cards.len()
        ),
    );
    svg.push_str("</text>\n");
    render_legend(&mut svg);

    for slot in 0..options.rooms_per_page {
        let column = slot % options.columns;
        let row = slot / options.columns;
        let x = PAGE_MARGIN + column * (CARD_WIDTH + CARD_GAP);
        let y = HEADER_HEIGHT + row * (CARD_HEIGHT + CARD_GAP);
        if let Some(card) = cards.get(slot) {
            let nearest = neighbors
                .iter()
                .find(|entry| entry.room_id == card.record.room_id);
            render_card(&mut svg, card, nearest, page_index, slot, x, y);
        } else {
            writeln!(
                svg,
                "  <rect x=\"{x}\" y=\"{y}\" width=\"{CARD_WIDTH}\" height=\"{CARD_HEIGHT}\" rx=\"5\" fill=\"none\" stroke=\"#172033\" stroke-width=\"1\"/>"
            )
            .unwrap();
        }
    }

    let footer_y = page_height - 9;
    writeln!(
        svg,
        "  <text x=\"20\" y=\"{footer_y}\" class=\"footer\">visual audit v{VISUAL_AUDIT_VERSION} / fixed logical room scale {LOGICAL_ROOM_WIDTH}x{LOGICAL_ROOM_HEIGHT} / static preview colors are diagnostic, not gameplay presentation</text>"
    )
    .unwrap();
    svg.push_str("</svg>\n");
    svg
}

fn render_legend(svg: &mut String) {
    const ENTRIES: [(&str, &str); 8] = [
        ("#66758f", "solid"),
        ("#24c7d9", "one-way"),
        ("#ef5350", "hazard"),
        ("#f6d44a", "pickup"),
        ("#4299e1", "left door"),
        ("#f59e0b", "right door"),
        ("#a855f7", "ceiling door"),
        ("#10b981", "floor door"),
    ];
    let mut x = 20;
    for (color, label) in ENTRIES {
        writeln!(
            svg,
            "  <rect x=\"{x}\" y=\"40\" width=\"10\" height=\"10\" fill=\"{color}\"/>"
        )
        .unwrap();
        x += 14;
        writeln!(
            svg,
            "  <text x=\"{x}\" y=\"49\" class=\"legend\">{label}</text>"
        )
        .unwrap();
        x += label.len() * 7 + 14;
    }
    writeln!(
        svg,
        "  <rect x=\"{x}\" y=\"39\" width=\"12\" height=\"12\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2\"/>"
    )
    .unwrap();
    writeln!(
        svg,
        "  <text x=\"{}\" y=\"49\" class=\"legend\">source side</text>",
        x + 16
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn render_card(
    svg: &mut String,
    card: &AuditCard<'_>,
    nearest: Option<&VisualAuditNearestNeighbors>,
    page_index: usize,
    slot: usize,
    x: usize,
    y: usize,
) {
    let record = card.record;
    let clip_id = format!("card-p{page_index}-s{slot}");
    writeln!(
        svg,
        "  <defs><clipPath id=\"{clip_id}\"><rect x=\"{x}\" y=\"{y}\" width=\"{CARD_WIDTH}\" height=\"{CARD_HEIGHT}\" rx=\"5\"/></clipPath></defs>"
    )
    .unwrap();
    svg.push_str("  <g data-room-id=\"");
    push_xml_escaped(svg, &record.room_id.0);
    writeln!(svg, "\" clip-path=\"url(#{clip_id})\">").unwrap();
    writeln!(
        svg,
        "    <rect x=\"{x}\" y=\"{y}\" width=\"{CARD_WIDTH}\" height=\"{CARD_HEIGHT}\" rx=\"5\" fill=\"#111827\" stroke=\"#334155\" stroke-width=\"1\"/>"
    )
    .unwrap();

    let text_x = x + 14;
    svg.push_str(&format!(
        "    <text x=\"{text_x}\" y=\"{}\" class=\"room-id\">",
        y + 18
    ));
    push_xml_escaped(svg, &short_room_id_label(&record.room_id.0));
    svg.push_str("</text>\n");

    let (provenance, construction) = provenance_labels(record);
    svg.push_str(&format!(
        "    <text x=\"{text_x}\" y=\"{}\" class=\"provenance\">",
        y + 31
    ));
    push_xml_escaped(svg, &provenance);
    svg.push_str("</text>\n");
    svg.push_str(&format!(
        "    <text x=\"{text_x}\" y=\"{}\" class=\"provenance\">",
        y + 42
    ));
    push_xml_escaped(svg, &construction);
    svg.push_str("</text>\n");

    let room_x = x + 14;
    let room_y = y + 50;
    render_room(svg, record, room_x, room_y);

    let context = card.context.clone().unwrap_or_else(|| {
        nearest.map_or_else(
            || "nearest static - / collision -".to_owned(),
            |nearest| {
                format!(
                    "near S {} / C {}",
                    neighbor_label(nearest.static_visual.as_ref()),
                    neighbor_label(nearest.collision_topology.as_ref())
                )
            },
        )
    });
    svg.push_str(&format!(
        "    <text x=\"{text_x}\" y=\"{}\" class=\"neighbor\">",
        y + 257
    ));
    push_xml_escaped(svg, &context);
    svg.push_str("</text>\n  </g>\n");
}

fn neighbor_label(neighbor: Option<&VisualAuditNeighbor>) -> String {
    neighbor.map_or_else(
        || "-".to_owned(),
        |neighbor| {
            format!(
                "{} {:.6}",
                compact_room_id(&neighbor.room_id.0),
                neighbor.distance
            )
        },
    )
}

fn provenance_labels(record: &VisualAuditRecord) -> (String, String) {
    let provenance = format!(
        "seed {:016x}  {} / {}",
        record.seed,
        ellipsize_middle(&record.strategy, STRATEGY_LABEL_MAX_CHARS),
        ellipsize_middle(&record.intent, INTENT_LABEL_MAX_CHARS),
    );
    let construction = format!(
        "{}  {}  source {}",
        record.construction_loadout.slug(),
        feature_stage_slug(record.feature_stage),
        record.source_side.map_or("-", boundary_side_label),
    );
    (provenance, construction)
}

fn short_room_id_label(room_id: &str) -> String {
    generated_room_fingerprints(room_id).map_or_else(
        || ellipsize_middle(room_id, ROOM_ID_LABEL_MAX_CHARS),
        |(version, static_fingerprint, simulation_fingerprint)| {
            format!("room-v{version} {static_fingerprint}/{simulation_fingerprint}")
        },
    )
}

fn compact_room_id(room_id: &str) -> String {
    generated_room_fingerprints(room_id).map_or_else(
        || ellipsize_middle(room_id, NEIGHBOR_ID_MAX_CHARS),
        |(_, static_fingerprint, simulation_fingerprint)| {
            format!(
                "{}/{}",
                &static_fingerprint[..8],
                &simulation_fingerprint[..8]
            )
        },
    )
}

fn generated_room_fingerprints(room_id: &str) -> Option<(&str, &str, &str)> {
    let remainder = room_id.strip_prefix("room-v")?;
    let (version, remainder) = remainder.split_once('-')?;
    let (static_fingerprint, remainder) = remainder.split_once('-')?;
    let (simulation_fingerprint, _) = remainder.split_once('-')?;
    (!version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_digit())
        && static_fingerprint.len() == 16
        && simulation_fingerprint.len() == 16
        && static_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && simulation_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()))
    .then_some((version, static_fingerprint, simulation_fingerprint))
}

fn ellipsize_middle(value: &str, maximum_chars: usize) -> String {
    let character_count = value.chars().count();
    if character_count <= maximum_chars {
        return value.to_owned();
    }
    if maximum_chars == 0 {
        return String::new();
    }
    if maximum_chars == 1 {
        return "…".to_owned();
    }
    let retained = maximum_chars - 1;
    let left_count = retained.div_ceil(2);
    let right_count = retained - left_count;
    let mut result = value.chars().take(left_count).collect::<String>();
    result.push('…');
    let mut suffix = value.chars().rev().take(right_count).collect::<Vec<_>>();
    suffix.reverse();
    result.extend(suffix);
    result
}

fn render_room(svg: &mut String, record: &VisualAuditRecord, x: usize, y: usize) {
    let visual = &record.static_visual;
    let pixel_width = i64::from(visual.width) * i64::from(visual.tile_size);
    let pixel_height = i64::from(visual.height) * i64::from(visual.tile_size);
    svg.push_str(&format!(
        "    <svg x=\"{x}\" y=\"{y}\" width=\"{LOGICAL_ROOM_WIDTH}\" height=\"{LOGICAL_ROOM_HEIGHT}\" viewBox=\"0 0 {pixel_width} {pixel_height}\" preserveAspectRatio=\"none\" aria-label=\""
    ));
    push_xml_escaped(svg, &format!("room preview {}", record.room_id.0));
    svg.push_str("\">\n");
    writeln!(
        svg,
        "      <rect width=\"{pixel_width}\" height=\"{pixel_height}\" fill=\"#050912\"/>"
    )
    .unwrap();

    for (index, tile) in visual.tiles.iter().copied().enumerate() {
        if tile == VisualTile::Empty {
            continue;
        }
        let tile_x = (index % usize::from(visual.width)) as i64 * i64::from(visual.tile_size);
        let tile_y = (index / usize::from(visual.width)) as i64 * i64::from(visual.tile_size);
        match tile {
            VisualTile::Empty => {}
            VisualTile::Solid => {
                writeln!(
                    svg,
                    "      <rect x=\"{tile_x}\" y=\"{tile_y}\" width=\"{}\" height=\"{}\" fill=\"#66758f\" stroke=\"#8290aa\" stroke-width=\"0.35\"/>",
                    visual.tile_size, visual.tile_size
                )
                .unwrap();
            }
            VisualTile::HazardUp
            | VisualTile::HazardDown
            | VisualTile::HazardLeft
            | VisualTile::HazardRight => {
                writeln!(
                    svg,
                    "      <rect x=\"{tile_x}\" y=\"{tile_y}\" width=\"{}\" height=\"{}\" fill=\"#ef5350\"/>",
                    visual.tile_size, visual.tile_size
                )
                .unwrap();
                let tile_right = tile_x + i64::from(visual.tile_size);
                let tile_bottom = tile_y + i64::from(visual.tile_size);
                let center_x = tile_x + i64::from(visual.tile_size) / 2;
                let center_y = tile_y + i64::from(visual.tile_size) / 2;
                let points = match tile {
                    VisualTile::HazardUp => {
                        format!(
                            "{tile_x},{tile_bottom} {center_x},{tile_y} {tile_right},{tile_bottom}"
                        )
                    }
                    VisualTile::HazardDown => {
                        format!("{tile_x},{tile_y} {tile_right},{tile_y} {center_x},{tile_bottom}")
                    }
                    VisualTile::HazardLeft => {
                        format!(
                            "{tile_right},{tile_y} {tile_x},{center_y} {tile_right},{tile_bottom}"
                        )
                    }
                    VisualTile::HazardRight => {
                        format!("{tile_x},{tile_y} {tile_right},{center_y} {tile_x},{tile_bottom}")
                    }
                    VisualTile::Empty | VisualTile::Solid | VisualTile::OneWay => unreachable!(),
                };
                writeln!(
                    svg,
                    "      <polygon points=\"{points}\" fill=\"#ef5350\" stroke=\"#7f1d1d\" stroke-width=\"0.8\"/>",
                )
                .unwrap();
            }
            VisualTile::OneWay => {
                writeln!(
                    svg,
                    "      <rect x=\"{tile_x}\" y=\"{tile_y}\" width=\"{}\" height=\"{}\" fill=\"#24c7d9\" fill-opacity=\"0.16\"/>",
                    visual.tile_size, visual.tile_size
                )
                .unwrap();
                writeln!(
                    svg,
                    "      <path d=\"M {tile_x} {} H {}\" fill=\"none\" stroke=\"#24c7d9\" stroke-width=\"2\"/>",
                    tile_y + 1,
                    tile_x + i64::from(visual.tile_size),
                )
                .unwrap();
            }
        }
    }

    for bounds in &visual.timed_hazards {
        render_rect(svg, *bounds, "#f472b6", "#f9a8d4", "0.22", "2", Some("4 2"));
    }
    for bounds in &visual.exits {
        render_rect(svg, *bounds, "#84cc16", "#bef264", "0.18", "2", None);
    }
    for bounds in &visual.pickups {
        render_pickup(svg, *bounds);
    }
    for door in &visual.doors {
        let color = boundary_side_color(door.side);
        let source = record.source_side == Some(door.side);
        if source {
            render_rect(
                svg,
                door.trigger_bounds,
                color,
                "#ffffff",
                "0.32",
                "5",
                None,
            );
        }
        render_rect(svg, door.trigger_bounds, color, color, "0.35", "2", None);
    }
    render_source_side(svg, record.source_side, pixel_width, pixel_height);

    let spawn_x = visual.spawn.x;
    let spawn_y = visual.spawn.y;
    writeln!(
        svg,
        "      <circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"none\" stroke=\"#f8fafc\" stroke-width=\"1.2\"/>",
        spawn_x + 4,
        spawn_y + 4
    )
    .unwrap();
    writeln!(
        svg,
        "      <path d=\"M {} {} H {} M {} {} V {}\" stroke=\"#f8fafc\" stroke-width=\"0.8\"/>",
        spawn_x + 1,
        spawn_y + 4,
        spawn_x + 7,
        spawn_x + 4,
        spawn_y + 1,
        spawn_y + 7,
    )
    .unwrap();
    writeln!(
        svg,
        "      <rect x=\"0.75\" y=\"0.75\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#d8dee9\" stroke-width=\"1.5\"/>",
        pixel_width - 1,
        pixel_height - 1
    )
    .unwrap();
    svg.push_str("    </svg>\n");
}

fn render_rect(
    svg: &mut String,
    bounds: DescriptorRect,
    fill: &str,
    stroke: &str,
    fill_opacity: &str,
    stroke_width: &str,
    dash: Option<&str>,
) {
    write!(
        svg,
        "      <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" fill-opacity=\"{fill_opacity}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\"",
        bounds.x, bounds.y, bounds.width, bounds.height
    )
    .unwrap();
    if let Some(dash) = dash {
        write!(svg, " stroke-dasharray=\"{dash}\"").unwrap();
    }
    svg.push_str("/>\n");
}

fn render_pickup(svg: &mut String, bounds: DescriptorRect) {
    let center_x = bounds.x + bounds.width / 2;
    let center_y = bounds.y + bounds.height / 2;
    let radius = (bounds.width.min(bounds.height) / 2).max(1);
    writeln!(
        svg,
        "      <path d=\"M {center_x} {} L {} {center_y} L {center_x} {} L {} {center_y} Z\" fill=\"#f6d44a\" stroke=\"#fff3a3\" stroke-width=\"1\"/>",
        center_y - radius,
        center_x + radius,
        center_y + radius,
        center_x - radius,
    )
    .unwrap();
}

fn render_source_side(
    svg: &mut String,
    source_side: Option<BoundarySide>,
    width: i64,
    height: i64,
) {
    let Some(source_side) = source_side else {
        return;
    };
    let path = match source_side {
        BoundarySide::Left => format!("M 2 0 V {height}"),
        BoundarySide::Right => format!("M {} 0 V {height}", width - 2),
        BoundarySide::Ceiling => format!("M 0 2 H {width}"),
        BoundarySide::Floor => format!("M 0 {} H {width}", height - 2),
    };
    writeln!(
        svg,
        "      <path d=\"{path}\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"3\" data-source-side=\"{}\"/>",
        boundary_side_label(source_side)
    )
    .unwrap();
}

const fn boundary_side_color(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "#4299e1",
        BoundarySide::Right => "#f59e0b",
        BoundarySide::Ceiling => "#a855f7",
        BoundarySide::Floor => "#10b981",
    }
}

const fn boundary_side_label(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "left",
        BoundarySide::Right => "right",
        BoundarySide::Ceiling => "ceiling",
        BoundarySide::Floor => "floor",
    }
}

const fn feature_stage_slug(stage: FeatureStageRecord) -> &'static str {
    match stage {
        FeatureStageRecord::TerrainOnly => "terrain-only",
        FeatureStageRecord::StaticHazards => "static-hazards",
        FeatureStageRecord::TimedHazards => "timed-hazards",
    }
}

/// Escape text for either SVG text content or a double-quoted XML attribute.
/// XML 1.0-disallowed control characters are replaced deterministically.
fn push_xml_escaped(output: &mut String, input: &str) {
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            '\t' => output.push_str("&#9;"),
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}' => {
                output.push(character);
            }
            _ => output.push_str("&#xFFFD;"),
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{Door, Pickup, Point, Rect, Room, Tile};

    use super::*;

    #[test]
    fn svg_is_byte_repeatable_and_independent_of_record_order() {
        let first = record("room-c", 3, 4, false);
        let second = record("room-a", 1, 2, true);
        let third = record("room-b", 2, 3, false);
        let records = vec![first.clone(), second.clone(), third.clone()];
        let reversed = vec![third, second, first];

        let forward =
            render_visual_audit_contact_sheets(&records, VisualAuditOptions::default()).unwrap();
        let again =
            render_visual_audit_contact_sheets(&records, VisualAuditOptions::default()).unwrap();
        let backward =
            render_visual_audit_contact_sheets(&reversed, VisualAuditOptions::default()).unwrap();

        assert_eq!(forward, again);
        assert_eq!(forward, backward);
        assert_eq!(
            forward[0].suggested_file_name(),
            "visual-audit-v1-page-001-of-001.svg"
        );
        assert!(forward[0].svg.contains("#24c7d9"));
        assert!(forward[0].svg.contains("#f6d44a"));
        assert!(forward[0].svg.contains("data-source-side=\"left\""));
    }

    #[test]
    fn dynamic_xml_text_and_attributes_are_safely_escaped() {
        let mut escaped = record("room<&\"'>\u{0}", 7, 1, false);
        escaped.strategy = "strategy<&\"'>\n".to_owned();
        escaped.intent = "intent<&\"'>".to_owned();

        let page = render_visual_audit_page(&[escaped], VisualAuditOptions::default(), 0).unwrap();

        assert!(page.svg.contains("room&lt;&amp;&quot;&apos;&gt;&#xFFFD;"));
        assert!(page.svg.contains("strategy&lt;&amp;&quot;&apos;&gt;&#10;"));
        assert!(!page.svg.contains("room<&"));
        assert!(!page.svg.contains("strategy<&"));
    }

    #[test]
    fn longest_generated_ids_are_shortened_and_visible_card_text_stays_bounded() {
        let first_id = format!(
            "room-v2-0123456789abcdef-fedcba9876543210-s{}",
            "candidate-provenance".repeat(40)
        );
        let second_id = format!(
            "room-v2-1111111111111111-2222222222222222-s{}",
            "other-provenance".repeat(40)
        );
        let mut first = record(&first_id, 1, 1, false);
        first.strategy = "strategy-with-an-intentionally-enormous-human-label".repeat(8);
        first.intent = "intent-with-an-intentionally-enormous-human-label".repeat(8);
        let second = record(&second_id, 2, 2, false);

        let page = render_visual_audit_page(
            &[first.clone(), second],
            VisualAuditOptions::new(2, 2).unwrap(),
            0,
        )
        .unwrap();
        let short_id = short_room_id_label(&first_id);
        let compact_id = compact_room_id(&first_id);
        let (provenance, construction) = provenance_labels(&first);

        assert_eq!(short_id, "room-v2 0123456789abcdef/fedcba9876543210");
        assert_eq!(compact_id, "01234567/fedcba98");
        assert!(short_id.chars().count() <= ROOM_ID_LABEL_MAX_CHARS);
        assert!(compact_id.chars().count() <= NEIGHBOR_ID_MAX_CHARS);
        assert!(provenance.chars().count() <= 64);
        assert!(construction.chars().count() <= 48);
        assert!(
            page.svg
                .contains(&format!("class=\"room-id\">{short_id}</text>"))
        );
        assert!(
            !page
                .svg
                .contains(&format!("class=\"room-id\">{first_id}</text>"))
        );
        assert_eq!(page.svg.matches("class=\"room-id\"").count(), 2);
        assert_eq!(page.svg.matches("clip-path=\"url(#card-").count(), 2);
    }

    #[test]
    fn pagination_is_bounded_and_keeps_a_fixed_page_extent() {
        let records = (0..25)
            .map(|index| record(&format!("room-{index:02}"), index as u64, index, false))
            .collect::<Vec<_>>();
        let pages =
            render_visual_audit_contact_sheets(&records, VisualAuditOptions::default()).unwrap();

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].rendered_rooms, 24);
        assert_eq!(pages[1].rendered_rooms, 1);
        let first_extent = pages[0]
            .svg
            .lines()
            .find(|line| line.starts_with("<svg "))
            .unwrap();
        let second_extent = pages[1]
            .svg
            .lines()
            .find(|line| line.starts_with("<svg "))
            .unwrap();
        assert_eq!(first_extent, second_extent);
        assert_eq!(
            render_visual_audit_page(&records, VisualAuditOptions::default(), 2).unwrap_err(),
            VisualAuditError::PageOutOfBounds {
                page_index: 2,
                page_count: 2,
            }
        );
        assert!(matches!(
            VisualAuditOptions::new(0, 1),
            Err(VisualAuditError::InvalidRoomsPerPage { .. })
        ));
        assert!(matches!(
            VisualAuditOptions::new(MAX_VISUAL_AUDIT_ROOMS_PER_PAGE + 1, 4),
            Err(VisualAuditError::InvalidRoomsPerPage { .. })
        ));
    }

    #[test]
    fn nearest_neighbors_and_pair_ranking_use_room_ids_for_stable_ties() {
        let empty = record("zeta", 0, 0, false);
        let alpha = record("alpha", 0, 1, false);
        let beta = record("beta", 0, 1, false);
        let records = vec![beta, empty, alpha];

        let nearest = list_visual_audit_nearest_neighbors(&records).unwrap();
        let zeta = nearest
            .iter()
            .find(|neighbors| neighbors.room_id.0 == "zeta")
            .unwrap();
        assert_eq!(zeta.static_visual.as_ref().unwrap().room_id.0, "alpha");
        assert_eq!(zeta.collision_topology.as_ref().unwrap().room_id.0, "alpha");

        let static_pairs = rank_visual_audit_neighbor_pairs(
            &records,
            VisualAuditNeighborMetric::StaticVisual,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(
            static_pairs
                .iter()
                .map(|pair| (pair.left_room_id.0.as_str(), pair.right_room_id.0.as_str()))
                .collect::<Vec<_>>(),
            vec![("alpha", "beta"), ("alpha", "zeta"), ("beta", "zeta")]
        );
        assert_eq!(static_pairs[0].distance, 0.0);
        assert_eq!(static_pairs[1].distance, static_pairs[2].distance);
    }

    #[test]
    fn suspicious_pair_sheets_keep_pairs_adjacent_and_reject_odd_layouts() {
        let records = vec![
            record("c", 2, 4, false),
            record("a", 0, 1, false),
            record("b", 1, 1, false),
        ];
        let options = VisualAuditOptions::new(4, 2).unwrap();
        let pages = render_visual_audit_suspicious_pair_sheets(
            &records,
            VisualAuditNeighborMetric::StaticVisual,
            2,
            options,
        )
        .unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].rendered_rooms, 4);
        assert!(pages[0].svg.contains("pair 001  static distance 0.000000"));

        assert_eq!(
            render_visual_audit_suspicious_pair_sheets(
                &records,
                VisualAuditNeighborMetric::StaticVisual,
                1,
                VisualAuditOptions::new(3, 3).unwrap(),
            )
            .unwrap_err(),
            VisualAuditError::InvalidPairLayout {
                rooms_per_page: 3,
                columns: 3,
            }
        );
    }

    fn record(id: &str, seed: u64, solid_x: usize, source: bool) -> VisualAuditRecord {
        let mut tiles = vec![Tile::Empty; 32 * 18];
        if solid_x > 0 {
            tiles[32 * 12 + solid_x.min(30)] = Tile::Solid;
        }
        tiles[32 * 8 + 8] = Tile::OneWay;
        tiles[32 * 16 + 16] = Tile::HazardUp;
        let room = Room::new(
            format!("core-{id}"),
            "Visual audit fixture",
            32,
            18,
            10,
            tiles,
            Point::new(100, 100),
            Vec::new(),
        )
        .unwrap()
        .with_doors(vec![Door {
            id: "west".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 60, 10, 30),
            arrival: Point::new(20, 60),
            destination_room: None,
            destination_door: None,
        }])
        .unwrap()
        .with_objects(
            Vec::new(),
            vec![Pickup::new("cache", Rect::new(150, 100, 8, 8)).unwrap()],
        )
        .unwrap();
        VisualAuditRecord {
            room_id: RoomId(id.to_owned()),
            seed,
            construction_loadout: EvaluationLoadout::Baseline,
            strategy: "cyclic-graph".to_owned(),
            intent: "gentle".to_owned(),
            feature_stage: FeatureStageRecord::TerrainOnly,
            source_side: source.then_some(BoundarySide::Left),
            static_visual: StaticVisualDescriptor::from_room(&room),
            collision_topology: CollisionTopologyDescriptor::from_room(&room),
        }
    }
}
