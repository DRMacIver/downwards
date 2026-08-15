//! Deterministic construction-only boundary socket inventory for partition rooms.

use std::{collections::BTreeMap, env, error::Error};

use downwards_core::{AbilitySet, BoundarySide, DoorSocket};
use downwards_gen::experimental::{
    ChallengeIntent, PARTITION_ROUTE_GENERATION_VERSION, PartitionRouteKey, PartitionRouteProfile,
};

const USAGE: &str = "usage: cargo run --bin partition_route_socket_audit -- <start-seed> <seed-count> [embedding-attempt]";

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [start_seed, seed_count, tail @ ..] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    if tail.len() > 1 {
        return Err(USAGE.into());
    }
    let start_seed = start_seed.parse::<u64>()?;
    let seed_count = seed_count.parse::<usize>()?;
    if seed_count == 0 {
        return Err("seed-count must be positive".into());
    }
    let embedding_attempt = tail.first().map_or(Ok(0), |value| value.parse::<u8>())?;

    let mut attempted = 0_usize;
    let mut constructed = 0_usize;
    let mut failures = BTreeMap::<String, usize>::new();
    let mut port_counts = BTreeMap::<usize, usize>::new();
    let mut side_combinations = BTreeMap::<String, usize>::new();
    let mut sockets = BTreeMap::<DoorSocket, usize>::new();
    let mut socket_occurrences = Vec::<SocketOccurrence>::new();
    let mut per_profile = BTreeMap::<&'static str, ProfileInventory>::new();

    for profile in PartitionRouteProfile::ALL {
        for intent in ChallengeIntent::ALL {
            for offset in 0..seed_count {
                let seed = start_seed.wrapping_add(offset as u64);
                attempted += 1;
                let key = PartitionRouteKey::new(seed, AbilitySet::NONE, intent, profile)
                    .with_embedding_attempt(embedding_attempt);
                let candidate = match key.regenerate() {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        *failures.entry(format!("{:?}", error.cause)).or_default() += 1;
                        continue;
                    }
                };
                let room_index = constructed;
                constructed += 1;
                let profile_inventory = per_profile.entry(profile.slug()).or_default();
                profile_inventory.rooms += 1;
                profile_inventory
                    .graph_signatures
                    .insert(candidate.derivation.graph_topology_signature, ());
                profile_inventory
                    .route_signatures
                    .insert(candidate.derivation.route_derivation_signature, ());
                profile_inventory
                    .derivation_fingerprints
                    .insert(candidate.derivation.derivation_fingerprint, ());

                *port_counts
                    .entry(candidate.boundary_ports.len())
                    .or_default() += 1;
                let mut sides = candidate
                    .boundary_ports
                    .iter()
                    .map(|port| port.door.side)
                    .collect::<Vec<_>>();
                sides.sort_unstable();
                *side_combinations.entry(side_slug(&sides)).or_default() += 1;
                for port in &candidate.boundary_ports {
                    let socket = port.door.socket();
                    *sockets.entry(socket).or_default() += 1;
                    socket_occurrences.push(SocketOccurrence { room_index, socket });
                }
            }
        }
    }

    let occurrence_has_different_room_mate = |occurrence: &SocketOccurrence| {
        socket_occurrences.iter().any(|candidate| {
            candidate.room_index != occurrence.room_index
                && occurrence.socket.matches(candidate.socket)
        })
    };
    let occurrences_missing = socket_occurrences
        .iter()
        .filter(|occurrence| !occurrence_has_different_room_mate(occurrence))
        .count();
    let distinct_missing = socket_occurrences
        .iter()
        .filter(|occurrence| !occurrence_has_different_room_mate(occurrence))
        .map(|occurrence| (occurrence.socket, ()))
        .collect::<BTreeMap<_, ()>>()
        .len();
    println!(
        "source=partition-route-v{PARTITION_ROUTE_GENERATION_VERSION}-sockets start-seed={start_seed} seed-count={seed_count} attempt={embedding_attempt} attempted={attempted} constructed={constructed} distinct-sockets={} socket-occurrences={} distinct-missing-different-room-mates={distinct_missing} occurrences-missing-different-room-mates={occurrences_missing}",
        sockets.len(),
        sockets.values().sum::<usize>(),
    );
    for (profile, inventory) in per_profile {
        println!(
            "profile={profile} rooms={} graph-signatures={} route-signatures={} full-fingerprints={}",
            inventory.rooms,
            inventory.graph_signatures.len(),
            inventory.route_signatures.len(),
            inventory.derivation_fingerprints.len(),
        );
    }
    for (count, rooms) in port_counts {
        println!("port-count={count} rooms={rooms}");
    }
    for (sides, rooms) in side_combinations {
        println!("side-combination={sides} rooms={rooms}");
    }
    for (socket, occurrences) in &sockets {
        let other_room_mates = socket_occurrences
            .iter()
            .filter(|occurrence| occurrence.socket == *socket)
            .map(|occurrence| {
                socket_occurrences
                    .iter()
                    .filter(|candidate| {
                        candidate.room_index != occurrence.room_index
                            && occurrence.socket.matches(candidate.socket)
                    })
                    .count()
            })
            .sum::<usize>();
        println!(
            "socket side={} offset={} span={} occurrences={} different-room-mate-links={} different-room-mate-present={}",
            side_name(socket.side),
            socket.offset,
            socket.span,
            occurrences,
            other_room_mates,
            if other_room_mates > 0 { "yes" } else { "no" },
        );
    }
    for (cause, count) in failures {
        println!("construction-failure count={count} cause={cause}");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct SocketOccurrence {
    room_index: usize,
    socket: DoorSocket,
}

#[derive(Debug, Default)]
struct ProfileInventory {
    rooms: usize,
    graph_signatures: BTreeMap<u64, ()>,
    route_signatures: BTreeMap<u64, ()>,
    derivation_fingerprints: BTreeMap<u64, ()>,
}

fn side_slug(sides: &[BoundarySide]) -> String {
    sides
        .iter()
        .map(|side| side_name(*side))
        .collect::<Vec<_>>()
        .join("+")
}

const fn side_name(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "left",
        BoundarySide::Right => "right",
        BoundarySide::Ceiling => "ceiling",
        BoundarySide::Floor => "floor",
    }
}
