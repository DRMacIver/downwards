//! Checked loader for the mechanically generated demo-dungeon route artifact.

use std::{collections::BTreeMap, sync::OnceLock};

use downwards_core::{Action, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION};
use downwards_gen::DUNGEON_PALETTE_GENERATION_VERSION;

use crate::{DemoDungeonRoom, demo_dungeon_route_specs};

const GENERATED_WITNESSES: &str = include_str!("../generated/demo-dungeon-witnesses-v1.txt");

static ACTIONS_BY_ROOM: OnceLock<BTreeMap<DemoDungeonRoom, Vec<Action>>> = OnceLock::new();

/// Return the exact checked route witness for one authored dungeon floor.
///
/// The embedded artifact is validated in full on first use. A mismatch is a content build defect,
/// not recoverable player input, so this API deliberately fails loudly rather than returning a
/// partially trusted route.
#[must_use]
pub fn demo_dungeon_witness_actions(room: DemoDungeonRoom) -> Vec<Action> {
    parsed_actions()
        .get(&room)
        .unwrap_or_else(|| panic!("generated dungeon witness artifact is missing {room:?}"))
        .clone()
}

fn parsed_actions() -> &'static BTreeMap<DemoDungeonRoom, Vec<Action>> {
    ACTIONS_BY_ROOM.get_or_init(parse_generated_witnesses)
}

fn parse_generated_witnesses() -> BTreeMap<DemoDungeonRoom, Vec<Action>> {
    let mut lines = GENERATED_WITNESSES.lines().peekable();
    assert_eq!(
        lines.next(),
        Some("schema downwards-demo-dungeon-witnesses-v1"),
        "dungeon witness schema differs"
    );
    assert_eq!(
        parse_single_u32(lines.next(), "palette-generation"),
        DUNGEON_PALETTE_GENERATION_VERSION,
        "dungeon witness palette generation differs"
    );
    assert_eq!(
        parse_single_u32(lines.next(), "player-movement-policy"),
        PLAYER_MOVEMENT_POLICY_VERSION,
        "dungeon witness movement policy differs"
    );
    assert_eq!(
        parse_tuning(lines.next()),
        MovementTuning::GAMEPLAY_DEFAULT,
        "dungeon witness tuning differs"
    );
    assert_shaky_policy(lines.next());

    let mut parsed = BTreeMap::new();
    for spec in demo_dungeon_route_specs() {
        assert_eq!(
            lines.next(),
            Some(format!("route {}", spec.id()).as_str()),
            "dungeon witness route order differs"
        );
        assert_eq!(
            lines.next(),
            Some(format!("room {}", spec.room.id()).as_str()),
            "dungeon witness room differs for {}",
            spec.id()
        );
        let expected_entry = spec.entry_door.unwrap_or("none");
        assert_eq!(
            lines.next(),
            Some(format!("entry {expected_entry}").as_str()),
            "dungeon witness entry differs for {}",
            spec.id()
        );
        assert_eq!(
            lines.next(),
            Some(format!("target {} {}", spec.target.kind(), spec.target.id()).as_str()),
            "dungeon witness target differs for {}",
            spec.id()
        );
        let abilities = spec.inventory.abilities();
        assert_eq!(
            lines.next(),
            Some(
                format!(
                    "abilities {} {}",
                    u8::from(abilities.wall_jump),
                    u8::from(abilities.dash)
                )
                .as_str()
            ),
            "dungeon witness abilities differ for {}",
            spec.id()
        );
        let expected_ticks = parse_single_usize(lines.next(), "ticks");
        assert_observation(lines.next());

        let mut shaky_rows = 0;
        while lines.peek().is_some_and(|line| line.starts_with("shaky ")) {
            assert_shaky_row(lines.next());
            shaky_rows += 1;
        }
        assert!(shaky_rows > 0, "{} lacks shaky-hand evidence", spec.id());

        let mut actions = Vec::with_capacity(expected_ticks);
        loop {
            let line = lines
                .next()
                .unwrap_or_else(|| panic!("{} lacks an end marker", spec.id()));
            if line == "end" {
                break;
            }
            let (action, ticks) = parse_span(line);
            assert!(ticks > 0, "{} contains a zero-length span", spec.id());
            assert!(
                actions.last().copied() != Some(action),
                "{} contains adjacent identical spans",
                spec.id()
            );
            actions.extend(std::iter::repeat_n(action, ticks));
        }
        assert_eq!(
            actions.len(),
            expected_ticks,
            "{} tick count differs",
            spec.id()
        );
        assert!(!actions.is_empty(), "{} has an empty route", spec.id());
        assert!(parsed.insert(spec.room, actions).is_none());
    }
    assert_eq!(
        lines.next(),
        None,
        "dungeon witness artifact has trailing records"
    );
    parsed
}

