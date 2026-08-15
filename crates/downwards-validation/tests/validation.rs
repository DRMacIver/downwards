use downwards_ai::{ComplexityBand, InconclusiveReason};
use downwards_core::{
    AbilitySet, BoundarySide, Door, Exit, Pickup, Point, Rect, Room, Simulation, Tile,
};
use downwards_gen::{
    AbilityTier, GENERATION_VERSION, GeneratedLevel, GeneratedMetadata, GenerationStats,
    LayoutFamily, generate,
};
use downwards_validation::{
    BoundedTargetEvidence, DoorReachabilityObjective, DoorTargetBatchValidationError,
    DoorValidationError, PickupFromDoorValidationError, PickupReachabilityObjective,
    PickupValidationError, ScenarioObjective, ValidationConfig, ValidationError,
    evaluate_generated_door_targets_for_loadout, fingerprint_door_witness,
    fingerprint_pickup_from_door_witness, fingerprint_pickup_witness, fingerprint_witness,
    validate_all_generated_door_pairs, validate_all_generated_door_pairs_with_config,
    validate_all_generated_door_targets_with_config, validate_all_generated_pickups,
    validate_all_generated_pickups_from_every_door,
    validate_all_generated_pickups_from_every_door_with_config,
    validate_generated_door_reachability, validate_generated_door_reachability_with_config,
    validate_generated_pickup_from_door_with_config, validate_generated_pickup_reachability,
    validate_generated_scenario, validate_generated_scenario_with_config,
};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 18;
const TILE_SIZE: i32 = 10;

#[test]
fn baseline_seed_one_optional_cache_has_a_verified_pickup_witness() {
    let level = generate(1).unwrap();
    assert_eq!(level.metadata.ability_tier, AbilityTier::Baseline);
    assert_eq!(level.metadata.layout_family, LayoutFamily::TerracedAscent);
    let objective = PickupReachabilityObjective::for_generated(&level, "optional-cache");

    let certificate = validate_generated_pickup_reachability(level.clone(), objective).unwrap();

    assert!(!certificate.solution().replay.frames.is_empty());
    assert!(certificate.solution().replay.frames.len() <= 600);
    let verification = certificate
        .solution()
        .replay
        .verify(&downwards_core::Simulation::new(level.room))
        .unwrap();
    assert_eq!(verification.collected_pickup_ids, ["optional-cache"]);
    assert_eq!(verification.reached_exit, None);
}

#[test]
fn pickup_witness_fingerprint_covers_event_digests() {
    let level = flat_pickup_level(
        148,
        vec![Pickup::new("coin", Rect::new(20, 148, 6, 6)).unwrap()],
    );
    let objective = PickupReachabilityObjective::for_generated(&level, "coin");
    let certificate =
        validate_generated_pickup_reachability(level, objective).expect("spawn pickup solves");
    let original = certificate.witness_fingerprint();
    let mut tampered = certificate.solution().clone();
    tampered.replay.frames[0].expected_event_digest.0 ^= 1;

    assert_ne!(
        fingerprint_pickup_witness(
            certificate.generated_level(),
            certificate.objective(),
            &tampered,
        ),
        original
    );
}

#[test]
fn all_pickups_are_certified_independently_in_declaration_order() {
    let level = flat_pickup_level(
        149,
        vec![
            Pickup::new("spawn-coin", Rect::new(20, 148, 6, 6)).unwrap(),
            Pickup::new("east-coin", Rect::new(90, 148, 6, 6)).unwrap(),
        ],
    );

    let certificates = validate_all_generated_pickups(&level).unwrap();

    assert_eq!(certificates.len(), 2);
    assert_eq!(
        certificates
            .iter()
            .map(|certificate| certificate.objective().required_pickup_id.as_str())
            .collect::<Vec<_>>(),
        ["spawn-coin", "east-coin"]
    );
}

