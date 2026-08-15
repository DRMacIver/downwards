//! Easiest-known controller evidence for one directed room route.
//!
//! This module deliberately does not assign a difficulty band.  It exhausts
//! the AI's finite direct-controller vocabulary under every subset of an
//! authoritative loadout, retains every materially distinct exact positive,
//! and orders those positives using controller demand only.  Search effort is
//! retained as operational evidence, but never participates in the ordering.

use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    error::Error,
    fmt,
};

use downwards_ai::{
    BatchTargetSolveError, DIRECT_PROBE_AUDIT_VERSION, DirectProbeAuditStatus,
    DirectProbeBudgetLimit, DirectProbeProvenance, DirectProbeWitness, ReachedTarget, Replay,
    SearchStats, SearchTarget, SolverConfig, audit_direct_controller_probes,
};
use downwards_core::{AbilitySet, DashDirection, DoorEntryError, Room, Simulation};
use downwards_lab::{
    ActionSpan, SemanticAction, SemanticActionTrace, SemanticEvent, TraversalGrid,
    WitnessObservationError, observe_successful_replay,
};
use serde::{Deserialize, Serialize};

use super::EvaluationLoadout;

/// Version of the complete policy implemented by this module.
///
/// Increment this whenever subset enumeration, semantic deduplication,
/// controller-demand measurement, Pareto dominance, or deterministic ordering
/// changes.
pub const ROUTE_CONTROLLER_ASSESSMENT_POLICY_VERSION: u32 = 2;

/// Version of the timing-sensitive semantic input trace used for deduplication.
pub const ROUTE_CONTROLLER_TRACE_VERSION: u32 = 1;

/// Version of the transparent controller-demand coordinate vector.
pub const CONTROLLER_DEMAND_POLICY_VERSION: u32 = 1;

/// Human-readable warning that must accompany persisted assessment evidence.
pub const ROUTE_CONTROLLER_ASSESSMENT_DISCLAIMER: &str = "complete means the configured finite direct-controller vocabulary was exhausted; bounded non-success is not evidence that a route is unreachable";

/// Stable identity of every policy that can change this assessment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteControllerAssessmentPolicy {
    pub assessment_version: u32,
    pub direct_probe_audit_version: u32,
    pub semantic_trace_version: u32,
    pub controller_demand_version: u32,
}

pub const CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY: RouteControllerAssessmentPolicy =
    RouteControllerAssessmentPolicy {
        assessment_version: ROUTE_CONTROLLER_ASSESSMENT_POLICY_VERSION,
        direct_probe_audit_version: DIRECT_PROBE_AUDIT_VERSION,
        semantic_trace_version: ROUTE_CONTROLLER_TRACE_VERSION,
        controller_demand_version: CONTROLLER_DEMAND_POLICY_VERSION,
    };

/// The seven nonnegative coordinates used by both dominance and display
/// ordering.  Smaller is easier-known on every coordinate.
///
/// `controller_class` is `0` for run-only, `1` for a monotone simple
/// controller, and `2` otherwise.  The remaining fields are direct counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControllerDemandCoordinates {
    pub controller_class: u8,
    pub ability_events: usize,
    pub horizontal_reversals: usize,
    pub vertical_decisions: usize,
    pub semantic_spans: usize,
    pub semantic_transitions: usize,
    pub duration_ticks: usize,
}

/// Transparent controller-only observations for one exact positive replay.
///
/// A vertical decision is a jump press, a meaningful change of vertical-axis
/// intent, or a change between accepted dash directions.  Ability events are
/// successful wall jumps plus successful dashes.  These definitions are
/// intentionally local and auditable; geometry, hazard pressure, robustness,
/// and solver effort belong to other metric layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerDemand {
    pub run_only: bool,
    pub monotone_simple: bool,
    pub ordinary_jump_events: usize,
    pub wall_jump_events: usize,
    pub dash_events: usize,
    pub jump_press_edges: usize,
    pub dash_press_edges: usize,
    pub horizontal_reversals: usize,
    pub vertical_input_changes: usize,
    pub dash_direction_changes: usize,
    pub vertical_decisions: usize,
    pub semantic_spans: usize,
    pub semantic_transitions: usize,
    pub duration_ticks: usize,
}

impl ControllerDemand {
    #[must_use]
    pub const fn ability_events(self) -> usize {
        self.wall_jump_events.saturating_add(self.dash_events)
    }

    #[must_use]
    pub const fn coordinates(self) -> ControllerDemandCoordinates {
        ControllerDemandCoordinates {
            controller_class: if self.run_only {
                0
            } else if self.monotone_simple {
                1
            } else {
                2
            },
            ability_events: self.ability_events(),
            horizontal_reversals: self.horizontal_reversals,
            vertical_decisions: self.vertical_decisions,
            semantic_spans: self.semantic_spans,
            semantic_transitions: self.semantic_transitions,
            duration_ticks: self.duration_ticks,
        }
    }
}

/// One exact positive retained under the loadout in which it succeeded.
///
/// `operational_discovery_stats` records cost only.  It is explicitly absent
/// from [`ControllerDemandCoordinates`] and all ordering/dominance code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteControllerWitness {
    pub loadout: EvaluationLoadout,
    pub replay: Replay,
    pub semantic_trace: SemanticActionTrace,
    pub demand: ControllerDemand,
    pub probes: Vec<DirectProbeProvenance>,
    pub operational_discovery_stats: SearchStats,
}

