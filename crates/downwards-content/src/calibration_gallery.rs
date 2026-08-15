//! Stable metadata and content-owned factories for the authored calibration gallery.
//!
//! The gallery is deliberately separate from generated rooms and corpus/catalogue
//! provenance. Its ordering and IDs are part of the playtest interface; difficulty
//! predictions are not. Each entry owns an exact target, locked loadout, and stored
//! tractability witness.

use std::sync::OnceLock;

use downwards_core::{AbilitySet, Action, Simulation};

use crate::{
    CALIBRATION_GALLERY_A_ABILITIES, CALIBRATION_GALLERY_A_TARGET, HARD_NO_DASH_ABILITIES,
    HARD_NO_DASH_TARGET, MEDIUM_NO_DASH_ABILITIES, MEDIUM_NO_DASH_TARGET,
    calibration_gallery_a_cases, calibration_gallery_b_cases, hard_no_dash_scenario,
    hard_no_dash_witness_actions, medium_no_dash_scenario, medium_no_dash_witness_actions,
    movement_obstacle_course_cases,
};

/// One stable, hand-authored gallery entry.
#[derive(Clone, Copy)]
pub struct CalibrationLevel {
    id: &'static str,
    title: &'static str,
    mechanic_axis: &'static str,
    target: &'static str,
    abilities: AbilitySet,
    scenario_factory: fn() -> Simulation,
    witness_factory: fn() -> Vec<Action>,
}

impl CalibrationLevel {
    /// Register an authored calibration level without exposing its module to the client.
    #[must_use]
    pub const fn new(
        id: &'static str,
        title: &'static str,
        mechanic_axis: &'static str,
        target: &'static str,
        abilities: AbilitySet,
        scenario_factory: fn() -> Simulation,
        witness_factory: fn() -> Vec<Action>,
    ) -> Self {
        Self {
            id,
            title,
            mechanic_axis,
            target,
            abilities,
            scenario_factory,
            witness_factory,
        }
    }

    /// Stable public calibration ID, independent of the room's native ID.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// Human-facing room title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        self.title
    }

    /// Mechanics varied by this room. This is intentionally not a difficulty claim.
    #[must_use]
    pub const fn mechanic_axis(self) -> &'static str {
        self.mechanic_axis
    }

    /// Exact exit ID reached by the stored witness.
    #[must_use]
    pub const fn target(self) -> &'static str {
        self.target
    }

    /// Content-owned traversal loadout.
    #[must_use]
    pub const fn abilities(self) -> AbilitySet {
        self.abilities
    }

    /// Construct a fresh authoritative scenario.
    #[must_use]
    pub fn scenario(self) -> Simulation {
        (self.scenario_factory)()
    }

    /// Expand the exact stored witness into authoritative per-tick actions.
    #[must_use]
    pub fn witness_actions(self) -> Vec<Action> {
        (self.witness_factory)()
    }
}

static CALIBRATION_GALLERY: OnceLock<[CalibrationLevel; 15]> = OnceLock::new();