#[test]
fn undefined_pickup_is_rejected_before_bounded_search() {
    let level = flat_level(150, vec![right_exit("right")], false);
    let objective = PickupReachabilityObjective::for_generated(&level, "missing");

    assert_eq!(
        validate_generated_pickup_reachability(level, objective).unwrap_err(),
        PickupValidationError::RequiredPickupNotDefined {
            required_pickup_id: "missing".to_owned(),
        }
    );
}

#[test]
fn certifies_a_verified_owned_witness() {
    let level = flat_level(41, vec![right_exit("right")], false);
    let objective = ScenarioObjective::for_generated(&level, "right");

    let certificate = validate_generated_scenario(level.clone(), objective.clone()).unwrap();

    assert_eq!(certificate.generated_level(), &level);
    assert_eq!(certificate.objective(), &objective);
    assert_eq!(certificate.solution().exit_id, "right");
    assert_eq!(certificate.difficulty().exit_id, "right");
    assert_eq!(
        certificate
            .solution()
            .replay
            .verify(&downwards_core::Simulation::with_abilities(
                level.room,
                AbilitySet::NONE,
            ))
            .unwrap()
            .reached_exit
            .as_deref(),
        Some("right")
    );
}

#[test]
fn identical_inputs_produce_identical_certificates_and_fingerprints() {
    let level = flat_level(42, vec![right_exit("right")], false);
    let objective = ScenarioObjective::for_generated(&level, "right");

    let first = validate_generated_scenario(level.clone(), objective.clone()).unwrap();
    let second = validate_generated_scenario(level, objective).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.witness_fingerprint(), second.witness_fingerprint());
    assert_eq!(
        first.witness_fingerprint().to_string(),
        format!(
            "downwards-witness-v2-{:016x}",
            first.witness_fingerprint().as_u64()
        )
    );
}

#[test]
fn witness_fingerprint_covers_each_frame_event_digest() {
    let level = flat_level(142, vec![right_exit("right")], false);
    let objective = ScenarioObjective::for_generated(&level, "right");
    let certificate = validate_generated_scenario(level, objective).unwrap();
    let original = certificate.witness_fingerprint();
    let mut tampered = certificate.solution().clone();
    let first_frame = tampered
        .replay
        .frames
        .first_mut()
        .expect("a traversed room has replay frames");
    first_frame.expected_event_digest.0 ^= 1;

    assert_eq!(
        first_frame.expected_digest,
        certificate.solution().replay.frames[0].expected_digest
    );
    assert_ne!(
        fingerprint_witness(
            certificate.generated_level(),
            certificate.objective(),
            &tampered,
        ),
        original
    );
}

#[test]
fn sealed_required_exit_remains_typed_inconclusive() {
    let level = flat_level(43, vec![right_exit("right")], true);
    let objective = ScenarioObjective::for_generated(&level, "right");
    let mut config = ValidationConfig::for_loadout(AbilitySet::NONE);
    config.solver.max_expanded_nodes = 2_000;
    config.solver.max_simulated_ticks = 50_000;

    let error = validate_generated_scenario_with_config(level, objective, &config).unwrap_err();

    assert!(matches!(
        error,
        ValidationError::Inconclusive {
            reason: InconclusiveReason::ExpandedNodeBudget
                | InconclusiveReason::SimulatedTickBudget
                | InconclusiveReason::PathHorizon
                | InconclusiveReason::FrontierExhausted,
            ..
        }
    ));
}

#[test]
fn requested_provisional_band_is_enforced() {
    let level = flat_level(44, vec![right_exit("right")], false);
    let objective = ScenarioObjective::for_generated(&level, "right")
        .requesting_band(ComplexityBand::Technical);

    let error = validate_generated_scenario(level, objective).unwrap_err();

    assert_eq!(
        error,
        ValidationError::ComplexityBandMismatch {
            requested: ComplexityBand::Technical,
            observed: ComplexityBand::Gentle,
        }
    );
}

