use std::{cmp::Ordering, collections::VecDeque};

use downwards_core::{BoundarySide, Rect, Room, Tile};

/// Version of the exact static-preview encoding.
pub const STATIC_VISUAL_DESCRIPTOR_VERSION: u32 = 2;
/// Version of the collision-structure encoding.
pub const COLLISION_TOPOLOGY_DESCRIPTOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DescriptorPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DescriptorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl From<Rect> for DescriptorRect {
    fn from(bounds: Rect) -> Self {
        Self {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DescriptorDoor {
    pub side: BoundarySide,
    pub trigger_bounds: DescriptorRect,
}

impl Ord for DescriptorDoor {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.side as u8, self.trigger_bounds).cmp(&(other.side as u8, other.trigger_bounds))
    }
}

impl PartialOrd for DescriptorDoor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VisualTile {
    Empty = 0,
    Solid = 1,
    Hazard = 2,
    OneWay = 3,
}

impl From<Tile> for VisualTile {
    fn from(tile: Tile) -> Self {
        match tile {
            Tile::Empty => Self::Empty,
            Tile::Solid => Self::Solid,
            Tile::Hazard => Self::Hazard,
            Tile::OneWay => Self::OneWay,
        }
    }
}

/// Canonical geometry visible in a static room preview.
///
/// Room/object IDs, destinations, declaration order, and timed-hazard period,
/// duty cycle, and phase are excluded. Object bounds are sorted, so equal
/// descriptors mean equal static previews under the prototype renderer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StaticVisualDescriptor {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub tile_size: i32,
    pub spawn: DescriptorPoint,
    pub tiles: Box<[VisualTile]>,
    pub exits: Box<[DescriptorRect]>,
    pub doors: Box<[DescriptorDoor]>,
    pub pickups: Box<[DescriptorRect]>,
    pub timed_hazards: Box<[DescriptorRect]>,
}

impl StaticVisualDescriptor {
    #[must_use]
    pub fn from_room(room: &Room) -> Self {
        let spawn = room.spawn();
        Self {
            version: STATIC_VISUAL_DESCRIPTOR_VERSION,
            width: room.width(),
            height: room.height(),
            tile_size: room.tile_size(),
            spawn: DescriptorPoint {
                x: spawn.x,
                y: spawn.y,
            },
            tiles: room.tiles().iter().copied().map(VisualTile::from).collect(),
            exits: sorted_rects(room.exits().iter().map(|exit| exit.bounds)),
            doors: sorted_doors(room),
            pickups: sorted_rects(room.pickups().iter().map(|pickup| pickup.bounds())),
            timed_hazards: sorted_rects(room.timed_hazards().iter().map(|hazard| hazard.bounds())),
        }
    }
}

fn sorted_doors(room: &Room) -> Box<[DescriptorDoor]> {
    let mut result = room
        .doors()
        .iter()
        .map(|door| DescriptorDoor {
            side: door.side,
            trigger_bounds: door.trigger_bounds.into(),
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|door| (door.side as u8, door.trigger_bounds));
    result.into_boxed_slice()
}

fn sorted_rects(bounds: impl Iterator<Item = Rect>) -> Box<[DescriptorRect]> {
    let mut result = bounds.map(DescriptorRect::from).collect::<Vec<_>>();
    result.sort_unstable();
    result.into_boxed_slice()
}

/// Collision behavior at a tile cell. Hazards are passable collision-wise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CollisionCell {
    Passable = 0,
    Solid = 1,
    OneWay = 2,
}