/// Result of exhausting one loadout's finite direct-controller vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadoutControllerAuditStatus {
    CompleteFiniteVocabulary,
    BoundedIncomplete { limit: DirectProbeBudgetLimit },
}

impl LoadoutControllerAuditStatus {
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::CompleteFiniteVocabulary)
    }
}

/// Auditable result for one exact physics loadout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadoutControllerAudit {
    pub loadout: EvaluationLoadout,
    pub status: LoadoutControllerAuditStatus,
    pub operational_stats: SearchStats,
    pub raw_positive_witnesses: usize,
    pub retained_semantic_witnesses: usize,
}

/// One source/loadout audit shared by every target in a batch.
///
/// These operational stats describe one call to the direct-controller audit.
/// They must not be multiplied by the number of target assessments which
/// refer to them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedLoadoutControllerAudit {
    pub loadout: EvaluationLoadout,
    pub status: LoadoutControllerAuditStatus,
    pub operational_stats: SearchStats,
    pub target_count: usize,
    pub raw_positive_witnesses: usize,
    pub retained_semantic_witnesses: usize,
}

/// Aggregate completeness across every expected subset loadout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteControllerAuditCompleteness {
    CompleteFiniteVocabulary,
    BoundedIncomplete {
        incomplete_loadouts: Vec<BoundedIncompleteLoadout>,
    },
}

impl RouteControllerAuditCompleteness {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::CompleteFiniteVocabulary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedIncompleteLoadout {
    pub loadout: EvaluationLoadout,
    pub limit: DirectProbeBudgetLimit,
}

/// Exact positive witnesses proving that a strict subset of the authoritative
/// loadout can traverse the directed route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositiveBypassEvidence {
    pub loadout: EvaluationLoadout,
    /// Indices into [`RouteControllerAssessment::easiest_first_witnesses`].
    pub witness_indices: Vec<usize>,
}

/// Room-centric result for one pinned source-door -> target-door challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteControllerAssessment {
    pub policy: RouteControllerAssessmentPolicy,
    pub source_door_id: String,
    pub target_door_id: String,
    pub authoritative_loadout: EvaluationLoadout,
    pub expected_subset_loadouts: Vec<EvaluationLoadout>,
    pub audits: Vec<LoadoutControllerAudit>,
    pub completeness: RouteControllerAuditCompleteness,
    /// All retained semantic witnesses in deterministic easiest-first order.
    /// This is an ordering for display, not a claim of total difficulty.
    pub easiest_first_witnesses: Vec<RouteControllerWitness>,
    /// Nondominated indices into `easiest_first_witnesses`, in that same
    /// deterministic order.  Equal measured demand does not dominate, so
    /// distinct equal-demand traces remain represented.
    pub easiest_known_front: Vec<usize>,
    pub positive_bypasses: Vec<PositiveBypassEvidence>,
}

/// Target-ordered route assessments produced by shared source/loadout audits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRouteControllerAssessmentBatch {
    pub policy: RouteControllerAssessmentPolicy,
    pub source_door_id: String,
    pub target_door_ids: Vec<String>,
    pub authoritative_loadout: EvaluationLoadout,
    pub expected_subset_loadouts: Vec<EvaluationLoadout>,
    pub shared_audits: Vec<SharedLoadoutControllerAudit>,
    pub routes: Vec<RouteControllerAssessment>,
}

impl SourceRouteControllerAssessmentBatch {
    /// Sum operational work once per source/loadout audit.
    ///
    /// Per-route audit rows intentionally repeat the relevant shared stats for
    /// standalone interpretability.  Consumers measuring batch cost must use
    /// this method (or `shared_audits`) rather than summing route rows.
    #[must_use]
    pub fn total_operational_stats(&self) -> SearchStats {
        self.shared_audits
            .iter()
            .fold(SearchStats::default(), |mut total, audit| {
                total.expanded_nodes = total
                    .expanded_nodes
                    .saturating_add(audit.operational_stats.expanded_nodes);
                total.generated_nodes = total
                    .generated_nodes
                    .saturating_add(audit.operational_stats.generated_nodes);
                total.simulated_ticks = total
                    .simulated_ticks
                    .saturating_add(audit.operational_stats.simulated_ticks);
                total.deepest_path_ticks = total
                    .deepest_path_ticks
                    .max(audit.operational_stats.deepest_path_ticks);
                total
            })
    }
}

impl RouteControllerAssessment {
    #[must_use]
    pub fn easiest_known(&self) -> Option<&RouteControllerWitness> {
        self.easiest_first_witnesses.first()
    }

    pub fn front_witnesses(&self) -> impl Iterator<Item = &RouteControllerWitness> {
        self.easiest_known_front
            .iter()
            .map(|&index| &self.easiest_first_witnesses[index])
    }

    #[must_use]
    pub fn has_positive_without_wall_jump(&self) -> bool {
        self.positive_bypasses
            .iter()
            .any(|evidence| !evidence.loadout.abilities().wall_jump)
    }