#[test]
fn sole_non_required_exit_is_a_typed_wrong_exit() {
    let level = flat_level(45, vec![right_exit("right")], false);
    let objective = ScenarioObjective::for_generated(&level, "left");

    let error = validate_generated_scenario(level, objective).unwrap_err();

    assert_eq!(
        error,
        ValidationError::WrongExit {
            required_exit_id: "left".to_owned(),
            actual_exit_id: "right".to_owned(),
        }
    );
}

#[test]
fn multiple_exits_are_rejected_as_an_unsupported_targeted_search() {
    let level = flat_level(
        46,
        vec![
            right_exit("right"),
            Exit {
                id: "upper".to_owned(),
                bounds: Rect::new(150, 10, 10, 20),
                destination: None,
                destination_entrance: None,
            },
        ],
        false,
    );
    let objective = ScenarioObjective::for_generated(&level, "right");

    let error = validate_generated_scenario(level, objective).unwrap_err();

    assert_eq!(
        error,
        ValidationError::UnsupportedAmbiguousObjective {
            required_exit_id: "right".to_owned(),
            available_exit_ids: vec!["right".to_owned(), "upper".to_owned()],
        }
    );
}

#[test]
fn stale_metadata_and_loadout_are_rejected_before_search() {
    let level = flat_level(47, vec![right_exit("right")], false);
    let mut stale_objective = ScenarioObjective::for_generated(&level, "right");
    stale_objective.expected_metadata.seed += 1;
    assert!(matches!(
        validate_generated_scenario(level.clone(), stale_objective),
        Err(ValidationError::MetadataMismatch { .. })
    ));

    let mut wrong_loadout = ScenarioObjective::for_generated(&level, "right");
    wrong_loadout.loadout = AbilitySet::ALL;
    assert_eq!(
        validate_generated_scenario(level, wrong_loadout).unwrap_err(),
        ValidationError::ObjectiveLoadoutMismatch {
            objective_loadout: AbilitySet::ALL,
            metadata_loadout: AbilitySet::NONE,
        }
    );
}

#[test]
fn all_ordered_door_pairs_are_certified_in_canonical_order() {
    let level = flat_door_level(201, "west", "east");

    let certificates = validate_all_generated_door_pairs(&level).unwrap();

    assert_eq!(certificates.len(), 2);
    assert_eq!(
        certificates
            .iter()
            .map(|certificate| (
                certificate.objective().source_door_id.as_str(),
                certificate.objective().target_door_id.as_str(),
            ))
            .collect::<Vec<_>>(),
        [("east", "west"), ("west", "east")]
    );
    for certificate in &certificates {
        assert_eq!(
            certificate.solution().reached,
            downwards_ai::ReachedTarget::Door(certificate.objective().target_door_id.clone())
        );
        assert_eq!(
            certificate.difficulty().exit_id,
            certificate.objective().target_door_id
        );
        let initial = Simulation::enter_via_door(
            level.room.clone(),
            AbilitySet::NONE,
            &certificate.objective().source_door_id,
        )
        .unwrap();
        assert_eq!(
            certificate
                .solution()
                .replay
                .verify(&initial)
                .unwrap()
                .reached_exit,
            Some(certificate.objective().target_door_id.clone())
        );
    }
    assert_ne!(
        certificates[0].witness_fingerprint(),
        certificates[1].witness_fingerprint()
    );
}

#[test]
fn door_certificate_fingerprint_covers_event_stream_and_direction() {
    let level = flat_door_level(202, "west", "east");
    let objective = DoorReachabilityObjective::for_generated(&level, "west", "east");
    let certificate = validate_generated_door_reachability(level, objective).unwrap();
    let mut tampered = certificate.solution().clone();
    tampered.replay.frames[0].expected_event_digest.0 ^= 1;

    assert_ne!(
        fingerprint_door_witness(
            certificate.generated_level(),
            certificate.objective(),
            &tampered,
        ),
        certificate.witness_fingerprint()
    );
}

