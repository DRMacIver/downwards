use downwards_core::{BoundarySide, Room};

use crate::{DescriptorPoint, DescriptorRect, VisualTile};

/// Version of the canonical physics-relevant room encoding.
pub const SIMULATION_GEOMETRY_DESCRIPTOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimulationDoorGeometry {
    pub side: BoundarySide,
    pub trigger_bounds: DescriptorRect,
    pub arrival: DescriptorPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimulationTimedHazardGeometry {
    pub bounds: DescriptorRect,
    pub period_ticks: u32,
    pub active_ticks: u32,
    pub phase_ticks: u32,
}

/// Canonical room data that can change an in-room simulation rollout.
///
/// Unlike [`crate::StaticVisualDescriptor`], this includes door arrivals and
/// complete timed-hazard schedules. Human-facing names, object IDs,
/// destinations, and declaration order are excluded: they label the same
/// physical challenge rather than changing it. Equal descriptors are the
/// collision-and-timing deduplication truth; [`Self::stable_digest`] is only a
/// compact index/display key and must not be used as collision-free proof.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SimulationGeometryDescriptor {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub tile_size: i32,
    pub spawn: DescriptorPoint,
    pub tiles: Box<[VisualTile]>,
    pub exits: Box<[DescriptorRect]>,
    pub doors: Box<[SimulationDoorGeometry]>,
    pub pickups: Box<[DescriptorRect]>,
    pub timed_hazards: Box<[SimulationTimedHazardGeometry]>,
}

impl SimulationGeometryDescriptor {
    #[must_use]
    pub fn from_room(room: &Room) -> Self {
        let spawn = room.spawn();
        let mut exits = room
            .exits()
            .iter()
            .map(|exit| exit.bounds.into())
            .collect::<Vec<DescriptorRect>>();
        exits.sort_unstable();

        let mut doors = room
            .doors()
            .iter()
            .map(|door| SimulationDoorGeometry {
                side: door.side,
                trigger_bounds: door.trigger_bounds.into(),
                arrival: DescriptorPoint {
                    x: door.arrival.x,
                    y: door.arrival.y,
                },
            })
            .collect::<Vec<_>>();
        doors.sort_unstable();

        let mut pickups = room
            .pickups()
            .iter()
            .map(|pickup| pickup.bounds().into())
            .collect::<Vec<DescriptorRect>>();
        pickups.sort_unstable();

        let mut timed_hazards = room
            .timed_hazards()
            .iter()
            .map(|hazard| SimulationTimedHazardGeometry {
                bounds: hazard.bounds().into(),
                period_ticks: hazard.period_ticks(),
                active_ticks: hazard.active_ticks(),
                phase_ticks: hazard.phase_ticks(),
            })
            .collect::<Vec<_>>();
        timed_hazards.sort_unstable();

        Self {
            version: SIMULATION_GEOMETRY_DESCRIPTOR_VERSION,
            width: room.width(),
            height: room.height(),
            tile_size: room.tile_size(),
            spawn: DescriptorPoint {
                x: spawn.x,
                y: spawn.y,
            },
            tiles: room.tiles().iter().copied().map(VisualTile::from).collect(),
            exits: exits.into_boxed_slice(),
            doors: doors.into_boxed_slice(),
            pickups: pickups.into_boxed_slice(),
            timed_hazards: timed_hazards.into_boxed_slice(),
        }
    }

    /// Stable 64-bit digest of this versioned descriptor.
    ///
    /// Descriptor equality, rather than digest equality, is authoritative.
    #[must_use]
    pub fn stable_digest(&self) -> u64 {
        let mut hash = StableHash::new();
        hash.bytes(b"downwards-simulation-geometry");
        hash.u32(self.version);
        hash.u16(self.width);
        hash.u16(self.height);
        hash.i32(self.tile_size);
        hash.point(self.spawn);
        hash.length(self.tiles.len());
        for tile in &self.tiles {
            hash.byte(*tile as u8);
        }
        hash.length(self.exits.len());
        for bounds in &self.exits {
            hash.rect(*bounds);
        }
        hash.length(self.doors.len());
        for door in &self.doors {
            hash.byte(door.side as u8);
            hash.rect(door.trigger_bounds);
            hash.point(door.arrival);
        }
        hash.length(self.pickups.len());
        for bounds in &self.pickups {
            hash.rect(*bounds);
        }
        hash.length(self.timed_hazards.len());
        for hazard in &self.timed_hazards {
            hash.rect(hazard.bounds);
            hash.u32(hazard.period_ticks);
            hash.u32(hazard.active_ticks);
            hash.u32(hazard.phase_ticks);
        }
        hash.finish()
    }
}

struct StableHash(u64);

impl StableHash {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        self.length(values.len());
        self.raw_bytes(values);
    }

    fn u16(&mut self, value: u16) {
        self.raw_bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.raw_bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.raw_bytes(&value.to_le_bytes());
    }

    fn length(&mut self, value: usize) {
        self.raw_bytes(&(value as u64).to_le_bytes());
    }

    fn point(&mut self, point: DescriptorPoint) {
        self.i32(point.x);
        self.i32(point.y);
    }

    fn rect(&mut self, bounds: DescriptorRect) {
        self.i32(bounds.x);
        self.i32(bounds.y);
        self.i32(bounds.width);
        self.i32(bounds.height);
    }

    fn raw_bytes(&mut self, values: &[u8]) {
        for value in values {
            self.byte(*value);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}