    #[must_use]
    pub fn has_positive_without_dash(&self) -> bool {
        self.positive_bypasses
            .iter()
            .any(|evidence| !evidence.loadout.abilities().dash)
    }
}

/// Exhaust the direct-controller vocabulary for one directed door pair under
/// every subset of `authoritative_loadout`.
///
/// Each audit uses an exact [`Simulation::enter_via_door`] initial state for
/// its own loadout.  A replay is never tried under, or relabelled as, another
/// loadout: added abilities can change initial state and trajectory physics.
pub fn assess_easiest_known_route(
    room: &Room,
    source_door_id: impl Into<String>,
    target_door_id: impl Into<String>,
    authoritative_loadout: EvaluationLoadout,
    solver: &SolverConfig,
) -> Result<RouteControllerAssessment, RouteControllerAssessmentError> {
    let source_door_id = source_door_id.into();
    let target_door_id = target_door_id.into();
    let mut batch = assess_easiest_known_routes_from_source(
        room,
        source_door_id,
        std::iter::once(target_door_id),
        authoritative_loadout,
        solver,
    )?;
    Ok(batch
        .routes
        .pop()
        .expect("a one-target source batch returns exactly one route"))
}

/// Exhaust the direct-controller vocabulary once per subset loadout for all
/// requested targets from one exact source-door entry.
///
/// Target order is preserved, including repeated target IDs.  Each positive
/// is distributed by the audit's typed `target_index`; semantic deduplication,
/// controller fronts, and lower-loadout bypass evidence remain target-local.
/// Operational stats are reported once in `shared_audits` and copied into
/// each route row only so that a route remains interpretable in isolation.
pub fn assess_easiest_known_routes_from_source<TargetId>(
    room: &Room,
    source_door_id: impl Into<String>,
    target_door_ids: impl IntoIterator<Item = TargetId>,
    authoritative_loadout: EvaluationLoadout,
    solver: &SolverConfig,
) -> Result<SourceRouteControllerAssessmentBatch, RouteControllerAssessmentError>
where
    TargetId: Into<String>,
{
    let source_door_id = source_door_id.into();
    let target_door_ids = target_door_ids
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    if let Some(door_id) = target_door_ids
        .iter()
        .find(|target_door_id| **target_door_id == source_door_id)
    {
        return Err(RouteControllerAssessmentError::SameSourceAndTarget {
            door_id: door_id.clone(),
        });
    }

    let expected_subset_loadouts = subset_loadouts(authoritative_loadout);
    let targets = target_door_ids
        .iter()
        .map(SearchTarget::door)
        .collect::<Vec<_>>();
    let mut pending = target_door_ids
        .iter()
        .map(|target_door_id| PendingTargetAssessment {
            target_door_id: target_door_id.clone(),
            witnesses: Vec::new(),
            semantic_indices: HashMap::new(),
            audits: Vec::with_capacity(expected_subset_loadouts.len()),
        })
        .collect::<Vec<_>>();
    let mut shared_audits = Vec::with_capacity(expected_subset_loadouts.len());

    for &loadout in &expected_subset_loadouts {
        let initial =
            Simulation::enter_via_door(room.clone(), loadout.abilities(), &source_door_id)
                .map_err(|source| RouteControllerAssessmentError::DoorEntry {
                    loadout,
                    source: Box::new(source),
                })?;
        let mut audit_config = solver.clone();
        audit_config.macros = SolverConfig::for_abilities(loadout.abilities()).macros;
        let audit = audit_direct_controller_probes(&initial, &targets, &audit_config).map_err(
            |source| RouteControllerAssessmentError::DirectProbeAudit {
                loadout,
                source: Box::new(source),
            },
        )?;
        let status = match audit.status {
            DirectProbeAuditStatus::Complete => {
                LoadoutControllerAuditStatus::CompleteFiniteVocabulary
            }
            DirectProbeAuditStatus::BudgetLimited(limit) => {
                LoadoutControllerAuditStatus::BoundedIncomplete { limit }
            }
        };
        let raw_positive_witnesses = audit.witnesses.len();
        let retained_before = pending
            .iter()
            .map(|target| target.witnesses.len())
            .collect::<Vec<_>>();
        let mut raw_positives_by_target = vec![0_usize; pending.len()];
        for direct_witness in audit.witnesses {
            let target_index = direct_witness.target_index;
            let Some(target) = pending.get_mut(target_index) else {
                return Err(RouteControllerAssessmentError::MalformedPositiveWitness {
                    loadout,
                    target_index,
                    target: direct_witness.target,
                    reached: direct_witness.reached,
                });
            };
            raw_positives_by_target[target_index] += 1;
            let observed = observe_direct_witness(
                &initial,
                loadout,
                target_index,
                &target.target_door_id,
                direct_witness,
            )?;
            retain_semantic_witness(
                &mut target.witnesses,
                &mut target.semantic_indices,
                observed,
            );
        }
        let retained_semantic_witnesses = pending
            .iter()
            .enumerate()
            .map(|(target_index, target)| target.witnesses.len() - retained_before[target_index])
            .sum();
        for (target_index, target) in pending.iter_mut().enumerate() {
            target.audits.push(LoadoutControllerAudit {
                loadout,
                status,
                operational_stats: audit.stats,
                raw_positive_witnesses: raw_positives_by_target[target_index],
                retained_semantic_witnesses: target.witnesses.len() - retained_before[target_index],
            });
        }
        shared_audits.push(SharedLoadoutControllerAudit {
            loadout,
            status,
            operational_stats: audit.stats,
            target_count: pending.len(),
            raw_positive_witnesses,
            retained_semantic_witnesses,
        });
    }

    let routes = pending
        .into_iter()
        .map(|pending| {
            finalize_target_assessment(
                &source_door_id,
                authoritative_loadout,
                &expected_subset_loadouts,
                pending,
            )
        })
        .collect();

    Ok(SourceRouteControllerAssessmentBatch {
        policy: CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY,
        source_door_id,
        target_door_ids,
        authoritative_loadout,
        expected_subset_loadouts,
        shared_audits,
        routes,
    })
}