#[test]
fn door_validation_requires_a_canonical_multi_door_topology() {
    let one_door = flat_door_level(203, "west", "east");
    let room = one_door
        .room
        .clone()
        .with_doors(vec![door(
            "west",
            BoundarySide::Left,
            0,
            Point::new(20, 158),
        )])
        .unwrap();
    let one_door = GeneratedLevel {
        room,
        metadata: one_door.metadata,
    };
    assert_eq!(
        validate_all_generated_door_pairs(&one_door).unwrap_err(),
        DoorValidationError::FewerThanTwoDoors { actual: 1 }
    );

    let noncanonical = flat_door_level(204, " west", "east");
    assert_eq!(
        validate_all_generated_door_pairs(&noncanonical).unwrap_err(),
        DoorValidationError::NonCanonicalDoorId {
            door_id: " west".to_owned(),
        }
    );
}

#[test]
fn every_pickup_is_certified_from_every_door() {
    let mut level = flat_door_level(205, "west", "east");
    level.room = level
        .room
        .with_objects(
            vec![],
            vec![Pickup::new("middle", Rect::new(150, 158, 6, 6)).unwrap()],
        )
        .unwrap();

    let certificates = validate_all_generated_pickups_from_every_door(&level).unwrap();

    assert_eq!(certificates.len(), 2);
    assert_eq!(
        certificates
            .iter()
            .map(|certificate| (
                certificate.objective().source_door_id.as_str(),
                certificate.objective().required_pickup_id.as_str(),
            ))
            .collect::<Vec<_>>(),
        [("east", "middle"), ("west", "middle")]
    );
    for certificate in &certificates {
        let initial = Simulation::enter_via_door(
            level.room.clone(),
            certificate.objective().loadout,
            &certificate.objective().source_door_id,
        )
        .unwrap();
        assert_eq!(
            certificate
                .solution()
                .replay
                .verify(&initial)
                .unwrap()
                .collected_pickup_ids,
            ["middle"]
        );
    }
    assert_ne!(
        certificates[0].witness_fingerprint(),
        certificates[1].witness_fingerprint()
    );
}

#[test]
fn shared_door_search_is_deterministic_and_semantically_matches_individual_certification() {
    let level = flat_multi_door_level(206, vec![]);
    let config = ValidationConfig::default();

    let first = validate_all_generated_door_pairs_with_config(&level, &config).unwrap();
    let second = validate_all_generated_door_pairs_with_config(&level, &config).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 12);

    for shared in first {
        let individual = validate_generated_door_reachability_with_config(
            level.clone(),
            shared.objective().clone(),
            &config,
        )
        .unwrap();
        assert_eq!(shared.solution().reached, individual.solution().reached);
        assert_eq!(shared.difficulty().exit_id, individual.difficulty().exit_id);
        let initial = Simulation::enter_via_door(
            level.room.clone(),
            shared.objective().loadout,
            &shared.objective().source_door_id,
        )
        .unwrap();
        assert_eq!(
            shared
                .solution()
                .replay
                .verify(&initial)
                .unwrap()
                .reached_exit,
            individual
                .solution()
                .replay
                .verify(&initial)
                .unwrap()
                .reached_exit
        );
        assert_eq!(
            fingerprint_door_witness(
                shared.generated_level(),
                shared.objective(),
                shared.solution(),
            ),
            shared.witness_fingerprint()
        );
    }
}

#[test]
fn shared_pickup_search_is_deterministic_and_semantically_matches_individual_certification() {
    let level = flat_multi_door_level(
        207,
        vec![
            Pickup::new("left-coin", Rect::new(90, 158, 6, 6)).unwrap(),
            Pickup::new("right-coin", Rect::new(220, 158, 6, 6)).unwrap(),
        ],
    );
    let config = ValidationConfig::default();

    let first =
        validate_all_generated_pickups_from_every_door_with_config(&level, &config).unwrap();
    let second =
        validate_all_generated_pickups_from_every_door_with_config(&level, &config).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 8);

    for shared in first {
        let individual = validate_generated_pickup_from_door_with_config(
            level.clone(),
            shared.objective().clone(),
            &config,
        )
        .unwrap();
        assert_eq!(shared.solution().reached, individual.solution().reached);
        let initial = Simulation::enter_via_door(
            level.room.clone(),
            shared.objective().loadout,
            &shared.objective().source_door_id,
        )
        .unwrap();
        let shared_verification = shared.solution().replay.verify(&initial).unwrap();
        let individual_verification = individual.solution().replay.verify(&initial).unwrap();
        assert!(
            shared_verification
                .collected_pickup_ids
                .contains(&shared.objective().required_pickup_id)
        );
        assert!(
            individual_verification
                .collected_pickup_ids
                .contains(&shared.objective().required_pickup_id)
        );
        assert_eq!(
            fingerprint_pickup_from_door_witness(
                shared.generated_level(),
                shared.objective(),
                shared.solution(),
            ),
            shared.witness_fingerprint()
        );
    }
}