impl From<Tile> for CollisionCell {
    fn from(tile: Tile) -> Self {
        match tile {
            Tile::Solid => Self::Solid,
            Tile::OneWay => Self::OneWay,
            Tile::Empty | Tile::Hazard => Self::Passable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CollisionFaceKind {
    SolidTop,
    SolidBottom,
    SolidLeft,
    SolidRight,
    OneWayTop,
}

/// A maximal exposed collision face in tile-grid vertex coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionFace {
    pub kind: CollisionFaceKind,
    pub start_x: u16,
    pub start_y: u16,
    pub end_x: u16,
    pub end_y: u16,
}

impl CollisionFace {
    #[must_use]
    pub const fn length_cells(self) -> u16 {
        self.start_x.abs_diff(self.end_x) + self.start_y.abs_diff(self.end_y)
    }
}

/// Bit flags describing which screen boundaries a passable region touches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoundaryMask(pub u8);

impl BoundaryMask {
    pub const TOP: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const BOTTOM: u8 = 4;
    pub const LEFT: u8 = 8;

    #[must_use]
    pub const fn touches(self, boundary: u8) -> bool {
        self.0 & boundary != 0
    }
}

/// Canonically numbered four-neighbor component of cells not occupied by a
/// solid tile. One-way platforms belong to the passable region because their
/// directional face, rather than their whole cell, supplies collision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PassableRegion {
    pub label: u32,
    pub anchor_index: u32,
    pub area_cells: u32,
    pub min_x: u16,
    pub min_y: u16,
    pub max_x: u16,
    pub max_y: u16,
    pub boundaries: BoundaryMask,
}

/// Canonical collision geometry and coarse traversable-region topology.
///
/// The raw collision field preserves exact directional collision semantics;
/// exposed faces and flood-filled regions give experiments structural signals
/// that are distinct from visual tile Hamming. Static and timed hazards,
/// pickups, exits, and all IDs are deliberately absent.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CollisionTopologyDescriptor {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub tile_size: i32,
    pub cells: Box<[CollisionCell]>,
    pub faces: Box<[CollisionFace]>,
    /// Row-major region label per cell; `u32::MAX` marks a solid cell.
    pub region_labels: Box<[u32]>,
    pub regions: Box<[PassableRegion]>,
}

impl CollisionTopologyDescriptor {
    #[must_use]
    pub fn from_room(room: &Room) -> Self {
        let cells = room
            .tiles()
            .iter()
            .copied()
            .map(CollisionCell::from)
            .collect::<Vec<_>>();
        let faces = exposed_faces(room.width(), room.height(), &cells);
        let (region_labels, regions) = passable_regions(room.width(), room.height(), &cells);
        Self {
            version: COLLISION_TOPOLOGY_DESCRIPTOR_VERSION,
            width: room.width(),
            height: room.height(),
            tile_size: room.tile_size(),
            cells: cells.into_boxed_slice(),
            faces: faces.into_boxed_slice(),
            region_labels: region_labels.into_boxed_slice(),
            regions: regions.into_boxed_slice(),
        }
    }
}

fn cell_at(cells: &[CollisionCell], width: u16, x: u16, y: u16) -> CollisionCell {
    cells[usize::from(y) * usize::from(width) + usize::from(x)]
}

fn exposed_faces(width: u16, height: u16, cells: &[CollisionCell]) -> Vec<CollisionFace> {
    let mut result = Vec::new();

    for y in 0..height {
        horizontal_runs(
            width,
            |x| {
                let cell = cell_at(cells, width, x, y);
                if cell == CollisionCell::OneWay {
                    Some(CollisionFaceKind::OneWayTop)
                } else if cell == CollisionCell::Solid
                    && (y == 0 || cell_at(cells, width, x, y - 1) != CollisionCell::Solid)
                {
                    Some(CollisionFaceKind::SolidTop)
                } else {
                    None
                }
            },
            |kind, start, end| {
                result.push(CollisionFace {
                    kind,
                    start_x: start,
                    start_y: y,
                    end_x: end,
                    end_y: y,
                });
            },
        );

        horizontal_runs(
            width,
            |x| {
                let cell = cell_at(cells, width, x, y);
                (cell == CollisionCell::Solid
                    && (y + 1 == height || cell_at(cells, width, x, y + 1) != CollisionCell::Solid))
                    .then_some(CollisionFaceKind::SolidBottom)
            },
            |kind, start, end| {
                result.push(CollisionFace {
                    kind,
                    start_x: start,
                    start_y: y + 1,
                    end_x: end,
                    end_y: y + 1,
                });
            },
        );
    }

    for x in 0..width {
        vertical_runs(
            height,
            |y| {
                (cell_at(cells, width, x, y) == CollisionCell::Solid
                    && (x == 0 || cell_at(cells, width, x - 1, y) != CollisionCell::Solid))
                    .then_some(CollisionFaceKind::SolidLeft)
            },
            |kind, start, end| {
                result.push(CollisionFace {
                    kind,
                    start_x: x,
                    start_y: start,
                    end_x: x,
                    end_y: end,
                });
            },
        );

        vertical_runs(
            height,
            |y| {
                (cell_at(cells, width, x, y) == CollisionCell::Solid
                    && (x + 1 == width || cell_at(cells, width, x + 1, y) != CollisionCell::Solid))
                    .then_some(CollisionFaceKind::SolidRight)
            },
            |kind, start, end| {
                result.push(CollisionFace {
                    kind,
                    start_x: x + 1,
                    start_y: start,
                    end_x: x + 1,
                    end_y: end,
                });
            },
        );
    }

    result.sort_unstable();
    result
}

