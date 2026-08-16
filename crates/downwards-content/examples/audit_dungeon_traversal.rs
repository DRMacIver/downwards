//! Audit door-to-door traversability of every authored dungeon room, then
//! check the whole-dungeon graph for reachability and soft-locks.
//!
//! For every room this solves each ordered (entry door -> exit door) pair —
//! including a door back to itself, which is what a player bounced off a
//! sealed gate must do — under each ability loadout, exploiting monotonicity
//! (extra abilities never remove a route). Ability pickups compose through
//! room resets: dying returns the player to the entry door with abilities
//! kept, so "entry -> door solvable with current abilities" is the exact edge
//! relation.
//!
//! Results are cached in `generated/demo-dungeon-traversal-v1.txt`; delete the
//! file or bump the palette/movement versions to re-solve. Solved claims carry
//! an exact witness; `inconclusive` records are bounded-search evidence only,
//! never proof of impossibility.
//!
//! Graph checks:
//! 1. Every room is reachable from the Hollow Landing spawn, granting coins
//!    optimistically (they are collectable) but abilities only via pickups.
//! 2. From every reachable (room, entry door, abilities) state the player can
//!    get back to Hollow Landing using no coin-gated doors at all — retreat
//!    must never depend on coins the player might not have.
//!
//! Run from the workspace root with:
//! `cargo run --release -p downwards-content --example audit_dungeon_traversal`

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::PathBuf,
    sync::Mutex,
};

use downwards_ai::{SearchTarget, SolverConfig, TargetSolveOutcome, solve_target};
use downwards_content::{
    DEMO_DUNGEON_BOOT_PICKUP, DEMO_DUNGEON_GLOVE_PICKUP, DemoDungeonInventory, DemoDungeonRoom,
    demo_dungeon_definition, demo_dungeon_door_requirement, demo_dungeon_room,
};
use downwards_core::{Action, PLAYER_MOVEMENT_POLICY_VERSION, Simulation};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;

const ARTIFACT: &str = "crates/downwards-content/generated/demo-dungeon-traversal-v1.txt";

const LOADOUTS: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