#[test]
fn combined_door_target_batch_matches_specialized_batches_exactly() {
    let level = flat_multi_door_level(
        208,
        vec![
            Pickup::new("left-coin", Rect::new(90, 158, 6, 6)).unwrap(),
            Pickup::new("right-coin", Rect::new(220, 158, 6, 6)).unwrap(),
        ],
    );
    let config = ValidationConfig::default();

    let first = validate_all_generated_door_targets_with_config(&level, &config).unwrap();
    let second = validate_all_generated_door_targets_with_config(&level, &config).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.door_pairs(),
        validate_all_generated_door_pairs_with_config(&level, &config).unwrap()
    );
    assert_eq!(
        first.pickups_from_doors(),
        validate_all_generated_pickups_from_every_door_with_config(&level, &config).unwrap()
    );
    assert_eq!(
        first
            .source_search_effort()
            .iter()
            .map(|effort| effort.source_door_id.as_str())
            .collect::<Vec<_>>(),
        ["east-high", "east-low", "west-high", "west-low"]
    );
}

#[test]
fn shared_batches_preserve_canonical_first_error_semantics() {
    let level = flat_multi_door_level(
        209,
        vec![Pickup::new("middle", Rect::new(150, 158, 6, 6)).unwrap()],
    );
    let mut config = ValidationConfig::default();
    config.solver.max_expanded_nodes = 0;

    assert_eq!(
        validate_all_generated_door_targets_with_config(&level, &config).unwrap_err(),
        DoorTargetBatchValidationError::Door {
            source_door_id: "east-high".to_owned(),
            target_door_id: "east-low".to_owned(),
            error: Box::new(DoorValidationError::Inconclusive {
                reason: InconclusiveReason::ExpandedNodeBudget,
                search_effort: Default::default(),
            }),
        }
    );
    assert_eq!(
        validate_all_generated_door_pairs_with_config(&level, &config).unwrap_err(),
        DoorValidationError::Inconclusive {
            reason: InconclusiveReason::ExpandedNodeBudget,
            search_effort: Default::default(),
        }
    );
    assert_eq!(
        validate_all_generated_pickups_from_every_door_with_config(&level, &config).unwrap_err(),
        PickupFromDoorValidationError::Inconclusive {
            reason: InconclusiveReason::ExpandedNodeBudget,
            search_effort: Default::default(),
        }
    );
}

