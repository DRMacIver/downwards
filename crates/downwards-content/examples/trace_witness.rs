//! Print the simulation events of retained witness routes for named rooms.
//!
//! Run with: `cargo run -p downwards-content --example trace_witness -- <slug>...`

use downwards_content::{
    demo_dungeon_room, demo_dungeon_route_specs, demo_dungeon_witness_actions,
};
use downwards_core::{Action, JumpKind, Simulation, SimulationEvent};

fn main() {
    let slugs: Vec<String> = std::env::args().skip(1).collect();
    for spec in demo_dungeon_route_specs() {
        if !slugs.iter().any(|slug| spec.id().ends_with(slug.as_str())) {
            continue;
        }
        println!("== {} ==", spec.id());
        let room = demo_dungeon_room(spec.room, spec.inventory);
        let mut simulation = match spec.entry_door {
            Some(door) => {
                Simulation::enter_via_door(room, spec.inventory.abilities(), door).unwrap()
            }
            None => Simulation::with_abilities(room, spec.inventory.abilities()),
        };
        simulation.enable_current_player_movement();
        let mut previous = Action::default();
        for (tick, action) in demo_dungeon_witness_actions(spec.room)
            .into_iter()
            .enumerate()
        {
            let jump_press = action.jump && !previous.jump;
            let dash_press = action.dash && !previous.dash;
            let pre = simulation.player().bounds();
            previous = action;
            let report = simulation.step(action);
            for event in report.events {
                let bounds = simulation.player().bounds();
                match event {
                    SimulationEvent::Jumped(kind) => {
                        let label = match kind {
                            JumpKind::Wall { side } => format!("wall-jump {side:?}"),
                            other => format!("{other:?}"),
                        };
                        println!(
                            "t{tick:>4} {label} pre=({},{}) post=({},{})",
                            pre.x, pre.y, bounds.x, bounds.y
                        );
                    }
                    SimulationEvent::Dashed { .. } => {
                        println!(
                            "t{tick:>4} dash pre=({},{}) post=({},{}) press={dash_press}",
                            pre.x, pre.y, bounds.x, bounds.y
                        );
                    }
                    SimulationEvent::Landed => {
                        println!("t{tick:>4} landed at ({},{})", bounds.x, bounds.y);
                    }
                    SimulationEvent::Died(reason) => println!("t{tick:>4} DIED {reason:?}"),
                    SimulationEvent::Reset => println!("t{tick:>4} RESET"),
                    other => println!("t{tick:>4} {other:?}"),
                }
                let _ = jump_press;
            }
        }
        println!("reached exit: {:?}", simulation.reached_exit());
        println!(
            "collected: {:?}",
            simulation
                .collected_pickups()
                .map(|pickup| pickup.id().to_owned())
                .collect::<Vec<_>>()
        );
    }
}
