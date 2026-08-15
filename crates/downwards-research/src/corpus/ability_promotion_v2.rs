//! Variant-specific promotion evidence for constructed ability gates.
//!
//! A structural ability edge is not a gameplay claim by itself. Promotion
//! requires an accepted ability event on the advertised forward route, a
//! baseline-positive reverse route, no positive lower-loadout matrix bypass,
//! and a complete finite direct-controller audit with no lower-loadout
//! positive. Complete-no-positive remains finite-vocabulary evidence, never
//! an unreachability proof.

use std::{error::Error, fmt};

use downwards_core::{AbilitySet, Simulation};
use downwards_gen::experimental::{
    AbilityGateEmbeddingPendingReason, AbilityGateEmbeddingState,
    COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
    COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
    COMPOSITIONAL_ABILITY_GENERATION_VERSION, GateAbility,
};
use downwards_lab::{SemanticEvent, TraversalGrid, observe_successful_replay};
use downwards_validation::BoundedTargetEvidence;
use serde::{Deserialize, Serialize};

use super::{
    CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY, CorpusCandidate, CorpusCandidateKeyRecord,
    CorpusRoomAnalysisConfigRecord, EvaluationLoadout, LoadoutControllerAuditStatus,
    LoadoutRouteMatrix, RouteControllerAssessment, RouteControllerAssessmentError,
    assess_easiest_known_route,
};

/// Version of ability-promotion evidence and its canonical eligibility rule.
pub const CORPUS_ABILITY_PROMOTION_GATE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbilityPromotionClaimV2 {
    WallJump,
    Dash,
}