struct PendingTargetAssessment {
    target_door_id: String,
    witnesses: Vec<RouteControllerWitness>,
    semantic_indices: HashMap<SemanticWitnessKey, usize>,
    audits: Vec<LoadoutControllerAudit>,
}

fn finalize_target_assessment(
    source_door_id: &str,
    authoritative_loadout: EvaluationLoadout,
    expected_subset_loadouts: &[EvaluationLoadout],
    pending: PendingTargetAssessment,
) -> RouteControllerAssessment {
    let PendingTargetAssessment {
        target_door_id,
        mut witnesses,
        semantic_indices: _,
        audits,
    } = pending;
    witnesses.sort_by(compare_controller_witnesses);
    let easiest_known_front = easiest_front_indices(&witnesses);
    let positive_bypasses = expected_subset_loadouts
        .iter()
        .copied()
        .filter(|&loadout| loadout != authoritative_loadout)
        .filter_map(|loadout| {
            let witness_indices = witnesses
                .iter()
                .enumerate()
                .filter_map(|(index, witness)| (witness.loadout == loadout).then_some(index))
                .collect::<Vec<_>>();
            (!witness_indices.is_empty()).then_some(PositiveBypassEvidence {
                loadout,
                witness_indices,
            })
        })
        .collect();
    let incomplete_loadouts = audits
        .iter()
        .filter_map(|audit| match audit.status {
            LoadoutControllerAuditStatus::CompleteFiniteVocabulary => None,
            LoadoutControllerAuditStatus::BoundedIncomplete { limit } => {
                Some(BoundedIncompleteLoadout {
                    loadout: audit.loadout,
                    limit,
                })
            }
        })
        .collect::<Vec<_>>();
    let completeness = if incomplete_loadouts.is_empty() {
        RouteControllerAuditCompleteness::CompleteFiniteVocabulary
    } else {
        RouteControllerAuditCompleteness::BoundedIncomplete {
            incomplete_loadouts,
        }
    };

    RouteControllerAssessment {
        policy: CURRENT_ROUTE_CONTROLLER_ASSESSMENT_POLICY,
        source_door_id: source_door_id.to_owned(),
        target_door_id,
        authoritative_loadout,
        expected_subset_loadouts: expected_subset_loadouts.to_vec(),
        audits,
        completeness,
        easiest_first_witnesses: witnesses,
        easiest_known_front,
        positive_bypasses,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteControllerAssessmentError {
    SameSourceAndTarget {
        door_id: String,
    },
    DoorEntry {
        loadout: EvaluationLoadout,
        source: Box<DoorEntryError>,
    },
    DirectProbeAudit {
        loadout: EvaluationLoadout,
        source: Box<BatchTargetSolveError>,
    },
    MalformedPositiveWitness {
        loadout: EvaluationLoadout,
        target_index: usize,
        target: SearchTarget,
        reached: ReachedTarget,
    },
    ReplayObservation {
        loadout: EvaluationLoadout,
        source: Box<WitnessObservationError>,
    },
    ReachedUnexpectedDoor {
        loadout: EvaluationLoadout,
        expected_door_id: String,
        actual_door_id: String,
    },
}

impl fmt::Display for RouteControllerAssessmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameSourceAndTarget { door_id } => write!(
                formatter,
                "directed route source and target must differ, both were {door_id:?}"
            ),
            Self::DoorEntry { loadout, source } => write!(
                formatter,
                "cannot enter route source under {}: {source}",
                loadout.slug()
            ),
            Self::DirectProbeAudit { loadout, source } => write!(
                formatter,
                "direct-controller audit failed under {}: {source}",
                loadout.slug()
            ),
            Self::MalformedPositiveWitness {
                loadout,
                target_index,
                target,
                reached,
            } => write!(
                formatter,
                "direct-controller audit returned malformed positive under {}: index={target_index}, target={target:?}, reached={reached:?}",
                loadout.slug()
            ),
            Self::ReplayObservation { loadout, source } => write!(
                formatter,
                "cannot verify direct-controller replay under {}: {source}",
                loadout.slug()
            ),
            Self::ReachedUnexpectedDoor {
                loadout,
                expected_door_id,
                actual_door_id,
            } => write!(
                formatter,
                "direct-controller replay under {} reached {actual_door_id:?}, expected {expected_door_id:?}",
                loadout.slug()
            ),
        }
    }
}

