//! Write one markdown context file per dungeon room for design review.
//!
//! Run with: `cargo run -p downwards-content --example dump_room_context -- <output-dir>`

use std::{collections::BTreeMap, fs, path::PathBuf};

use downwards_content::{
    DemoDungeonRoom, demo_dungeon_definition, demo_dungeon_door_requirement,
    demo_dungeon_route_specs,
};

const WITNESSES: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");

fn main() {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: dump_room_context <output-dir>"),
    );
    fs::create_dir_all(&output).expect("create output dir");

    let mut witness_sections: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in WITNESSES.lines() {
        if let Some(route) = line.strip_prefix("route ") {
            current = Some((route.to_owned(), String::new()));
        }
        if let Some((_, section)) = current.as_mut() {
            if !line.starts_with("span ") {
                section.push_str(line);
                section.push('\n');
            }
        }
        if line == "end"
            && let Some((route, section)) = current.take()
        {
            witness_sections.insert(route, section);
        }
    }

    let definition = demo_dungeon_definition();
    let specs = demo_dungeon_route_specs();
    for room in DemoDungeonRoom::ALL {
        let slug = room.id().trim_start_matches("demo-dungeon.");
        let grid = fs::read_to_string(format!("crates/downwards-gen/rooms/{slug}.txt"))
            .expect("room grid file");
        let floor = definition
            .floor(room.authored_key())
            .expect("room has a floor definition");
        let spec = specs
            .iter()
            .find(|spec| spec.room == room)
            .expect("room has a route spec");
        let mut rendered = format!(
            "# {} ({})\n\nFloor key {} of {}. Analysis loadout: wall_jump={} dash={}. Entry: {}.\nAnalysis target: {} {}.\nCoins in room: {:?}. Traversal unlock here: {:?}. Crown here: {}.\n\n## Connections\n",
            floor.title,
            room.id(),
            floor.key.0,
            DemoDungeonRoom::ALL.len(),
            spec.inventory.abilities().wall_jump,
            spec.inventory.abilities().dash,
            spec.entry_door.unwrap_or("spawn"),
            spec.target.kind(),
            spec.target.id(),
            floor.coin_indices,
            floor.traversal_unlock,
            floor.contains_crown,
        );
        for connection in &floor.connections {
            let destination = definition
                .floor(connection.destination_floor)
                .expect("destination floor");
            let requirement = demo_dungeon_door_requirement(room, &connection.door_id);
            rendered.push_str(&format!(
                "- {} -> {} (via their {}), requires coins={} methods={:?}\n",
                connection.door_id,
                destination.title,
                connection.destination_door,
                requirement.coins,
                requirement.traversal_methods,
            ));
        }
        rendered.push_str(
            "\n## Tile grid (32x18; # solid, - one-way, ^v<> spikes lethal except their back face, . empty; door mouths are the boundary gaps)\n\n```\n",
        );
        rendered.push_str(&grid);
        rendered.push_str("```\n\n## Retained AI witness (observation = spans, jump presses, accepted jumps, wall jumps, dashes, reversals; shaky lines = successes/trials/deaths under 1-tick input noise)\n\n```\n");
        rendered.push_str(
            witness_sections
                .get(room.id())
                .map(String::as_str)
                .unwrap_or("no witness recorded\n"),
        );
        rendered.push_str("```\n");
        fs::write(output.join(format!("{slug}.md")), rendered).expect("write context file");
    }
    eprintln!("wrote {} context files", DemoDungeonRoom::ALL.len());
}