fn horizontal_runs(
    length: u16,
    mut classify: impl FnMut(u16) -> Option<CollisionFaceKind>,
    mut push: impl FnMut(CollisionFaceKind, u16, u16),
) {
    let mut cursor = 0;
    while cursor < length {
        let Some(kind) = classify(cursor) else {
            cursor += 1;
            continue;
        };
        let start = cursor;
        cursor += 1;
        while cursor < length && classify(cursor) == Some(kind) {
            cursor += 1;
        }
        push(kind, start, cursor);
    }
}

fn vertical_runs(
    length: u16,
    classify: impl FnMut(u16) -> Option<CollisionFaceKind>,
    push: impl FnMut(CollisionFaceKind, u16, u16),
) {
    horizontal_runs(length, classify, push);
}

fn passable_regions(
    width: u16,
    height: u16,
    cells: &[CollisionCell],
) -> (Vec<u32>, Vec<PassableRegion>) {
    let mut labels = vec![u32::MAX; cells.len()];
    let mut regions = Vec::new();
    let mut queue = VecDeque::new();

    for anchor in 0..cells.len() {
        if cells[anchor] == CollisionCell::Solid || labels[anchor] != u32::MAX {
            continue;
        }
        let label = u32::try_from(regions.len()).expect("room has at most u16::MAX tile cells");
        labels[anchor] = label;
        queue.push_back(anchor);
        let anchor_x = (anchor % usize::from(width)) as u16;
        let anchor_y = (anchor / usize::from(width)) as u16;
        let mut region = PassableRegion {
            label,
            anchor_index: anchor as u32,
            area_cells: 0,
            min_x: anchor_x,
            min_y: anchor_y,
            max_x: anchor_x,
            max_y: anchor_y,
            boundaries: BoundaryMask::default(),
        };

        while let Some(index) = queue.pop_front() {
            let x = (index % usize::from(width)) as u16;
            let y = (index / usize::from(width)) as u16;
            region.area_cells += 1;
            region.min_x = region.min_x.min(x);
            region.min_y = region.min_y.min(y);
            region.max_x = region.max_x.max(x);
            region.max_y = region.max_y.max(y);
            if y == 0 {
                region.boundaries.0 |= BoundaryMask::TOP;
            }
            if x + 1 == width {
                region.boundaries.0 |= BoundaryMask::RIGHT;
            }
            if y + 1 == height {
                region.boundaries.0 |= BoundaryMask::BOTTOM;
            }
            if x == 0 {
                region.boundaries.0 |= BoundaryMask::LEFT;
            }

            let neighbors = [
                x.checked_sub(1).map(|next_x| (next_x, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|next_y| (x, next_y)),
                (y + 1 < height).then_some((x, y + 1)),
            ];
            for (next_x, next_y) in neighbors.into_iter().flatten() {
                let next = usize::from(next_y) * usize::from(width) + usize::from(next_x);
                if cells[next] != CollisionCell::Solid && labels[next] == u32::MAX {
                    labels[next] = label;
                    queue.push_back(next);
                }
            }
        }
        regions.push(region);
    }

    (labels, regions)
}