/// The stable ordered authored calibration gallery.
#[must_use]
pub fn calibration_gallery() -> &'static [CalibrationLevel] {
    CALIBRATION_GALLERY.get_or_init(|| {
        let [a1, a2, a3, a4, a5] = calibration_gallery_a_cases();
        let [b1, b2, b3, b4, b5] = calibration_gallery_b_cases();
        let [course1, course2, course3] = movement_obstacle_course_cases();
        [
            CalibrationLevel::new(
                "cal-01",
                "Stepping Stones",
                "alternating wall-jump climb / generous recovery",
                MEDIUM_NO_DASH_TARGET,
                MEDIUM_NO_DASH_ABILITIES,
                medium_no_dash_scenario,
                medium_no_dash_witness_actions,
            ),
            CalibrationLevel::new(
                "cal-02",
                a1.title,
                a1.mechanic_axis,
                CALIBRATION_GALLERY_A_TARGET,
                CALIBRATION_GALLERY_A_ABILITIES,
                a1.scenario_factory,
                a1.witness_factory,
            ),
            CalibrationLevel::new(
                "cal-03",
                a2.title,
                a2.mechanic_axis,
                CALIBRATION_GALLERY_A_TARGET,
                CALIBRATION_GALLERY_A_ABILITIES,
                a2.scenario_factory,
                a2.witness_factory,
            ),
            CalibrationLevel::new(
                "cal-04",
                a3.title,
                a3.mechanic_axis,
                CALIBRATION_GALLERY_A_TARGET,
                CALIBRATION_GALLERY_A_ABILITIES,
                a3.scenario_factory,
                a3.witness_factory,
            ),
            CalibrationLevel::new(
                "cal-05",
                a4.title,
                a4.mechanic_axis,
                CALIBRATION_GALLERY_A_TARGET,
                CALIBRATION_GALLERY_A_ABILITIES,
                a4.scenario_factory,
                a4.witness_factory,
            ),
            CalibrationLevel::new(
                "cal-06",
                a5.title,
                a5.mechanic_axis,
                CALIBRATION_GALLERY_A_TARGET,
                CALIBRATION_GALLERY_A_ABILITIES,
                a5.scenario_factory,
                a5.witness_factory,
            ),
            b1,
            b2,
            b3,
            b4,
            b5,
            CalibrationLevel::new(
                "cal-12",
                "Needle's Eye",
                "alternating wall-jump windows / precision landings",
                HARD_NO_DASH_TARGET,
                HARD_NO_DASH_ABILITIES,
                hard_no_dash_scenario,
                hard_no_dash_witness_actions,
            ),
            course1,
            course2,
            course3,
        ]
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use downwards_ai::Replay;
    use downwards_core::SimulationEvent;

    #[test]
    fn all_entries_have_exact_order_ids_and_content_owned_loadouts() {
        let gallery = calibration_gallery();
        let expected_titles = [
            "Stepping Stones",
            "Open Chimney",
            "Two-Tile Turn",
            "Even Tempo",
            "Low Clearance",
            "Safe Harbor",
            "Three Pins",
            "Long Ascent",
            "Open Shaft",
            "Broken Causeway",
            "Low Bridge",
            "Needle's Eye",
            "Long-Jump Yard",
            "Wall Gym",
            "Mixed Circuit",
        ];
        let expected_native_ids = [
            "dev.medium_no_dash",
            "calibration.wall_jump.a1_broad_ascent",
            "calibration.wall_jump.a2_needle_step",
            "calibration.wall_jump.a3_even_tempo",
            "calibration.wall_jump.a4_low_clearance",
            "calibration.wall_jump.a5_safe_harbor",
            "calibration.wall_jump.b1_needle_chimney",
            "calibration.wall_jump.b2_long_ascent",
            "calibration.wall_jump.b3_open_shaft",
            "calibration.wall_jump.b4_broken_causeway",
            "calibration.wall_jump.b5_low_bridge",
            "dev.hard_no_dash",
            "movement.course.long_jumps",
            "movement.course.wall_gym",
            "movement.course.mixed_circuit",
        ];
        assert_eq!(gallery.len(), 15);
        assert_eq!(gallery.first().map(|level| level.id()), Some("cal-01"));
        assert_eq!(gallery.last().map(|level| level.id()), Some("cal-15"));

        let mut ids = HashSet::new();
        for (index, level) in gallery.iter().copied().enumerate() {
            assert_eq!(level.id(), format!("cal-{:02}", index + 1));
            assert_eq!(level.title(), expected_titles[index]);
            assert!(ids.insert(level.id()));
            assert!(!level.title().is_empty());
            assert!(!level.mechanic_axis().is_empty());
            assert!(!level.abilities().dash);

            let scenario = level.scenario();
            assert_eq!(scenario.room().id(), expected_native_ids[index]);
            assert_eq!(scenario.abilities(), level.abilities());
            assert!(
                scenario
                    .room()
                    .exits()
                    .iter()
                    .any(|exit| exit.id == level.target())
            );
        }
    }

    #[test]
    fn every_witness_is_clean_and_first_reaches_its_target_on_the_final_tick() {
        for level in calibration_gallery().iter().copied() {
            let mut scenario = level.scenario();
            let actions = level.witness_actions();
            assert!(!actions.is_empty());
            assert!(actions.iter().all(|action| !action.dash && !action.restart));
            for (index, action) in actions.iter().copied().enumerate() {
                assert_eq!(scenario.reached_exit(), None);
                let report = scenario.step(action);
                assert!(report.events.iter().all(|event| !matches!(
                    event,
                    SimulationEvent::Died(_)
                        | SimulationEvent::Reset
                        | SimulationEvent::Dashed { .. }
                )));
                if index + 1 < actions.len() {
                    assert_eq!(
                        scenario.reached_exit(),
                        None,
                        "{} {} reaches before its final stored action",
                        level.id(),
                        level.title()
                    );
                }
            }
            assert_eq!(scenario.reached_exit(), Some(level.target()));
            assert_eq!(scenario.deaths(), 0);
        }
    }

    #[test]
    fn every_witness_is_valid_under_the_current_player_movement_policy() {
        let mut failures = Vec::new();
        for level in calibration_gallery().iter().copied() {
            let mut scenario = level.scenario();
            scenario.enable_current_player_movement();
            let replay = Replay::record(&scenario, level.witness_actions());
            match replay.verify(&scenario) {
                Ok(result) if result.reached_exit.as_deref() == Some(level.target()) => {}
                Ok(result) => failures.push(format!(
                    "{} {} reached {:?}",
                    level.id(),
                    level.title(),
                    result.reached_exit
                )),
                Err(error) => failures.push(format!("{} {}: {error}", level.id(), level.title())),
            }
        }
        assert!(
            failures.is_empty(),
            "stored routes need player-movement retuning:\n{}",
            failures.join("\n")
        );
    }
}