impl AbilityPromotionClaimV2 {
    #[must_use]
    pub const fn is_available(self, abilities: AbilitySet) -> bool {
        match self {
            Self::WallJump => abilities.wall_jump,
            Self::Dash => abilities.dash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbilityPromotionMatrixStateV2 {
    ReplayCertifiedPositive,
    BoundedInconclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbilityPromotionIntendedStateV2 {
    ReplayCertifiedRequiredEventAccepted,
    ReplayCertifiedRequiredEventMissing,
    BoundedInconclusive,
}

/// Canonical provenance for the intended-route state. The direct-witness
/// index refers to `direct_route_assessment.easiest_first_witnesses`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum AbilityPromotionIntendedEvidenceSourceV2 {
    DirectController { witness_index: usize },
    CanonicalMatrix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbilityPromotionBudgetLimitV2 {
    ExpandedNodes,
    SimulatedTicks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AbilityPromotionDirectAuditStateV2 {
    CompleteFiniteVocabularyNoPositive,
    PositiveBypass {
        raw_positive_witnesses: usize,
        retained_semantic_witnesses: usize,
    },
    BoundedInconclusive {
        limit: AbilityPromotionBudgetLimitV2,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityPromotionMissingLoadoutEvidenceV2 {
    pub loadout: EvaluationLoadout,
    pub matrix: AbilityPromotionMatrixStateV2,
    pub direct_audit: AbilityPromotionDirectAuditStateV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AbilityPromotionDecisionV2 {
    NotApplicable,
    PromotedStructuralNoKnownBypass,
    RefusedAliasRoomMismatch,
    BoundedIntendedRoute,
    IntendedRouteMissingRequiredEvent,
    BoundedBaselineReverse,
    PositiveMatrixBypass {
        loadout: EvaluationLoadout,
    },
    PositiveDirectBypass {
        loadout: EvaluationLoadout,
    },
    BoundedDirectAudit {
        loadout: EvaluationLoadout,
        limit: AbilityPromotionBudgetLimitV2,
    },
}

impl AbilityPromotionDecisionV2 {
    /// Ordinary sources are eligible without making an ability claim;
    /// ability sources are eligible only after full promotion.
    #[must_use]
    pub const fn allows_canonical_selection(self) -> bool {
        matches!(
            self,
            Self::NotApplicable | Self::PromotedStructuralNoKnownBypass
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariantAbilityPromotionEvidenceV2 {
    NotApplicable,
    Ability {
        claim: AbilityPromotionClaimV2,
        source_door_id: String,
        sink_door_id: String,
        edge_rewrite_version: u32,
        gate_embedding_contract_version: u32,
        generation_version: u32,
        alias_room_compatible: bool,
        intended_route: Option<AbilityPromotionIntendedStateV2>,
        intended_evidence_source: Option<AbilityPromotionIntendedEvidenceSourceV2>,
        baseline_reverse: Option<AbilityPromotionMatrixStateV2>,
        missing_ability_loadouts: Vec<AbilityPromotionMissingLoadoutEvidenceV2>,
        /// Exact advertised-pair finite audit. `None` is permitted only for
        /// an alias whose exact Room is incompatible with shared matrix
        /// replay; such an alias is refused rather than inheriting evidence.
        direct_route_assessment: Option<Box<RouteControllerAssessment>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantAbilityPromotionGateV2 {
    pub gate_version: u32,
    pub key: CorpusCandidateKeyRecord,
    pub evidence: VariantAbilityPromotionEvidenceV2,
    pub decision: AbilityPromotionDecisionV2,
}

/// Derive a gate, running exactly one finite advertised-pair direct audit for
/// a CompositionalAbility alias. Ordinary candidates return NotApplicable.
pub fn evaluate_variant_ability_promotion_gate_v2(
    candidate: &CorpusCandidate,
    physical_evidence_source: &CorpusCandidate,
    matrices: &[LoadoutRouteMatrix],
    audit_config: &CorpusRoomAnalysisConfigRecord,
) -> Result<VariantAbilityPromotionGateV2, AbilityPromotionGateV2Error> {
    audit_config
        .validate()
        .map_err(|error| invalid(format!("invalid promotion-audit config: {error}")))?;
    let Some((claim, source_door_id, sink_door_id)) = ability_contract(candidate)? else {
        return Ok(VariantAbilityPromotionGateV2 {
            gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
            key: candidate.exact_key(),
            evidence: VariantAbilityPromotionEvidenceV2::NotApplicable,
            decision: AbilityPromotionDecisionV2::NotApplicable,
        });
    };
    if candidate.generated().room != physical_evidence_source.generated().room {
        return Ok(alias_mismatch_gate(
            candidate,
            claim,
            source_door_id,
            sink_door_id,
        ));
    }
    let assessment = assess_easiest_known_route(
        &candidate.generated().room,
        source_door_id.clone(),
        sink_door_id.clone(),
        EvaluationLoadout::Both,
        &audit_config.direct_controller_solver.to_solver_config(),
    )
    .map_err(|source| AbilityPromotionGateV2Error::DirectAudit {
        key: Box::new(candidate.exact_key()),
        source: Box::new(source),
    })?;
    derive_gate_from_retained_audit(
        candidate,
        physical_evidence_source,
        matrices,
        Some(assessment),
    )
}

/// Recompute every derived field from retained matrix rows and retained
/// direct-audit evidence. Complete-no-positive is treated as an observation
/// here; production/artifact callers should additionally use the rerun seam.
pub fn validate_variant_ability_promotion_gate_v2(
    candidate: &CorpusCandidate,
    physical_evidence_source: &CorpusCandidate,
    matrices: &[LoadoutRouteMatrix],
    gate: &VariantAbilityPromotionGateV2,
) -> Result<(), AbilityPromotionGateV2Error> {
    let retained = match &gate.evidence {
        VariantAbilityPromotionEvidenceV2::NotApplicable => None,
        VariantAbilityPromotionEvidenceV2::Ability {
            direct_route_assessment,
            ..
        } => direct_route_assessment.as_deref().cloned(),
    };
    let expected =
        derive_gate_from_retained_audit(candidate, physical_evidence_source, matrices, retained)?;
    if *gate != expected {
        return Err(invalid(format!(
            "stored promotion gate for {} differs from retained evidence",
            candidate.exact_key().stable_slug()
        )));
    }
    Ok(())
}

/// Production trust seam: rerun the tiny native advertised-pair finite audit
/// under the exact content-addressed config and require full gate equality.
pub fn rerun_validate_variant_ability_promotion_gate_v2(
    candidate: &CorpusCandidate,
    physical_evidence_source: &CorpusCandidate,
    matrices: &[LoadoutRouteMatrix],
    audit_config: &CorpusRoomAnalysisConfigRecord,
    gate: &VariantAbilityPromotionGateV2,
) -> Result<(), AbilityPromotionGateV2Error> {
    let rerun = evaluate_variant_ability_promotion_gate_v2(
        candidate,
        physical_evidence_source,
        matrices,
        audit_config,
    )?;
    if *gate != rerun {
        return Err(invalid(format!(
            "stored promotion gate for {} differs from a deterministic audit rerun",
            candidate.exact_key().stable_slug()
        )));
    }
    Ok(())
}

fn derive_gate_from_retained_audit(
    candidate: &CorpusCandidate,
    physical_evidence_source: &CorpusCandidate,
    matrices: &[LoadoutRouteMatrix],
    assessment: Option<RouteControllerAssessment>,
) -> Result<VariantAbilityPromotionGateV2, AbilityPromotionGateV2Error> {
    let Some((claim, source_door_id, sink_door_id)) = ability_contract(candidate)? else {
        if assessment.is_some() {
            return Err(invalid("ordinary candidate retained an ability audit"));
        }
        return Ok(VariantAbilityPromotionGateV2 {
            gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
            key: candidate.exact_key(),
            evidence: VariantAbilityPromotionEvidenceV2::NotApplicable,
            decision: AbilityPromotionDecisionV2::NotApplicable,
        });
    };
    if candidate.generated().room != physical_evidence_source.generated().room {
        if assessment.is_some() {
            return Err(invalid(
                "room-incompatible ability alias retained an inherited direct audit",
            ));
        }
        return Ok(alias_mismatch_gate(
            candidate,
            claim,
            source_door_id,
            sink_door_id,
        ));
    }
    let assessment = assessment.ok_or_else(|| {
        invalid("room-compatible ability alias is missing its advertised-pair direct audit")
    })?;
    validate_direct_assessment(&assessment, &source_door_id, &sink_door_id)?;

    let construction = candidate.exact_key().construction_loadout();
    let intended_row = door_row(matrices, construction, &source_door_id, &sink_door_id)?;
    let intended_audit = assessment
        .audits
        .iter()
        .find(|audit| audit.loadout == construction)
        .expect("exact direct-audit order was validated");
    let (intended_route, intended_evidence_source) = match intended_audit.status {
        // Even a retained positive cannot establish the governing easiest
        // controller if this loadout's finite audit stopped at its budget.
        LoadoutControllerAuditStatus::BoundedIncomplete { .. } => {
            (AbilityPromotionIntendedStateV2::BoundedInconclusive, None)
        }
        LoadoutControllerAuditStatus::CompleteFiniteVocabulary
            if intended_audit.raw_positive_witnesses > 0 =>
        {
            let (witness_index, easiest_intended) = assessment
                .easiest_first_witnesses
                .iter()
                .enumerate()
                .find(|(_, witness)| witness.loadout == construction)
                .ok_or_else(|| {
                    invalid(
                        "intended-loadout audit reports raw positives but retains no semantic witness",
                    )
                })?;
            let state = replay_intended_state(
                candidate,
                construction,
                &source_door_id,
                &sink_door_id,
                &easiest_intended.replay,
                claim,
                Some(&easiest_intended.semantic_trace),
            )?;
            (
                state,
                Some(AbilityPromotionIntendedEvidenceSourceV2::DirectController { witness_index }),
            )
        }
        LoadoutControllerAuditStatus::CompleteFiniteVocabulary => match &intended_row.evidence {
            BoundedTargetEvidence::Inconclusive(_) => {
                (AbilityPromotionIntendedStateV2::BoundedInconclusive, None)
            }
            BoundedTargetEvidence::Positive(positive) => (
                replay_intended_state(
                    candidate,
                    construction,
                    &source_door_id,
                    &sink_door_id,
                    &positive.solution().replay,
                    claim,
                    None,
                )?,
                Some(AbilityPromotionIntendedEvidenceSourceV2::CanonicalMatrix),
            ),
        },
    };
    let reverse_row = door_row(
        matrices,
        EvaluationLoadout::Baseline,
        &sink_door_id,
        &source_door_id,
    )?;
    let baseline_reverse = replay_matrix_route_state(
        candidate,
        EvaluationLoadout::Baseline,
        &sink_door_id,
        &source_door_id,
        &reverse_row.evidence,
    )?;

    let mut missing_ability_loadouts = Vec::new();
    for loadout in EvaluationLoadout::ALL
        .into_iter()
        .filter(|loadout| !claim.is_available(loadout.abilities()))
    {
        let row = door_row(matrices, loadout, &source_door_id, &sink_door_id)?;
        let audit = assessment
            .audits
            .iter()
            .find(|audit| audit.loadout == loadout)
            .expect("exact direct-audit order was validated");
        let direct_audit = if audit.raw_positive_witnesses > 0 {
            AbilityPromotionDirectAuditStateV2::PositiveBypass {
                raw_positive_witnesses: audit.raw_positive_witnesses,
                retained_semantic_witnesses: audit.retained_semantic_witnesses,
            }
        } else {
            match audit.status {
                LoadoutControllerAuditStatus::CompleteFiniteVocabulary => {
                    AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive
                }
                LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                    AbilityPromotionDirectAuditStateV2::BoundedInconclusive {
                        limit: limit.into(),
                    }
                }
            }
        };
        missing_ability_loadouts.push(AbilityPromotionMissingLoadoutEvidenceV2 {
            loadout,
            matrix: matrix_state(&row.evidence),
            direct_audit,
        });
    }

    let decision = promotion_decision(intended_route, baseline_reverse, &missing_ability_loadouts);
    Ok(VariantAbilityPromotionGateV2 {
        gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
        key: candidate.exact_key(),
        evidence: VariantAbilityPromotionEvidenceV2::Ability {
            claim,
            source_door_id,
            sink_door_id,
            edge_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            gate_embedding_contract_version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            alias_room_compatible: true,
            intended_route: Some(intended_route),
            intended_evidence_source,
            baseline_reverse: Some(baseline_reverse),
            missing_ability_loadouts,
            direct_route_assessment: Some(Box::new(assessment)),
        },
        decision,
    })
}

fn ability_contract(
    candidate: &CorpusCandidate,
) -> Result<Option<(AbilityPromotionClaimV2, String, String)>, AbilityPromotionGateV2Error> {
    let Some(ability) = candidate.compositional_ability_candidate() else {
        return Ok(None);
    };
    let plan = &ability.rewritten_mission.plan;
    let [gate] = plan.gates.as_slice() else {
        return Err(invalid("ability candidate does not have exactly one gate"));
    };
    let claim = match gate.required_ability {
        GateAbility::WallJump => AbilityPromotionClaimV2::WallJump,
        GateAbility::Dash => AbilityPromotionClaimV2::Dash,
    };
    if candidate.exact_key().construction_loadout().abilities()
        != match claim {
            AbilityPromotionClaimV2::WallJump => AbilitySet::new(true, false),
            AbilityPromotionClaimV2::Dash => AbilitySet::new(false, true),
        }
    {
        return Err(invalid(
            "ability candidate key/loadout disagrees with its single graph gate",
        ));
    }
    if ability.rewritten_mission.provenance.rewrite_version
        != COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION
        || ability.embedding.graph_rewrite_version != COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION
        || ability.embedding.generation_version != COMPOSITIONAL_ABILITY_GENERATION_VERSION
        || gate.embedding_contract.version != COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION
        || ability.rewritten_mission.certificates.len() != 1
        || ability.embedding.gate_realizations.len() != 1
        || ability.rewritten_mission.certificates[0].gate_ordinal != gate.ordinal
        || ability.embedding.gate_realizations[0].gate != *gate
        || ability.evidence_state
            != (AbilityGateEmbeddingState::Pending {
                reasons: vec![
                    AbilityGateEmbeddingPendingReason::AuthoritativePositiveReplayNotObserved,
                    AbilityGateEmbeddingPendingReason::ReducedLoadoutBypassAuditNotObserved,
                ],
            })
    {
        return Err(invalid(
            "ability candidate carries stale or inconsistent graph/embedding evidence",
        ));
    }
    let source_node = plan.source_node_id();
    let sink_node = plan.sink_node_id();
    let missing_ability = match claim {
        AbilityPromotionClaimV2::WallJump => AbilitySet::new(false, true),
        AbilityPromotionClaimV2::Dash => AbilitySet::new(true, false),
    };
    if !plan.graph_can_reach(source_node, sink_node, candidate.construction_abilities())
        || plan.graph_can_reach(source_node, sink_node, missing_ability)
        || !plan.graph_can_reach(sink_node, source_node, AbilitySet::NONE)
        || plan.graph_can_reach_without_gate_edge(gate.ordinal) != Some(false)
    {
        return Err(invalid(
            "ability candidate fails its current graph reachability certificate",
        ));
    }
    let source_ports = plan
        .base
        .ports
        .iter()
        .filter(|port| port.node_id == source_node)
        .collect::<Vec<_>>();
    let sink_ports = plan
        .base
        .ports
        .iter()
        .filter(|port| port.node_id == sink_node)
        .collect::<Vec<_>>();
    let ([_source_port], [_sink_port]) = (source_ports.as_slice(), sink_ports.as_slice()) else {
        return Err(invalid(
            "ability mission does not bind unique boundary ports to its source/sink nodes",
        ));
    };

    // Mission-node IDs and embedded route-node IDs are separate namespaces.
    // Bind them explicitly before consulting physical boundary ports.
    let source_route_nodes = ability
        .mission_route_nodes
        .iter()
        .filter(|mapping| mapping.mission_node_id == source_node)
        .collect::<Vec<_>>();
    let sink_route_nodes = ability
        .mission_route_nodes
        .iter()
        .filter(|mapping| mapping.mission_node_id == sink_node)
        .collect::<Vec<_>>();
    let ([source_route_node], [sink_route_node]) =
        (source_route_nodes.as_slice(), sink_route_nodes.as_slice())
    else {
        return Err(invalid(
            "ability mission source/sink do not have unique mission-to-route mappings",
        ));
    };
    if source_route_node.route_node_id == sink_route_node.route_node_id {
        return Err(invalid(
            "ability mission source/sink map to the same embedded route node",
        ));
    }
    let gate_from_route_nodes = ability
        .mission_route_nodes
        .iter()
        .filter(|mapping| mapping.mission_node_id == gate.ascent_from)
        .collect::<Vec<_>>();
    let gate_to_route_nodes = ability
        .mission_route_nodes
        .iter()
        .filter(|mapping| mapping.mission_node_id == gate.ascent_to)
        .collect::<Vec<_>>();
    let ([gate_from_route_node], [gate_to_route_node]) = (
        gate_from_route_nodes.as_slice(),
        gate_to_route_nodes.as_slice(),
    ) else {
        return Err(invalid(
            "ability gate endpoints do not have unique mission-to-route mappings",
        ));
    };
    let realization = &ability.embedding.gate_realizations[0];
    if realization.from_route_node_id != gate_from_route_node.route_node_id
        || realization.to_route_node_id != gate_to_route_node.route_node_id
    {
        return Err(invalid(
            "ability gate realization is not bound to its mapped graph endpoints",
        ));
    }
    let source_doors = ability
        .boundary_ports
        .iter()
        .filter(|port| port.node_id == source_route_node.route_node_id)
        .collect::<Vec<_>>();
    let sink_doors = ability
        .boundary_ports
        .iter()
        .filter(|port| port.node_id == sink_route_node.route_node_id)
        .collect::<Vec<_>>();
    let ([source_door], [sink_door]) = (source_doors.as_slice(), sink_doors.as_slice()) else {
        return Err(invalid(
            "ability source/sink nodes do not bind unique physical boundary doors",
        ));
    };
    if source_door.door.id == sink_door.door.id {
        return Err(invalid(
            "ability mission source/sink bind the same physical boundary door",
        ));
    }
    Ok(Some((
        claim,
        source_door.door.id.clone(),
        sink_door.door.id.clone(),
    )))
}

fn replay_intended_state(
    candidate: &CorpusCandidate,
    loadout: EvaluationLoadout,
    source_door_id: &str,
    sink_door_id: &str,
    replay: &downwards_ai::Replay,
    claim: AbilityPromotionClaimV2,
    retained_trace: Option<&downwards_lab::SemanticActionTrace>,
) -> Result<AbilityPromotionIntendedStateV2, AbilityPromotionGateV2Error> {
    let initial = Simulation::enter_via_door(
        candidate.generated().room.clone(),
        loadout.abilities(),
        source_door_id,
    )
    .map_err(|error| invalid(format!("cannot enter ability source door: {error}")))?;
    let observation = observe_successful_replay(&initial, replay, TraversalGrid::default())
        .map_err(|error| invalid(format!("cannot observe intended positive replay: {error}")))?;
    if observation.reached_exit_id != sink_door_id {
        return Err(invalid(format!(
            "intended positive replay reached {:?} instead of {:?}",
            observation.reached_exit_id, sink_door_id
        )));
    }
    if retained_trace.is_some_and(|trace| trace != &observation.actions) {
        return Err(invalid(
            "retained intended direct witness trace differs from native replay observation",
        ));
    }
    let accepted = observation.actions.events.iter().any(|event| match claim {
        AbilityPromotionClaimV2::WallJump => matches!(event.event, SemanticEvent::WallJump(_)),
        AbilityPromotionClaimV2::Dash => matches!(event.event, SemanticEvent::Dash(_)),
    });
    Ok(if accepted {
        AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted
    } else {
        AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventMissing
    })
}

fn replay_matrix_route_state(
    candidate: &CorpusCandidate,
    loadout: EvaluationLoadout,
    source_door_id: &str,
    target_door_id: &str,
    evidence: &BoundedTargetEvidence,
) -> Result<AbilityPromotionMatrixStateV2, AbilityPromotionGateV2Error> {
    let BoundedTargetEvidence::Positive(positive) = evidence else {
        return Ok(AbilityPromotionMatrixStateV2::BoundedInconclusive);
    };
    let initial = Simulation::enter_via_door(
        candidate.generated().room.clone(),
        loadout.abilities(),
        source_door_id,
    )
    .map_err(|error| invalid(format!("cannot enter matrix source door: {error}")))?;
    let observation = observe_successful_replay(
        &initial,
        &positive.solution().replay,
        TraversalGrid::default(),
    )
    .map_err(|error| invalid(format!("cannot replay matrix positive: {error}")))?;
    if observation.reached_exit_id != target_door_id {
        return Err(invalid(format!(
            "matrix positive replay reached {:?} instead of {:?}",
            observation.reached_exit_id, target_door_id
        )));
    }
    Ok(AbilityPromotionMatrixStateV2::ReplayCertifiedPositive)
}

fn validate_direct_assessment(
    assessment: &RouteControllerAssessment,
    source_door_id: &str,
    sink_door_id: &str,
) -> Result<(), AbilityPromotionGateV2Error> {
    if assessment.policy != CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY
        || assessment.source_door_id != source_door_id
        || assessment.target_door_id != sink_door_id
        || assessment.authoritative_loadout != EvaluationLoadout::Both
        || assessment.expected_subset_loadouts != EvaluationLoadout::ALL
        || assessment
            .audits
            .iter()
            .map(|audit| audit.loadout)
            .ne(EvaluationLoadout::ALL)
    {
        return Err(invalid(
            "advertised-pair direct audit has stale policy or noncanonical coordinates/loadouts",
        ));
    }
    for audit in &assessment.audits {
        let retained = assessment
            .easiest_first_witnesses
            .iter()
            .filter(|witness| witness.loadout == audit.loadout)
            .count();
        if audit.raw_positive_witnesses < audit.retained_semantic_witnesses
            || audit.retained_semantic_witnesses != retained
        {
            return Err(invalid(
                "advertised-pair direct audit has inconsistent positive witness counts",
            ));
        }
    }
    Ok(())
}

fn door_row<'a>(
    matrices: &'a [LoadoutRouteMatrix],
    loadout: EvaluationLoadout,
    source_door_id: &str,
    target_door_id: &str,
) -> Result<&'a downwards_validation::DoorRouteEvidence, AbilityPromotionGateV2Error> {
    let rows = matrices
        .iter()
        .filter(|matrix| matrix.loadout == loadout)
        .flat_map(|matrix| matrix.evidence.door_routes())
        .filter(|row| row.source_door_id == source_door_id && row.target_door_id == target_door_id)
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return Err(invalid(format!(
            "{} matrix has {} rows for {:?}->{:?} instead of one",
            loadout.slug(),
            rows.len(),
            source_door_id,
            target_door_id
        )));
    };
    Ok(row)
}

fn matrix_state(evidence: &BoundedTargetEvidence) -> AbilityPromotionMatrixStateV2 {
    match evidence {
        BoundedTargetEvidence::Positive(_) => {
            AbilityPromotionMatrixStateV2::ReplayCertifiedPositive
        }
        BoundedTargetEvidence::Inconclusive(_) => {
            AbilityPromotionMatrixStateV2::BoundedInconclusive
        }
    }
}

fn promotion_decision(
    intended: AbilityPromotionIntendedStateV2,
    reverse: AbilityPromotionMatrixStateV2,
    missing: &[AbilityPromotionMissingLoadoutEvidenceV2],
) -> AbilityPromotionDecisionV2 {
    match intended {
        AbilityPromotionIntendedStateV2::BoundedInconclusive => {
            return AbilityPromotionDecisionV2::BoundedIntendedRoute;
        }
        AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventMissing => {
            return AbilityPromotionDecisionV2::IntendedRouteMissingRequiredEvent;
        }
        AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted => {}
    }
    if reverse != AbilityPromotionMatrixStateV2::ReplayCertifiedPositive {
        return AbilityPromotionDecisionV2::BoundedBaselineReverse;
    }
    if let Some(evidence) = missing
        .iter()
        .find(|evidence| evidence.matrix == AbilityPromotionMatrixStateV2::ReplayCertifiedPositive)
    {
        return AbilityPromotionDecisionV2::PositiveMatrixBypass {
            loadout: evidence.loadout,
        };
    }
    for evidence in missing {
        match evidence.direct_audit {
            AbilityPromotionDirectAuditStateV2::PositiveBypass { .. } => {
                return AbilityPromotionDecisionV2::PositiveDirectBypass {
                    loadout: evidence.loadout,
                };
            }
            AbilityPromotionDirectAuditStateV2::BoundedInconclusive { limit } => {
                return AbilityPromotionDecisionV2::BoundedDirectAudit {
                    loadout: evidence.loadout,
                    limit,
                };
            }
            AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive => {}
        }
    }
    AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass
}

fn alias_mismatch_gate(
    candidate: &CorpusCandidate,
    claim: AbilityPromotionClaimV2,
    source_door_id: String,
    sink_door_id: String,
) -> VariantAbilityPromotionGateV2 {
    VariantAbilityPromotionGateV2 {
        gate_version: CORPUS_ABILITY_PROMOTION_GATE_VERSION,
        key: candidate.exact_key(),
        evidence: VariantAbilityPromotionEvidenceV2::Ability {
            claim,
            source_door_id,
            sink_door_id,
            edge_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            gate_embedding_contract_version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            alias_room_compatible: false,
            intended_route: None,
            intended_evidence_source: None,
            baseline_reverse: None,
            missing_ability_loadouts: Vec::new(),
            direct_route_assessment: None,
        },
        decision: AbilityPromotionDecisionV2::RefusedAliasRoomMismatch,
    }
}

impl From<downwards_ai::DirectProbeBudgetLimit> for AbilityPromotionBudgetLimitV2 {
    fn from(value: downwards_ai::DirectProbeBudgetLimit) -> Self {
        match value {
            downwards_ai::DirectProbeBudgetLimit::ExpandedNodes => Self::ExpandedNodes,
            downwards_ai::DirectProbeBudgetLimit::SimulatedTicks => Self::SimulatedTicks,
        }
    }
}

#[derive(Debug)]
pub enum AbilityPromotionGateV2Error {
    InvalidContract(String),
    DirectAudit {
        key: Box<CorpusCandidateKeyRecord>,
        source: Box<RouteControllerAssessmentError>,
    },
}

fn invalid(detail: impl Into<String>) -> AbilityPromotionGateV2Error {
    AbilityPromotionGateV2Error::InvalidContract(detail.into())
}

impl fmt::Display for AbilityPromotionGateV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContract(detail) => {
                write!(formatter, "invalid ability-promotion evidence: {detail}")
            }
            Self::DirectAudit { key, source } => write!(
                formatter,
                "ability-promotion direct audit failed for {}: {source}",
                key.stable_slug()
            ),
        }
    }
}

impl Error for AbilityPromotionGateV2Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContract(_) => None,
            Self::DirectAudit { source, .. } => Some(source.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use downwards_ai::DirectProbeBudgetLimit;
    use downwards_gen::experimental::{
        ChallengeIntent, CompositionalAbilityGateProfile, CompositionalAbilityGenerationKey,
    };
    use downwards_validation::{ValidationConfig, evaluate_generated_door_targets_for_loadout};

    use super::*;
    use crate::corpus::{CorpusRoomAnalysisConfig, RouteMatrixSummary};

    #[test]
    fn missing_ability_set_includes_the_alternative_single_ability() {
        let missing = |claim: AbilityPromotionClaimV2| {
            EvaluationLoadout::ALL
                .into_iter()
                .filter(|loadout| !claim.is_available(loadout.abilities()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            missing(AbilityPromotionClaimV2::WallJump),
            vec![EvaluationLoadout::Baseline, EvaluationLoadout::Dash]
        );
        assert_eq!(
            missing(AbilityPromotionClaimV2::Dash),
            vec![EvaluationLoadout::Baseline, EvaluationLoadout::WallJump]
        );
    }

    #[test]
    fn promotion_decision_fails_closed_for_every_uncertain_or_bypass_state() {
        let complete = |loadout| AbilityPromotionMissingLoadoutEvidenceV2 {
            loadout,
            matrix: AbilityPromotionMatrixStateV2::BoundedInconclusive,
            direct_audit: AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive,
        };
        let accepted = AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted;
        let reverse = AbilityPromotionMatrixStateV2::ReplayCertifiedPositive;
        let clean = vec![
            complete(EvaluationLoadout::Baseline),
            complete(EvaluationLoadout::Dash),
        ];
        assert_eq!(
            promotion_decision(accepted, reverse, &clean),
            AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass
        );
        assert_eq!(
            promotion_decision(
                AbilityPromotionIntendedStateV2::BoundedInconclusive,
                reverse,
                &clean,
            ),
            AbilityPromotionDecisionV2::BoundedIntendedRoute
        );
        assert_eq!(
            promotion_decision(
                AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventMissing,
                reverse,
                &clean,
            ),
            AbilityPromotionDecisionV2::IntendedRouteMissingRequiredEvent
        );
        assert_eq!(
            promotion_decision(
                accepted,
                AbilityPromotionMatrixStateV2::BoundedInconclusive,
                &clean,
            ),
            AbilityPromotionDecisionV2::BoundedBaselineReverse
        );

        let mut matrix_bypass = clean.clone();
        matrix_bypass[1].matrix = AbilityPromotionMatrixStateV2::ReplayCertifiedPositive;
        assert_eq!(
            promotion_decision(accepted, reverse, &matrix_bypass),
            AbilityPromotionDecisionV2::PositiveMatrixBypass {
                loadout: EvaluationLoadout::Dash,
            }
        );

        let mut direct_bypass = clean.clone();
        direct_bypass[0].direct_audit = AbilityPromotionDirectAuditStateV2::PositiveBypass {
            raw_positive_witnesses: 2,
            retained_semantic_witnesses: 1,
        };
        assert_eq!(
            promotion_decision(accepted, reverse, &direct_bypass),
            AbilityPromotionDecisionV2::PositiveDirectBypass {
                loadout: EvaluationLoadout::Baseline,
            }
        );

        let mut bounded = clean;
        bounded[1].direct_audit = AbilityPromotionDirectAuditStateV2::BoundedInconclusive {
            limit: AbilityPromotionBudgetLimitV2::SimulatedTicks,
        };
        assert_eq!(
            promotion_decision(accepted, reverse, &bounded),
            AbilityPromotionDecisionV2::BoundedDirectAudit {
                loadout: EvaluationLoadout::Dash,
                limit: AbilityPromotionBudgetLimitV2::SimulatedTicks,
            }
        );
    }

    #[test]
    fn malformed_mission_to_route_mapping_is_rejected_before_door_evidence() {
        let mut native = exact_native(CompositionalAbilityGateProfile::WallJump);
        let source_node = native.rewritten_mission.plan.source_node_id();
        let sink_node = native.rewritten_mission.plan.sink_node_id();
        let sink_route_node = native
            .mission_route_nodes
            .iter()
            .find(|mapping| mapping.mission_node_id == sink_node)
            .unwrap()
            .route_node_id;
        native
            .mission_route_nodes
            .iter_mut()
            .find(|mapping| mapping.mission_node_id == source_node)
            .unwrap()
            .route_node_id = sink_route_node;
        let candidate = CorpusCandidate::CompositionalAbility(Box::new(native));
        assert!(matches!(
            ability_contract(&candidate),
            Err(AbilityPromotionGateV2Error::InvalidContract(detail))
                if detail.contains("same embedded route node")
        ));
    }

    #[test]
    fn room_incompatible_alias_is_refused_without_inheriting_shared_evidence() {
        let candidate = exact_candidate(CompositionalAbilityGateProfile::WallJump);
        let other = exact_candidate(CompositionalAbilityGateProfile::Dash);
        assert_ne!(candidate.generated().room, other.generated().room);
        let config = CorpusRoomAnalysisConfig::default()
            .identity_record()
            .unwrap();
        let gate =
            evaluate_variant_ability_promotion_gate_v2(&candidate, &other, &[], &config).unwrap();
        assert_eq!(
            gate.decision,
            AbilityPromotionDecisionV2::RefusedAliasRoomMismatch
        );
        assert!(!gate.decision.allows_canonical_selection());
        assert!(matches!(
            gate.evidence,
            VariantAbilityPromotionEvidenceV2::Ability {
                alias_room_compatible: false,
                intended_route: None,
                intended_evidence_source: None,
                baseline_reverse: None,
                direct_route_assessment: None,
                ..
            }
        ));
    }

    /// This is the exact frozen Wall0/Dash0 gameplay claim. It is deliberately
    /// one regression despite being costlier than the pure policy tests: a
    /// future easier no-event controller must make the fixture fail.
    #[test]
    fn wall0_and_dash0_report_exact_generic_promotion_outcomes() {
        for profile in [
            CompositionalAbilityGateProfile::WallJump,
            CompositionalAbilityGateProfile::Dash,
        ] {
            let candidate = exact_candidate(profile);
            let matrices = exact_matrices(&candidate);
            let config = CorpusRoomAnalysisConfig::default()
                .identity_record()
                .unwrap();
            let gate = evaluate_variant_ability_promotion_gate_v2(
                &candidate, &candidate, &matrices, &config,
            )
            .unwrap();
            validate_variant_ability_promotion_gate_v2(&candidate, &candidate, &matrices, &gate)
                .unwrap();
            rerun_validate_variant_ability_promotion_gate_v2(
                &candidate, &candidate, &matrices, &config, &gate,
            )
            .unwrap();
            let expected_decision = match profile {
                CompositionalAbilityGateProfile::WallJump => {
                    AbilityPromotionDecisionV2::PositiveMatrixBypass {
                        loadout: EvaluationLoadout::Dash,
                    }
                }
                CompositionalAbilityGateProfile::Dash => {
                    AbilityPromotionDecisionV2::PromotedStructuralNoKnownBypass
                }
                CompositionalAbilityGateProfile::Both => unreachable!(),
            };
            assert_eq!(gate.decision, expected_decision, "{profile:?}");
            let VariantAbilityPromotionEvidenceV2::Ability {
                intended_route,
                intended_evidence_source,
                baseline_reverse,
                missing_ability_loadouts,
                direct_route_assessment,
                ..
            } = &gate.evidence
            else {
                panic!("{profile:?} should retain ability evidence")
            };
            assert_eq!(
                *intended_route,
                Some(AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted)
            );
            assert!(matches!(
                intended_evidence_source,
                Some(AbilityPromotionIntendedEvidenceSourceV2::CanonicalMatrix)
            ));
            assert_eq!(
                *baseline_reverse,
                Some(AbilityPromotionMatrixStateV2::ReplayCertifiedPositive)
            );
            assert_eq!(missing_ability_loadouts.len(), 2);
            assert!(missing_ability_loadouts.iter().all(|evidence| matches!(
                evidence.direct_audit,
                AbilityPromotionDirectAuditStateV2::CompleteFiniteVocabularyNoPositive
            )));
            assert_eq!(
                direct_route_assessment
                    .as_deref()
                    .unwrap()
                    .expected_subset_loadouts,
                EvaluationLoadout::ALL
            );

            if profile == CompositionalAbilityGateProfile::WallJump {
                let retained = direct_route_assessment.as_deref().unwrap();

                let mut bounded_assessment = retained.clone();
                let intended_audit = bounded_assessment
                    .audits
                    .iter_mut()
                    .find(|audit| audit.loadout == EvaluationLoadout::WallJump)
                    .unwrap();
                assert_eq!(intended_audit.raw_positive_witnesses, 0);
                intended_audit.status = LoadoutControllerAuditStatus::BoundedIncomplete {
                    limit: DirectProbeBudgetLimit::ExpandedNodes,
                };
                let bounded = derive_gate_from_retained_audit(
                    &candidate,
                    &candidate,
                    &matrices,
                    Some(bounded_assessment),
                )
                .unwrap();
                assert_eq!(
                    bounded.decision,
                    AbilityPromotionDecisionV2::BoundedIntendedRoute
                );
                assert!(matches!(
                    bounded.evidence,
                    VariantAbilityPromotionEvidenceV2::Ability {
                        intended_route: Some(AbilityPromotionIntendedStateV2::BoundedInconclusive),
                        intended_evidence_source: None,
                        ..
                    }
                ));

                let mut fallback_assessment = retained.clone();
                fallback_assessment
                    .easiest_first_witnesses
                    .retain(|witness| witness.loadout != EvaluationLoadout::WallJump);
                let intended_audit = fallback_assessment
                    .audits
                    .iter_mut()
                    .find(|audit| audit.loadout == EvaluationLoadout::WallJump)
                    .unwrap();
                intended_audit.raw_positive_witnesses = 0;
                intended_audit.retained_semantic_witnesses = 0;
                let fallback = derive_gate_from_retained_audit(
                    &candidate,
                    &candidate,
                    &matrices,
                    Some(fallback_assessment),
                )
                .unwrap();
                assert!(matches!(
                    fallback.evidence,
                    VariantAbilityPromotionEvidenceV2::Ability {
                        intended_route: Some(
                            AbilityPromotionIntendedStateV2::ReplayCertifiedRequiredEventAccepted
                        ),
                        intended_evidence_source: Some(
                            AbilityPromotionIntendedEvidenceSourceV2::CanonicalMatrix
                        ),
                        ..
                    }
                ));
            }
        }
    }

    fn exact_native(
        profile: CompositionalAbilityGateProfile,
    ) -> downwards_gen::experimental::CompositionalAbilityCandidate {
        CompositionalAbilityGenerationKey::new(0, profile, ChallengeIntent::Standard)
            .generate()
            .unwrap()
    }

    fn exact_candidate(profile: CompositionalAbilityGateProfile) -> CorpusCandidate {
        CorpusCandidate::try_from(exact_native(profile)).unwrap()
    }

    fn exact_matrices(candidate: &CorpusCandidate) -> Vec<LoadoutRouteMatrix> {
        EvaluationLoadout::ALL
            .into_iter()
            .map(|loadout| {
                let evidence = evaluate_generated_door_targets_for_loadout(
                    candidate.generated(),
                    loadout.abilities(),
                    &ValidationConfig::for_loadout(loadout.abilities()),
                )
                .unwrap();
                let positive_door_rows = evidence
                    .door_routes()
                    .iter()
                    .filter(|row| row.evidence.positive().is_some())
                    .count();
                let positive_pickup_rows = evidence
                    .pickup_routes()
                    .iter()
                    .filter(|row| row.evidence.positive().is_some())
                    .count();
                LoadoutRouteMatrix {
                    loadout,
                    summary: RouteMatrixSummary {
                        door_rows: evidence.door_routes().len(),
                        positive_door_rows,
                        inconclusive_door_rows: evidence.door_routes().len() - positive_door_rows,
                        pickup_rows: evidence.pickup_routes().len(),
                        positive_pickup_rows,
                        inconclusive_pickup_rows: evidence.pickup_routes().len()
                            - positive_pickup_rows,
                    },
                    evidence,
                }
            })
            .collect()
    }
}
