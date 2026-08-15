use std::{collections::VecDeque, error::Error, fmt};

use downwards_core::{DoorError, Room, RoomError, Tile};

/// Version of the room-feature ablation contract.
pub const ROOM_ABLATION_VERSION: u32 = 1;

/// One deterministic removal experiment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomAblation {
    pub kind: RoomAblationKind,
    pub room: Room,
}

/// Feature removed from an otherwise identical room.
///
/// Connected components use four-neighbour tile adjacency and canonical
/// row-major indices. Boundary-connected solid/one-way terrain is excluded:
/// the boundary shell is part of the room contract rather than optional
/// interior decoration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RoomAblationKind {
    InteriorTerrainComponent {
        component_index: usize,
        tile_count: usize,
    },
    StaticHazardComponent {
        component_index: usize,
        tile_count: usize,
    },
    TimedHazard {
        hazard_index: usize,
    },
}

#[derive(Debug)]
pub enum RoomAblationError {
    Room(RoomError),
    Door(DoorError),
}

impl fmt::Display for RoomAblationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Room(error) => write!(formatter, "ablated room is invalid: {error}"),
            Self::Door(error) => write!(formatter, "ablated room doors are invalid: {error}"),
        }
    }
}

impl Error for RoomAblationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Room(error) => Some(error),
            Self::Door(error) => Some(error),
        }
    }
}

impl From<RoomError> for RoomAblationError {
    fn from(error: RoomError) -> Self {
        Self::Room(error)
    }
}

impl From<DoorError> for RoomAblationError {
    fn from(error: DoorError) -> Self {
        Self::Door(error)
    }
}

/// Construct canonical single-feature removal variants.
///
/// Construction alone makes no claim that a removed feature was unnecessary.
/// Callers must replay or re-solve relevant objectives and compare the full
/// route/metric evidence. A surviving old replay is positive evidence of
/// redundancy for that controller, not proof that the feature never matters.
pub fn room_ablation_variants(room: &Room) -> Result<Vec<RoomAblation>, RoomAblationError> {
    let terrain_components =
        tile_components(room, |tile| matches!(tile, Tile::Solid | Tile::OneWay));
    let hazard_components = tile_components(room, |tile| tile == Tile::Hazard);
    let mut variants = Vec::new();

    let mut interior_component_index = 0;
    for component in terrain_components {
        if component
            .iter()
            .any(|&tile_index| tile_touches_boundary(room, tile_index))
        {
            continue;
        }
        let kind = RoomAblationKind::InteriorTerrainComponent {
            component_index: interior_component_index,
            tile_count: component.len(),
        };
        variants.push(RoomAblation {
            kind,
            room: rebuild_without_tiles(room, &component, None)?,
        });
        interior_component_index += 1;
    }

    for (component_index, component) in hazard_components.into_iter().enumerate() {
        let kind = RoomAblationKind::StaticHazardComponent {
            component_index,
            tile_count: component.len(),
        };
        variants.push(RoomAblation {
            kind,
            room: rebuild_without_tiles(room, &component, None)?,
        });
    }

    for hazard_index in 0..room.timed_hazards().len() {
        variants.push(RoomAblation {
            kind: RoomAblationKind::TimedHazard { hazard_index },
            room: rebuild_without_tiles(room, &[], Some(hazard_index))?,
        });
    }
    Ok(variants)
}

fn tile_components(room: &Room, included: impl Fn(Tile) -> bool) -> Vec<Vec<usize>> {
    let width = usize::from(room.width());
    let height = usize::from(room.height());
    let mut visited = vec![false; room.tiles().len()];
    let mut components = Vec::new();
    for start in 0..room.tiles().len() {
        if visited[start] || !included(room.tiles()[start]) {
            continue;
        }
        visited[start] = true;
        let mut pending = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(index) = pending.pop_front() {
            component.push(index);
            let x = index % width;
            let y = index / width;
            let neighbors = [
                x.checked_sub(1).map(|x| y * width + x),
                (x + 1 < width).then_some(y * width + x + 1),
                y.checked_sub(1).map(|y| y * width + x),
                (y + 1 < height).then_some((y + 1) * width + x),
            ];
            for neighbor in neighbors.into_iter().flatten() {
                if !visited[neighbor] && included(room.tiles()[neighbor]) {
                    visited[neighbor] = true;
                    pending.push_back(neighbor);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

fn tile_touches_boundary(room: &Room, tile_index: usize) -> bool {
    let width = usize::from(room.width());
    let height = usize::from(room.height());
    let x = tile_index % width;
    let y = tile_index / width;
    x == 0 || y == 0 || x + 1 == width || y + 1 == height
}

fn rebuild_without_tiles(
    room: &Room,
    removed_tiles: &[usize],
    removed_timed_hazard: Option<usize>,
) -> Result<Room, RoomAblationError> {
    let mut tiles = room.tiles().to_vec();
    for &tile_index in removed_tiles {
        tiles[tile_index] = Tile::Empty;
    }
    let timed_hazards = room
        .timed_hazards()
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != removed_timed_hazard)
        .map(|(_, hazard)| hazard.clone())
        .collect();
    let rebuilt = Room::new(
        room.id(),
        room.name(),
        room.width(),
        room.height(),
        room.tile_size(),
        tiles,
        room.spawn(),
        room.exits().to_vec(),
    )?
    .with_objects(timed_hazards, room.pickups().to_vec())?
    .with_doors(room.doors().to_vec())?;
    Ok(rebuilt)
}