#[test]
fn shared_multi_door_search_uses_fewer_simulated_ticks_than_individual_searches() {
    let mut level = flat_door_level(210, "west", "east");
    level.room = level
        .room
        .with_objects(
            vec![],
            vec![
                Pickup::new("west-coin", Rect::new(80, 158, 6, 6)).unwrap(),
                Pickup::new("middle-coin", Rect::new(150, 158, 6, 6)).unwrap(),
                Pickup::new("east-coin", Rect::new(230, 158, 6, 6)).unwrap(),
            ],
        )
        .unwrap();
    let config = ValidationConfig::default();
    let shared = validate_all_generated_door_targets_with_config(&level, &config).unwrap();
    let shared_ticks: usize = shared
        .source_search_effort()
        .iter()
        .map(|effort| effort.stats.simulated_ticks)
        .sum();
    let individual_door_ticks: usize = shared
        .door_pairs()
        .iter()
        .map(|shared_certificate| {
            validate_generated_door_reachability_with_config(
                level.clone(),
                shared_certificate.objective().clone(),
                &config,
            )
            .unwrap()
            .solution()
            .stats
            .simulated_ticks
        })
        .sum();
    let individual_pickup_ticks: usize = shared
        .pickups_from_doors()
        .iter()
        .map(|shared_certificate| {
            validate_generated_pickup_from_door_with_config(
                level.clone(),
                shared_certificate.objective().clone(),
                &config,
            )
            .unwrap()
            .solution()
            .stats
            .simulated_ticks
        })
        .sum();
    let individual_ticks = individual_door_ticks + individual_pickup_ticks;

    eprintln!(
        "shared multi-door search: {shared_ticks} simulated ticks versus {individual_ticks} individually"
    );
    assert!(
        shared_ticks < individual_ticks,
        "shared search used {shared_ticks} ticks versus {individual_ticks} individually"
    );
}

#[test]
fn route_evidence_matches_the_strict_success_batch_without_difficulty_work() {
    let level = flat_multi_door_level(
        211,
        vec![
            Pickup::new("alpha", Rect::new(90, 158, 6, 6)).unwrap(),
            Pickup::new("beta", Rect::new(220, 158, 6, 6)).unwrap(),
        ],
    );
    let loadout = level.metadata.intended_abilities;
    let config = ValidationConfig::for_loadout(loadout);

    let strict = validate_all_generated_door_targets_with_config(&level, &config).unwrap();
    let evidence = evaluate_generated_door_targets_for_loadout(&level, loadout, &config).unwrap();

    assert_eq!(evidence.loadout(), loadout);
    assert_eq!(evidence.door_routes().len(), strict.door_pairs().len());
    assert_eq!(
        evidence.pickup_routes().len(),
        strict.pickups_from_doors().len()
    );
    assert_eq!(
        evidence.source_search_effort(),
        strict.source_search_effort()
    );

    for (observed, expected) in evidence.door_routes().iter().zip(strict.door_pairs()) {
        assert_eq!(observed.source_door_id, expected.objective().source_door_id);
        assert_eq!(observed.target_door_id, expected.objective().target_door_id);
        let positive = observed
            .evidence
            .positive()
            .expect("strict route is positive");
        assert_eq!(positive.solution(), expected.solution());
        assert_eq!(
            positive.witness_fingerprint(),
            expected.witness_fingerprint()
        );
    }
    for (observed, expected) in evidence
        .pickup_routes()
        .iter()
        .zip(strict.pickups_from_doors())
    {
        assert_eq!(observed.source_door_id, expected.objective().source_door_id);
        assert_eq!(
            observed.required_pickup_id,
            expected.objective().required_pickup_id
        );
        let positive = observed
            .evidence
            .positive()
            .expect("strict pickup is positive");
        assert_eq!(positive.solution(), expected.solution());
        assert_eq!(
            positive.witness_fingerprint(),
            expected.witness_fingerprint()
        );
    }

    let expected_aggregate = evidence.source_search_effort().iter().fold(
        downwards_ai::SearchStats::default(),
        |mut aggregate, source| {
            aggregate.expanded_nodes += source.stats.expanded_nodes;
            aggregate.generated_nodes += source.stats.generated_nodes;
            aggregate.simulated_ticks += source.stats.simulated_ticks;
            aggregate.deepest_path_ticks = aggregate
                .deepest_path_ticks
                .max(source.stats.deepest_path_ticks);
            aggregate
        },
    );
    assert_eq!(evidence.aggregate_search_effort(), expected_aggregate);
}

