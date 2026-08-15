//! Replay-certified generated rooms awaiting human calibration.
//!
//! The included witness file is produced mechanically by the
//! `retune_calibrated_generator` example. These entries are deliberately kept
//! separate from the hand-authored calibration gallery.

use std::sync::OnceLock;

use downwards_core::{Action, PLAYER_MOVEMENT_POLICY_VERSION, Simulation};
use downwards_gen::{
    CALIBRATED_WALL_JUMP_ABILITIES, CALIBRATED_WALL_JUMP_GENERATION_VERSION,
    CALIBRATED_WALL_JUMP_TARGET, CalibratedWallJumpCourse, CalibratedWallJumpKey,
};

const PLAYTEST_ARTIFACT: &str = include_str!("../generated/calibrated-wall-jump-witnesses-v1.txt");
const PLAYTEST_SEEDS: [u64; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

#[derive(Clone, Debug)]
struct GeneratedWitness {
    seed: u64,
    room_id: String,
    actions: Vec<Action>,
}

/// One exact generated key in the initial human playtest batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CalibratedGeneratorPlaytestLevel {
    seed: u64,
}

impl CalibratedGeneratorPlaytestLevel {
    #[must_use]
    pub const fn seed(self) -> u64 {
        self.seed
    }

    #[must_use]
    pub fn course(self) -> CalibratedWallJumpCourse {
        CalibratedWallJumpKey::new(self.seed)
            .generate()
            .parameters
            .course
    }

    #[must_use]
    pub fn title(self) -> String {
        format!("Generated {} {:02}", self.course().slug(), self.seed)
    }

    #[must_use]
    pub fn mechanic_axis(self) -> &'static str {
        match self.course() {
            CalibratedWallJumpCourse::ShortTurns => "short climb / staged turns",
            CalibratedWallJumpCourse::EvenTempo => "regular alternating wall rhythm",
            CalibratedWallJumpCourse::RecoveryAscent => "alternating wall rhythm / recovery shelf",
            CalibratedWallJumpCourse::Causeway => "wall climb / rising landing chain",
            CalibratedWallJumpCourse::LowBridge => "wall climb / rising landing chain / jump cut",
        }
    }

    /// Construct a fresh authoritative current-movement scenario.
    #[must_use]
    pub fn scenario(self) -> Simulation {
        let candidate = CalibratedWallJumpKey::new(self.seed).generate();
        let witness = witness(self.seed);
        assert_eq!(candidate.room.id(), witness.room_id);
        let mut simulation =
            Simulation::with_abilities(candidate.room, CALIBRATED_WALL_JUMP_ABILITIES);
        simulation.enable_current_player_movement();
        simulation
    }

    /// Expand the mechanically regenerated exact witness.
    #[must_use]
    pub fn witness_actions(self) -> Vec<Action> {
        witness(self.seed).actions.clone()
    }

    #[must_use]
    pub const fn target(self) -> &'static str {
        CALIBRATED_WALL_JUMP_TARGET
    }
}

static PLAYTEST_LEVELS: [CalibratedGeneratorPlaytestLevel; 12] = [
    CalibratedGeneratorPlaytestLevel { seed: 0 },
    CalibratedGeneratorPlaytestLevel { seed: 1 },
    CalibratedGeneratorPlaytestLevel { seed: 2 },
    CalibratedGeneratorPlaytestLevel { seed: 3 },
    CalibratedGeneratorPlaytestLevel { seed: 4 },
    CalibratedGeneratorPlaytestLevel { seed: 5 },
    CalibratedGeneratorPlaytestLevel { seed: 6 },
    CalibratedGeneratorPlaytestLevel { seed: 7 },
    CalibratedGeneratorPlaytestLevel { seed: 8 },
    CalibratedGeneratorPlaytestLevel { seed: 9 },
    CalibratedGeneratorPlaytestLevel { seed: 10 },
    CalibratedGeneratorPlaytestLevel { seed: 11 },
];

/// Stable seed-ordered generated playtest batch.
#[must_use]
pub fn calibrated_generator_playtest() -> &'static [CalibratedGeneratorPlaytestLevel] {
    &PLAYTEST_LEVELS
}

fn witness(seed: u64) -> &'static GeneratedWitness {
    witnesses()
        .iter()
        .find(|witness| witness.seed == seed)
        .unwrap_or_else(|| panic!("generated playtest seed {seed} has no witness"))
}

fn witnesses() -> &'static [GeneratedWitness] {
    static WITNESSES: OnceLock<Vec<GeneratedWitness>> = OnceLock::new();
    WITNESSES.get_or_init(parse_artifact)
}

