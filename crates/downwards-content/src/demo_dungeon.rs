//! Hand-assembled multi-room vertical slice built from the deterministic dungeon palette.

use downwards_core::{AbilitySet, Exit, Pickup, Rect, Room};
use downwards_gen::{DungeonPaletteConnection, DungeonPaletteCourse, DungeonPaletteKey};

pub const DEMO_DUNGEON_START_ABILITIES: AbilitySet = AbilitySet::new(true, false);
pub const DEMO_DUNGEON_BOOT_PICKUP: &str = "winged-boots";
pub const DEMO_DUNGEON_CROWN_PICKUP: &str = "crown";
pub const DEMO_DUNGEON_GOAL_EXIT: &str = "crown-goal";
pub const DEMO_DUNGEON_TOTAL_COINS: u8 = 10;
pub const DEMO_DUNGEON_BOOT_GATE_REQUIREMENT: u8 = 6;
pub const DEMO_DUNGEON_TREASURY_REQUIREMENT: u8 = 8;
pub const DEMO_DUNGEON_CROWN_GATE_REQUIREMENT: u8 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DemoDungeonInventory {
    pub winged_boots: bool,
    pub crown: bool,
    coin_mask: u16,
}

impl DemoDungeonInventory {
    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        AbilitySet::new(true, self.winged_boots)
    }

    #[must_use]
    pub const fn coin_count(self) -> u8 {
        self.coin_mask.count_ones() as u8
    }

    #[must_use]
    pub fn has_coin(self, id: &str) -> bool {
        coin_index(id).is_some_and(|index| self.coin_mask & (1 << index) != 0)
    }

    pub fn collect_coin(&mut self, id: &str) -> bool {
        let Some(index) = coin_index(id) else {
            return false;
        };
        let bit = 1 << index;
        let newly_collected = self.coin_mask & bit == 0;
        self.coin_mask |= bit;
        newly_collected
    }

    #[must_use]
    pub fn owns_persistent_pickup(self, id: &str) -> bool {
        (id == DEMO_DUNGEON_BOOT_PICKUP && self.winged_boots)
            || (id == DEMO_DUNGEON_CROWN_PICKUP && self.crown)
            || self.has_coin(id)
    }

    #[must_use]
    pub const fn with_coin_count_for_validation(count: u8) -> Self {
        let count = if count > DEMO_DUNGEON_TOTAL_COINS {
            DEMO_DUNGEON_TOTAL_COINS
        } else {
            count
        };
        Self {
            winged_boots: false,
            crown: false,
            coin_mask: (1_u16 << count) - 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DemoDungeonRoom {
    Threshold,
    Crossroads,
    WallGallery,
    BootsVault,
    Underpass,
    DashChasm,
    CoinLoft,
    NeedleRoom,
    Treasury,
    Gatehouse,
    CrownSanctum,
}

impl DemoDungeonRoom {
    pub const ALL: [Self; 11] = [
        Self::Threshold,
        Self::Crossroads,
        Self::WallGallery,
        Self::BootsVault,
        Self::Underpass,
        Self::DashChasm,
        Self::CoinLoft,
        Self::NeedleRoom,
        Self::Treasury,
        Self::Gatehouse,
        Self::CrownSanctum,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Threshold => "demo-dungeon.threshold",
            Self::Crossroads => "demo-dungeon.crossroads",
            Self::WallGallery => "demo-dungeon.wall-gallery",
            Self::BootsVault => "demo-dungeon.boots-vault",
            Self::Underpass => "demo-dungeon.underpass",
            Self::DashChasm => "demo-dungeon.dash-chasm",
            Self::CoinLoft => "demo-dungeon.coin-loft",
            Self::NeedleRoom => "demo-dungeon.needle-room",
            Self::Treasury => "demo-dungeon.treasury",
            Self::Gatehouse => "demo-dungeon.gatehouse",
            Self::CrownSanctum => "demo-dungeon.crown-sanctum",
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Threshold => "Mosslit Threshold",
            Self::Crossroads => "Three-Way Hall",
            Self::WallGallery => "Climbers' Gallery",
            Self::BootsVault => "The Winged Vault",
            Self::Underpass => "Rootbound Underpass",
            Self::DashChasm => "Gale Chasm",
            Self::CoinLoft => "Rafter Mint",
            Self::NeedleRoom => "Needle Belfry",
            Self::Treasury => "Eight-Coin Treasury",
            Self::Gatehouse => "Ten-Coin Gate",
            Self::CrownSanctum => "The Empty Throne",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|room| room.id() == id)
    }

    const fn course(self) -> DungeonPaletteCourse {
        match self {
            Self::Threshold => DungeonPaletteCourse::Threshold,
            Self::Crossroads => DungeonPaletteCourse::Crossroads,
            Self::WallGallery => DungeonPaletteCourse::WallGallery,
            Self::BootsVault => DungeonPaletteCourse::BootsVault,
            Self::Underpass => DungeonPaletteCourse::Underpass,
            Self::DashChasm => DungeonPaletteCourse::DashChasm,
            Self::CoinLoft => DungeonPaletteCourse::CoinLoft,
            Self::NeedleRoom => DungeonPaletteCourse::NeedleRoom,
            Self::Treasury => DungeonPaletteCourse::Treasury,
            Self::Gatehouse => DungeonPaletteCourse::Gatehouse,
            Self::CrownSanctum => DungeonPaletteCourse::CrownSanctum,
        }
    }

    fn connections(self) -> Vec<DungeonPaletteConnection> {
        let connection = |door: &str, room: Self, destination_door: &str| {
            DungeonPaletteConnection::new(door, room.id(), destination_door)
        };
        match self {
            Self::Threshold => vec![connection("east", Self::Crossroads, "west")],
            Self::Crossroads => vec![
                connection("west", Self::Threshold, "east"),
                connection("east", Self::WallGallery, "west"),
                connection("ceiling", Self::BootsVault, "floor"),
                connection("floor", Self::CoinLoft, "ceiling"),
            ],
            Self::WallGallery => vec![
                connection("west", Self::Crossroads, "east"),
                connection("east", Self::DashChasm, "west"),
                connection("ceiling", Self::NeedleRoom, "floor"),
                connection("floor", Self::Underpass, "ceiling"),
            ],
            Self::BootsVault => vec![
                connection("floor", Self::Crossroads, "ceiling"),
                connection("east", Self::Underpass, "west"),
            ],
            Self::Underpass => vec![
                connection("west", Self::BootsVault, "east"),
                connection("east", Self::Treasury, "west"),
                connection("ceiling", Self::WallGallery, "floor"),
            ],
            Self::DashChasm => vec![
                connection("west", Self::WallGallery, "east"),
                connection("east", Self::Gatehouse, "west"),
            ],
            Self::CoinLoft => vec![connection("ceiling", Self::Crossroads, "floor")],
            Self::NeedleRoom => vec![connection("floor", Self::WallGallery, "ceiling")],
            Self::Treasury => vec![connection("west", Self::Underpass, "east")],
            Self::Gatehouse => vec![
                connection("west", Self::DashChasm, "east"),
                connection("east", Self::CrownSanctum, "west"),
            ],
            Self::CrownSanctum => vec![connection("west", Self::Gatehouse, "east")],
        }
    }
}

#[must_use]
pub const fn demo_dungeon_door_coin_requirement(
    room: DemoDungeonRoom,
    door_id: &str,
) -> Option<u8> {
    match (room, door_id.as_bytes()) {
        (DemoDungeonRoom::Crossroads, b"ceiling") | (DemoDungeonRoom::WallGallery, b"floor") => {
            Some(DEMO_DUNGEON_BOOT_GATE_REQUIREMENT)
        }
        (DemoDungeonRoom::Underpass, b"east") => Some(DEMO_DUNGEON_TREASURY_REQUIREMENT),
        (DemoDungeonRoom::Gatehouse, b"east") => Some(DEMO_DUNGEON_CROWN_GATE_REQUIREMENT),
        _ => None,
    }
}

fn coin_id(index: u8) -> String {
    format!("dungeon-coin-{index:02}")
}

fn coin_index(id: &str) -> Option<u8> {
    let suffix = id.strip_prefix("dungeon-coin-")?;
    let index = suffix.parse::<u8>().ok()?;
    (index < DEMO_DUNGEON_TOTAL_COINS).then_some(index)
}

/// Build one room against the persistent run inventory.
///
/// Collected run items are deliberately omitted when a room is reconstructed, so returning
/// through the graph cannot respawn boots or the crown.
#[must_use]
pub fn demo_dungeon_room(room: DemoDungeonRoom, inventory: DemoDungeonInventory) -> Room {
    let exits = (room == DemoDungeonRoom::CrownSanctum && !inventory.crown)
        .then(|| Exit {
            id: DEMO_DUNGEON_GOAL_EXIT.to_owned(),
            bounds: Rect::new(255, 12, 30, 18),
            destination: None,
            destination_entrance: None,
        })
        .into_iter()
        .collect();
    let candidate = DungeonPaletteKey::new(0xD06E_0A11, room.course()).generate();
    let built = candidate
        .materialize(room.id(), room.title(), &room.connections(), exits)
        .expect("built-in dungeon palette and graph must satisfy room invariants");
    let mut pickups = room_coin_specs(room)
        .into_iter()
        .filter_map(|(index, bounds)| {
            let id = coin_id(index);
            (!inventory.has_coin(&id))
                .then(|| Pickup::new(id, bounds).expect("authored dungeon coin bounds are valid"))
        })
        .collect::<Vec<_>>();
    if room == DemoDungeonRoom::BootsVault && !inventory.winged_boots {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_BOOT_PICKUP, Rect::new(214, 24, 16, 16))
                .expect("boots bounds are valid"),
        );
    }
    if room == DemoDungeonRoom::CrownSanctum && !inventory.crown {
        pickups.push(
            Pickup::new(DEMO_DUNGEON_CROWN_PICKUP, Rect::new(262, 14, 16, 16))
                .expect("crown bounds are valid"),
        );
    }
    built
        .with_objects(vec![], pickups)
        .expect("built-in dungeon pickups must satisfy room invariants")
}