#[test]
fn route_evidence_is_canonical_deterministic_and_supports_all_four_loadouts() {
    let mut level = flat_door_level(212, "west", "east");
    level.room = level
        .room
        .with_objects(
            vec![],
            vec![
                Pickup::new("zeta", Rect::new(220, 158, 6, 6)).unwrap(),
                Pickup::new("alpha", Rect::new(90, 158, 6, 6)).unwrap(),
            ],
        )
        .unwrap();
    let loadouts = [
        AbilitySet::NONE,
        AbilitySet::new(true, false),
        AbilitySet::new(false, true),
        AbilitySet::ALL,
    ];

    for loadout in loadouts {
        let config = ValidationConfig::for_loadout(loadout);
        let batch = evaluate_generated_door_targets_for_loadout(&level, loadout, &config).unwrap();
        assert_eq!(
            batch,
            evaluate_generated_door_targets_for_loadout(&level, loadout, &config).unwrap()
        );
        assert_eq!(batch.loadout(), loadout);
        assert_eq!(
            batch
                .door_routes()
                .iter()
                .map(|route| (route.source_door_id.as_str(), route.target_door_id.as_str()))
                .collect::<Vec<_>>(),
            [("east", "west"), ("west", "east")]
        );
        assert_eq!(
            batch
                .pickup_routes()
                .iter()
                .map(|route| (
                    route.source_door_id.as_str(),
                    route.required_pickup_id.as_str(),
                ))
                .collect::<Vec<_>>(),
            [
                ("east", "alpha"),
                ("east", "zeta"),
                ("west", "alpha"),
                ("west", "zeta"),
            ]
        );

        for route in batch.door_routes() {
            let positive = route
                .evidence
                .positive()
                .expect("flat door route should solve");
            let initial =
                Simulation::enter_via_door(level.room.clone(), loadout, &route.source_door_id)
                    .unwrap();
            assert_eq!(
                positive
                    .solution()
                    .replay
                    .verify(&initial)
                    .unwrap()
                    .reached_exit,
                Some(route.target_door_id.clone())
            );
        }
        for route in batch.pickup_routes() {
            let positive = route
                .evidence
                .positive()
                .expect("flat pickup route should solve");
            let initial =
                Simulation::enter_via_door(level.room.clone(), loadout, &route.source_door_id)
                    .unwrap();
            assert!(
                positive
                    .solution()
                    .replay
                    .verify(&initial)
                    .unwrap()
                    .collected_pickup_ids
                    .contains(&route.required_pickup_id)
            );
        }
    }
}

#[test]
fn route_evidence_retains_mixed_positive_and_bounded_inconclusive_results() {
    let mut level = flat_door_level(213, "west", "east");
    level.room = level
        .room
        .with_objects(
            vec![],
            vec![Pickup::new("near-west", Rect::new(29, 158, 6, 6)).unwrap()],
        )
        .unwrap();
    let mut config = ValidationConfig::default();
    config.solver.max_expanded_nodes = 1;
    config.solver.max_simulated_ticks = 40;
    config.solver.max_ticks_per_path = 40;

    let batch =
        evaluate_generated_door_targets_for_loadout(&level, AbilitySet::NONE, &config).unwrap();
    let outcomes = batch
        .door_routes()
        .iter()
        .map(|route| &route.evidence)
        .chain(batch.pickup_routes().iter().map(|route| &route.evidence))
        .collect::<Vec<_>>();

    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, BoundedTargetEvidence::Positive(_)))
    );
    assert!(outcomes.iter().any(|outcome| matches!(
        outcome,
        BoundedTargetEvidence::Inconclusive(inconclusive)
            if inconclusive.reason == InconclusiveReason::ExpandedNodeBudget
    )));
    assert_eq!(outcomes.len(), 4);
}