fn parse_artifact() -> Vec<GeneratedWitness> {
    let mut lines = PLAYTEST_ARTIFACT.lines();
    assert_eq!(
        lines.next(),
        Some("calibrated-wall-jump-witnesses-v1"),
        "generated playtest witness schema differs"
    );
    assert_eq!(
        lines.next(),
        Some(format!("generation-version {CALIBRATED_WALL_JUMP_GENERATION_VERSION}").as_str()),
        "generated playtest witness generation version differs"
    );
    let movement_version = lines
        .next()
        .and_then(|line| line.strip_prefix("movement-policy-version "))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("generated witness movement policy is malformed");
    assert_eq!(
        movement_version, PLAYER_MOVEMENT_POLICY_VERSION,
        "generated witness movement policy differs"
    );

    let mut parsed = Vec::new();
    while let Some(seed_line) = lines.next() {
        let seed = parse_value::<u64>(seed_line, "seed ");
        let room_id = lines
            .next()
            .and_then(|line| line.strip_prefix("room-id "))
            .filter(|id| !id.is_empty())
            .expect("generated witness room ID")
            .to_owned();
        consume_field(&mut lines, "course ");
        consume_field(&mut lines, "reflected ");
        let ticks = parse_value::<usize>(
            lines.next().expect("generated witness tick count"),
            "ticks ",
        );
        for prefix in [
            "jump-presses ",
            "accepted-jumps ",
            "wall-jumps ",
            "repeated-wall-sides ",
            "action-spans ",
            "direct-positives ",
            "baseline-status ",
            "baseline-search ",
        ] {
            consume_field(&mut lines, prefix);
        }

        let mut actions = Vec::with_capacity(ticks);
        loop {
            let line = lines.next().expect("generated witness record terminator");
            if line == "end" {
                break;
            }
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            assert_eq!(fields.len(), 7, "malformed generated witness span");
            assert_eq!(fields[0], "span", "malformed generated witness span");
            let count = fields[1].parse::<usize>().expect("witness span count");
            assert!(count > 0, "generated witness spans must be nonempty");
            let action = Action {
                move_x: fields[2].parse().expect("witness horizontal input"),
                move_y: fields[3].parse().expect("witness vertical input"),
                jump: parse_bit(fields[4]),
                dash: parse_bit(fields[5]),
                restart: parse_bit(fields[6]),
            };
            actions.extend(std::iter::repeat_n(action, count));
        }
        assert_eq!(actions.len(), ticks, "generated witness tick count differs");
        parsed.push(GeneratedWitness {
            seed,
            room_id,
            actions,
        });
    }
    assert_eq!(
        parsed
            .iter()
            .map(|witness| witness.seed)
            .collect::<Vec<_>>(),
        PLAYTEST_SEEDS,
        "generated playtest witness seeds differ"
    );
    parsed
}

fn consume_field<'a>(lines: &mut impl Iterator<Item = &'a str>, prefix: &str) {
    let line = lines.next().expect("generated witness field");
    let value = line
        .strip_prefix(prefix)
        .expect("generated witness field name");
    assert!(!value.is_empty(), "generated witness field is empty");
}

fn parse_value<T: std::str::FromStr>(line: &str, prefix: &str) -> T {
    line.strip_prefix(prefix)
        .expect("generated witness field name")
        .parse()
        .unwrap_or_else(|_| panic!("generated witness numeric field {prefix:?}"))
}

fn parse_bit(value: &str) -> bool {
    match value {
        "0" => false,
        "1" => true,
        _ => panic!("generated witness bit differs"),
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{JumpKind, SimulationEvent, WallSide};

    use super::*;

    #[test]
    fn generated_playtest_artifact_exactly_replays_current_rooms() {
        for level in calibrated_generator_playtest() {
            let mut simulation = level.scenario();
            let actions = level.witness_actions();
            let mut previous = Action::default();
            let mut presses = 0;
            let mut jumps = 0;
            let mut wall_sides = Vec::new();
            let mut first_reached = None;

            for (index, action) in actions.iter().copied().enumerate() {
                presses += usize::from(action.jump && !previous.jump);
                assert!(!action.dash && !action.restart);
                for event in simulation.step(action).events {
                    match event {
                        SimulationEvent::Jumped(kind) => {
                            jumps += 1;
                            if let JumpKind::Wall { side } = kind {
                                wall_sides.push(side);
                            }
                        }
                        SimulationEvent::ExitReached { ref id } if id == level.target() => {
                            first_reached.get_or_insert(index + 1);
                        }
                        SimulationEvent::Died(_)
                        | SimulationEvent::Reset
                        | SimulationEvent::Dashed { .. } => {
                            panic!("generated seed {} has an invalid event", level.seed());
                        }
                        SimulationEvent::Landed
                        | SimulationEvent::PickupCollected { .. }
                        | SimulationEvent::ExitReached { .. } => {}
                    }
                }
                previous = action;
            }

            assert_eq!(presses, jumps, "seed {} has input thrash", level.seed());
            assert_eq!(first_reached, Some(actions.len()));
            assert_eq!(simulation.reached_exit(), Some(level.target()));
            assert!((3..=7).contains(&wall_sides.len()));
            assert!(
                wall_sides
                    .windows(2)
                    .filter(|pair| pair[0] == pair[1])
                    .count()
                    <= 1
            );
            assert!(
                wall_sides
                    .iter()
                    .all(|side| matches!(side, WallSide::Left | WallSide::Right))
            );
        }
    }
}