fn room_coin_specs(room: DemoDungeonRoom) -> Vec<(u8, Rect)> {
    match room {
        DemoDungeonRoom::Threshold => vec![(0, Rect::new(158, 100, 8, 10))],
        DemoDungeonRoom::Crossroads => vec![(1, Rect::new(148, 100, 8, 10))],
        DemoDungeonRoom::CoinLoft => vec![
            (2, Rect::new(144, 110, 8, 10)),
            (3, Rect::new(244, 90, 8, 10)),
        ],
        DemoDungeonRoom::WallGallery => vec![(4, Rect::new(188, 80, 8, 10))],
        DemoDungeonRoom::NeedleRoom => vec![(5, Rect::new(188, 110, 8, 10))],
        DemoDungeonRoom::BootsVault => vec![(6, Rect::new(246, 30, 8, 10))],
        DemoDungeonRoom::Underpass => vec![(7, Rect::new(224, 80, 8, 10))],
        DemoDungeonRoom::Treasury => vec![
            (8, Rect::new(144, 100, 8, 10)),
            (9, Rect::new(244, 70, 8, 10)),
        ],
        DemoDungeonRoom::DashChasm | DemoDungeonRoom::Gatehouse | DemoDungeonRoom::CrownSanctum => {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_ai::{SearchTarget, SolverConfig, TargetSolveOutcome, solve_target};
    use downwards_core::{JumpKind, Simulation, SimulationEvent};

    #[test]
    fn graph_connections_are_reciprocal_and_socket_matched() {
        let inventory = DemoDungeonInventory::default();
        for room_id in DemoDungeonRoom::ALL {
            let room = demo_dungeon_room(room_id, inventory);
            for door in room.doors() {
                let destination_id = DemoDungeonRoom::from_id(
                    door.destination_room
                        .as_deref()
                        .expect("every dungeon door is connected"),
                )
                .expect("destination room is in this dungeon");
                let destination = demo_dungeon_room(destination_id, inventory);
                let return_door = destination
                    .doors()
                    .iter()
                    .find(|candidate| {
                        candidate.id
                            == door
                                .destination_door
                                .as_deref()
                                .expect("every dungeon door names its mate")
                    })
                    .expect("destination door exists");
                assert_eq!(return_door.destination_room.as_deref(), Some(room_id.id()));
                assert_eq!(
                    return_door.destination_door.as_deref(),
                    Some(door.id.as_str())
                );
                assert!(door.geometrically_matches(return_door));
            }
        }
    }

    #[test]
    fn persistent_items_disappear_and_boots_upgrade_the_loadout() {
        let empty = DemoDungeonInventory::default();
        assert!(!empty.abilities().dash);
        let uncollected = demo_dungeon_room(DemoDungeonRoom::BootsVault, empty);
        assert!(
            uncollected
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            uncollected
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == coin_id(6))
        );
        let acquired = DemoDungeonInventory {
            winged_boots: true,
            crown: false,
            ..DemoDungeonInventory::default()
        };
        assert!(acquired.abilities().dash);
        let after_boots = demo_dungeon_room(DemoDungeonRoom::BootsVault, acquired);
        assert!(
            after_boots
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            after_boots
                .pickups()
                .iter()
                .any(|pickup| pickup.id() == coin_id(6))
        );

        let mut collected = acquired;
        assert!(collected.collect_coin(&coin_id(6)));
        assert!(
            demo_dungeon_room(DemoDungeonRoom::BootsVault, collected)
                .pickups()
                .is_empty()
        );
    }

    #[test]
    fn coins_are_unique_persistent_and_bind_every_progression_gate() {
        let mut inventory = DemoDungeonInventory::default();
        for index in 0..DEMO_DUNGEON_TOTAL_COINS {
            let id = coin_id(index);
            assert!(
                inventory.collect_coin(&id),
                "first collection of {id} is new"
            );
            assert!(
                !inventory.collect_coin(&id),
                "second collection of {id} is not new"
            );
            assert!(inventory.has_coin(&id));
        }
        assert_eq!(inventory.coin_count(), DEMO_DUNGEON_TOTAL_COINS);
        let mut placed_indices = DemoDungeonRoom::ALL
            .into_iter()
            .flat_map(room_coin_specs)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        placed_indices.sort_unstable();
        assert_eq!(
            placed_indices,
            (0..DEMO_DUNGEON_TOTAL_COINS).collect::<Vec<_>>()
        );
        for (rooms, expected_indices) in [
            (
                vec![
                    DemoDungeonRoom::Threshold,
                    DemoDungeonRoom::Crossroads,
                    DemoDungeonRoom::CoinLoft,
                    DemoDungeonRoom::WallGallery,
                    DemoDungeonRoom::NeedleRoom,
                ],
                (0..6).collect::<Vec<_>>(),
            ),
            (
                vec![DemoDungeonRoom::BootsVault, DemoDungeonRoom::Underpass],
                (6..8).collect::<Vec<_>>(),
            ),
            (vec![DemoDungeonRoom::Treasury], (8..10).collect::<Vec<_>>()),
        ] {
            let mut actual = rooms
                .into_iter()
                .flat_map(room_coin_specs)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            actual.sort_unstable();
            assert_eq!(actual, expected_indices);
        }
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Crossroads, "ceiling"),
            Some(DEMO_DUNGEON_BOOT_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::WallGallery, "floor"),
            Some(DEMO_DUNGEON_BOOT_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Underpass, "east"),
            Some(DEMO_DUNGEON_TREASURY_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Gatehouse, "east"),
            Some(DEMO_DUNGEON_CROWN_GATE_REQUIREMENT)
        );
        assert_eq!(
            demo_dungeon_door_coin_requirement(DemoDungeonRoom::Gatehouse, "west"),
            None
        );
    }

    #[test]
    fn crown_is_the_only_terminal_goal() {
        let empty = DemoDungeonInventory::default();
        for room_id in DemoDungeonRoom::ALL {
            let room = demo_dungeon_room(room_id, empty);
            assert_eq!(
                room.exits()
                    .iter()
                    .any(|exit| exit.id == DEMO_DUNGEON_GOAL_EXIT),
                room_id == DemoDungeonRoom::CrownSanctum
            );
        }
        let collected = DemoDungeonInventory {
            winged_boots: true,
            crown: true,
            ..DemoDungeonInventory::default()
        };
        let sanctum = demo_dungeon_room(DemoDungeonRoom::CrownSanctum, collected);
        assert!(sanctum.pickups().is_empty());
        assert!(sanctum.exits().is_empty());
    }

    fn assert_route(
        room_id: DemoDungeonRoom,
        entry_door: Option<&str>,
        inventory: DemoDungeonInventory,
        target: SearchTarget,
    ) {
        let room = demo_dungeon_room(room_id, inventory);
        let mut simulation = match entry_door {
            Some(door) => Simulation::enter_via_door(room, inventory.abilities(), door).unwrap(),
            None => Simulation::with_abilities(room, inventory.abilities()),
        };
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            target.clone(),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Solved(_)),
            "{room_id:?} from {entry_door:?} did not reach {target:?}: {outcome:?}"
        );
    }

    fn solve_and_advance(mut simulation: Simulation, target: SearchTarget) -> Simulation {
        let abilities = simulation.abilities();
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            target.clone(),
            &SolverConfig::for_abilities(abilities),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("sequential dungeon route did not reach {target:?}: {outcome:?}");
        };
        for action in solution.replay.actions() {
            simulation.step(action);
        }
        simulation
    }

    #[test]
    fn authored_critical_route_is_solver_tractable_under_each_run_loadout() {
        let empty = DemoDungeonInventory::default();
        assert_route(
            DemoDungeonRoom::Threshold,
            None,
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            empty,
            SearchTarget::door("floor"),
        );
        assert_route(
            DemoDungeonRoom::CoinLoft,
            Some("ceiling"),
            empty,
            SearchTarget::pickup(coin_id(2)),
        );
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            empty,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::WallGallery,
            Some("west"),
            empty,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::NeedleRoom,
            Some("floor"),
            empty,
            SearchTarget::pickup(coin_id(5)),
        );
        let pre_boots = DemoDungeonInventory::with_coin_count_for_validation(
            DEMO_DUNGEON_BOOT_GATE_REQUIREMENT,
        );
        assert_route(
            DemoDungeonRoom::Crossroads,
            Some("west"),
            pre_boots,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::BootsVault,
            Some("floor"),
            pre_boots,
            SearchTarget::pickup(DEMO_DUNGEON_BOOT_PICKUP),
        );
        let boots = DemoDungeonInventory {
            winged_boots: true,
            crown: false,
            ..DemoDungeonInventory::with_coin_count_for_validation(7)
        };
        assert_route(
            DemoDungeonRoom::BootsVault,
            None,
            boots,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            boots,
            SearchTarget::pickup(coin_id(7)),
        );
        let treasury_ready = DemoDungeonInventory {
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_TREASURY_REQUIREMENT,
            )
        };
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            treasury_ready,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Treasury,
            Some("west"),
            treasury_ready,
            SearchTarget::pickup(coin_id(9)),
        );
        let complete = DemoDungeonInventory {
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_CROWN_GATE_REQUIREMENT,
            )
        };
        assert_route(
            DemoDungeonRoom::Underpass,
            Some("west"),
            complete,
            SearchTarget::door("ceiling"),
        );
        assert_route(
            DemoDungeonRoom::WallGallery,
            Some("floor"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::DashChasm,
            Some("west"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::Gatehouse,
            Some("west"),
            complete,
            SearchTarget::door("east"),
        );
        assert_route(
            DemoDungeonRoom::CrownSanctum,
            Some("west"),
            complete,
            SearchTarget::exit(DEMO_DUNGEON_GOAL_EXIT),
        );
    }

    #[test]
    fn winged_boots_require_a_real_climb_in_the_known_positive_route() {
        let inventory = DemoDungeonInventory::with_coin_count_for_validation(
            DEMO_DUNGEON_BOOT_GATE_REQUIREMENT,
        );
        let room = demo_dungeon_room(DemoDungeonRoom::BootsVault, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "floor").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::pickup(DEMO_DUNGEON_BOOT_PICKUP),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Winged Boots climb lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut accepted_jumps = 0;
        let mut wall_sides = Vec::new();
        for action in solution.replay.actions() {
            for event in replayed.step(action).events {
                if let SimulationEvent::Jumped(kind) = event {
                    accepted_jumps += 1;
                    if let JumpKind::Wall { side } = kind {
                        wall_sides.push(side);
                    }
                }
            }
        }
        assert!(
            replayed
                .collected_pickups()
                .any(|pickup| pickup.id() == DEMO_DUNGEON_BOOT_PICKUP)
        );
        assert!(
            accepted_jumps >= 5 && wall_sides.len() >= 4,
            "the boots route must contain the alternating wall climb, got {accepted_jumps} jumps / {} wall jumps",
            wall_sides.len()
        );
        assert!(
            wall_sides.windows(2).all(|pair| pair[0] != pair[1]),
            "the boots route must alternate wall sides instead of hopping one wall: {wall_sides:?}"
        );
    }

    #[test]
    fn required_coin_branches_are_solver_tractable() {
        let empty = DemoDungeonInventory::default();
        for (room, entry, targets) in [
            (
                DemoDungeonRoom::CoinLoft,
                "ceiling",
                vec![
                    SearchTarget::pickup(coin_id(2)),
                    SearchTarget::pickup(coin_id(3)),
                ],
            ),
            (
                DemoDungeonRoom::NeedleRoom,
                "floor",
                vec![SearchTarget::pickup(coin_id(5))],
            ),
        ] {
            for target in targets {
                assert_route(room, Some(entry), empty, target);
            }
        }
        let boots = DemoDungeonInventory {
            winged_boots: true,
            ..DemoDungeonInventory::default()
        };
        for target in [
            SearchTarget::pickup(coin_id(8)),
            SearchTarget::pickup(coin_id(9)),
        ] {
            assert_route(DemoDungeonRoom::Treasury, Some("west"), boots, target);
        }
    }

    #[test]
    fn required_coin_branches_can_return_after_their_deepest_pickup() {
        let empty = DemoDungeonInventory::default();
        let coin_loft = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::CoinLoft, empty),
            empty.abilities(),
            "ceiling",
        )
        .unwrap();
        let coin_loft = solve_and_advance(coin_loft, SearchTarget::pickup(coin_id(3)));
        let coin_loft = solve_and_advance(coin_loft, SearchTarget::door("ceiling"));
        assert_eq!(coin_loft.reached_exit(), Some("ceiling"));

        let needle = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::NeedleRoom, empty),
            empty.abilities(),
            "floor",
        )
        .unwrap();
        let needle = solve_and_advance(needle, SearchTarget::pickup(coin_id(5)));
        let needle = solve_and_advance(needle, SearchTarget::door("floor"));
        assert_eq!(needle.reached_exit(), Some("floor"));

        let treasury_ready = DemoDungeonInventory {
            winged_boots: true,
            ..DemoDungeonInventory::with_coin_count_for_validation(
                DEMO_DUNGEON_TREASURY_REQUIREMENT,
            )
        };
        let treasury = Simulation::enter_via_door(
            demo_dungeon_room(DemoDungeonRoom::Treasury, treasury_ready),
            treasury_ready.abilities(),
            "west",
        )
        .unwrap();
        let treasury = solve_and_advance(treasury, SearchTarget::pickup(coin_id(9)));
        let treasury = solve_and_advance(treasury, SearchTarget::door("west"));
        assert_eq!(treasury.reached_exit(), Some("west"));
    }

    #[test]
    fn dash_chasm_has_no_known_wall_jump_only_route_under_the_same_search_budget() {
        let inventory = DemoDungeonInventory::default();
        let room = demo_dungeon_room(DemoDungeonRoom::DashChasm, inventory);
        let mut simulation =
            Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        simulation.enable_current_player_movement();
        let outcome = solve_target(
            &simulation,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        assert!(
            matches!(outcome, TargetSolveOutcome::Inconclusive { .. }),
            "wall-jump-only search unexpectedly crossed the Dash gate: {outcome:?}"
        );
    }

    #[test]
    fn gatehouse_known_positive_uses_the_low_dash_passage() {
        let inventory = DemoDungeonInventory {
            winged_boots: true,
            ..DemoDungeonInventory::default()
        };
        let room = demo_dungeon_room(DemoDungeonRoom::Gatehouse, inventory);
        let mut initial = Simulation::enter_via_door(room, inventory.abilities(), "west").unwrap();
        initial.enable_current_player_movement();
        let outcome = solve_target(
            &initial,
            SearchTarget::door("east"),
            &SolverConfig::for_abilities(inventory.abilities()),
        )
        .unwrap();
        let TargetSolveOutcome::Solved(solution) = outcome else {
            panic!("Dash-squeeze gatehouse lost its known positive: {outcome:?}");
        };
        let mut replayed = initial;
        let mut observed_low_posture = false;
        for action in solution.replay.actions() {
            replayed.step(action);
            observed_low_posture |= replayed.player().dash_compressed();
        }
        assert_eq!(replayed.reached_exit(), Some("east"));
        assert!(
            observed_low_posture,
            "the gatehouse route must traverse its one-tile passage in low Dash posture"
        );
    }
}