fn parse_single_u32(line: Option<&str>, field: &str) -> u32 {
    parse_single_value(line, field)
        .parse()
        .unwrap_or_else(|_| panic!("dungeon witness {field} is not a u32"))
}

fn parse_single_usize(line: Option<&str>, field: &str) -> usize {
    parse_single_value(line, field)
        .parse()
        .unwrap_or_else(|_| panic!("dungeon witness {field} is not a usize"))
}

fn parse_single_value<'a>(line: Option<&'a str>, field: &str) -> &'a str {
    let line = line.unwrap_or_else(|| panic!("dungeon witness lacks {field}"));
    let (actual_field, value) = line
        .split_once(' ')
        .unwrap_or_else(|| panic!("malformed dungeon witness {field}"));
    assert_eq!(actual_field, field);
    value
}

fn parse_tuning(line: Option<&str>) -> MovementTuning {
    let mut fields = line
        .expect("dungeon witness lacks tuning")
        .split_ascii_whitespace();
    assert_eq!(fields.next(), Some("tuning"));
    let mut next = || {
        fields
            .next()
            .expect("dungeon witness tuning is incomplete")
            .parse::<u16>()
            .expect("dungeon witness tuning is not a u16")
    };
    let tuning = MovementTuning {
        top_speed_pixels_per_second: next(),
        acceleration_milliseconds: next(),
        braking_milliseconds: next(),
        wall_ascent_carry_percent: next(),
        wall_carry_percent: next(),
        wall_momentum_milliseconds: next(),
    };
    assert_eq!(fields.next(), None, "dungeon witness tuning has extras");
    tuning
}

fn assert_shaky_policy(line: Option<&str>) {
    let mut fields = line
        .expect("dungeon witness lacks shaky policy")
        .split_ascii_whitespace();
    assert_eq!(fields.next(), Some("shaky-policy"));
    assert_eq!(fields.count(), 4, "dungeon witness shaky policy differs");
}

fn assert_observation(line: Option<&str>) {
    let mut fields = line
        .expect("dungeon witness lacks observation")
        .split_ascii_whitespace();
    assert_eq!(fields.next(), Some("observation"));
    assert_eq!(fields.count(), 6, "dungeon witness observation differs");
}

fn assert_shaky_row(line: Option<&str>) {
    let mut fields = line
        .expect("dungeon witness lacks shaky row")
        .split_ascii_whitespace();
    assert_eq!(fields.next(), Some("shaky"));
    assert!(
        fields.next().is_some(),
        "dungeon witness shaky family is missing"
    );
    assert_eq!(fields.count(), 3, "dungeon witness shaky row differs");
}

fn parse_span(line: &str) -> (Action, usize) {
    let mut fields = line.split_ascii_whitespace();
    assert_eq!(
        fields.next(),
        Some("span"),
        "unknown dungeon witness record"
    );
    let move_x = parse_i8(fields.next(), "move_x");
    let move_y = parse_i8(fields.next(), "move_y");
    let jump = parse_bool(fields.next(), "jump");
    let dash = parse_bool(fields.next(), "dash");
    let restart = parse_bool(fields.next(), "restart");
    let ticks = fields
        .next()
        .expect("dungeon witness span lacks ticks")
        .parse::<usize>()
        .expect("dungeon witness span ticks are invalid");
    assert_eq!(fields.next(), None, "dungeon witness span has extras");
    assert!((-1..=1).contains(&move_x));
    assert!((-1..=1).contains(&move_y));
    (
        Action {
            move_x,
            move_y,
            jump,
            dash,
            restart,
        },
        ticks,
    )
}

fn parse_i8(value: Option<&str>, field: &str) -> i8 {
    value
        .unwrap_or_else(|| panic!("dungeon witness span lacks {field}"))
        .parse()
        .unwrap_or_else(|_| panic!("dungeon witness span {field} is invalid"))
}

fn parse_bool(value: Option<&str>, field: &str) -> bool {
    match value.unwrap_or_else(|| panic!("dungeon witness span lacks {field}")) {
        "0" => false,
        "1" => true,
        _ => panic!("dungeon witness span {field} is not 0 or 1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_artifact_is_complete_and_policy_bound() {
        let parsed = parsed_actions();
        assert_eq!(parsed.len(), DemoDungeonRoom::ALL.len());
        assert!(
            DemoDungeonRoom::ALL
                .into_iter()
                .all(|room| parsed.get(&room).is_some_and(|actions| !actions.is_empty()))
        );
    }
}