impl Error for RouteControllerAssessmentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DoorEntry { source, .. } => Some(source.as_ref()),
            Self::DirectProbeAudit { source, .. } => Some(source.as_ref()),
            Self::ReplayObservation { source, .. } => Some(source.as_ref()),
            Self::SameSourceAndTarget { .. }
            | Self::MalformedPositiveWitness { .. }
            | Self::ReachedUnexpectedDoor { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SemanticWitnessKey {
    loadout: EvaluationLoadout,
    spans: Vec<(SemanticAction, usize)>,
}

fn observe_direct_witness(
    initial: &Simulation,
    loadout: EvaluationLoadout,
    expected_target_index: usize,
    target_door_id: &str,
    direct: DirectProbeWitness,
) -> Result<RouteControllerWitness, RouteControllerAssessmentError> {
    if direct.target_index != expected_target_index
        || direct.target != SearchTarget::door(target_door_id)
        || direct.reached != ReachedTarget::Door(target_door_id.to_owned())
    {
        return Err(RouteControllerAssessmentError::MalformedPositiveWitness {
            loadout,
            target_index: direct.target_index,
            target: direct.target,
            reached: direct.reached,
        });
    }
    let observation = observe_successful_replay(initial, &direct.replay, TraversalGrid::default())
        .map_err(|source| RouteControllerAssessmentError::ReplayObservation {
            loadout,
            source: Box::new(source),
        })?;
    if observation.reached_exit_id != target_door_id {
        return Err(RouteControllerAssessmentError::ReachedUnexpectedDoor {
            loadout,
            expected_door_id: target_door_id.to_owned(),
            actual_door_id: observation.reached_exit_id,
        });
    }

    // Direct-probe witnesses currently end exactly on target contact.  Keep
    // this defensive trim so the semantic identity remains "through target"
    // if a future audit records inert trailing frames.
    let replay = Replay {
        initial_digest: direct.replay.initial_digest,
        frames: direct.replay.frames[..observation.completion_ticks].to_vec(),
    };
    let demand = controller_demand(&observation.actions);
    Ok(RouteControllerWitness {
        loadout,
        replay,
        semantic_trace: observation.actions,
        demand,
        probes: direct.probes,
        operational_discovery_stats: direct.stats_at_first_discovery,
    })
}

fn retain_semantic_witness(
    witnesses: &mut Vec<RouteControllerWitness>,
    indices: &mut HashMap<SemanticWitnessKey, usize>,
    witness: RouteControllerWitness,
) {
    let key = SemanticWitnessKey {
        loadout: witness.loadout,
        spans: semantic_span_key(&witness.semantic_trace),
    };
    if let Some(&index) = indices.get(&key) {
        let existing = &mut witnesses[index];
        debug_assert_eq!(existing.semantic_trace, witness.semantic_trace);
        debug_assert_eq!(existing.demand, witness.demand);
        existing.probes.extend(witness.probes);
        existing
            .probes
            .sort_by_key(|probe| (probe.ordinal, probe.move_x));
        existing.probes.dedup();
    } else {
        let index = witnesses.len();
        witnesses.push(witness);
        indices.insert(key, index);
    }
}

pub(super) fn controller_demand(actions: &SemanticActionTrace) -> ControllerDemand {
    let horizontal_directions = actions
        .spans
        .iter()
        .filter_map(|span| (span.action.move_x != 0).then_some(span.action.move_x))
        .collect::<BTreeSet<_>>();
    let uses_vertical_input = actions.spans.iter().any(|span| span.action.move_y != 0);
    let uses_jump_input = actions.spans.iter().any(|span| span.action.jump_held);
    let uses_dash_input = actions.spans.iter().any(|span| span.action.dash_held);
    let uses_restart_input = actions.spans.iter().any(|span| span.action.restart);
    let horizontal_reversals = direction_reversals(&actions.spans, |action| action.move_x);
    let vertical_input_changes = input_direction_changes(&actions.spans, |action| action.move_y);
    let dash_direction_changes = accepted_dash_direction_changes(actions);
    let ordinary_jump_events = actions
        .successful_jumps
        .saturating_sub(actions.successful_wall_jumps);
    let vertical_decisions = actions
        .jump_presses
        .saturating_add(vertical_input_changes)
        .saturating_add(dash_direction_changes);
    let run_only = horizontal_reversals == 0
        && horizontal_directions.len() <= 1
        && !uses_vertical_input
        && !uses_jump_input
        && !uses_dash_input
        && !uses_restart_input
        && actions.successful_jumps == 0
        && actions.successful_dashes == 0;
    let monotone_simple = horizontal_reversals == 0
        && horizontal_directions.len() <= 1
        && !uses_vertical_input
        && !uses_dash_input
        && !uses_restart_input
        && actions.successful_dashes == 0;

    ControllerDemand {
        run_only,
        monotone_simple,
        ordinary_jump_events,
        wall_jump_events: actions.successful_wall_jumps,
        dash_events: actions.successful_dashes,
        jump_press_edges: actions.jump_presses,
        dash_press_edges: actions.dash_presses,
        horizontal_reversals,
        vertical_input_changes,
        dash_direction_changes,
        vertical_decisions,
        semantic_spans: actions.spans.len(),
        semantic_transitions: semantic_transitions(&actions.spans),
        duration_ticks: actions.total_ticks,
    }
}

fn semantic_transitions(spans: &[ActionSpan]) -> usize {
    spans.len().saturating_sub(usize::from(
        spans
            .first()
            .is_some_and(|span| span.action == SemanticAction::default()),
    ))
}

fn direction_reversals(spans: &[ActionSpan], direction: impl Fn(SemanticAction) -> i8) -> usize {
    let mut previous_nonzero = 0;
    let mut reversals = 0;
    for span in spans {
        let current = direction(span.action);
        if current == 0 {
            continue;
        }
        if previous_nonzero != 0 && current != previous_nonzero {
            reversals += 1;
        }
        previous_nonzero = current;
    }
    reversals
}

fn input_direction_changes(
    spans: &[ActionSpan],
    direction: impl Fn(SemanticAction) -> i8,
) -> usize {
    let Some(first) = spans.first() else {
        return 0;
    };
    let mut previous = direction(first.action);
    spans[1..]
        .iter()
        .filter(|span| {
            let current = direction(span.action);
            let changed = current != previous && (current != 0 || previous != 0);
            previous = current;
            changed
        })
        .count()
}

fn accepted_dash_direction_changes(actions: &SemanticActionTrace) -> usize {
    let mut previous = None::<DashDirection>;
    let mut changes = 0;
    for direction in actions.events.iter().filter_map(|event| match event.event {
        SemanticEvent::Dash(direction) => Some(direction),
        _ => None,
    }) {
        if previous.is_some_and(|previous| previous != direction) {
            changes += 1;
        }
        previous = Some(direction);
    }
    changes
}

fn compare_controller_witnesses(
    left: &RouteControllerWitness,
    right: &RouteControllerWitness,
) -> Ordering {
    left.demand
        .coordinates()
        .cmp(&right.demand.coordinates())
        // These final fields only make equal measured demand repeatable.  They
        // do not affect Pareto dominance or assert a difficulty distinction.
        .then_with(|| compare_semantic_spans(&left.semantic_trace, &right.semantic_trace))
        .then_with(|| left.loadout.cmp(&right.loadout))
}

fn compare_semantic_spans(left: &SemanticActionTrace, right: &SemanticActionTrace) -> Ordering {
    semantic_span_key(left).cmp(&semantic_span_key(right))
}

fn semantic_span_key(actions: &SemanticActionTrace) -> Vec<(SemanticAction, usize)> {
    actions
        .spans
        .iter()
        .map(|span| (span.action, span.ticks))
        .collect()
}

fn easiest_front_indices(witnesses: &[RouteControllerWitness]) -> Vec<usize> {
    witnesses
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            let dominated = witnesses.iter().enumerate().any(|(other_index, other)| {
                other_index != candidate_index
                    && strictly_controller_dominates(other.demand, candidate.demand)
            });
            (!dominated).then_some(candidate_index)
        })
        .collect()
}