fn flat_level(seed: u64, exits: Vec<Exit>, seal_route: bool) -> GeneratedLevel {
    let mut tiles = vec![Tile::Empty; usize::from(WIDTH) * usize::from(HEIGHT)];
    for x in 0..WIDTH {
        set_tile(&mut tiles, x, 0, Tile::Solid);
        set_tile(&mut tiles, x, HEIGHT - 1, Tile::Solid);
    }
    for y in 1..HEIGHT - 1 {
        set_tile(&mut tiles, 0, y, Tile::Solid);
        set_tile(&mut tiles, WIDTH - 1, y, Tile::Solid);
        if seal_route {
            set_tile(&mut tiles, WIDTH / 2, y, Tile::Solid);
        }
    }

    let room = Room::new(
        format!("validation-fixture-{seed}"),
        "Validation fixture",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(20, 158),
        exits,
    )
    .unwrap();
    GeneratedLevel {
        room,
        metadata: GeneratedMetadata {
            generation_version: GENERATION_VERSION,
            seed,
            layout_family: LayoutFamily::HazardRun,
            ability_tier: AbilityTier::Baseline,
            intended_abilities: AbilitySet::NONE,
            stats: GenerationStats::default(),
        },
    }
}

fn flat_pickup_level(seed: u64, pickups: Vec<Pickup>) -> GeneratedLevel {
    let mut level = flat_level(seed, vec![right_exit("right")], false);
    level.room = level.room.with_objects(vec![], pickups).unwrap();
    level
}

fn flat_door_level(seed: u64, west_id: &str, east_id: &str) -> GeneratedLevel {
    let mut level = flat_level(seed, vec![], false);
    let mut tiles = level.room.tiles().to_vec();
    for y in 14..=16 {
        set_tile(&mut tiles, 0, y, Tile::Empty);
        set_tile(&mut tiles, WIDTH - 1, y, Tile::Empty);
    }
    let room = Room::new(
        format!("door-validation-fixture-{seed}"),
        "Door validation fixture",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(150, 158),
        vec![],
    )
    .unwrap()
    .with_doors(vec![
        door(west_id, BoundarySide::Left, 0, Point::new(20, 158)),
        door(east_id, BoundarySide::Right, 316, Point::new(292, 158)),
    ])
    .unwrap();
    level.room = room;
    level
}

fn flat_multi_door_level(seed: u64, pickups: Vec<Pickup>) -> GeneratedLevel {
    let mut level = flat_level(seed, vec![], false);
    let mut tiles = level.room.tiles().to_vec();
    for y in 10..=16 {
        set_tile(&mut tiles, 0, y, Tile::Empty);
        set_tile(&mut tiles, WIDTH - 1, y, Tile::Empty);
    }
    let room = Room::new(
        format!("multi-door-validation-fixture-{seed}"),
        "Multi-door validation fixture",
        WIDTH,
        HEIGHT,
        TILE_SIZE,
        tiles,
        Point::new(150, 158),
        vec![],
    )
    .unwrap()
    .with_objects(vec![], pickups)
    .unwrap()
    .with_doors(vec![
        Door {
            id: "west-low".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 140, 4, 30),
            arrival: Point::new(20, 158),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east-high".to_owned(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 105, 4, 25),
            arrival: Point::new(292, 118),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "west-high".to_owned(),
            side: BoundarySide::Left,
            trigger_bounds: Rect::new(0, 105, 4, 25),
            arrival: Point::new(20, 118),
            destination_room: None,
            destination_door: None,
        },
        Door {
            id: "east-low".to_owned(),
            side: BoundarySide::Right,
            trigger_bounds: Rect::new(316, 140, 4, 30),
            arrival: Point::new(292, 158),
            destination_room: None,
            destination_door: None,
        },
    ])
    .unwrap();
    level.room = room;
    level
}

fn door(id: &str, side: BoundarySide, x: i32, arrival: Point) -> Door {
    Door {
        id: id.to_owned(),
        side,
        trigger_bounds: Rect::new(x, 140, 4, 30),
        arrival,
        destination_room: None,
        destination_door: None,
    }
}

fn set_tile(tiles: &mut [Tile], x: u16, y: u16, tile: Tile) {
    tiles[usize::from(y) * usize::from(WIDTH) + usize::from(x)] = tile;
}

fn right_exit(id: &str) -> Exit {
    Exit {
        id: id.to_owned(),
        bounds: Rect::new(300, 140, 10, 30),
        destination: None,
        destination_entrance: None,
    }
}
