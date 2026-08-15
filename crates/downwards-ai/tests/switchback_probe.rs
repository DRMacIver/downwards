use downwards_ai::{
    DirectProbeAuditStatus, DirectProbePolicy, SearchTarget, SolverConfig, TargetSolveOutcome,
    audit_direct_controller_probes, solve_targets,
};
use downwards_core::{AbilitySet, Simulation};
use downwards_gen::experimental::{ChallengeIntent, SwitchbackCutGrammar, SwitchbackCutKey};

fn frozen_switchback_initial() -> Simulation {
    let candidate = SwitchbackCutKey::new(0, AbilitySet::NONE, ChallengeIntent::Gentle)
        .with_embedding(SwitchbackCutGrammar::ShelfReturnV1, 0)
        .regenerate()
        .expect("the frozen experimental key constructs");
    Simulation::enter_via_door(candidate.generated.room, AbilitySet::NONE, "port-bottom")
        .expect("the frozen room has its bottom entry door")
}

#[test]
fn staged_homing_probe_composes_a_baseline_switchback_route() {
    let initial = frozen_switchback_initial();
    let targets = [
        SearchTarget::door("port-ceiling"),
        SearchTarget::pickup("switchback-cache"),
    ];
    let config = SolverConfig::for_abilities(AbilitySet::NONE);

    let first = solve_targets(&initial, &targets, &config).expect("targets are valid");
    let second = solve_targets(&initial, &targets, &config).expect("targets are valid");
    assert_eq!(
        first, second,
        "ordinary batch solving must remain deterministic"
    );
    for result in &first.results {
        let TargetSolveOutcome::Solved(solution) = &result.outcome else {
            panic!("the bounded solver missed the positive switchback route: {result:?}");
        };
        assert!(solution.replay.frames.len() <= config.max_ticks_per_path);
        let verification = solution
            .replay
            .verify(&initial)
            .expect("every retained witness must replay exactly");
        match &result.target {
            SearchTarget::Door(id) => {
                assert_eq!(verification.reached_exit.as_deref(), Some(id.as_str()));
            }
            SearchTarget::Pickup(id) => {
                assert!(verification.collected_pickup_ids.contains(id));
            }
            SearchTarget::AnyExit | SearchTarget::Exit(_) => {
                panic!("this regression requests only one door and one pickup")
            }
        }
    }

    let audit = audit_direct_controller_probes(&initial, &targets, &config)
        .expect("the finite direct-controller audit runs");
    assert_eq!(audit.status, DirectProbeAuditStatus::Complete);
    for target_index in 0..targets.len() {
        let matching = audit
            .witnesses
            .iter()
            .filter(|witness| witness.target_index == target_index)
            .collect::<Vec<_>>();
        assert!(
            !matching.is_empty(),
            "the audit must retain a positive for target {target_index}"
        );
        assert!(matching.iter().any(|witness| {
            witness.probes.iter().any(|provenance| {
                matches!(
                    provenance.policy,
                    DirectProbePolicy::DetourHomingClimb { .. }
                )
            })
        }));
    }
}