fn loadout_key(wall: bool, dash: bool) -> String {
    format!("{}{}", u8::from(wall), u8::from(dash))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PairTarget {
    Door(&'static str),
    Pickup(&'static str),
}

impl PairTarget {
    fn kind(self) -> &'static str {
        match self {
            Self::Door(_) => "door",
            Self::Pickup(_) => "pickup",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Door(id) | Self::Pickup(id) => id,
        }
    }
}

/// One (room, entry, target) job; loadouts are solved inside the job so the
/// monotonicity short-circuit stays local.
#[derive(Clone, Copy, Debug)]
struct Job {
    room: DemoDungeonRoom,
    entry: Option<&'static str>,
    target: PairTarget,
}

#[derive(Clone, Debug)]
enum Verdict {
    Solved(Vec<Action>),
    Inconclusive(String),
}

type PairKey = (String, String, String, String, String);

fn pair_key(job: &Job, wall: bool, dash: bool) -> PairKey {
    (
        job.room.id().to_owned(),
        job.entry.unwrap_or("spawn").to_owned(),
        job.target.kind().to_owned(),
        job.target.id().to_owned(),
        loadout_key(wall, dash),
    )
}

fn initial_simulation(
    room: DemoDungeonRoom,
    entry: Option<&str>,
    wall: bool,
    dash: bool,
) -> Simulation {
    let mut inventory = DemoDungeonInventory::default();
    inventory.climbing_gloves = wall;
    inventory.winged_boots = dash;
    let built = demo_dungeon_room(room, inventory);
    let mut simulation = match entry {
        Some(door) => Simulation::enter_via_door(built, inventory.abilities(), door)
            .unwrap_or_else(|error| panic!("{} cannot enter by {door:?}: {error}", room.id())),
        None => Simulation::with_abilities(built, inventory.abilities()),
    };
    simulation.enable_current_player_movement();
    simulation
}

fn solve_pair(job: &Job, wall: bool, dash: bool) -> Verdict {
    let initial = initial_simulation(job.room, job.entry, wall, dash);
    let target = match job.target {
        PairTarget::Door(id) => SearchTarget::door(id),
        PairTarget::Pickup(id) => SearchTarget::pickup(id),
    };
    let config = SolverConfig::for_abilities(initial.abilities());
    match solve_target(&initial, target, &config) {
        Ok(TargetSolveOutcome::Solved(solution)) => {
            Verdict::Solved(solution.replay.actions().collect())
        }
        Ok(outcome) => Verdict::Inconclusive(
            format!("{outcome:?}")
                .split('{')
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_owned(),
        ),
        Err(error) => Verdict::Inconclusive(format!("error: {error}")),
    }
}

fn render_span(action: Action, length: usize) -> String {
    format!(
        "span {} {} {} {} {} {length}\n",
        action.move_x,
        action.move_y,
        u8::from(action.jump),
        u8::from(action.dash),
        u8::from(action.restart),
    )
}

fn render_actions(actions: &[Action]) -> String {
    let mut rendered = String::new();
    let mut start = 0;
    while start < actions.len() {
        let action = actions[start];
        let end = actions[start..]
            .iter()
            .position(|candidate| *candidate != action)
            .map_or(actions.len(), |offset| start + offset);
        rendered.push_str(&render_span(action, end - start));
        start = end;
    }
    rendered
}

fn parse_actions(spans: &[String]) -> Vec<Action> {
    let mut actions = Vec::new();
    for span in spans {
        let fields: Vec<i32> = span
            .split_whitespace()
            .skip(1)
            .map(|field| field.parse().expect("span field"))
            .collect();
        let [move_x, move_y, jump, dash, restart, length] = fields[..] else {
            panic!("malformed span line: {span}");
        };
        for _ in 0..length {
            actions.push(Action {
                move_x: move_x as i8,
                move_y: move_y as i8,
                jump: jump != 0,
                dash: dash != 0,
                restart: restart != 0,
            });
        }
    }
    actions
}

fn artifact_header() -> String {
    format!(
        "schema downwards-demo-dungeon-traversal-v1\npalette-generation {DUNGEON_PALETTE_GENERATION_VERSION}\nplayer-movement-policy {PLAYER_MOVEMENT_POLICY_VERSION}\n"
    )
}

fn load_cache(path: &PathBuf) -> BTreeMap<PairKey, Verdict> {
    let mut cache = BTreeMap::new();
    let Ok(existing) = fs::read_to_string(path) else {
        return cache;
    };
    let mut lines = existing.lines();
    let header: Vec<&str> = (&mut lines).take(3).collect();
    if header.join("\n") + "\n" != artifact_header() {
        eprintln!("traversal cache has stale versions; re-solving everything");
        return cache;
    }
    let mut current: Option<(PairKey, bool, String, Vec<String>)> = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("pair ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let [room, entry, kind, id, loadout, verdict] = fields[..] else {
                panic!("malformed pair line: {line}");
            };
            current = Some((
                (
                    room.to_owned(),
                    entry.to_owned(),
                    kind.to_owned(),
                    id.to_owned(),
                    loadout.to_owned(),
                ),
                verdict == "solved",
                verdict.to_owned(),
                Vec::new(),
            ));
        } else if line.starts_with("span ") {
            current
                .as_mut()
                .expect("span outside a pair")
                .3
                .push(line.to_owned());
        } else if line == "end" {
            let (key, solved, verdict, spans) = current.take().expect("end without a pair");
            let verdict = if solved {
                Verdict::Solved(parse_actions(&spans))
            } else {
                Verdict::Inconclusive(verdict.trim_start_matches("inconclusive:").to_owned())
            };
            cache.insert(key, verdict);
        }
    }
    cache
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // `--pair <room-slug> <entry|spawn> <exit-door> <wall01><dash01>` solves a
    // single pair fresh (no cache) for fast design iteration.
    if arguments.first().map(String::as_str) == Some("--pair") {
        let [_, room, entry, exit, loadout] = &arguments[..] else {
            panic!("--pair needs <room-slug> <entry|spawn> <exit-door> <wall><dash>");
        };
        let room = DemoDungeonRoom::ALL
            .into_iter()
            .find(|candidate| candidate.id().ends_with(room.as_str()))
            .unwrap_or_else(|| panic!("unknown room {room}"));
        let entry = DOOR_NAMES
            .iter()
            .copied()
            .find(|name| *name == entry.as_str());
        let exit = DOOR_NAMES
            .iter()
            .copied()
            .find(|name| *name == exit.as_str())
            .expect("canonical exit door");
        let job = Job {
            room,
            entry,
            target: PairTarget::Door(exit),
        };
        let wall = loadout.as_bytes()[0] == b'1';
        let dash = loadout.as_bytes()[1] == b'1';
        match solve_pair(&job, wall, dash) {
            Verdict::Solved(actions) => println!("solved in {} ticks", actions.len()),
            Verdict::Inconclusive(reason) => println!("inconclusive: {reason}"),
        }
        return;
    }
    let graph_only = std::env::args().any(|argument| argument == "--graph-only");
    let path = PathBuf::from(ARTIFACT);
    let cache = load_cache(&path);

    let mut jobs = Vec::new();
    for room in DemoDungeonRoom::ALL {
        let built = demo_dungeon_room(room, DemoDungeonInventory::default());
        let door_ids: Vec<&'static str> = built
            .doors()
            .iter()
            .map(|door| {
                DOOR_NAMES
                    .iter()
                    .copied()
                    .find(|name| *name == door.id)
                    .expect("dungeon doors use the four canonical names")
            })
            .collect();
        let mut entries: Vec<Option<&'static str>> =
            door_ids.iter().map(|door| Some(*door)).collect();
        if room == DemoDungeonRoom::HollowLanding {
            entries.push(None);
        }
        let mut targets: Vec<PairTarget> =
            door_ids.iter().map(|door| PairTarget::Door(door)).collect();
        if room == DemoDungeonRoom::ClimberVault {
            targets.push(PairTarget::Pickup(DEMO_DUNGEON_GLOVE_PICKUP));
        }
        if room == DemoDungeonRoom::BootsVault {
            targets.push(PairTarget::Pickup(DEMO_DUNGEON_BOOT_PICKUP));
        }
        for entry in &entries {
            for target in &targets {
                jobs.push(Job {
                    room,
                    entry: *entry,
                    target: *target,
                });
            }
        }
    }

    let results: Mutex<BTreeMap<PairKey, Verdict>> = Mutex::new(cache.clone());
    let pending: Vec<&Job> = if graph_only {
        Vec::new()
    } else {
        jobs.iter()
            .filter(|job| {
                LOADOUTS
                    .iter()
                    .any(|&(wall, dash)| !cache.contains_key(&pair_key(job, wall, dash)))
            })
            .collect()
    };
    eprintln!(
        "{} pairs total, {} jobs need solving",
        jobs.len(),
        pending.len()
    );
    let queue = Mutex::new(pending.into_iter().collect::<VecDeque<_>>());
    std::thread::scope(|scope| {
        for _ in 0..10 {
            scope.spawn(|| {
                loop {
                    let Some(job) = queue.lock().unwrap().pop_front() else {
                        return;
                    };
                    // Monotone lattice: none first, then single abilities,
                    // then both; a weaker witness replays under any superset.
                    let mut solved: BTreeMap<String, Verdict> = BTreeMap::new();
                    let none = solve_pair(job, false, false);
                    solved.insert(loadout_key(false, false), none.clone());
                    let mut best_single: Option<Verdict> = None;
                    for (wall, dash) in [(true, false), (false, true)] {
                        let verdict = if matches!(none, Verdict::Solved(_)) {
                            none.clone()
                        } else {
                            solve_pair(job, wall, dash)
                        };
                        if matches!(verdict, Verdict::Solved(_)) && best_single.is_none() {
                            best_single = Some(verdict.clone());
                        }
                        solved.insert(loadout_key(wall, dash), verdict);
                    }
                    let both = if let Some(single) = best_single {
                        single
                    } else {
                        solve_pair(job, true, true)
                    };
                    solved.insert(loadout_key(true, true), both);
                    let mut results = results.lock().unwrap();
                    for (loadout, verdict) in solved {
                        let mut key = pair_key(job, false, false);
                        key.4 = loadout;
                        results.insert(key, verdict);
                    }
                    eprintln!(
                        "  solved {} {} -> {} ({} left)",
                        job.room.id(),
                        job.entry.unwrap_or("spawn"),
                        job.target.id(),
                        queue.lock().unwrap().len()
                    );
                }
            });
        }
    });

    let results = results.into_inner().unwrap();
    let mut rendered = artifact_header();
    for ((room, entry, kind, id, loadout), verdict) in &results {
        match verdict {
            Verdict::Solved(actions) => {
                rendered.push_str(&format!(
                    "pair {room} {entry} {kind} {id} {loadout} solved\n"
                ));
                rendered.push_str(&render_actions(actions));
            }
            Verdict::Inconclusive(reason) => {
                rendered.push_str(&format!(
                    "pair {room} {entry} {kind} {id} {loadout} inconclusive:{}\n",
                    reason.replace(char::is_whitespace, "-")
                ));
            }
        }
        rendered.push_str("end\n");
    }
    if !graph_only {
        fs::write(&path, &rendered).expect("write traversal artifact");
        eprintln!("wrote {ARTIFACT}");
    }

    run_graph_checks(&results);
}