fn strictly_controller_dominates(left: ControllerDemand, right: ControllerDemand) -> bool {
    let left = left.coordinates();
    let right = right.coordinates();
    let pairs = [
        (
            usize::from(left.controller_class),
            usize::from(right.controller_class),
        ),
        (left.ability_events, right.ability_events),
        (left.horizontal_reversals, right.horizontal_reversals),
        (left.vertical_decisions, right.vertical_decisions),
        (left.semantic_spans, right.semantic_spans),
        (left.semantic_transitions, right.semantic_transitions),
        (left.duration_ticks, right.duration_ticks),
    ];
    pairs.iter().all(|(left, right)| left <= right)
        && pairs.iter().any(|(left, right)| left < right)
}

fn subset_loadouts(authoritative: EvaluationLoadout) -> Vec<EvaluationLoadout> {
    let authoritative = authoritative.abilities();
    EvaluationLoadout::ALL
        .into_iter()
        .filter(|loadout| ability_subset(loadout.abilities(), authoritative))
        .collect()
}

const fn ability_subset(candidate: AbilitySet, superset: AbilitySet) -> bool {
    (!candidate.wall_jump || superset.wall_jump) && (!candidate.dash || superset.dash)
}

#[cfg(test)]
mod tests {
    use downwards_ai::{DirectProbePolicy, ReplayDivergence};
    use downwards_core::{Action, BoundarySide, Door, Point, Rect, Tile};

    use super::*;

    const WIDTH: usize = 32;
    const HEIGHT: usize = 18;
    type BehaviorProjection = (
        EvaluationLoadout,
        Vec<(SemanticAction, usize)>,
        ControllerDemand,
    );

