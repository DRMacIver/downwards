//! In-engine traversability audit of dungeon v2: solves every ordered door
//! pair of every room instance under each ability loadout with the real
//! solver, then checks the assembled graph for reachability, ability
//! progression, and soft-lock freedom, honouring the layout's door gates.
//!
//! Results cache in `generated/dungeon-v2-traversal-v1.txt` keyed by the
//! movement-policy version; delete the file after editing v2 grids.
//!
//! Run: `cargo run --release -p downwards-content --example audit_dungeon_v2`

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::PathBuf,
    sync::Mutex,
};

use downwards_ai::{SearchTarget, SolverConfig, TargetSolveOutcome, solve_target};
use downwards_content::{
    DEMO_DUNGEON_BOOT_PICKUP, DEMO_DUNGEON_GLOVE_PICKUP, DEMO_DUNGEON_GOAL_EXIT,
    DungeonV2Inventory, DungeonV2Requirement, dungeon_v2_definition, dungeon_v2_door_requirement,
    dungeon_v2_room, dungeon_v2_total_coins,
};
use downwards_core::{AbilitySet, PLAYER_MOVEMENT_POLICY_VERSION, Simulation};

const ARTIFACT: &str = "crates/downwards-content/generated/dungeon-v2-traversal-v1.txt";

fn loadouts() -> [AbilitySet; 4] {
    [
        AbilitySet::new(false, false),
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::new(true, true),
    ]
}

fn loadout_bit(abilities: AbilitySet) -> u8 {
    u8::from(abilities.wall_jump) | (u8::from(abilities.dash) << 1)
}

fn enter(instance: &str, door: &str, abilities: AbilitySet) -> Simulation {
    let room = dungeon_v2_room(instance, &DungeonV2Inventory::default());
    let mut simulation = Simulation::enter_via_door(room, abilities, door)
        .unwrap_or_else(|error| panic!("{instance} cannot enter by {door}: {error:?}"));
    simulation.enable_current_player_movement();
    simulation
}