const DOOR_NAMES: [&str; 4] = ["west", "east", "ceiling", "floor"];

type State = (DemoDungeonRoom, &'static str, u8); // entry "spawn" for the run start

fn pair_solved(
    results: &BTreeMap<PairKey, Verdict>,
    room: DemoDungeonRoom,
    entry: &str,
    target: PairTarget,
    abilities: u8,
) -> bool {
    let key = (
        room.id().to_owned(),
        entry.to_owned(),
        target.kind().to_owned(),
        target.id().to_owned(),
        loadout_key(abilities & 1 != 0, abilities & 2 != 0),
    );
    matches!(results.get(&key), Some(Verdict::Solved(_)))
}

fn requirement_passable(
    room: DemoDungeonRoom,
    door: &str,
    abilities: u8,
    allow_coins: bool,
) -> bool {
    let requirement = demo_dungeon_door_requirement(room, door);
    if requirement.coins > 0 && !allow_coins {
        return false;
    }
    let mut holder = DemoDungeonInventory::with_coin_count_for_validation(u8::MAX);
    holder.climbing_gloves = abilities & 1 != 0;
    holder.winged_boots = abilities & 2 != 0;
    let methods = holder.authored_progression_inventory();
    let mut check = requirement;
    if allow_coins {
        check.coins = 0;
    }
    check.is_satisfied_by(&methods)
}

type ConnectionMap = BTreeMap<DemoDungeonRoom, Vec<(&'static str, DemoDungeonRoom, &'static str)>>;

fn connection_map() -> ConnectionMap {
    let definition = demo_dungeon_definition();
    let room_by_key: BTreeMap<u16, DemoDungeonRoom> = DemoDungeonRoom::ALL
        .into_iter()
        .map(|room| (room.authored_key().0, room))
        .collect();
    let canonical = |name: &str| -> &'static str {
        DOOR_NAMES
            .iter()
            .copied()
            .find(|candidate| *candidate == name)
            .expect("canonical door name")
    };
    definition
        .floors
        .iter()
        .map(|floor| {
            let room = room_by_key[&floor.key.0];
            let connections = floor
                .connections
                .iter()
                .map(|connection| {
                    (
                        canonical(&connection.door_id),
                        room_by_key[&connection.destination_floor.0],
                        canonical(&connection.destination_door),
                    )
                })
                .collect();
            (room, connections)
        })
        .collect()
}

