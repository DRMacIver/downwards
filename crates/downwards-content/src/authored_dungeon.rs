//! Generator-neutral contracts for a large, hand-authored dungeon.
//!
//! Geometry remains native [`downwards_core::Room`] data (and may be bootstrapped by a generator),
//! while this module owns the progression facts that must not be inferred by the client: stable
//! floor identity, reciprocal connections, coins, traversal-method unlocks, and sealed-door
//! requirements.

use std::{collections::BTreeSet, error::Error, fmt};

pub const AUTHORED_DUNGEON_SCHEMA_VERSION: u32 = 1;
pub const AUTHORED_DUNGEON_MAX_COINS: u16 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TraversalMethod {
    WallJump,
    Dash,
}

impl TraversalMethod {
    const fn bit(self) -> u8 {
        match self {
            Self::WallJump => 1 << 0,
            Self::Dash => 1 << 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TraversalMethods(u8);

impl TraversalMethods {
    pub const NONE: Self = Self(0);
    pub const ALL_CURRENT: Self =
        Self(TraversalMethod::WallJump.bit() | TraversalMethod::Dash.bit());

    #[must_use]
    pub const fn one(method: TraversalMethod) -> Self {
        Self(method.bit())
    }

    #[must_use]
    pub const fn contains(self, method: TraversalMethod) -> bool {
        self.0 & method.bit() != 0
    }

    #[must_use]
    pub const fn contains_all(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AuthoredDoorRequirement {
    pub coins: u16,
    pub traversal_methods: TraversalMethods,
}

impl AuthoredDoorRequirement {
    pub const NONE: Self = Self {
        coins: 0,
        traversal_methods: TraversalMethods::NONE,
    };

    #[must_use]
    pub const fn new(coins: u16, traversal_methods: TraversalMethods) -> Self {
        Self {
            coins,
            traversal_methods,
        }
    }

    #[must_use]
    pub const fn is_satisfied_by(self, inventory: &AuthoredDungeonInventory) -> bool {
        inventory.coin_count() >= self.coins
            && inventory.methods.contains_all(self.traversal_methods)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.coins == 0 && self.traversal_methods.0 == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AuthoredFloorKey(pub u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoredConnection {
    pub door_id: String,
    pub destination_floor: AuthoredFloorKey,
    pub destination_door: String,
    pub requirement: AuthoredDoorRequirement,
}

impl AuthoredConnection {
    #[must_use]
    pub fn new(
        door_id: impl Into<String>,
        destination_floor: AuthoredFloorKey,
        destination_door: impl Into<String>,
        requirement: AuthoredDoorRequirement,
    ) -> Self {
        Self {
            door_id: door_id.into(),
            destination_floor,
            destination_door: destination_door.into(),
            requirement,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoredFloorDefinition {
    pub key: AuthoredFloorKey,
    pub id: String,
    pub title: String,
    /// Stable generator/authoring source identity. This is provenance, not gameplay evidence.
    pub geometry_key: String,
    pub connections: Vec<AuthoredConnection>,
    pub coin_indices: Vec<u16>,
    pub traversal_unlock: Option<TraversalMethod>,
    pub contains_crown: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoredDungeonDefinition {
    pub schema_version: u32,
    pub id: String,
    pub start_floor: AuthoredFloorKey,
    pub start_methods: TraversalMethods,
    pub crown_floor: AuthoredFloorKey,
    pub total_coins: u16,
    pub crown_requirement: AuthoredDoorRequirement,
    pub required_floor_count: u16,
    pub floors: Vec<AuthoredFloorDefinition>,
}

impl AuthoredDungeonDefinition {
    pub fn validate(&self) -> Result<AuthoredDungeonProgressionAudit, AuthoredDungeonError> {
        if self.schema_version != AUTHORED_DUNGEON_SCHEMA_VERSION {
            return Err(AuthoredDungeonError::UnsupportedSchema {
                actual: self.schema_version,
                supported: AUTHORED_DUNGEON_SCHEMA_VERSION,
            });
        }
        if self.total_coins > AUTHORED_DUNGEON_MAX_COINS {
            return Err(AuthoredDungeonError::TooManyCoins(self.total_coins));
        }
        if self.floors.len() < usize::from(self.required_floor_count) {
            return Err(AuthoredDungeonError::TooFewFloors {
                actual: self.floors.len(),
                required: self.required_floor_count,
            });
        }
        if self.crown_requirement.coins < self.total_coins.div_ceil(3) {
            return Err(AuthoredDungeonError::WeakCrownCoinGate {
                actual: self.crown_requirement.coins,
                minimum: self.total_coins.div_ceil(3),
            });
        }
        if !self
            .crown_requirement
            .traversal_methods
            .contains_all(TraversalMethods::ALL_CURRENT)
        {
            return Err(AuthoredDungeonError::CrownDoesNotRequireAllMethods);
        }

        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut coins = BTreeSet::new();
        let mut unlocks = BTreeSet::new();
        let mut crown_floors = Vec::new();
        for floor in &self.floors {
            if !keys.insert(floor.key) {
                return Err(AuthoredDungeonError::DuplicateFloorKey(floor.key));
            }
            if !ids.insert(floor.id.as_str()) {
                return Err(AuthoredDungeonError::DuplicateFloorId(floor.id.clone()));
            }
            for &coin in &floor.coin_indices {
                if coin >= self.total_coins {
                    return Err(AuthoredDungeonError::CoinOutOfRange {
                        floor: floor.key,
                        coin,
                    });
                }
                if !coins.insert(coin) {
                    return Err(AuthoredDungeonError::DuplicateCoin(coin));
                }
            }
            if let Some(method) = floor.traversal_unlock
                && (!unlocks.insert(method) || self.start_methods.contains(method))
            {
                return Err(AuthoredDungeonError::TraversalUnlockNotUnique(method));
            }
            if floor.contains_crown {
                crown_floors.push(floor.key);
            }
        }
        if !keys.contains(&self.start_floor) {
            return Err(AuthoredDungeonError::UnknownStartFloor(self.start_floor));
        }
        if crown_floors != [self.crown_floor] {
            return Err(AuthoredDungeonError::InvalidCrownFloors(crown_floors));
        }
        let expected_coins = (0..self.total_coins).collect::<BTreeSet<_>>();
        if coins != expected_coins {
            return Err(AuthoredDungeonError::IncompleteCoinSet);
        }

        for floor in &self.floors {
            let mut door_ids = BTreeSet::new();
            for connection in &floor.connections {
                if !door_ids.insert(connection.door_id.as_str()) {
                    return Err(AuthoredDungeonError::DuplicateDoor {
                        floor: floor.key,
                        door: connection.door_id.clone(),
                    });
                }
                if connection.requirement.coins > self.total_coins {
                    return Err(AuthoredDungeonError::ImpossibleCoinGate {
                        floor: floor.key,
                        door: connection.door_id.clone(),
                        coins: connection.requirement.coins,
                    });
                }
                let destination = self.floor(connection.destination_floor).ok_or(
                    AuthoredDungeonError::UnknownDestination {
                        floor: floor.key,
                        destination: connection.destination_floor,
                    },
                )?;
                let reciprocal = destination.connections.iter().find(|candidate| {
                    candidate.door_id == connection.destination_door
                        && candidate.destination_floor == floor.key
                        && candidate.destination_door == connection.door_id
                });
                if reciprocal.is_none() {
                    return Err(AuthoredDungeonError::MissingReciprocalDoor {
                        floor: floor.key,
                        door: connection.door_id.clone(),
                    });
                }
            }
        }

        let crown = self
            .floor(self.crown_floor)
            .expect("crown floor existence checked above");
        for source in &self.floors {
            for connection in source
                .connections
                .iter()
                .filter(|connection| connection.destination_floor == crown.key)
            {
                if connection.requirement.coins < self.crown_requirement.coins
                    || !connection
                        .requirement
                        .traversal_methods
                        .contains_all(self.crown_requirement.traversal_methods)
                {
                    return Err(AuthoredDungeonError::WeakCrownIngress {
                        floor: source.key,
                        door: connection.door_id.clone(),
                    });
                }
            }
        }

        self.progression_closure()
    }

    #[must_use]
    pub fn floor(&self, key: AuthoredFloorKey) -> Option<&AuthoredFloorDefinition> {
        self.floors.iter().find(|floor| floor.key == key)
    }

    fn progression_closure(&self) -> Result<AuthoredDungeonProgressionAudit, AuthoredDungeonError> {
        let mut reachable = BTreeSet::from([self.start_floor]);
        let mut inventory = AuthoredDungeonInventory::new(self.start_methods);
        loop {
            let previous_reachable = reachable.clone();
            let previous_inventory = inventory;
            for &key in &previous_reachable {
                let floor = self
                    .floor(key)
                    .expect("reachable keys came from validated floors");
                for &coin in &floor.coin_indices {
                    inventory.collect_coin(coin);
                }
                if let Some(method) = floor.traversal_unlock {
                    inventory.grant(method);
                }
            }
            for &key in &previous_reachable {
                let floor = self
                    .floor(key)
                    .expect("reachable keys came from validated floors");
                for connection in &floor.connections {
                    if connection.requirement.is_satisfied_by(&inventory) {
                        reachable.insert(connection.destination_floor);
                    }
                }
            }
            if reachable == previous_reachable && inventory == previous_inventory {
                break;
            }
        }
        if reachable.len() != self.floors.len() {
            let unreachable = self
                .floors
                .iter()
                .filter_map(|floor| (!reachable.contains(&floor.key)).then_some(floor.key))
                .collect();
            return Err(AuthoredDungeonError::ProgressionDeadEnd(unreachable));
        }
        if !inventory
            .methods
            .contains_all(self.crown_requirement.traversal_methods)
            || inventory.coin_count() < self.crown_requirement.coins
        {
            return Err(AuthoredDungeonError::CrownRequirementUnobtainable);
        }
        Ok(AuthoredDungeonProgressionAudit {
            reachable_floors: reachable.len(),
            collected_coins: inventory.coin_count(),
            traversal_methods: inventory.methods,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthoredDungeonInventory {
    coin_words: [u64; 2],
    methods: TraversalMethods,
}

impl AuthoredDungeonInventory {
    #[must_use]
    pub const fn new(methods: TraversalMethods) -> Self {
        Self {
            coin_words: [0; 2],
            methods,
        }
    }

    #[must_use]
    pub const fn methods(self) -> TraversalMethods {
        self.methods
    }

    #[must_use]
    pub const fn coin_count(self) -> u16 {
        (self.coin_words[0].count_ones() + self.coin_words[1].count_ones()) as u16
    }

    #[must_use]
    pub const fn has_coin(self, index: u16) -> bool {
        if index >= AUTHORED_DUNGEON_MAX_COINS {
            return false;
        }
        let word = (index / 64) as usize;
        let bit = index % 64;
        self.coin_words[word] & (1_u64 << bit) != 0
    }

    pub fn collect_coin(&mut self, index: u16) -> bool {
        if index >= AUTHORED_DUNGEON_MAX_COINS {
            return false;
        }
        let word = usize::from(index / 64);
        let mask = 1_u64 << (index % 64);
        let newly_collected = self.coin_words[word] & mask == 0;
        self.coin_words[word] |= mask;
        newly_collected
    }

    pub fn grant(&mut self, method: TraversalMethod) -> bool {
        let newly_granted = !self.methods.contains(method);
        self.methods = self.methods.union(TraversalMethods::one(method));
        newly_granted
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthoredDungeonProgressionAudit {
    pub reachable_floors: usize,
    pub collected_coins: u16,
    pub traversal_methods: TraversalMethods,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoredDungeonError {
    UnsupportedSchema {
        actual: u32,
        supported: u32,
    },
    TooManyCoins(u16),
    TooFewFloors {
        actual: usize,
        required: u16,
    },
    WeakCrownCoinGate {
        actual: u16,
        minimum: u16,
    },
    CrownDoesNotRequireAllMethods,
    DuplicateFloorKey(AuthoredFloorKey),
    DuplicateFloorId(String),
    CoinOutOfRange {
        floor: AuthoredFloorKey,
        coin: u16,
    },
    DuplicateCoin(u16),
    TraversalUnlockNotUnique(TraversalMethod),
    UnknownStartFloor(AuthoredFloorKey),
    InvalidCrownFloors(Vec<AuthoredFloorKey>),
    IncompleteCoinSet,
    DuplicateDoor {
        floor: AuthoredFloorKey,
        door: String,
    },
    ImpossibleCoinGate {
        floor: AuthoredFloorKey,
        door: String,
        coins: u16,
    },
    UnknownDestination {
        floor: AuthoredFloorKey,
        destination: AuthoredFloorKey,
    },
    MissingReciprocalDoor {
        floor: AuthoredFloorKey,
        door: String,
    },
    WeakCrownIngress {
        floor: AuthoredFloorKey,
        door: String,
    },
    ProgressionDeadEnd(Vec<AuthoredFloorKey>),
    CrownRequirementUnobtainable,
}

impl fmt::Display for AuthoredDungeonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid authored dungeon: {self:?}")
    }
}

impl Error for AuthoredDungeonError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(
        key: u16,
        connections: Vec<AuthoredConnection>,
        coins: Vec<u16>,
        unlock: Option<TraversalMethod>,
        crown: bool,
    ) -> AuthoredFloorDefinition {
        AuthoredFloorDefinition {
            key: AuthoredFloorKey(key),
            id: format!("test.floor-{key:03}"),
            title: format!("Floor {key}"),
            geometry_key: format!("test-geometry-{key}"),
            connections,
            coin_indices: coins,
            traversal_unlock: unlock,
            contains_crown: crown,
        }
    }

    fn connection(
        door: &str,
        destination: u16,
        destination_door: &str,
        requirement: AuthoredDoorRequirement,
    ) -> AuthoredConnection {
        AuthoredConnection::new(
            door,
            AuthoredFloorKey(destination),
            destination_door,
            requirement,
        )
    }

    fn valid_definition() -> AuthoredDungeonDefinition {
        let all = TraversalMethods::ALL_CURRENT;
        AuthoredDungeonDefinition {
            schema_version: AUTHORED_DUNGEON_SCHEMA_VERSION,
            id: "test-dungeon".to_owned(),
            start_floor: AuthoredFloorKey(0),
            start_methods: TraversalMethods::NONE,
            crown_floor: AuthoredFloorKey(3),
            total_coins: 3,
            crown_requirement: AuthoredDoorRequirement::new(1, all),
            required_floor_count: 4,
            floors: vec![
                floor(
                    0,
                    vec![connection("east", 1, "west", AuthoredDoorRequirement::NONE)],
                    vec![0],
                    Some(TraversalMethod::WallJump),
                    false,
                ),
                floor(
                    1,
                    vec![
                        connection("west", 0, "east", AuthoredDoorRequirement::NONE),
                        connection(
                            "east",
                            2,
                            "west",
                            AuthoredDoorRequirement::new(
                                1,
                                TraversalMethods::one(TraversalMethod::WallJump),
                            ),
                        ),
                    ],
                    vec![1],
                    Some(TraversalMethod::Dash),
                    false,
                ),
                floor(
                    2,
                    vec![
                        connection(
                            "west",
                            1,
                            "east",
                            AuthoredDoorRequirement::new(
                                1,
                                TraversalMethods::one(TraversalMethod::WallJump),
                            ),
                        ),
                        connection("east", 3, "west", AuthoredDoorRequirement::new(1, all)),
                    ],
                    vec![2],
                    None,
                    false,
                ),
                floor(
                    3,
                    vec![connection(
                        "west",
                        2,
                        "east",
                        AuthoredDoorRequirement::new(1, all),
                    )],
                    vec![],
                    None,
                    true,
                ),
            ],
        }
    }

    #[test]
    fn validates_reciprocal_progression_and_a_128_coin_inventory() {
        let audit = valid_definition().validate().unwrap();
        assert_eq!(audit.reachable_floors, 4);
        assert_eq!(audit.collected_coins, 3);
        assert_eq!(audit.traversal_methods, TraversalMethods::ALL_CURRENT);

        let mut inventory = AuthoredDungeonInventory::new(TraversalMethods::NONE);
        assert!(inventory.collect_coin(0));
        assert!(inventory.collect_coin(64));
        assert!(inventory.collect_coin(127));
        assert!(!inventory.collect_coin(127));
        assert!(!inventory.collect_coin(128));
        assert_eq!(inventory.coin_count(), 3);
        assert!(inventory.has_coin(0));
        assert!(inventory.has_coin(64));
        assert!(inventory.has_coin(127));
    }

    #[test]
    fn crown_gate_cannot_be_weaker_than_one_third_or_omit_a_method() {
        let mut definition = valid_definition();
        definition.total_coins = 6;
        definition.floors[0].coin_indices.extend([3, 4, 5]);
        assert_eq!(
            definition.validate(),
            Err(AuthoredDungeonError::WeakCrownCoinGate {
                actual: 1,
                minimum: 2,
            })
        );

        let mut definition = valid_definition();
        definition.crown_requirement.traversal_methods =
            TraversalMethods::one(TraversalMethod::WallJump);
        assert_eq!(
            definition.validate(),
            Err(AuthoredDungeonError::CrownDoesNotRequireAllMethods)
        );
    }

    #[test]
    fn validates_every_crown_ingress_and_progression_dead_end() {
        let mut weak_ingress = valid_definition();
        weak_ingress.floors[2].connections[1].requirement = AuthoredDoorRequirement::NONE;
        assert_eq!(
            weak_ingress.validate(),
            Err(AuthoredDungeonError::WeakCrownIngress {
                floor: AuthoredFloorKey(2),
                door: "east".to_owned(),
            })
        );

        let mut dead_end = valid_definition();
        let unreachable_requirement =
            AuthoredDoorRequirement::new(3, TraversalMethods::one(TraversalMethod::WallJump));
        dead_end.floors[1].connections[1].requirement = unreachable_requirement;
        dead_end.floors[2].connections[0].requirement = unreachable_requirement;
        assert_eq!(
            dead_end.validate(),
            Err(AuthoredDungeonError::ProgressionDeadEnd(vec![
                AuthoredFloorKey(2),
                AuthoredFloorKey(3),
            ]))
        );
    }
}
