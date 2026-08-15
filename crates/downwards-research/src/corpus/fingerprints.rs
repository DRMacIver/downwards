use downwards_lab::StaticVisualDescriptor;

pub const CORPUS_VISUAL_FINGERPRINT_VERSION: u32 = 1;

pub fn fingerprint_static_visual(visual: &StaticVisualDescriptor) -> u64 {
    let mut hash = StableHash::new();
    hash.bytes(b"downwards-corpus-static-visual");
    hash.u32(CORPUS_VISUAL_FINGERPRINT_VERSION);
    hash.u32(visual.version);
    hash.u16(visual.width);
    hash.u16(visual.height);
    hash.i32(visual.tile_size);
    hash.i32(visual.spawn.x);
    hash.i32(visual.spawn.y);
    hash.length(visual.tiles.len());
    for tile in &visual.tiles {
        hash.byte(*tile as u8);
    }
    hash.length(visual.exits.len());
    for bounds in &visual.exits {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.length(visual.doors.len());
    for door in &visual.doors {
        hash.byte(door.side as u8);
        let bounds = door.trigger_bounds;
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.length(visual.pickups.len());
    for bounds in &visual.pickups {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.length(visual.timed_hazards.len());
    for bounds in &visual.timed_hazards {
        hash.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    hash.finish()
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

    fn rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.i32(x);
        self.i32(y);
        self.i32(width);
        self.i32(height);
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