fn expand(
    results: &BTreeMap<PairKey, Verdict>,
    connections: &ConnectionMap,
    state: State,
    allow_coins: bool,
    allow_upgrades: bool,
) -> Vec<State> {
    let (room, entry, abilities) = state;
    let mut next = Vec::new();
    if allow_upgrades {
        if room == DemoDungeonRoom::ClimberVault
            && abilities & 1 == 0
            && pair_solved(
                results,
                room,
                entry,
                PairTarget::Pickup(DEMO_DUNGEON_GLOVE_PICKUP),
                abilities,
            )
        {
            next.push((room, entry, abilities | 1));
        }
        if room == DemoDungeonRoom::BootsVault
            && abilities & 2 == 0
            && pair_solved(
                results,
                room,
                entry,
                PairTarget::Pickup(DEMO_DUNGEON_BOOT_PICKUP),
                abilities,
            )
        {
            next.push((room, entry, abilities | 2));
        }
    }
    for &(door, destination, destination_door) in &connections[&room] {
        if !pair_solved(results, room, entry, PairTarget::Door(door), abilities) {
            continue;
        }
        if !requirement_passable(room, door, abilities, allow_coins) {
            continue;
        }
        next.push((destination, destination_door, abilities));
    }
    next
}

fn run_graph_checks(results: &BTreeMap<PairKey, Verdict>) {
    let connections = connection_map();
    // Check 1: forward reachability from the spawn, coins optimistic.
    let start: State = (DemoDungeonRoom::HollowLanding, "spawn", 0);
    let mut reachable: BTreeSet<State> = BTreeSet::new();
    let mut frontier = VecDeque::from([start]);
    reachable.insert(start);
    while let Some(state) = frontier.pop_front() {
        for next in expand(results, &connections, state, true, true) {
            if reachable.insert(next) {
                frontier.push_back(next);
            }
        }
    }
    let reachable_rooms: BTreeSet<DemoDungeonRoom> =
        reachable.iter().map(|(room, _, _)| *room).collect();
    let unreachable: Vec<&str> = DemoDungeonRoom::ALL
        .iter()
        .filter(|room| !reachable_rooms.contains(room))
        .map(|room| room.id())
        .collect();
    println!(
        "check 1: {}/{} rooms reachable from spawn",
        reachable_rooms.len(),
        DemoDungeonRoom::ALL.len()
    );
    for room in &unreachable {
        println!("  UNREACHABLE {room}");
    }

    // Check 2: from every reachable state, Hollow Landing must be reachable
    // through zero-coin doors only (upgrades still allowed: pickups are free).
    let mut stuck = Vec::new();
    for &state in &reachable {
        let mut seen = BTreeSet::from([state]);
        let mut frontier = VecDeque::from([state]);
        let mut returned = state.0 == DemoDungeonRoom::HollowLanding;
        while let Some(current) = frontier.pop_front() {
            if returned {
                break;
            }
            for next in expand(results, &connections, current, false, true) {
                if next.0 == DemoDungeonRoom::HollowLanding {
                    returned = true;
                    break;
                }
                if seen.insert(next) {
                    frontier.push_back(next);
                }
            }
        }
        if !returned {
            stuck.push(state);
        }
    }
    println!(
        "check 2: {}/{} reachable states can retreat to the entrance without coins",
        reachable.len() - stuck.len(),
        reachable.len()
    );
    for (room, entry, abilities) in &stuck {
        println!(
            "  STUCK {} entered by {entry} with wall={} dash={}",
            room.id(),
            abilities & 1 != 0,
            abilities & 2 != 0
        );
    }
    if !unreachable.is_empty() || !stuck.is_empty() {
        std::process::exit(1);
    }
    println!("dungeon graph audit passed");
}