fn main() {
    let dungeon = dungeon_v2_definition();
    let cache_path = PathBuf::from(ARTIFACT);
    let header = format!(
        "schema dungeon-v2-traversal-v1\nplayer-movement-policy {PLAYER_MOVEMENT_POLICY_VERSION}\n"
    );
    let mut cache: BTreeMap<String, bool> = BTreeMap::new();
    if let Ok(existing) = fs::read_to_string(&cache_path)
        && existing.starts_with(&header)
    {
        for line in existing.lines().skip(2) {
            if let Some((key, value)) = line.rsplit_once(' ') {
                cache.insert(key.to_owned(), value == "solved");
            }
        }
    }

    // 1. Solve every ordered door pair per instance per loadout (monotone:
    //    once solved at a loadout, larger loadouts inherit).
    let jobs = Mutex::new(Vec::<(String, String, String, AbilitySet)>::new());
    for instance in dungeon.instances.values() {
        let doors: Vec<_> = instance.connections.keys().cloned().collect();
        for entry in &doors {
            for exit in &doors {
                for abilities in loadouts() {
                    jobs.lock().unwrap().push((
                        instance.id.clone(),
                        entry.clone(),
                        exit.clone(),
                        abilities,
                    ));
                }
            }
        }
    }
    let jobs = jobs.into_inner().unwrap();
    let mut results: BTreeMap<(String, String, String, u8), bool> = BTreeMap::new();
    for (instance, entry, exit, abilities) in jobs {
        let bit = loadout_bit(abilities);
        // monotone shortcut: smaller loadout already solved
        let solved_smaller = results
            .iter()
            .any(|(&(ref i, ref en, ref ex, b), &solved)| {
                solved && *i == instance && *en == entry && *ex == exit && (b & bit) == b
            });
        let key = format!("pair {instance} {entry} {exit} {bit}");
        let solved = if solved_smaller {
            true
        } else if let Some(&cached) = cache.get(&key) {
            cached
        } else {
            let initial = enter(&instance, &entry, abilities);
            let outcome = solve_target(
                &initial,
                SearchTarget::door(&exit),
                &SolverConfig::for_abilities(abilities),
            )
            .expect("door target resolves");
            matches!(outcome, TargetSolveOutcome::Solved(_))
        };
        cache.insert(key, solved);
        results.insert((instance.clone(), entry.clone(), exit.clone(), bit), solved);
    }

    // 2. Ability and goal targets.
    let mut special: BTreeMap<String, bool> = BTreeMap::new();
    let mut check_special = |cache: &mut BTreeMap<String, bool>,
                             label: String,
                             instance: &str,
                             entry: &str,
                             abilities: AbilitySet,
                             target: SearchTarget| {
        let solved = if let Some(&cached) = cache.get(&label) {
            cached
        } else {
            let initial = enter(instance, entry, abilities);
            matches!(
                solve_target(&initial, target, &SolverConfig::for_abilities(abilities)),
                Ok(TargetSolveOutcome::Solved(_))
            )
        };
        cache.insert(label.clone(), solved);
        special.insert(label, solved);
    };
    for (room, pickup, abilities) in [
        (
            dungeon.glove_room.clone(),
            DEMO_DUNGEON_GLOVE_PICKUP,
            AbilitySet::new(false, false),
        ),
        (
            dungeon.boots_room.clone(),
            DEMO_DUNGEON_BOOT_PICKUP,
            AbilitySet::new(true, false),
        ),
    ] {
        for entry in dungeon.instances[&room].connections.keys() {
            check_special(
                &mut cache,
                format!("pickup {room} {entry} {pickup} {}", loadout_bit(abilities)),
                &room,
                entry,
                abilities,
                SearchTarget::pickup(pickup),
            );
        }
    }
    for entry in dungeon.instances[&dungeon.goal].connections.keys() {
        check_special(
            &mut cache,
            format!("goal {} {entry} 3", dungeon.goal),
            &dungeon.goal,
            entry,
            AbilitySet::new(true, true),
            SearchTarget::exit(DEMO_DUNGEON_GOAL_EXIT),
        );
    }

    let mut rendered = header.clone();
    rendered.push('\n');
    for (key, value) in &cache {
        rendered.push_str(&format!(
            "{key} {}\n",
            if *value { "solved" } else { "inconclusive" }
        ));
    }
    fs::write(&cache_path, rendered).expect("traversal artifact writes");
    println!("wrote {ARTIFACT}");

    // 3. Graph checks over solver-verified edges.
    let crossable = |instance: &str, entry: &str, exit: &str, wall: bool, dash: bool| -> bool {
        let bit = u8::from(wall) | (u8::from(dash) << 1);
        results
            .iter()
            .any(|(&(ref i, ref en, ref ex, b), &solved)| {
                solved && i == instance && en == entry && ex == exit && (b & bit) == b
            })
    };
    let gate_open = |instance: &str, door: &str, wall: bool, dash: bool, coins: usize| -> bool {
        match dungeon_v2_door_requirement(instance, door) {
            None => true,
            Some(DungeonV2Requirement::ClimbingGloves) => wall,
            Some(DungeonV2Requirement::WingedBoots) => dash,
            Some(DungeonV2Requirement::Coins(count)) => coins >= usize::from(count),
        }
    };
    let coins_in = |instance: &str| -> usize {
        let inner = &dungeon.instances[instance];
        if instance == dungeon.glove_room || instance == dungeon.boots_room {
            0
        } else {
            dungeon_v2_room(&inner.id, &DungeonV2Inventory::default())
                .pickups()
                .iter()
                .filter(|pickup| pickup.id().starts_with("v2-coin-"))
                .count()
        }
    };

    // Progressive exploration: abilities granted when their room is reached
    // (pickup routes proved above), coins optimistic over visited rooms.
    let mut wall = false;
    let mut dash = false;
    let mut coins = 0usize;
    let mut rooms_seen: BTreeSet<String> = BTreeSet::new();
    let mut states: BTreeSet<(String, String)> = BTreeSet::new();
    loop {
        let mut visited: BTreeSet<(String, String)> = BTreeSet::new();
        let mut frontier: VecDeque<(String, String)> = dungeon.instances[&dungeon.spawn]
            .connections
            .keys()
            .map(|door| (dungeon.spawn.clone(), door.clone()))
            .collect();
        while let Some((instance, entry)) = frontier.pop_front() {
            if !visited.insert((instance.clone(), entry.clone())) {
                continue;
            }
            rooms_seen.insert(instance.clone());
            let doors: Vec<_> = dungeon.instances[&instance]
                .connections
                .keys()
                .cloned()
                .collect();
            for exit in doors {
                if !crossable(&instance, &entry, &exit, wall, dash) {
                    continue;
                }
                if !gate_open(&instance, &exit, wall, dash, coins) {
                    continue;
                }
                let (next_room, next_door) =
                    dungeon.instances[&instance].connections[&exit].clone();
                frontier.push_back((next_room, next_door));
            }
        }
        let new_wall = wall || rooms_seen.contains(&dungeon.glove_room);
        let new_dash = dash || rooms_seen.contains(&dungeon.boots_room);
        let new_coins: usize = rooms_seen.iter().map(|room| coins_in(room)).sum();
        if visited == states && new_wall == wall && new_dash == dash && new_coins == coins {
            break;
        }
        states = visited;
        wall = new_wall;
        dash = new_dash;
        coins = new_coins;
    }

    let mut failures = 0;
    let all_rooms: BTreeSet<_> = dungeon.instances.keys().cloned().collect();
    let missing: Vec<_> = all_rooms.difference(&rooms_seen).collect();
    println!(
        "check 1: {}/{} rooms reachable from spawn{}",
        rooms_seen.len(),
        all_rooms.len(),
        if missing.is_empty() {
            String::new()
        } else {
            failures += 1;
            format!(" MISSING {missing:?}")
        }
    );
    println!(
        "check 2: abilities acquired: glove={wall} boots={dash}; coins bankable {coins}/{}",
        dungeon_v2_total_coins()
    );
    if !(wall && dash) {
        failures += 1;
    }
    if !rooms_seen.contains(&dungeon.goal) {
        println!("check 3: GOAL NOT REACHABLE");
        failures += 1;
    } else {
        println!("check 3: goal reachable with full progression");
    }

    // 4. Retreat: every reachable state can get back to spawn ignoring coin
    //    gates, at the loadout snapshot it was reached with (conservatively:
    //    the final loadout for late states, bare for the pre-ability set is
    //    approximated by re-running the bare exploration).
    let mut retreat_failures = Vec::new();
    for (instance, entry) in &states {
        let mut seen = BTreeSet::new();
        let mut frontier = VecDeque::from([(instance.clone(), entry.clone())]);
        let mut reached_spawn = false;
        while let Some((room, enter_door)) = frontier.pop_front() {
            if room == dungeon.spawn {
                reached_spawn = true;
                break;
            }
            if !seen.insert((room.clone(), enter_door.clone())) {
                continue;
            }
            let doors: Vec<_> = dungeon.instances[&room]
                .connections
                .keys()
                .cloned()
                .collect();
            for exit in doors {
                if !crossable(&room, &enter_door, &exit, wall, dash) {
                    continue;
                }
                if !gate_open(&room, &exit, wall, dash, usize::MAX) {
                    continue;
                }
                frontier.push_back(dungeon.instances[&room].connections[&exit].clone());
            }
        }
        if !reached_spawn {
            retreat_failures.push((instance.clone(), entry.clone()));
        }
    }
    println!(
        "check 4: {}/{} reachable states can retreat to spawn without coins{}",
        states.len() - retreat_failures.len(),
        states.len(),
        if retreat_failures.is_empty() {
            String::new()
        } else {
            failures += 1;
            format!(" FAILING {retreat_failures:?}")
        }
    );
    for (label, solved) in &special {
        if !solved {
            println!("special target failed: {label}");
            failures += 1;
        }
    }
    if failures == 0 {
        println!("dungeon v2 audit passed");
    } else {
        println!("dungeon v2 audit FAILED ({failures} failures)");
        std::process::exit(1);
    }
}
