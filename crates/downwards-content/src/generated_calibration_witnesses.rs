//! Checked loader for the mechanically generated calibration witness artifact.

use std::{collections::BTreeMap, sync::OnceLock};

use downwards_core::{Action, MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION};

const GENERATED_WITNESSES: &str = include_str!("../generated/calibration-witnesses-v2.txt");

static ACTIONS_BY_LEVEL: OnceLock<BTreeMap<&'static str, Vec<Action>>> = OnceLock::new();

pub(crate) fn generated_witness_actions(level_id: &'static str) -> Vec<Action> {
    parsed_actions()
        .get(level_id)
        .unwrap_or_else(|| panic!("generated witness artifact is missing {level_id:?}"))
        .clone()
}

fn parsed_actions() -> &'static BTreeMap<&'static str, Vec<Action>> {
    ACTIONS_BY_LEVEL.get_or_init(parse_generated_witnesses)
}

fn parse_generated_witnesses() -> BTreeMap<&'static str, Vec<Action>> {
    let mut lines = GENERATED_WITNESSES.lines();
    assert_eq!(
        lines.next(),
        Some("schema downwards-calibration-witnesses-v2"),
        "generated witness schema differs"
    );
    assert_eq!(
        parse_single_u32(lines.next(), "player-movement-policy"),
        PLAYER_MOVEMENT_POLICY_VERSION,
        "generated witness movement policy differs"
    );
    assert_eq!(
        parse_tuning(lines.next()),
        MovementTuning::GAMEPLAY_DEFAULT,
        "generated witness tuning differs"
    );

    let expected_ids = [
        "cal-01", "cal-02", "cal-03", "cal-04", "cal-05", "cal-06", "cal-07", "cal-08", "cal-09",
        "cal-10", "cal-11", "cal-12", "cal-13", "cal-14", "cal-15",
    ];
    let mut parsed = BTreeMap::new();
    for expected_id in expected_ids {
        let line = lines.next().expect("generated witness level is missing");
        assert_eq!(line, format!("level {expected_id}"));
        let mut actions = Vec::new();
        loop {
            let line = lines
                .next()
                .expect("generated witness level lacks an end marker");
            if line == "end" {
                break;
            }
            let (action, ticks) = parse_span(line);
            assert!(ticks > 0, "generated witness contains a zero-length span");
            assert!(
                actions.last().copied() != Some(action),
                "generated witness contains adjacent identical spans"
            );
            actions.extend(std::iter::repeat_n(action, ticks));
        }
        assert!(!actions.is_empty(), "generated witness is empty");
        assert!(parsed.insert(expected_id, actions).is_none());
    }
    assert_eq!(lines.next(), None, "generated witness has trailing records");
    parsed
}

fn parse_single_u32(line: Option<&str>, field: &str) -> u32 {
    let line = line.unwrap_or_else(|| panic!("generated witness lacks {field}"));
    let (actual_field, value) = line
        .split_once(' ')
        .unwrap_or_else(|| panic!("malformed generated witness {field}"));
    assert_eq!(actual_field, field);
    value
        .parse()
        .unwrap_or_else(|_| panic!("generated witness {field} is not a u32"))
}

fn parse_tuning(line: Option<&str>) -> MovementTuning {
    let mut fields = line
        .expect("generated witness lacks tuning")
        .split_ascii_whitespace();
    assert_eq!(fields.next(), Some("tuning"));
    let mut next = || {
        fields
            .next()
            .expect("generated witness tuning is incomplete")
            .parse::<u16>()
            .expect("generated witness tuning is not a u16")
    };
    let tuning = MovementTuning {
        top_speed_pixels_per_second: next(),
        acceleration_milliseconds: next(),
        braking_milliseconds: next(),
        wall_ascent_carry_percent: next(),
        wall_carry_percent: next(),
        wall_momentum_milliseconds: next(),
    };
    assert_eq!(fields.next(), None, "generated witness tuning has extras");
    tuning
}

fn parse_span(line: &str) -> (Action, usize) {
    let mut fields = line.split_ascii_whitespace();
    assert_eq!(fields.next(), Some("span"), "unknown generated record");
    let move_x = parse_i8(fields.next(), "move_x");
    let move_y = parse_i8(fields.next(), "move_y");
    let jump = parse_bool(fields.next(), "jump");
    let dash = parse_bool(fields.next(), "dash");
    let restart = parse_bool(fields.next(), "restart");
    let ticks = fields
        .next()
        .expect("generated span lacks ticks")
        .parse::<usize>()
        .expect("generated span ticks are invalid");
    assert_eq!(fields.next(), None, "generated span has extras");
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
        .unwrap_or_else(|| panic!("generated span lacks {field}"))
        .parse()
        .unwrap_or_else(|_| panic!("generated span {field} is invalid"))
}

fn parse_bool(value: Option<&str>, field: &str) -> bool {
    match value.unwrap_or_else(|| panic!("generated span lacks {field}")) {
        "0" => false,
        "1" => true,
        _ => panic!("generated span {field} is not 0 or 1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_artifact_is_complete_and_policy_bound() {
        let parsed = parsed_actions();
        assert_eq!(parsed.len(), 15);
        assert_eq!(parsed.keys().next().copied(), Some("cal-01"));
        assert_eq!(parsed.keys().next_back().copied(), Some("cal-15"));
        assert!(parsed.values().all(|actions| !actions.is_empty()));
    }
}