    fn flat_door_room() -> Room {
        let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        for tile in &mut tiles[16 * WIDTH..17 * WIDTH] {
            *tile = Tile::Solid;
        }
        Room::new(
            "route-assessment-flat",
            "Route assessment flat",
            WIDTH as u16,
            HEIGHT as u16,
            10,
            tiles,
            Point::new(30, 148),
            vec![],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".into(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 140, 4, 20),
                arrival: Point::new(30, 148),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east".into(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 140, 4, 20),
                arrival: Point::new(270, 148),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    fn shared_source_room() -> Room {
        let mut tiles = vec![Tile::Empty; WIDTH * HEIGHT];
        for tile in &mut tiles[16 * WIDTH..17 * WIDTH] {
            *tile = Tile::Solid;
        }
        Room::new(
            "route-assessment-shared-source",
            "Route assessment shared source",
            WIDTH as u16,
            HEIGHT as u16,
            10,
            tiles,
            Point::new(30, 148),
            vec![],
        )
        .unwrap()
        .with_doors(vec![
            Door {
                id: "west".into(),
                side: BoundarySide::Left,
                trigger_bounds: Rect::new(0, 140, 4, 20),
                arrival: Point::new(30, 148),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east-high".into(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 90, 4, 24),
                arrival: Point::new(270, 98),
                destination_room: None,
                destination_door: None,
            },
            Door {
                id: "east-low".into(),
                side: BoundarySide::Right,
                trigger_bounds: Rect::new(316, 140, 4, 20),
                arrival: Point::new(270, 148),
                destination_room: None,
                destination_door: None,
            },
        ])
        .unwrap()
    }

    fn audit_config() -> SolverConfig {
        SolverConfig {
            max_expanded_nodes: 10_000,
            max_simulated_ticks: 2_000_000,
            max_ticks_per_path: 240,
            ..SolverConfig::default()
        }
    }

    fn replay_to_east(initial: &Simulation, mut action_at: impl FnMut(usize) -> Action) -> Replay {
        let mut simulation = initial.clone();
        let mut actions = Vec::new();
        for tick in 0..240 {
            let action = action_at(tick);
            simulation.step(action);
            actions.push(action);
            if simulation.reached_exit() == Some("east") {
                return Replay::record(initial, actions);
            }
        }
        panic!("test controller did not reach east door")
    }

    fn direct_witness(
        replay: Replay,
        policy: DirectProbePolicy,
        ordinal: usize,
    ) -> DirectProbeWitness {
        DirectProbeWitness {
            target_index: 0,
            target: SearchTarget::door("east"),
            reached: ReachedTarget::Door("east".to_owned()),
            replay,
            stats_at_first_discovery: SearchStats::default(),
            probes: vec![DirectProbeProvenance {
                ordinal,
                move_x: 1,
                policy,
            }],
        }
    }

    fn behavior_projection(assessment: &RouteControllerAssessment) -> Vec<BehaviorProjection> {
        assessment
            .easiest_first_witnesses
            .iter()
            .map(|witness| {
                (
                    witness.loadout,
                    semantic_span_key(&witness.semantic_trace),
                    witness.demand,
                )
            })
            .collect()
    }

    #[test]
    fn hard_first_easy_later_still_orders_the_easy_controller_first() {
        let initial =
            Simulation::enter_via_door(flat_door_room(), AbilitySet::NONE, "west").unwrap();
        let hard_replay = replay_to_east(&initial, |tick| Action {
            move_x: 1,
            jump: tick % 18 < 5,
            ..Action::default()
        });
        let easy_replay = replay_to_east(&initial, |_| Action {
            move_x: 1,
            ..Action::default()
        });
        let hard = observe_direct_witness(
            &initial,
            EvaluationLoadout::Baseline,
            0,
            "east",
            direct_witness(
                hard_replay,
                DirectProbePolicy::PeriodicJump {
                    period: 18,
                    hold_ticks: 5,
                    phase: 0,
                },
                0,
            ),
        )
        .unwrap();
        let easy = observe_direct_witness(
            &initial,
            EvaluationLoadout::Baseline,
            0,
            "east",
            direct_witness(easy_replay, DirectProbePolicy::Run, 1),
        )
        .unwrap();
        assert!(!hard.demand.run_only);
        assert!(easy.demand.run_only);

        let mut witnesses = vec![hard, easy];
        witnesses.sort_by(compare_controller_witnesses);
        assert!(witnesses[0].demand.run_only);
        assert_eq!(easiest_front_indices(&witnesses), vec![0]);
    }

    #[test]
    fn lower_loadout_positive_remains_exact_even_when_authoritative_physics_differs() {
        let room = flat_door_room();
        let assessment = assess_easiest_known_route(
            &room,
            "west",
            "east",
            EvaluationLoadout::Both,
            &audit_config(),
        )
        .unwrap();
        let baseline_bypass = assessment
            .positive_bypasses
            .iter()
            .find(|evidence| evidence.loadout == EvaluationLoadout::Baseline)
            .expect("flat route has an exact baseline bypass");
        let witness = &assessment.easiest_first_witnesses[baseline_bypass.witness_indices[0]];
        let baseline_initial =
            Simulation::enter_via_door(room.clone(), AbilitySet::NONE, "west").unwrap();
        witness.replay.verify(&baseline_initial).unwrap();

        let authoritative_initial =
            Simulation::enter_via_door(room, AbilitySet::ALL, "west").unwrap();
        assert!(matches!(
            witness.replay.verify(&authoritative_initial),
            Err(ReplayDivergence::InitialState { .. })
        ));
        assert_eq!(witness.loadout, EvaluationLoadout::Baseline);
        assert!(assessment.has_positive_without_wall_jump());
        assert!(assessment.has_positive_without_dash());
    }

    #[test]
    fn a_probe_budget_limit_taints_aggregate_completeness_but_keeps_positives() {
        let mut config = audit_config();
        config.max_expanded_nodes = 1;
        let assessment = assess_easiest_known_route(
            &flat_door_room(),
            "west",
            "east",
            EvaluationLoadout::Both,
            &config,
        )
        .unwrap();

        let RouteControllerAuditCompleteness::BoundedIncomplete {
            incomplete_loadouts,
        } = &assessment.completeness
        else {
            panic!("one-probe budget must be reported as incomplete")
        };
        assert_eq!(incomplete_loadouts.len(), 4);
        assert!(
            incomplete_loadouts
                .iter()
                .all(|audit| { audit.limit == DirectProbeBudgetLimit::ExpandedNodes })
        );
        assert!(!assessment.easiest_first_witnesses.is_empty());
    }

    #[test]
    fn assessment_is_deterministic_and_semantic_dedup_is_loadout_scoped() {
        let room = flat_door_room();
        let config = audit_config();
        let first =
            assess_easiest_known_route(&room, "west", "east", EvaluationLoadout::Both, &config)
                .unwrap();
        let second =
            assess_easiest_known_route(&room, "west", "east", EvaluationLoadout::Both, &config)
                .unwrap();
        assert_eq!(first, second);

        let run_loadouts = first
            .easiest_first_witnesses
            .iter()
            .filter(|witness| witness.demand.run_only)
            .map(|witness| witness.loadout)
            .collect::<BTreeSet<_>>();
        assert_eq!(run_loadouts, EvaluationLoadout::ALL.into_iter().collect());
    }

    #[test]
    fn source_batch_matches_individual_controller_evidence_and_preserves_target_order() {
        let room = shared_source_room();
        let config = audit_config();
        let target_order = ["east-high", "east-low"];
        let batch = assess_easiest_known_routes_from_source(
            &room,
            "west",
            target_order,
            EvaluationLoadout::Baseline,
            &config,
        )
        .unwrap();

        assert_eq!(batch.target_door_ids, target_order);
        assert_eq!(batch.routes.len(), target_order.len());
        for (route, target_door_id) in batch.routes.iter().zip(target_order) {
            let individual = assess_easiest_known_route(
                &room,
                "west",
                target_door_id,
                EvaluationLoadout::Baseline,
                &config,
            )
            .unwrap();
            assert_eq!(route.target_door_id, target_door_id);
            assert_eq!(route.completeness, individual.completeness);
            assert_eq!(behavior_projection(route), behavior_projection(&individual));
            assert_eq!(route.easiest_known_front, individual.easiest_known_front);
            assert_eq!(route.positive_bypasses, individual.positive_bypasses);
            assert_eq!(route.audits[0].status, individual.audits[0].status);
            assert_eq!(
                route.audits[0].retained_semantic_witnesses,
                individual.audits[0].retained_semantic_witnesses
            );
        }
    }

    #[test]
    fn source_batch_is_repeatable_and_counts_shared_work_only_once() {
        let room = shared_source_room();
        let config = audit_config();
        let evaluate = || {
            assess_easiest_known_routes_from_source(
                &room,
                "west",
                ["east-high", "east-low"],
                EvaluationLoadout::Baseline,
                &config,
            )
            .unwrap()
        };
        let first = evaluate();
        let second = evaluate();
        assert_eq!(first, second);

        assert_eq!(first.shared_audits.len(), 1);
        let shared = &first.shared_audits[0];
        assert_eq!(shared.target_count, 2);
        assert!(shared.operational_stats.expanded_nodes > 0);
        assert!(
            first
                .routes
                .iter()
                .all(|route| { route.audits[0].operational_stats == shared.operational_stats })
        );

        let incorrectly_multiplied_expansions = first
            .routes
            .iter()
            .map(|route| route.audits[0].operational_stats.expanded_nodes)
            .sum::<usize>();
        assert_eq!(
            incorrectly_multiplied_expansions,
            shared.operational_stats.expanded_nodes * first.routes.len()
        );
        assert_eq!(
            first.total_operational_stats().expanded_nodes,
            shared.operational_stats.expanded_nodes
        );
        assert!(first.total_operational_stats().expanded_nodes < incorrectly_multiplied_expansions);
    }

    #[test]
    fn semantic_duplicates_merge_probe_provenance_only_within_a_loadout() {
        let initial =
            Simulation::enter_via_door(flat_door_room(), AbilitySet::NONE, "west").unwrap();
        let replay = replay_to_east(&initial, |_| Action {
            move_x: 1,
            ..Action::default()
        });
        let first = observe_direct_witness(
            &initial,
            EvaluationLoadout::Baseline,
            0,
            "east",
            direct_witness(replay.clone(), DirectProbePolicy::Run, 0),
        )
        .unwrap();
        let second = observe_direct_witness(
            &initial,
            EvaluationLoadout::Baseline,
            0,
            "east",
            direct_witness(replay, DirectProbePolicy::AutoJump, 1),
        )
        .unwrap();
        let mut witnesses = Vec::new();
        let mut indices = HashMap::new();
        retain_semantic_witness(&mut witnesses, &mut indices, first);
        retain_semantic_witness(&mut witnesses, &mut indices, second);

        assert_eq!(witnesses.len(), 1);
        assert_eq!(witnesses[0].probes.len(), 2);
    }
}
